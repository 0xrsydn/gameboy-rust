//! ARM instruction buffering only. Timing and bus ownership remain separate.
use super::{Cpu, CpuError, InstructionSet};
use crate::memory::{Memory, MemoryError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ArmPipeline {
    pc: u32,
    // Deferred strict-fetch results for the next execute and decode positions.
    words: [Result<u32, MemoryError>; 2],
}

pub(super) struct Fetched {
    pub instruction: u32,
    pub continuation: Option<ArmPipeline>,
}

impl Cpu {
    /// Discard buffered instructions after debugger code edits or rebinding memory.
    /// Ordinary CPU/DMA stores and host inspection must not call this automatically.
    /// The next ARM step fills from its current PC without adding nominal cycles.
    pub fn invalidate_pipeline(&mut self) {
        self.arm_pipeline = None;
    }

    pub(super) fn fetch(&self, memory: &Memory) -> Result<Fetched, CpuError> {
        if self.instruction_set == InstructionSet::Thumb {
            return Ok(Fetched {
                instruction: u32::from(memory.read16(self.pc())?),
                continuation: None,
            });
        }
        let pc = self.pc();
        let retained = self.arm_pipeline.as_ref().filter(|pipe| pipe.pc == pc);
        let instruction = match retained {
            Some(pipe) => pipe.words[0].clone(),
            None => memory.fetch_arm_word(pc),
        }?;
        let decode = match retained {
            Some(pipe) => pipe.words[1].clone(),
            None => memory.fetch_arm_word(pc.wrapping_add(4)),
        };
        let fetched = memory.fetch_arm_word(pc.wrapping_add(8));
        Ok(Fetched {
            instruction,
            continuation: Some(ArmPipeline {
                pc: pc.wrapping_add(4),
                words: [decode, fetched],
            }),
        })
    }

    /// Capture target and target+4 after a successful refill, without early errors.
    /// Explicit exception entry without Memory leaves a cold buffer instead.
    pub(crate) fn refill_arm_pipeline(&mut self, memory: &Memory) {
        self.arm_pipeline = (self.instruction_set == InstructionSet::Arm).then(|| ArmPipeline {
            pc: self.pc(),
            words: [
                memory.fetch_arm_word(self.pc()),
                memory.fetch_arm_word(self.pc().wrapping_add(4)),
            ],
        });
    }
}
