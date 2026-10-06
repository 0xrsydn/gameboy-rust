//! ARM/Thumb instruction buffering and the next CPU fetch access kind.
use super::{Cpu, CpuError, InstructionSet};
use crate::{
    memory::{InstructionFetch, Memory, MemoryError},
    timing::AccessKind,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Pipeline {
    pc: u32,
    instruction_set: InstructionSet,
    // Deferred strict-fetch results for the next execute and decode positions.
    // Thumb halfwords are zero-extended; their upper bits are not another opcode.
    instructions: [Result<u32, MemoryError>; 2],
    pub(super) next_access: AccessKind,
}

pub(super) struct Fetched {
    pub instruction: u32,
    pub continuation: Pipeline,
    pub lookahead: InstructionFetch,
    pub fill: Option<[InstructionFetch; 2]>,
}

impl Cpu {
    /// Discard buffered instructions after debugger code edits or rebinding memory.
    /// Ordinary CPU/DMA stores and host inspection must not call this automatically.
    /// The next step uses nominal S startup with no separate fill charge.
    /// This also discards the prior CPU fetch kind, but not Memory's DMA override.
    pub fn invalidate_pipeline(&mut self) {
        self.pipeline = None;
    }

    /// Cold/debugger entry keeps the existing nominal S startup policy.
    /// A mismatched PC/state cannot reuse another sequence's access kind.
    pub(super) fn next_fetch_kind(&self) -> AccessKind {
        self.pipeline
            .as_ref()
            .filter(|pipe| pipe.pc == self.pc() && pipe.instruction_set == self.instruction_set)
            .map_or(AccessKind::Sequential, |pipe| pipe.next_access)
    }

    pub(super) fn fetch(&self, memory: &Memory) -> Result<Fetched, CpuError> {
        let pc = self.pc();
        let instruction_set = self.instruction_set;
        let width = instruction_set.width();
        let retained = self
            .pipeline
            .as_ref()
            .filter(|pipe| pipe.pc == pc && pipe.instruction_set == instruction_set);
        let (instruction, decode, fill) = if let Some(pipe) = retained {
            (
                pipe.instructions[0].clone()?,
                pipe.instructions[1].clone(),
                None,
            )
        } else {
            let current = memory.fetch_instruction(pc, instruction_set);
            let instruction = current.instruction.clone()?;
            let next = memory.fetch_instruction(pc.wrapping_add(width), instruction_set);
            (instruction, next.instruction.clone(), Some([current, next]))
        };
        let fetched = memory.fetch_instruction(pc.wrapping_add(2 * width), instruction_set);
        Ok(Fetched {
            instruction,
            continuation: Pipeline {
                pc: pc.wrapping_add(width),
                instruction_set,
                instructions: [decode, fetched.instruction.clone()],
                next_access: AccessKind::Sequential, // Set from execution only on success.
            },
            lookahead: fetched,
            fill,
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
        for (sample, kind) in fetches
            .iter()
            .zip([AccessKind::NonSequential, AccessKind::Sequential])
        {
            memory.record_code_access(sample.address(), instruction_set.access_width(), kind);
        }
        memory.refill_cpu_bus_history(&fetches);
        self.pipeline = Some(Pipeline {
            pc,
            instruction_set,
            instructions: fetches.map(|fetch| fetch.instruction),
            next_access: AccessKind::Sequential,
        });
    }
}
