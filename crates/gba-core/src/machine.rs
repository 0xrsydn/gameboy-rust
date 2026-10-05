//! CPU/device integration with nominal ARM7 instruction and GBA bus costs.
//! Device clocks update at instruction, DMA-unit, or HALT-idle boundaries, not per bus access.

use std::{error::Error, fmt};

use crate::{
    cpu::{Cpu, CpuError},
    dma::DmaError,
    memory::Memory,
    timing::StepTiming,
};

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
/// clocks, execute DMA, honor HALT/STOP, and deliver interrupts. No audio yet.
pub struct Machine {
    cpu: Cpu,
    memory: Memory,
    last_timing: StepTiming,
}

impl Machine {
    pub fn new(cpu: Cpu, memory: Memory) -> Self {
        Self {
            cpu,
            memory,
            last_timing: StepTiming::default(),
        }
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
    /// CPU writes take effect before this bulk device update. Within-instruction
    /// register timing, prefetch, and IRQ synchronization delays are not modeled.
    /// CPU/DMA diagnostics leave CPU, devices, and the clock unchanged for this step.
    /// Row-capture diagnostics are deferred to Memory::present_frame instead.
    pub fn step(&mut self) -> Result<StepKind, MachineError> {
        if self.memory.stopped() {
            self.last_timing = StepTiming::default();
            return Ok(StepKind::StopIdle);
        }
        if let Some((channel, timing)) = self.memory.step_dma().map_err(MachineError::Dma)? {
            self.last_timing = timing;
            return Ok(StepKind::Dma { channel });
        }
        if self.memory.halted() {
            let timing = StepTiming {
                idle_cycles: self.memory.next_event_cycles(),
                ..StepTiming::default()
            };
            self.memory.advance_cycles(timing.total());
            self.last_timing = timing;
            return Ok(StepKind::HaltIdle);
        }
        let (kind, timing) = if self.cpu.take_interrupt(self.memory.irq_pending(), false) {
            self.memory.invalidate_cpu_bus_history();
            self.cpu.refill_arm_pipeline(&self.memory);
            (StepKind::IrqEntry, self.cpu.exception_timing(&self.memory))
        } else {
            (
                StepKind::Instruction,
                self.cpu
                    .step_timed(&mut self.memory)
                    .map_err(MachineError::Cpu)?,
            )
        };
        self.memory.complete_cpu_step(timing.total());
        self.last_timing = timing;
        Ok(kind)
    }
}
