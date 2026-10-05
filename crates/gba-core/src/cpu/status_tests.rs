use super::*;
use crate::memory::ROM_START;

const MODES: [Mode; 7] = [
    Mode::User,
    Mode::Fiq,
    Mode::Irq,
    Mode::Supervisor,
    Mode::Abort,
    Mode::Undefined,
    Mode::System,
];

fn program(instruction: u32) -> Memory {
    Memory::new(instruction.to_le_bytes().to_vec()).unwrap()
}

#[test]
fn direct_start_and_reset_have_explicit_status() {
    let cpu = Cpu::new(ROM_START);
    assert_eq!(cpu.mode(), Mode::System);
    assert_eq!(cpu.cpsr(), 0x1f);
    assert_eq!(cpu.spsr(), None);
    let cpu = Cpu::at_reset();
    assert_eq!(cpu.pc(), 0);
    assert_eq!(cpu.cpsr(), 0xd3);
    assert_eq!(cpu.spsr(), Some(0));
}

#[test]
fn every_mode_has_the_correct_shared_and_private_registers() {
    let mut cpu = Cpu::new(ROM_START);
    for register in 0..15 {
        cpu.registers[register] = register as u32 + 100;
    }
    // Initialize all five exception banks independently.
    for (index, mode) in MODES[1..6].iter().copied().enumerate() {
        cpu.switch_mode(mode);
        assert_eq!(
            cpu.registers[0..8],
            [100, 101, 102, 103, 104, 105, 106, 107]
        );
        if mode == Mode::Fiq {
            assert_eq!(cpu.registers[8..13], [0; 5]);
            cpu.registers[8..13].copy_from_slice(&[208, 209, 210, 211, 212]);
        } else {
            assert_eq!(cpu.registers[8..13], [108, 109, 110, 111, 112]);
        }
        assert_eq!(cpu.registers[13..15], [0, 0]);
        cpu.registers[13] = 300 + index as u32;
        cpu.registers[14] = 400 + index as u32;
        cpu.set_spsr(500 + index as u32);
    }
    for (index, mode) in MODES[1..6].iter().copied().enumerate() {
        cpu.switch_mode(mode);
        assert_eq!(
            cpu.registers[13..15],
            [300 + index as u32, 400 + index as u32]
        );
        assert_eq!(cpu.spsr(), Some(500 + index as u32));
    }
    cpu.switch_mode(Mode::User);
    assert_eq!(cpu.registers[8..15], [108, 109, 110, 111, 112, 113, 114]);
    cpu.registers[13] = 999;
    cpu.switch_mode(Mode::System);
    assert_eq!(cpu.registers[13], 999);
    assert_eq!(cpu.spsr(), None);
    cpu.switch_mode(Mode::Fiq);
    assert_eq!(cpu.registers[8..13], [208, 209, 210, 211, 212]);
}

#[test]
fn cpsr_round_trips_all_modes_flags_masks_and_states() {
    let mut cpu = Cpu::new(ROM_START);
    for mode in MODES {
        for flags in 0..16 {
            for control in 0..8 {
                let value = (flags << 28) | (control << 5) | mode as u32;
                cpu.apply_status(value, mode);
                assert_eq!(cpu.cpsr(), value);
                assert_eq!(cpu.mode(), mode);
            }
        }
    }
}

#[test]
fn mrs_reads_current_and_saved_status() {
    let mut cpu = Cpu::new(ROM_START);
    cpu.apply_status(0xa000_00d3, Mode::Supervisor);
    cpu.set_spsr(0x5000_0030);
    let previous = cpu.cpsr();
    cpu.step(&mut program(0xe10f_0000)).unwrap(); // MRS r0, CPSR
    assert_eq!(cpu.registers[0], previous);
    assert_eq!(cpu.cpsr(), previous);
    cpu.registers[15] = ROM_START;
    cpu.step(&mut program(0xe14f_1000)).unwrap(); // MRS r1, SPSR
    assert_eq!(cpu.registers[1], 0x5000_0030);
    assert_eq!(cpu.pc(), ROM_START + 4);
}

#[test]
fn msr_register_and_immediate_write_selected_fields_only() {
    for fields in 0..16 {
        let mut cpu = Cpu::new(ROM_START);
        cpu.registers[0] = 0xffff_ffd3; // reserved bits are ignored
        cpu.step(&mut program(0xe120_f000 | (fields << 16)))
            .unwrap();
        let expected = if fields & 1 != 0 { 0xd3 } else { 0x1f }
            | if fields & 8 != 0 { 0xf000_0000 } else { 0 };
        assert_eq!(cpu.cpsr(), expected);
    }
    let mut cpu = Cpu::new(ROM_START);
    cpu.step(&mut program(0xe328_f480)).unwrap(); // MSR CPSR_f, #0x80000000
    assert_eq!(cpu.cpsr(), 0x8000_001f);
    cpu.registers[15] = ROM_START;
    cpu.step(&mut program(0xe321_f0d3)).unwrap(); // MSR CPSR_c, #0xd3
    assert_eq!(cpu.cpsr(), 0x8000_00d3);
}

#[test]
fn msr_reads_old_banked_operand_before_switching_mode() {
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[13] = 0xd3;
    cpu.step(&mut program(0xe121_f00d)).unwrap(); // MSR CPSR_c, sp
    assert_eq!(cpu.mode(), Mode::Supervisor);
    assert_eq!(cpu.registers[13], 0);
    cpu.switch_mode(Mode::System);
    assert_eq!(cpu.registers[13], 0xd3);
}

#[test]
fn user_msr_changes_flags_but_cannot_change_control() {
    let mut cpu = Cpu::new(ROM_START);
    cpu.switch_mode(Mode::User);
    cpu.registers[0] = 0xf000_00ff; // Includes T, invalid mode, and interrupt masks
    cpu.step(&mut program(0xe12f_f000)).unwrap();
    assert_eq!(cpu.cpsr(), 0xf000_0010);
}

#[test]
fn msr_spsr_accepts_thumb_and_preserves_unselected_fields() {
    for fields in 0..16 {
        let mut cpu = Cpu::new(ROM_START);
        cpu.switch_mode(Mode::Supervisor);
        cpu.set_spsr(0x2000_001f);
        cpu.registers[0] = 0x9fff_fff0;
        cpu.step(&mut program(0xe160_f000 | (fields << 16)))
            .unwrap();
        assert_eq!(cpu.cpsr(), 0x13);
        let expected = if fields & 1 != 0 { 0xf0 } else { 0x1f }
            | if fields & 8 != 0 {
                0x9000_0000
            } else {
                0x2000_0000
            };
        assert_eq!(cpu.spsr(), Some(expected));
    }
    let mut cpu = Cpu::at_reset();
    cpu.registers[15] = ROM_START;
    cpu.step(&mut program(0xe361_f03f)).unwrap(); // MSR SPSR_c, #0x3f
    assert_eq!(cpu.spsr(), Some(0x3f));
}

#[test]
fn invalid_status_transfers_preserve_all_banks() {
    for instruction in [
        0xe10f_f000, // MRS pc, CPSR
        0xe14f_0000, // MRS r0, SPSR without an SPSR
        0xe161_f000, // MSR SPSR_c, r0 without an SPSR
        0xe121_f00f, // MSR CPSR_c, pc
        0xe321_f000, // Invalid mode
        0xe321_f03f, // Attempt to change T
    ] {
        let mut cpu = Cpu::new(ROM_START);
        let before = cpu.clone();
        assert!(
            cpu.step(&mut program(instruction)).is_err(),
            "{instruction:08x}"
        );
        assert_eq!(cpu, before);
    }
}

#[test]
fn conditional_status_transfers_do_not_execute_when_condition_fails() {
    let mut cpu = Cpu::new(ROM_START);
    for instruction in [0x010f_0000, 0x0121_f000, 0x0161_f000] {
        cpu.registers[15] = ROM_START;
        let mut expected = cpu.clone();
        expected.registers[15] += 4;
        cpu.step(&mut program(instruction)).unwrap();
        assert_cpu_arch_eq!(cpu, expected);
    }
}
