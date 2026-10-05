use gba_core::{
    cpu::{Cpu, CpuError, InstructionSet},
    memory::{Memory, MemoryError, ROM_START},
};
use gba_demos::{demo_program, DEMO_STEPS};

fn program(words: &[u32]) -> Memory {
    Memory::new(words.iter().flat_map(|word| word.to_le_bytes()).collect()).unwrap()
}

#[test]
fn demo_computes_values_and_loops() {
    let mut memory = Memory::new(demo_program()).unwrap();
    let mut cpu = Cpu::new(ROM_START);
    let mut saw_thumb = false;
    for _ in 0..DEMO_STEPS + 5 {
        saw_thumb |= cpu.instruction_set() == InstructionSet::Thumb;
        cpu.step(&mut memory).unwrap();
    }
    assert!(saw_thumb);
    assert_eq!(cpu.instruction_set(), InstructionSet::Arm);
    assert_eq!(&cpu.registers()[..3], &[42, 7, 42]);
    assert_eq!(cpu.pc(), ROM_START + 96);
    assert_eq!(cpu.registers()[3], 0x0200_0000);
    assert_eq!(&cpu.registers()[4..8], &[42, 84, 3528, 42]);
    assert_eq!(memory.read32(0x0200_0000).unwrap(), 3528);
    assert_eq!(memory.read32(0x0200_0004).unwrap(), 42);
    assert_eq!(cpu.registers()[13], 0x0300_0100);
    assert_eq!(cpu.registers()[14], ROM_START + 109);
    assert_eq!(memory.read32(0x0300_00f8).unwrap(), 42);
    assert_eq!(memory.read32(0x0300_00fc).unwrap(), ROM_START + 109);
    assert!(cpu.flags().zero);
    assert!(cpu.flags().carry);
}

#[test]
fn memory_is_little_endian() {
    let memory = program(&[0x1234_5678]);
    assert_eq!(memory.read8(ROM_START).unwrap(), 0x78);
    assert_eq!(memory.read32(ROM_START).unwrap(), 0x1234_5678);
}

#[test]
fn ram_is_zeroed_writable_and_mirrored() {
    let mut memory = Memory::new(vec![]).unwrap();
    for (base, size) in [(0x0200_0000, 0x40000), (0x0300_0000, 0x8000)] {
        assert_eq!(memory.read32(base).unwrap(), 0);
        memory.write8(base, 0x12).unwrap();
        memory.write8(base + 1, 0x34).unwrap();
        assert_eq!(memory.read32(base + size).unwrap(), 0x3412);
        memory.write8(base + size - 1, 0xab).unwrap();
        assert_eq!(memory.read8(base + size * 2 - 1).unwrap(), 0xab);
    }
}

#[test]
fn cartridge_has_three_windows() {
    let memory = program(&[0x1234_5678]);
    for address in [0x0800_0000, 0x0a00_0000, 0x0c00_0000] {
        assert_eq!(memory.read32(address).unwrap(), 0x1234_5678);
    }
}

#[test]
fn invalid_memory_accesses_return_errors() {
    let mut memory = program(&[0]);
    assert_eq!(memory.read8(0), Err(MemoryError::Unmapped(0)));
    assert_eq!(
        memory.read32(ROM_START + 1),
        Err(MemoryError::Unaligned(ROM_START + 1))
    );
    assert_eq!(
        memory.read8(ROM_START + 4),
        Err(MemoryError::Unmapped(ROM_START + 4))
    );
    assert_eq!(
        memory.write8(ROM_START, 1),
        Err(MemoryError::ReadOnly(ROM_START))
    );
    assert_eq!(memory.write8(0, 1), Err(MemoryError::Unmapped(0)));
    assert_eq!(
        memory.read32(0xffff_fffc),
        Err(MemoryError::Unmapped(0xffff_fffc))
    );
}

#[test]
fn oversized_rom_is_rejected() {
    assert!(matches!(
        Memory::new(vec![0; 32 * 1024 * 1024 + 1]),
        Err(MemoryError::RomTooLarge(_))
    ));
}

#[test]
fn rotated_immediate_is_decoded() {
    let mut memory = program(&[0xe3a0_0480]); // MOV r0, #0x80000000
    let mut cpu = Cpu::new(ROM_START);
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers()[0], 0x8000_0000);
}

#[test]
fn arithmetic_wraps_at_32_bits() {
    let mut memory = program(&[
        0xe240_0001, // SUB r0, r0, #1
        0xe280_1001, // ADD r1, r0, #1
    ]);
    let mut cpu = Cpu::new(ROM_START);
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers()[0], u32::MAX);
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers()[1], 0);
}

#[test]
fn reading_pc_as_operand_includes_pipeline_offset() {
    let mut memory = program(&[0xe28f_0004]); // ADD r0, pc, #4
    let mut cpu = Cpu::new(ROM_START);
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers()[0], ROM_START + 12);
}

#[test]
fn branches_use_signed_displacement_and_pipeline_offset() {
    let mut memory = program(&[
        0xea00_0000, // B +8
        0xe3a0_0063, // skipped
        0xeaff_fffc, // B -8, back to start
    ]);
    let mut cpu = Cpu::new(ROM_START);
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.pc(), ROM_START + 8);
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.pc(), ROM_START);
    assert_eq!(cpu.registers()[0], 0);
}

#[test]
fn unsupported_instructions_do_not_change_cpu() {
    for instruction in [
        0xf3a0_0001, // reserved condition
        0xe340_0001, // CMP encoding with S=0
        0xe3b0_f001, // MOVS pc, #1 (status restore unsupported)
        0xe10f_f000, // MRS pc, CPSR
        0xe8f0_0001, // LDM user-bank transfer with writeback
        0xe100_f090, // SWP with PC destination
        0xe14f_0000, // MRS SPSR in System mode
        0xe16f_0f10, // CLZ (ARMv5)
        0xe12f_ff30, // BLX (ARMv5)
    ] {
        let mut memory = program(&[instruction]);
        let mut cpu = Cpu::new(ROM_START);
        let before = *cpu.registers();
        let flags_before = cpu.flags();
        assert_eq!(
            cpu.step(&mut memory),
            Err(CpuError::UnsupportedInstruction {
                address: ROM_START,
                instruction
            }),
        );
        assert_eq!(cpu.registers(), &before);
        assert_eq!(cpu.flags(), flags_before);
    }
}

#[test]
fn fetch_errors_do_not_advance_pc() {
    let mut memory = Memory::new(vec![]).unwrap();
    let mut cpu = Cpu::new(ROM_START);
    assert_eq!(
        cpu.step(&mut memory),
        Err(CpuError::Memory(MemoryError::Unmapped(ROM_START)))
    );
    assert_eq!(cpu.pc(), ROM_START);
}
