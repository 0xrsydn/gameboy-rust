//! CPU/device integration with nominal ARM7 instruction and GBA bus costs.
//! Device clocks update at instruction, DMA-unit, or HALT-idle boundaries, not per bus access.

use std::{error::Error, fmt};

use crate::{
    cpu::{Cpu, CpuError},
    dma::DmaError,
    io::HALT_WAKE_CYCLES,
    memory::{CpuSideState, Memory},
    timing::StepTiming,
};
mod snapshot;

/// Longest loop body, in bytes and in instruction steps, that idle detection considers.
const IDLE_LOOP_BYTES: u32 = 64;
const IDLE_LOOP_STEPS: u32 = 32;
/// Arming attempts to ignore at a loop head whose pass did not repeat, so loops that
/// count or poll devices do not pay for probing on every pass.
const IDLE_REJECT_ARMS: u16 = 256;

/// One pass of a candidate idle loop, measured from its head: the state there when the
/// pass began, the side-access count, and the clock.
struct IdleProbe {
    head: u32,
    cpu: Cpu,
    side: CpuSideState,
    accesses: u64,
    cycles: u64,
    steps: u32,
    /// The previous pass from this head also ended with different state.
    mismatched: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
    Instruction,
    IrqEntry,
    Dma {
        channel: usize,
    },
    /// Advance device clocks to the next event while the CPU stays paused.
    HaltIdle,
    /// System clock is off; no CPU, DMA, or device progress. Supply host input to wake.
    StopIdle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineError {
    Cpu(CpuError),
    Dma(DmaError),
}

impl fmt::Display for MachineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cpu(error) => write!(f, "{error}"),
            Self::Dma(error) => write!(f, "{error}"),
        }
    }
}

impl Error for MachineError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cpu(error) => Some(error),
            Self::Dma(error) => Some(error),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameRunError {
    Cpu(CpuError),
    Dma(DmaError),
    StepLimit(usize),
    Stopped,
}

impl fmt::Display for FrameRunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cpu(error) => write!(f, "{error}"),
            Self::Dma(error) => write!(f, "{error}"),
            Self::Stopped => write!(f, "machine is in STOP; no VBlank can occur until wake-up"),
            Self::StepLimit(limit) => write!(f, "no VBlank entry within {limit} machine steps"),
        }
    }
}

impl Error for FrameRunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cpu(error) => Some(error),
            Self::Dma(error) => Some(error),
            Self::StepLimit(_) | Self::Stopped => None,
        }
    }
}

/// Owns the CPU and memory bus. Use this instead of Cpu::step to advance device
/// clocks, execute DMA, honor HALT/STOP, and deliver interrupts. Host audio is external.
pub struct Machine {
    cpu: Cpu,
    memory: Memory,
    last_timing: StepTiming,
    idle_skip: bool,
    idle: Option<IdleProbe>,
    /// A loop head whose pass did not repeat, and how many more arms to ignore there.
    idle_rejected: Option<(u32, u16)>,
}

impl Machine {
    pub fn new(cpu: Cpu, memory: Memory) -> Self {
        Self {
            cpu,
            memory,
            last_timing: StepTiming::default(),
            idle_skip: false,
            idle: None,
            idle_rejected: None,
        }
    }

    /// Fast-forward verified idle loops (off by default). A short backward-branching
    /// loop qualifies when one whole pass is only instruction steps, makes no data access
    /// other than plain RAM and ROM reads, and leaves the CPU and its memory-side state
    /// exactly as they were at the loop head. Every further pass would then repeat it, so
    /// the step that completes such a pass also advances time over as many whole passes
    /// as end before the next display, timer, serial, capture, DMA, or interrupt event.
    /// Machine state at every step boundary is unchanged; only the number of steps falls.
    pub fn set_idle_skip(&mut self, enabled: bool) {
        self.idle_skip = enabled;
        self.idle = None;
        self.idle_rejected = None;
    }

    pub fn cpu(&self) -> &Cpu {
        &self.cpu
    }

    pub fn memory(&self) -> &Memory {
        &self.memory
    }

    /// Host setup and inspection. Bus writes do not automatically advance time.
    pub fn memory_mut(&mut self) -> &mut Memory {
        &mut self.memory
    }

    pub fn halted(&self) -> bool {
        self.memory.halted()
    }

    pub fn stopped(&self) -> bool {
        self.memory.stopped()
    }

    pub fn cycles(&self) -> u64 {
        self.memory.cycles()
    }

    /// Execute through the next entry to scanline 160, even when already in VBlank.
    /// Stops at the first instruction, DMA-unit, or HALT-idle boundary after the event.
    /// DMA requested by that event may still be pending. Prior successful
    /// steps remain committed on error or limit exhaustion. Zero steps always fails.
    /// StopIdle returns FrameRunError::Stopped immediately; no VBlank can advance.
    pub fn run_until_vblank(&mut self, max_steps: usize) -> Result<usize, FrameRunError> {
        let previous = self.memory.display_position().vblanks;
        for steps in 1..=max_steps {
            let kind = self.step().map_err(|error| match error {
                MachineError::Cpu(error) => FrameRunError::Cpu(error),
                MachineError::Dma(error) => FrameRunError::Dma(error),
            })?;
            if kind == StepKind::StopIdle {
                return Err(FrameRunError::Stopped);
            }
            if self.memory.display_position().vblanks != previous {
                return Ok(steps);
            }
        }
        Err(FrameRunError::StepLimit(max_steps))
    }

    /// Cost of the last successful step. Failed steps leave this unchanged.
    pub fn last_timing(&self) -> StepTiming {
        self.last_timing
    }

    /// STOP returns StopIdle with zero timing before DMA, IRQ, or CPU work.
    /// Otherwise, service one ready DMA unit before sampling IRQ or CPU code.
    /// Lower channel numbers take priority; higher-priority requests can preempt
    /// between units. The CPU and its registers remain paused throughout DMA.
    /// If HALT is still waiting, advance to the next display edge or timer overflow.
    /// HALT wakes on IE & IF; STOP wakes on the enabled live keypad condition.
    /// Both ignore IME and CPSR.I; IRQ delivery still respects both masks.
    /// Sample IRQ before executing the next instruction. GBA devices do not
    /// generate FIQ. IRQ entry consumes a separate step and does not clear IF.
    /// CPU/DMA work uses nominal costs; HALT advances exactly to the next device event.
    /// Timer, sound, and serial accesses use staged bus-completion phases. Other device writes
    /// precede their bulk clock update. Requests reach the CPU after `IRQ_DELAY` cycles. Opcode
    /// prefetch follows ordered nominal CPU/DMA costs, not per-cycle arbitration.
    /// CPU/DMA diagnostics leave CPU, devices, and the clock unchanged for this step.
    /// Row-capture diagnostics are deferred to Memory::present_frame instead.
    pub fn step(&mut self) -> Result<StepKind, MachineError> {
        if !self.idle_skip {
            return self.step_once();
        }
        let before = self.cpu.pc();
        let result = self.step_once();
        match result {
            Ok(StepKind::Instruction) => self.track_idle_loop(before),
            _ => self.idle = None,
        }
        result
    }

    /// After an instruction step: arm on a short backward branch, and at the end of each
    /// pass through the armed loop, skip whole passes if the pass repeated exactly.
    fn track_idle_loop(&mut self, before: u32) {
        let pc = self.cpu.pc();
        if let Some(probe) = &mut self.idle {
            probe.steps += 1;
            if pc != probe.head {
                if probe.steps >= IDLE_LOOP_STEPS {
                    self.idle = None; // Left the loop, or it is too long.
                }
                return;
            }
            // A write or device read is a property of the loop's code: give up at once.
            // Changed state can be a first pass's warm-up (after an IRQ return, say),
            // so give up only when two passes in a row differ, as in counting loops.
            let pure = self.memory.side_accesses() == probe.accesses;
            let repeated =
                pure && self.memory.cpu_side_state() == probe.side && self.cpu == probe.cpu;
            if !repeated {
                if !pure || probe.mismatched {
                    self.idle = None;
                    self.idle_rejected = Some((pc, IDLE_REJECT_ARMS));
                } else {
                    let mut next = self.probe_at(pc);
                    next.mismatched = true;
                    self.idle = Some(next);
                }
                return;
            }
            let period = self.memory.cycles().wrapping_sub(probe.cycles);
            if period != 0 && period < u64::from(u32::MAX) {
                let period = period as u32;
                let budget = self.memory.idle_budget();
                if budget > period {
                    self.memory
                        .advance_idle_cycles((budget - 1) / period * period);
                }
            }
            self.idle = Some(self.probe_at(pc));
        } else if pc < before && before - pc <= IDLE_LOOP_BYTES {
            match &mut self.idle_rejected {
                Some((head, arms)) if *head == pc && *arms != 0 => *arms -= 1,
                _ => self.idle = Some(self.probe_at(pc)),
            }
        }
    }

    fn probe_at(&self, head: u32) -> IdleProbe {
        IdleProbe {
            head,
            cpu: self.cpu.clone(),
            side: self.memory.cpu_side_state(),
            accesses: self.memory.side_accesses(),
            cycles: self.memory.cycles(),
            steps: 0,
            mismatched: false,
        }
    }

    fn step_once(&mut self) -> Result<StepKind, MachineError> {
        if self.memory.stopped() {
            self.last_timing = StepTiming::default();
            return Ok(StepKind::StopIdle);
        }
        if let Some((channel, timing)) = self.memory.step_dma().map_err(MachineError::Dma)? {
            self.last_timing = timing;
            return Ok(StepKind::Dma { channel });
        }
        if self.memory.halted() {
            let mut timing = StepTiming {
                idle_cycles: self.memory.next_event_cycles(),
                ..StepTiming::default()
            };
            self.memory.advance_halt_cycles(timing.total());
            if !self.memory.halted() && !self.memory.stopped() {
                // The CPU restarts a few cycles after the request line wakes it.
                self.memory.advance_halt_cycles(HALT_WAKE_CYCLES);
                timing.idle_cycles += HALT_WAKE_CYCLES;
            }
            self.last_timing = timing;
            return Ok(StepKind::HaltIdle);
        }
        self.memory.begin_timer_step();
        let result = if let Some(timing) = self.cpu.take_irq_timed(&mut self.memory) {
            Ok((StepKind::IrqEntry, timing))
        } else {
            self.cpu
                .step_timed(&mut self.memory)
                .map(|timing| (StepKind::Instruction, timing))
                .map_err(MachineError::Cpu)
        };
        let (kind, timing) = match result {
            Ok(result) => result,
            Err(error) => {
                self.memory.discard_timer_step();
                return Err(error);
            }
        };
        self.memory.complete_cpu_step(timing.total());
        self.last_timing = timing;
        Ok(kind)
    }
}
