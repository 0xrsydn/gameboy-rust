//! Independent fetch-observation checks. Mutation between sample and consumption
//! checks ownership of captured values, not sub-instruction hardware arbitration.
use super::*;

const WORD: u32 = 0x9abc_1234;
const CHANGED: u32 = 0x5678_def0;
const UNUSED: u32 = 0x8000_0000;

#[test]
fn mapped_fetches_separate_instruction_width_from_driven_bus_lanes() {
    let mut bios = vec![0; BIOS_SIZE];
    bios[..4].copy_from_slice(&WORD.to_le_bytes());
    let mut memory = Memory::with_bios(WORD.to_le_bytes().to_vec(), bios).unwrap();
    // Explicit expected bus words for each Thumb alignment; IWRAM needs prior lanes.
    for (address, expected) in [
        (0, [Some(WORD), Some(WORD)]),
        (0x0200_0000, [Some(0x1234_1234), Some(0x9abc_9abc)]),
        (0x0300_0000, [None, None]),
        (0x0500_0000, [Some(0x1234_1234), Some(0x9abc_9abc)]),
        (0x0600_0000, [Some(0x1234_1234), Some(0x9abc_9abc)]),
        (0x0700_0000, [Some(WORD), Some(WORD)]),
        (0x0800_0000, [Some(0x1234_1234), Some(0x9abc_9abc)]),
        (0x0a00_0000, [Some(0x1234_1234), Some(0x9abc_9abc)]),
        (0x0c00_0000, [Some(0x1234_1234), Some(0x9abc_9abc)]),
    ] {
        if (0x0200_0000..0x0800_0000).contains(&address) {
            memory.write32(address, WORD).unwrap();
        }
        let arm = memory.fetch_instruction(address, InstructionSet::Arm);
        assert_eq!(arm.instruction, Ok(WORD));
        assert_eq!(arm.bus_word, Some(WORD));
        for (lane, expected) in expected.into_iter().enumerate() {
            let thumb = memory.fetch_instruction(address + lane as u32 * 2, InstructionSet::Thumb);
            assert_eq!(
                thumb.instruction,
                Ok(if lane == 0 { 0x1234 } else { 0x9abc })
            );
            assert_eq!(thumb.bus_word, expected);
        }
    }
}

#[test]
fn fetch_errors_remain_strict_and_narrow_rom_fetches_need_only_two_bytes() {
    let memory = Memory::new(vec![0x34, 0x12]).unwrap();
    let thumb = memory.fetch_instruction(ROM_START, InstructionSet::Thumb);
    assert_eq!(thumb.instruction, Ok(0x1234));
    assert_eq!(thumb.bus_word, Some(0x1234_1234));
    for (address, state, error) in [
        (
            ROM_START,
            InstructionSet::Arm,
            MemoryError::Unmapped(ROM_START + 2),
        ),
        (
            ROM_START + 2,
            InstructionSet::Thumb,
            MemoryError::Unmapped(ROM_START + 2),
        ),
        (0, InstructionSet::Thumb, MemoryError::Unmapped(0)),
        (UNUSED, InstructionSet::Arm, MemoryError::Unmapped(UNUSED)),
        (
            ROM_START + 1,
            InstructionSet::Thumb,
            MemoryError::Unaligned(ROM_START + 1),
        ),
        (
            ROM_START + 2,
            InstructionSet::Arm,
            MemoryError::Unaligned(ROM_START + 2),
        ),
    ] {
        let fetch = memory.fetch_instruction(address, state);
        assert_eq!(fetch.instruction, Err(error));
        assert_eq!(fetch.bus_word, None);
    }
}

#[test]
fn refill_history_consumes_captured_target_halfwords_without_rereading_memory() {
    for target in [0x0300_0100, 0x0300_0102] {
        let mut memory = Memory::new(vec![]).unwrap();
        memory.write16(target, 0x1234).unwrap();
        memory.write16(target + 2, 0x9abc).unwrap();
        memory.write16(target + 4, 0x5678).unwrap();
        let fetches = [
            memory.fetch_instruction(target, InstructionSet::Thumb),
            memory.fetch_instruction(target + 2, InstructionSet::Thumb),
        ];
        memory.write16(target, 0xffff).unwrap();
        memory.write16(target + 2, 0xffff).unwrap();
        memory.refill_cpu_bus_history(&fetches);
        let lookahead = memory.fetch_instruction(target + 4, InstructionSet::Thumb);
        memory.begin_cpu_access(target, InstructionSet::Thumb, None, &lookahead);
        assert_eq!(
            memory.read32(UNUSED).unwrap(),
            if target & 2 == 0 {
                0x9abc_5678
            } else {
                0x5678_9abc
            }
        );
        memory.end_cpu_access(true);
    }
}

#[test]
fn bios_history_commits_the_captured_bus_word_not_later_image_bytes() {
    for (pc, state, fetch_address) in [
        (0x100, InstructionSet::Arm, 0x108),
        (0x100, InstructionSet::Thumb, 0x104),
        (0x102, InstructionSet::Thumb, 0x106),
    ] {
        let mut bios = vec![0; BIOS_SIZE];
        let word_address = (fetch_address & !3) as usize;
        bios[word_address..word_address + 4].copy_from_slice(&WORD.to_le_bytes());
        let mut memory = Memory::with_bios(vec![], bios).unwrap();
        let fetch = memory.fetch_instruction(fetch_address, state);
        // Test-only image mutation: BIOS remains read-only through normal APIs.
        memory.bios.as_mut().unwrap()[word_address..word_address + 4]
            .copy_from_slice(&CHANGED.to_le_bytes());
        memory.begin_cpu_access(pc, state, None, &fetch);
        memory.end_cpu_access(true);
        assert_eq!(memory.bios_prefetch, Some(WORD));
        let outside = memory.fetch_instruction(0x0200_0008, InstructionSet::Arm);
        memory.begin_cpu_access(0x0200_0000, InstructionSet::Arm, None, &outside);
        assert_eq!(memory.read32(word_address as u32).unwrap(), WORD);
        // Instruction reads still bypass the protected data value.
        assert_eq!(
            memory
                .fetch_instruction(word_address as u32, InstructionSet::Arm)
                .instruction,
            Ok(CHANGED)
        );
        memory.end_cpu_access(true);
    }
}

#[test]
fn sampling_has_no_timing_or_history_side_effects_and_never_uses_data_fallback() {
    let mut bios = vec![0; BIOS_SIZE];
    bios[8..12].copy_from_slice(&WORD.to_le_bytes());
    let mut memory = Memory::with_bios(vec![], bios).unwrap();
    memory.cpu_resume_nonsequential = true;
    memory.begin_cpu_timing();
    let fetch = memory.fetch_instruction(8, InstructionSet::Arm);
    memory.fetch_instruction(0x0300_0000, InstructionSet::Thumb);
    assert_eq!(memory.end_cpu_timing(true), StepTiming::default());
    assert_eq!(memory.bios_prefetch, None);
    assert!(memory.cpu_resume_nonsequential);
    assert_eq!(memory.cycles(), 0);
    memory.begin_cpu_access(0, InstructionSet::Arm, None, &fetch);
    assert_eq!(memory.read32(UNUSED).unwrap(), WORD);
    let missing = memory.fetch_instruction(UNUSED, InstructionSet::Arm);
    assert_eq!(missing.instruction, Err(MemoryError::Unmapped(UNUSED)));
    assert_eq!(missing.bus_word, None);
    memory.end_cpu_access(false);
    assert_eq!(memory.bios_prefetch, None);
    assert!(memory.cpu_resume_nonsequential);
}

#[test]
fn missing_fetch_observation_invalidates_bios_history_only_on_success() {
    let mut memory = Memory::with_bios(vec![], vec![0; BIOS_SIZE]).unwrap();
    memory.bios_prefetch = Some(WORD);
    for state in [InstructionSet::Arm, InstructionSet::Thumb] {
        let pc = BIOS_SIZE as u32 - state.width();
        let fetch = memory.fetch_instruction(pc + 2 * state.width(), state);
        assert!(fetch.instruction.is_err());
        memory.begin_cpu_access(pc, state, None, &fetch);
        memory.end_cpu_access(false);
        assert_eq!(memory.bios_prefetch, Some(WORD));
    }
    let fetch = memory.fetch_instruction(BIOS_SIZE as u32, InstructionSet::Thumb);
    memory.begin_cpu_access(BIOS_SIZE as u32 - 4, InstructionSet::Thumb, None, &fetch);
    memory.end_cpu_access(true);
    assert_eq!(memory.bios_prefetch, None);
}
