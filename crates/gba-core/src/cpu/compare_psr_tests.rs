//! Original regressions for ARM test/compare encodings with the unused Rd=15.
use super::*;
use crate::{
    memory::ROM_START,
    timing::{bus_cycles, AccessKind, AccessWidth},
};

const EXCEPTIONS: [Mode; 5] = [
    Mode::Fiq,
    Mode::Irq,
    Mode::Supervisor,
    Mode::Abort,
    Mode::Undefined,
];
fn instruction(opcode: u32, operand: u32) -> u32 {
    0xe010_f000 | (opcode << 21) | operand // AL, S=1, Rn=r0, unused Rd=15
}
fn memory(word: u32) -> Memory {
    Memory::new(word.to_le_bytes().to_vec()).unwrap()
}

#[test]
fn compare_psr_restores_saved_flags_masks_and_banks_without_branching() {
    for opcode in 8..=11 {
        for source in EXCEPTIONS {
            for flags in 0..16 {
                for masks in [0, 0x40, 0x80, 0xc0] {
                    let mut cpu = Cpu::new(ROM_START);
                    cpu.registers[8] = 0x55;
                    cpu.registers[13] = 0x0300_1000;
                    cpu.switch_mode(source);
                    cpu.registers[13] = 0x0300_7000;
                    if source == Mode::Fiq {
                        cpu.registers[8] = 0xaa;
                    }
                    cpu.registers[0] = 0x8000_0000;
                    cpu.registers[1] = 1;
                    let saved = flags << 28 | masks | Mode::System as u32;
                    cpu.set_spsr(saved);
                    cpu.step(&mut memory(instruction(opcode, 1))).unwrap();
                    assert_eq!(cpu.cpsr(), saved, "opcode={opcode}, source={source:?}");
                    assert_eq!(cpu.pc(), ROM_START + 4);
                    assert_eq!(cpu.registers[0], 0x8000_0000);
                    assert_eq!(cpu.registers[1], 1);
                    assert_eq!(cpu.registers[8], 0x55);
                    assert_eq!(cpu.registers[13], 0x0300_1000);
                    cpu.switch_mode(source);
                    assert_eq!(cpu.spsr(), Some(saved));
                    assert_eq!(cpu.registers[13], 0x0300_7000);
                    if source == Mode::Fiq {
                        assert_eq!(cpu.registers[8], 0xaa);
                    }
                }
            }
        }
    }
}

#[test]
fn compare_psr_in_user_and_system_modes_uses_ordinary_alu_flags() {
    for mode in [Mode::User, Mode::System] {
        for opcode in 8..=11 {
            for operand in [1, 0x0200_0081, 0x0000_0211] {
                // Rm, immediate, register shift
                let mut cpu = Cpu::new(ROM_START);
                cpu.apply_status(0x9000_00c0 | mode as u32, mode);
                cpu.registers[0] = 0x8000_0000;
                cpu.registers[1] = 7;
                cpu.registers[2] = 3;
                let mut ordinary = cpu.clone();
                let word = instruction(opcode, operand);
                cpu.step(&mut memory(word)).unwrap();
                ordinary.step(&mut memory(word & !0xf000)).unwrap();
                assert_eq!(cpu, ordinary);
                assert_eq!(cpu.mode(), mode);
                assert_eq!(cpu.spsr(), None);
                assert_eq!(cpu.pc(), ROM_START + 4);
            }
        }
    }
}

#[test]
fn compare_psr_has_sequential_timing_for_each_operand_form() {
    for opcode in 8..=11 {
        for (operand, internal) in [(1, 0), (0x0200_0001, 0), (0x0000_0211, 1)] {
            let mut cpu = Cpu::new(ROM_START);
            cpu.switch_mode(Mode::Irq);
            cpu.set_spsr(Mode::System as u32);
            let timing = cpu
                .step_timed(&mut memory(instruction(opcode, operand)))
                .unwrap();
            assert_eq!(
                timing.code_cycles,
                bus_cycles(0, ROM_START + 8, AccessWidth::Word, AccessKind::Sequential)
            );
            assert_eq!(timing.internal_cycles, internal);
            assert_eq!(timing.data_cycles, 0);
            assert_eq!(cpu.pc(), ROM_START + 4);
        }
    }
}

#[test]
fn compare_psr_invalid_saved_mode_and_bad_shift_preserve_all_cpu_state() {
    for opcode in 8..=11 {
        for source in EXCEPTIONS {
            for (saved, operand) in [(0, 1), (0x1f, 0x0f11)] {
                // invalid SPSR; forbidden Rs=pc
                let mut cpu = Cpu::new(ROM_START);
                cpu.switch_mode(source);
                cpu.set_spsr(saved);
                let before = cpu.clone();
                assert!(cpu
                    .step_timed(&mut memory(instruction(opcode, operand)))
                    .is_err());
                assert_eq!(cpu, before);
            }
        }
    }
}

#[test]
fn failed_conditions_do_not_restore_status_or_validate_spsr() {
    for opcode in 8..=11 {
        let mut cpu = Cpu::new(ROM_START);
        cpu.switch_mode(Mode::Fiq); // Z clear; SPSR mode invalid
        let mut expected = cpu.clone();
        expected.registers[15] += 4;
        cpu.step(&mut memory(instruction(opcode, 1) & 0x0fff_ffff))
            .unwrap(); // EQ
        assert_cpu_arch_eq!(cpu, expected);
    }
}

#[test]
fn ordinary_pc_result_returns_still_require_an_spsr() {
    for mode in [Mode::User, Mode::System] {
        let mut cpu = Cpu::new(ROM_START);
        cpu.switch_mode(mode);
        let before = cpu.clone();
        assert!(cpu.step(&mut memory(0xe1b0_f00e)).is_err()); // MOVS pc,lr
        assert_eq!(cpu, before);
    }
}
