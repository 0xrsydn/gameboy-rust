//! ARM/Thumb instruction buffering. Timing and bus ownership remain separate.
use super::{Cpu, CpuError, InstructionSet};
use crate::memory::{InstructionFetch, Memory, MemoryError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Pipeline {
    pc: u32,
    instruction_set: InstructionSet,
    // Deferred strict-fetch results for the next execute and decode positions.
    // Thumb halfwords are zero-extended; their upper bits are not another opcode.
    instructions: [Result<u32, MemoryError>; 2],
}

pub(super) struct Fetched {
    pub instruction: u32,
    pub continuation: Pipeline,
    pub lookahead: InstructionFetch,
}

impl Cpu {
    /// Discard buffered instructions after debugger code edits or rebinding memory.
    /// Ordinary CPU/DMA stores and host inspection must not call this automatically.
    /// The next step fills from its current PC/state without adding nominal cycles.
    pub fn invalidate_pipeline(&mut self) {
        self.pipeline = None;
    }

    pub(super) fn fetch(&self, memory: &Memory) -> Result<Fetched, CpuError> {
        let pc = self.pc();
        let instruction_set = self.instruction_set;
        let width = instruction_set.width();
        let retained = self
            .pipeline
            .as_ref()
            .filter(|pipe| pipe.pc == pc && pipe.instruction_set == instruction_set);
        let instruction = match retained {
            Some(pipe) => pipe.instructions[0].clone(),
            None => memory.fetch_instruction(pc, instruction_set).instruction,
        }?;
        let decode = match retained {
            Some(pipe) => pipe.instructions[1].clone(),
            None => {
                memory
                    .fetch_instruction(pc.wrapping_add(width), instruction_set)
                    .instruction
            }
        };
        let fetched = memory.fetch_instruction(pc.wrapping_add(2 * width), instruction_set);
        Ok(Fetched {
            instruction,
            continuation: Pipeline {
                pc: pc.wrapping_add(width),
                instruction_set,
                instructions: [decode, fetched.instruction.clone()],
            },
            lookahead: fetched,
        })
    }

    /// Capture the target pair using the resulting state, without early errors.
    /// Explicit exception entry without Memory leaves a cold buffer instead.
    pub(crate) fn refill_pipeline(&mut self, memory: &mut Memory) {
        let pc = self.pc();
        let instruction_set = self.instruction_set;
        let fetches = [
            memory.fetch_instruction(pc, instruction_set),
            memory.fetch_instruction(pc.wrapping_add(instruction_set.width()), instruction_set),
        ];
        memory.refill_cpu_bus_history(&fetches);
        self.pipeline = Some(Pipeline {
            pc,
            instruction_set,
            instructions: fetches.map(|fetch| fetch.instruction),
        });
    }
}
