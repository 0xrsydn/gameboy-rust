//! Nominal ARM7 instruction-cycle summaries, separate from instruction semantics.
//! Code costs use source-fetch and target-pair addresses; data costs use actual bus accesses.
//! Source access kinds remain instruction-local nominal summaries.

use super::{Cpu, CpuError, InstructionSet};
use crate::{
    memory::Memory,
    timing::{bus_cycles, refill_cycles, AccessKind, AccessWidth, StepTiming},
};

#[derive(Default)]
struct Summary {
    code_kind: AccessKind,
    refill: bool,
    internal: u32,
}

impl Summary {
    fn branch() -> Self {
        Self {
            refill: true,
            ..Self::default()
        }
    }

    fn transfer(load: bool, pc: bool) -> Self {
        Self {
            code_kind: if load {
                AccessKind::Sequential
            } else {
                AccessKind::NonSequential
            },
            refill: load && pc,
            internal: u32::from(load),
        }
    }
}

impl InstructionSet {
    fn access_width(self) -> AccessWidth {
        match self {
            Self::Arm => AccessWidth::Word,
            Self::Thumb => AccessWidth::Halfword,
        }
    }
}

impl Cpu {
    /// Execute one instruction and return its nominal cycle cost. Like Cpu::step,
    /// this does not advance device time or sample interrupts. Machine does that.
    /// Failed instructions discard their timing trace and retain existing diagnostics.
    pub fn step_timed(&mut self, memory: &mut Memory) -> Result<StepTiming, CpuError> {
        let fetched = self.fetch(memory)?;
        let instruction = fetched.instruction;
        let summary = match self.instruction_set {
            InstructionSet::Arm => self.arm_summary(instruction),
            InstructionSet::Thumb => self.thumb_summary(instruction),
        };
        let fetch_address = fetched.lookahead.address();
        let width = self.instruction_set.access_width();
        // A WAITCNT store affects subsequent instructions, not this code access.
        let waitcnt = memory.waitcnt();
        let code_kind = memory.cpu_code_kind(summary.code_kind);
        memory.begin_data_timing();
        let result = self.execute_fetched(fetched, memory);
        let data_cycles = memory.end_data_timing();
        result?;
        let mut code_cycles = bus_cycles(waitcnt, fetch_address, width, code_kind);
        if summary.refill {
            code_cycles += refill_cycles(waitcnt, self.pc(), self.instruction_set.access_width());
        }
        Ok(StepTiming {
            code_cycles,
            data_cycles,
            internal_cycles: summary.internal,
            idle_cycles: 0,
        })
    }

    /// Accept IRQ between instructions, discard the old-state fetch, then refill ARM.
    /// A missing discarded fetch cannot prevent entry or become a current-slot error.
    pub(crate) fn take_irq_timed(&mut self, memory: &mut Memory) -> Option<StepTiming> {
        let state = self.instruction_set;
        let address = self.pc().wrapping_add(2 * state.width());
        if !self.take_interrupt(memory.irq_pending(), false) {
            return None;
        }
        let waitcnt = memory.waitcnt();
        let kind = memory.cpu_code_kind(AccessKind::Sequential);
        let fetch = memory.fetch_instruction(address, state);
        memory.discarded_cpu_fetch(&fetch);
        self.refill_pipeline(memory);
        Some(StepTiming {
            code_cycles: bus_cycles(waitcnt, fetch.address(), state.access_width(), kind)
                + refill_cycles(waitcnt, self.pc(), self.instruction_set.access_width()),
            ..StepTiming::default()
        })
    }

    fn arm_summary(&self, instruction: u32) -> Summary {
        if !self.flags.condition_passed(instruction >> 28) {
            return Summary::default();
        }
        if instruction & 0x0f00_0000 == 0x0f00_0000
            || instruction & 0x0fff_fff0 == 0x012f_ff10
            || instruction & 0x0e00_0000 == 0x0a00_0000
        {
            return Summary::branch();
        }
        if instruction & 0x0fbf_0fff == 0x010f_0000
            || instruction & 0x0fb0_fff0 == 0x0120_f000
            || instruction & 0x0fb0_f000 == 0x0320_f000
        {
            return Summary::default(); // MRS/MSR overlap the data-processing space.
        }
        if instruction & 0x0e00_0000 == 0x0800_0000 {
            let list = instruction & 0xffff;
            return Summary::transfer(
                instruction & (1 << 20) != 0,
                list == 0 || list & 0x8000 != 0,
            );
        }
        if instruction & 0x0fb0_0ff0 == 0x0100_0090 {
            return Summary {
                internal: 1,
                ..Summary::default()
            }; // SWP: two N data accesses
        }
        if instruction & 0x0f80_00f0 == 0x0080_0090 {
            let signed = instruction & (1 << 22) != 0;
            let multiplier = self.registers[((instruction >> 8) & 15) as usize];
            return Summary {
                internal: multiply_cycles(multiplier, signed)
                    + 1
                    + u32::from(instruction & (1 << 21) != 0),
                ..Summary::default()
            };
        }
        if instruction & 0x0fc0_00f0 == 0x0000_0090 {
            let multiplier = self.registers[((instruction >> 8) & 15) as usize];
            return Summary {
                internal: multiply_cycles(multiplier, true)
                    + u32::from(instruction & (1 << 21) != 0),
                ..Summary::default()
            };
        }
        if instruction & 0x0e00_0090 == 0x0000_0090 {
            return Summary::transfer(instruction & (1 << 20) != 0, false);
        }
        if instruction & 0x0c00_0000 == 0x0400_0000 {
            return Summary::transfer(instruction & (1 << 20) != 0, (instruction >> 12) & 15 == 15);
        }
        let test = (8..=11).contains(&((instruction >> 21) & 15));
        Summary {
            refill: !test && (instruction >> 12) & 15 == 15,
            internal: u32::from(instruction & (1 << 25) == 0 && instruction & (1 << 4) != 0),
            ..Summary::default()
        }
    }

    // Share the existing control-flow classification with bounded bus history.
    // A PC write can refill even when its target equals the sequential address.
    pub(super) fn instruction_refills(&self, instruction: u32) -> bool {
        match self.instruction_set {
            InstructionSet::Arm => self.arm_summary(instruction).refill,
            InstructionSet::Thumb => self.thumb_summary(instruction).refill,
        }
    }

    fn thumb_summary(&self, instruction: u32) -> Summary {
        match instruction {
            0x4000..=0x43ff => {
                let opcode = (instruction >> 6) & 15;
                let internal = match opcode {
                    2 | 3 | 4 | 7 => 1,
                    // Thumb MUL maps the incoming destination to ARM's Rs.
                    13 => multiply_cycles(self.registers[(instruction & 7) as usize], true),
                    _ => 0,
                };
                Summary {
                    internal,
                    ..Summary::default()
                }
            }
            0x4400..=0x47ff => {
                let opcode = (instruction >> 8) & 3;
                let destination = (instruction & 7) | ((instruction >> 4) & 8);
                if opcode == 3 || (opcode != 1 && destination == 15) {
                    Summary::branch()
                } else {
                    Summary::default()
                }
            }
            0x4800..=0x4fff => Summary::transfer(true, false),
            0x5000..=0x5fff => Summary::transfer((instruction >> 9) & 7 >= 3, false),
            0x6000..=0x9fff => Summary::transfer(instruction & (1 << 11) != 0, false),
            0xb400..=0xb5ff | 0xbc00..=0xbdff => Summary::transfer(
                instruction & (1 << 11) != 0,
                instruction & 0x100 != 0 || instruction & 0xff == 0,
            ),
            0xc000..=0xcfff => {
                Summary::transfer(instruction & (1 << 11) != 0, instruction & 255 == 0)
            }
            0xd000..=0xddff if self.flags.condition_passed((instruction >> 8) & 15) => {
                Summary::branch()
            }
            0xdf00..=0xdfff | 0xe000..=0xe7ff | 0xf800..=0xffff => Summary::branch(),
            // Thumb BL prefix fetches once; its suffix adds the target N+S pair.
            _ => Summary::default(),
        }
    }
}

fn multiply_cycles(value: u32, signed: bool) -> u32 {
    for bytes in 1..=3 {
        let mask = u32::MAX << (bytes * 8);
        let high = value & mask;
        if high == 0 || signed && high == mask {
            return bytes;
        }
    }
    4
}
