use gba_rust::{
    cpu::{Cpu, InstructionSet, Mode},
    demo_bios, exception_demo_program,
    memory::{Memory, MemoryError, BIOS_SIZE, ROM_START},
    EXCEPTION_DEMO_STEPS,
};

#[test]
fn original_demo_enters_supervisor_and_returns_to_arm_and_thumb() {
    let mut memory = Memory::with_bios(exception_demo_program(), demo_bios()).unwrap();
    let mut cpu = Cpu::new(ROM_START);
    let expected = [
        (8, Mode::Supervisor, InstructionSet::Arm),
        (0x40, Mode::Supervisor, InstructionSet::Arm),
        (0x44, Mode::Supervisor, InstructionSet::Arm),
        (ROM_START + 4, Mode::System, InstructionSet::Arm),
        (ROM_START + 8, Mode::System, InstructionSet::Arm),
        (ROM_START + 12, Mode::System, InstructionSet::Thumb),
        (8, Mode::Supervisor, InstructionSet::Arm),
        (0x40, Mode::Supervisor, InstructionSet::Arm),
        (0x44, Mode::Supervisor, InstructionSet::Arm),
        (ROM_START + 14, Mode::System, InstructionSet::Thumb),
        (ROM_START + 14, Mode::System, InstructionSet::Thumb),
    ];
    assert_eq!(expected.len(), EXCEPTION_DEMO_STEPS);
    for (pc, mode, state) in expected {
        cpu.step(&mut memory).unwrap();
        assert_eq!(
            (cpu.pc(), cpu.mode(), cpu.instruction_set()),
            (pc, mode, state)
        );
    }
    assert_eq!(cpu.registers()[10], 2);
    assert_eq!(cpu.registers()[14], 0); // Supervisor LR did not replace System LR
    assert_eq!(cpu.cpsr(), 0x3f);
    for _ in 0..5 {
        cpu.step(&mut memory).unwrap();
    }
    assert_eq!(cpu.pc(), ROM_START + 14);
    assert_eq!(cpu.registers()[10], 2);
}

#[test]
fn optional_bios_image_is_exactly_16_kib_read_only_and_not_mirrored() {
    for size in [0, BIOS_SIZE - 1, BIOS_SIZE + 1] {
        assert!(matches!(
            Memory::with_bios(vec![], vec![0; size]),
            Err(MemoryError::InvalidBiosSize(actual)) if actual == size
        ));
    }
    let bios: Vec<u8> = (0..BIOS_SIZE).map(|index| index as u8).collect();
    let mut memory = Memory::with_bios(vec![], bios).unwrap();
    assert_eq!(memory.read32(0).unwrap(), 0x0302_0100);
    assert_eq!(memory.read16(0x3ffe).unwrap(), 0xfffe);
    assert_eq!(memory.read8(0x3fff).unwrap(), 255);
    for address in [0x4000, 0x0100_0000] {
        assert_eq!(memory.read8(address), Err(MemoryError::Unmapped(address)));
        assert_eq!(
            memory.write8(address, 1),
            Err(MemoryError::Unmapped(address))
        );
    }
    assert_eq!(memory.write8(0x3fff, 0), Err(MemoryError::ReadOnly(0x3fff)));
    assert_eq!(
        memory.write16(0x3ffe, 0),
        Err(MemoryError::ReadOnly(0x3ffe))
    );
    assert_eq!(memory.write32(0, 0), Err(MemoryError::ReadOnly(0)));
    assert_eq!(memory.read32(0).unwrap(), 0x0302_0100);
    assert_eq!(memory.read16(0x3ffe).unwrap(), 0xfffe);
}

#[test]
fn no_bios_is_mapped_by_default() {
    let mut memory = Memory::new(vec![]).unwrap();
    assert_eq!(memory.read32(0), Err(MemoryError::Unmapped(0)));
    assert_eq!(memory.write32(0, 0), Err(MemoryError::Unmapped(0)));
}

#[test]
fn reset_can_fetch_caller_supplied_vector_code() {
    let mut bios = vec![0; BIOS_SIZE];
    bios[0..4].copy_from_slice(&0xe3a0_002a_u32.to_le_bytes()); // MOV r0, #42
    let mut memory = Memory::with_bios(vec![], bios).unwrap();
    let mut cpu = Cpu::at_reset();
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers()[0], 42);
    assert_eq!(cpu.pc(), 4);
    assert_eq!(cpu.cpsr(), 0xd3);
}
