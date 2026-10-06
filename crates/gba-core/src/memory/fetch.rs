//! One mapped sample supplies instruction bits and the supported bus observation.
//! Sampling is side-effect-free; execution commits history under the diagnostic policy.
use super::{InstructionSet, Memory, MemoryError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstructionFetch {
    pub(super) address: u32,
    pub(super) state: InstructionSet,
    pub(crate) instruction: Result<u32, MemoryError>,
    // None for unsupported regions, failed reads, or lane-based Thumb IWRAM history.
    pub(super) bus_word: Option<u32>,
}

impl Memory {
    pub(crate) fn fetch_instruction(
        &self,
        address: u32,
        state: InstructionSet,
    ) -> InstructionFetch {
        let instruction = self.read_instruction(address, state);
        let bus_word = instruction.as_ref().ok().copied().and_then(|value| {
            if state == InstructionSet::Arm {
                return Some(value);
            }
            match address >> 24 {
                // BIOS/OAM drive the aligned word. Read only the other halfword;
                // do not sample the instruction bytes a second time.
                0x00 | 0x07 => {
                    let other = self
                        .read_instruction(address ^ 2, InstructionSet::Thumb)
                        .ok()?;
                    Some(if address & 2 == 0 {
                        value | (other << 16)
                    } else {
                        other | (value << 16)
                    })
                }
                // IWRAM updates its existing addressed lanes at execution entry.
                0x03 => None,
                0x02 | 0x05 | 0x06 | 0x08..=0x0d => Some(value * 0x0001_0001),
                _ => None,
            }
        });
        InstructionFetch {
            address,
            state,
            instruction,
            bus_word,
        }
    }

    /// Strict mapped bytes, without data context, timing, or bus-history changes.
    /// An unavailable wider bus observation must not invalidate a valid instruction.
    fn read_instruction(&self, address: u32, state: InstructionSet) -> Result<u32, MemoryError> {
        let width = state.width() as usize;
        if address & (width as u32 - 1) != 0 {
            return Err(MemoryError::Unaligned(address));
        }
        let mut bytes = [0; 4];
        for (offset, byte) in bytes[..width].iter_mut().enumerate() {
            *byte = self.read_mapped_byte(address.wrapping_add(offset as u32))?;
        }
        Ok(u32::from_le_bytes(bytes))
    }
}
