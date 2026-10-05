//! Original regressions for the stored PC in Thumb empty-register-list transfers.
use super::*;
use crate::{
    io::TIMER_BASE,
    machine::{Machine, MachineError, StepKind},
    memory::ROM_START,
    timing::StepTiming,
};

const CODE: u32 = 0x0300_0200;
const DATA: u32 = 0x0200_1000;

fn setup(instruction: u16, pc: u32) -> (Cpu, Memory) {
    let mut memory = Memory::new(vec![0; 4]).unwrap();
    memory.write16(pc, instruction).unwrap();
    let mut cpu = Cpu::new(pc);
    cpu.instruction_set = InstructionSet::Thumb;
    (cpu, memory)
}

#[test]
fn empty_thumb_stores_use_pc_plus_six_for_all_bases_banks_and_alignments() {
    for base_register in (0..8).chain(std::iter::once(13)) {
        let push = base_register == 13;
        let instruction = if push {
            0xb400
        } else {
            0xc000 | (base_register as u16) << 8
        };
        for mode in [
            Mode::User,
            Mode::System,
            Mode::Fiq,
            Mode::Irq,
            Mode::Supervisor,
            Mode::Abort,
            Mode::Undefined,
        ] {
            for code_low in [0, 2] {
                for data_low in 0..4 {
                    for timed in [false, true] {
                        let pc = CODE + code_low;
                        let (mut cpu, mut memory) = setup(instruction, pc);
                        cpu.apply_status(0xb000_00e0 | mode as u32, mode);
                        for reg in 0..15 {
                            cpu.registers[reg] = 0x1000 + reg as u32;
                        }
                        let base = DATA + data_low + if push { 64 } else { 0 };
                        cpu.registers[base_register] = base;
                        memory.write32(DATA - 4, 0x1122_3344).unwrap();
                        memory.write32(DATA + 4, 0x5566_7788).unwrap();
                        memory.write32(DATA + 60, 0x99aa_bbcc).unwrap();
                        let mut after = cpu.clone();
                        after.registers[base_register] = if push { base - 64 } else { base + 64 };
                        after.registers[15] = pc + 2;
                        if timed {
                            assert_eq!(
                                cpu.step_timed(&mut memory).unwrap(),
                                StepTiming {
                                    code_cycles: 1,
                                    data_cycles: 6,
                                    internal_cycles: 0,
                                    idle_cycles: 0,
                                }
                            );
                        } else {
                            cpu.step(&mut memory).unwrap();
                        }
                        assert_eq!(
                            cpu, after,
                            "{instruction:#06x} {mode:?} code={code_low} data={data_low}"
                        );
                        assert_eq!(memory.read32(DATA).unwrap(), pc + 6);
                        assert_eq!(memory.read32(DATA - 4).unwrap(), 0x1122_3344);
                        assert_eq!(memory.read32(DATA + 4).unwrap(), 0x5566_7788);
                        assert_eq!(memory.read32(DATA + 60).unwrap(), 0x99aa_bbcc);
                        assert_eq!(memory.cycles(), 0);
                    }
                }
            }
        }
    }
}

#[test]
fn empty_store_pc_matches_the_following_thumb_pc_operand() {
    let (mut cpu, mut memory) = setup(0xc300, CODE); // STMIA r3!, {}
    memory.write16(CODE + 2, 0x467c).unwrap(); // MOV r4, pc
    memory.write16(CODE + 4, 0x6835).unwrap(); // LDR r5, [r6]
    memory.write16(CODE + 6, 0x42ac).unwrap(); // CMP r4, r5
    cpu.registers[3] = DATA;
    cpu.registers[6] = DATA;
    for _ in 0..4 {
        cpu.step(&mut memory).unwrap();
    }
    assert_eq!(cpu.registers[3], DATA + 64);
    assert_eq!(cpu.registers[4], CODE + 6);
    assert_eq!(cpu.registers[5], CODE + 6);
    assert!(cpu.flags.zero && cpu.flags.carry);
    assert_eq!(cpu.pc(), CODE + 8);
}

#[test]
fn empty_store_uses_the_executing_rom_address_without_word_alignment() {
    for offset in [0, 2] {
        let mut rom = vec![0; 4];
        rom[offset..offset + 2].copy_from_slice(&0xc700u16.to_le_bytes());
        for window in [ROM_START, 0x0a00_0000, 0x0c00_0000] {
            let mut memory = Memory::new(rom.clone()).unwrap();
            let mut cpu = Cpu::new(window + offset as u32);
            cpu.instruction_set = InstructionSet::Thumb;
            cpu.registers[7] = DATA;
            cpu.step(&mut memory).unwrap();
            assert_eq!(memory.read32(DATA).unwrap(), window + offset as u32 + 6);
        }
    }
}

#[test]
fn empty_store_does_not_validate_the_writeback_address() {
    let (mut cpu, mut memory) = setup(0xc000, CODE);
    cpu.registers[0] = 0x03ff_ffff; // Last mirrored IWRAM word; writeback crosses into I/O.
    cpu.step(&mut memory).unwrap();
    assert_eq!(memory.read32(0x03ff_fffc).unwrap(), CODE + 6);
    assert_eq!(cpu.registers[0], 0x0400_003f);
    assert_eq!(cpu.pc(), CODE + 2);
}

#[test]
fn empty_stores_advance_devices_only_after_a_successful_single_word_write() {
    let (mut cpu, mut memory) = setup(0xc200, CODE);
    cpu.registers[2] = DATA;
    memory.write32(TIMER_BASE, 0x0080_0000).unwrap();
    let mut machine = Machine::new(cpu, memory);
    assert_eq!(machine.step().unwrap(), StepKind::Instruction);
    assert_eq!(machine.cycles(), 7); // One IWRAM code access and one EWRAM word store.
    assert_eq!(machine.memory().read16(TIMER_BASE).unwrap(), 7);
    assert_eq!(machine.memory().read32(DATA).unwrap(), CODE + 6);
    assert_eq!(machine.cpu().registers[2], DATA + 64);

    for instruction in [0xc200, 0xb400] {
        for (address, error) in [
            (ROM_START, MemoryError::ReadOnly(ROM_START)),
            (0x0e00_0000, MemoryError::Unmapped(0x0e00_0000)),
        ] {
            let (mut cpu, mut memory) = setup(instruction, CODE);
            let register = if instruction == 0xb400 { 13 } else { 2 };
            cpu.registers[register] = address + if register == 13 { 64 } else { 0 };
            memory.write32(TIMER_BASE, 0x0080_0000).unwrap();
            let before = cpu.clone();
            let mut machine = Machine::new(cpu, memory);
            assert_eq!(
                machine.step(),
                Err(MachineError::Cpu(CpuError::Memory(error)))
            );
            assert_eq!(machine.cpu(), &before);
            assert_eq!(machine.cycles(), 0);
            assert_eq!(machine.last_timing(), StepTiming::default());
            assert_eq!(machine.memory().read16(TIMER_BASE).unwrap(), 0);
        }
    }
}

#[test]
fn push_lr_is_not_an_empty_list_and_keeps_the_original_link_value() {
    let (mut cpu, mut memory) = setup(0xb500, CODE); // PUSH {lr}
    cpu.registers[13] = DATA + 4;
    cpu.registers[14] = 0x1234_5679;
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[13], DATA);
    assert_eq!(memory.read32(DATA).unwrap(), 0x1234_5679);
    assert_eq!(cpu.pc(), CODE + 2);
}
