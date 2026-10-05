use super::*;
use crate::memory::{BIOS_SIZE, ROM_START};

fn program(instruction: u32) -> Memory {
    Memory::new(instruction.to_le_bytes().to_vec()).unwrap()
}

fn vectors(instruction: u32) -> Memory {
    let bios = instruction.to_le_bytes().repeat(BIOS_SIZE / 4);
    Memory::with_bios(vec![], bios).unwrap()
}

#[test]
fn exception_entry_sets_vector_link_masks_and_saved_status() {
    for (exception, mode, vector, arm_offset, thumb_offset) in [
        (Exception::UndefinedInstruction, Mode::Undefined, 4, 4, 2),
        (Exception::SoftwareInterrupt, Mode::Supervisor, 8, 4, 2),
        (Exception::PrefetchAbort, Mode::Abort, 12, 4, 4),
        (Exception::DataAbort, Mode::Abort, 16, 8, 8),
        (Exception::Irq, Mode::Irq, 24, 4, 4),
        (Exception::Fiq, Mode::Fiq, 28, 4, 4),
    ] {
        for thumb in [false, true] {
            for fiq_mask in [0, 0x40] {
                let mut cpu = Cpu::new(ROM_START);
                let status = 0xb000_001f | if thumb { 0x20 } else { 0 } | fiq_mask;
                cpu.apply_status(status, Mode::System);
                cpu.registers[13] = 0x0300_0100;
                cpu.registers[14] = 123;
                cpu.enter_exception(exception);
                assert_eq!(cpu.mode(), mode);
                assert_eq!(cpu.pc(), vector);
                assert_eq!(
                    cpu.registers[14],
                    ROM_START + if thumb { thumb_offset } else { arm_offset }
                );
                assert_eq!(cpu.registers[13], 0);
                assert_eq!(cpu.spsr(), Some(status));
                assert_eq!(
                    cpu.cpsr(),
                    0xb000_0080
                        | mode as u32
                        | if exception == Exception::Fiq {
                            0x40
                        } else {
                            fiq_mask
                        }
                );
                cpu.switch_mode(Mode::System);
                assert_eq!(cpu.registers[13..15], [0x0300_0100, 123]);
            }
        }
    }
}

#[test]
fn arm_and_thumb_swi_execute_and_movs_pc_restores_caller() {
    for thumb in [false, true] {
        for comment in [0, 1, 255] {
            let rom = if thumb {
                (0xdf00_u16 | comment).to_le_bytes().to_vec()
            } else {
                (0xefab_0000_u32 | u32::from(comment))
                    .to_le_bytes()
                    .to_vec()
            };
            let mut memory =
                Memory::with_bios(rom, 0xe1b0_f00e_u32.to_le_bytes().repeat(BIOS_SIZE / 4))
                    .unwrap();
            let mut cpu = Cpu::new(ROM_START);
            let status = 0x6000_001f | if thumb { 0x20 } else { 0 };
            cpu.apply_status(status, Mode::System);
            cpu.registers[14] = 0x1234;
            cpu.step(&mut memory).unwrap();
            assert_eq!(cpu.pc(), 8);
            assert_eq!(cpu.mode(), Mode::Supervisor);
            assert_eq!(cpu.instruction_set(), InstructionSet::Arm);
            // MOVS pc, lr restores the saved flags, not ALU-result flags.
            cpu.step(&mut memory).unwrap();
            assert_eq!(cpu.pc(), ROM_START + if thumb { 2 } else { 4 });
            assert_eq!(cpu.cpsr(), status);
            assert_eq!(cpu.registers[14], 0x1234);
        }
    }
}

#[test]
fn conditional_swi_can_be_skipped_without_entering_supervisor() {
    let mut cpu = Cpu::new(ROM_START);
    let mut expected = cpu.clone();
    expected.registers[15] += 4;
    cpu.step(&mut program(0x0fff_ffff)).unwrap();
    assert_eq!(cpu, expected);
}

#[test]
fn swi_without_vector_image_enters_then_reports_fetch_error() {
    let mut cpu = Cpu::new(ROM_START);
    let mut memory = program(0xef00_0000);
    cpu.step(&mut memory).unwrap();
    let before = cpu.clone();
    assert_eq!(
        cpu.step(&mut memory),
        Err(CpuError::Memory(MemoryError::Unmapped(8)))
    );
    assert_eq!(cpu, before);
}

#[test]
fn interrupt_masks_and_fiq_priority_are_respected() {
    for irq_mask in [false, true] {
        for fiq_mask in [false, true] {
            for irq in [false, true] {
                for fiq in [false, true] {
                    let mut cpu = Cpu::new(ROM_START);
                    cpu.irq_disabled = irq_mask;
                    cpu.fiq_disabled = fiq_mask;
                    let before = cpu.clone();
                    let expected = if fiq && !fiq_mask {
                        Some(Mode::Fiq)
                    } else if irq && !irq_mask {
                        Some(Mode::Irq)
                    } else {
                        None
                    };
                    assert_eq!(cpu.take_interrupt(irq, fiq), expected.is_some());
                    if let Some(mode) = expected {
                        assert_eq!(cpu.mode(), mode);
                        assert_eq!(cpu.spsr(), Some(before.cpsr()));
                    } else {
                        assert_eq!(cpu, before);
                    }
                }
            }
        }
    }
}

#[test]
fn subs_exception_returns_resume_or_retry_the_correct_instruction() {
    for (exception, subtract) in [
        (Exception::Irq, 4),
        (Exception::Fiq, 4),
        (Exception::PrefetchAbort, 4),
        (Exception::DataAbort, 8),
    ] {
        for thumb in [false, true] {
            let mut cpu = Cpu::new(ROM_START + 2 * u32::from(thumb));
            let status = 0xa000_001f | if thumb { 0x20 } else { 0 };
            cpu.apply_status(status, Mode::System);
            let before = cpu.clone();
            cpu.enter_exception(exception);
            cpu.step(&mut vectors(0xe25e_f000 | subtract)).unwrap(); // SUBS pc, lr, #offset
            assert_eq!(cpu.registers, before.registers);
            assert_eq!(cpu.cpsr(), before.cpsr());
        }
    }
}

#[test]
fn nested_fiq_preserves_irq_saved_status_and_banked_registers() {
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[8] = 88;
    cpu.enter_exception(Exception::Irq);
    cpu.registers[13] = 0x0300_0200;
    let irq_status = cpu.cpsr();
    let irq_spsr = cpu.spsr();
    let irq_link = cpu.registers[14];
    assert!(cpu.take_interrupt(false, true));
    cpu.registers[8] = 888;
    cpu.step(&mut vectors(0xe25e_f004)).unwrap();
    assert_eq!(cpu.pc(), 0x18);
    assert_eq!(cpu.cpsr(), irq_status);
    assert_eq!(cpu.spsr(), irq_spsr);
    assert_eq!(cpu.registers[13..15], [0x0300_0200, irq_link]);
    assert_eq!(cpu.registers[8], 88);
    cpu.step(&mut vectors(0xe25e_f004)).unwrap();
    assert_eq!(cpu.pc(), ROM_START);
    assert_eq!(cpu.cpsr(), 0x1f);
    cpu.switch_mode(Mode::Fiq);
    assert_eq!(cpu.registers[8], 888);
}

#[test]
fn same_mode_exception_overwrites_link_and_saved_status() {
    let mut cpu = Cpu::new(ROM_START);
    cpu.enter_exception(Exception::SoftwareInterrupt);
    cpu.registers[15] = 0x100;
    let previous = cpu.cpsr();
    cpu.enter_exception(Exception::SoftwareInterrupt);
    assert_eq!(cpu.spsr(), Some(previous));
    assert_eq!(cpu.registers[14], 0x104);
}

#[test]
fn return_alignment_uses_saved_state_not_target_bit_zero() {
    for (status, expected) in [(0x1f, ROM_START), (0x3f, ROM_START + 2)] {
        let mut cpu = Cpu::at_reset();
        cpu.registers[15] = ROM_START;
        cpu.registers[14] = ROM_START + 3;
        cpu.set_spsr(status);
        cpu.step(&mut program(0xe1b0_f00e)).unwrap();
        assert_eq!(cpu.pc(), expected);
        assert_eq!(cpu.cpsr(), status);
    }
}

#[test]
fn invalid_saved_mode_rejects_return_without_partial_changes() {
    for instruction in [0xe1b0_f00e, 0xe8fd_8001] {
        let mut cpu = Cpu::at_reset();
        cpu.registers[15] = ROM_START;
        cpu.registers[13] = 0x0200_0000;
        cpu.set_spsr(0); // An invalid mode
        let before = cpu.clone();
        assert!(cpu.step(&mut program(instruction)).is_err());
        assert_eq!(cpu, before);
    }
}

#[test]
fn ldm_exception_return_loads_outgoing_bank_before_restoring_status() {
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[13] = 0x0300_0100;
    cpu.registers[14] = 123;
    cpu.enter_exception(Exception::SoftwareInterrupt);
    cpu.registers[15] = ROM_START;
    cpu.registers[13] = 0x0200_0000;
    cpu.set_spsr(0x9000_003f);
    let mut memory = program(0xe8fd_c001); // LDMIA sp!, {r0, lr, pc}^
    for (index, value) in [42, 456, ROM_START + 7].into_iter().enumerate() {
        memory
            .write32(0x0200_0000 + index as u32 * 4, value)
            .unwrap();
    }
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.cpsr(), 0x9000_003f);
    assert_eq!(cpu.pc(), ROM_START + 6);
    assert_eq!(cpu.registers[0], 42);
    assert_eq!(cpu.registers[13..15], [0x0300_0100, 123]);
    cpu.switch_mode(Mode::Supervisor);
    assert_eq!(cpu.registers[13..15], [0x0200_000c, 456]);
}

#[test]
fn failed_ldm_return_preserves_every_bank_and_status() {
    let mut cpu = Cpu::at_reset();
    cpu.registers[15] = ROM_START;
    cpu.registers[13] = 0x0400_0054;
    cpu.set_spsr(0x3f);
    let mut memory = program(0xe8fd_8001);
    memory.write32(0x0400_0054, 42).unwrap();
    let before = cpu.clone();
    assert!(matches!(cpu.step(&mut memory), Err(CpuError::Memory(_))));
    assert_eq!(cpu, before);
}

#[test]
fn user_bank_transfers_use_current_base_and_user_values() {
    for mode in [Mode::System, Mode::Supervisor, Mode::Fiq] {
        let mut cpu = Cpu::new(ROM_START);
        for register in 0..15 {
            cpu.registers[register] = 100 + register as u32;
        }
        cpu.switch_mode(mode);
        cpu.registers[13] = 0x0200_0100;
        let original = cpu.clone();
        let mut memory = program(0xe8cd_7fff); // STMIA sp, {r0-r14}^
        cpu.step(&mut memory).unwrap();
        for register in 0..15 {
            assert_eq!(
                memory.read32(0x0200_0100 + register as u32 * 4).unwrap(),
                original.user_register(register)
            );
            memory
                .write32(0x0200_0100 + register as u32 * 4, 200 + register as u32)
                .unwrap();
        }
        // Use RAM for the next instruction without changing the transfer base.
        memory.write32(0x0300_0000, 0xe8dd_7fff).unwrap(); // LDMIA sp, {r0-r14}^
        cpu.registers[15] = 0x0300_0000;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.cpsr(), original.cpsr());
        if mode != Mode::System {
            assert_eq!(cpu.registers[13..15], original.registers[13..15]);
        }
        if mode == Mode::Fiq {
            assert_eq!(cpu.registers[8..13], original.registers[8..13]);
        }
        cpu.switch_mode(Mode::System);
        for register in 0..15 {
            assert_eq!(cpu.registers[register], 200 + register as u32);
        }
    }
}

#[test]
fn user_bank_store_pc_uses_pipeline_offset() {
    let mut cpu = Cpu::at_reset();
    cpu.registers[15] = ROM_START;
    cpu.registers[0] = 0x0200_0000;
    let mut memory = program(0xe8c0_8000); // STMIA r0, {pc}^
    cpu.step(&mut memory).unwrap();
    assert_eq!(memory.read32(0x0200_0000).unwrap(), ROM_START + 12);
}

#[test]
fn user_mode_cannot_request_user_bank_transfers_or_status_return() {
    for instruction in [0xe8c0_0002, 0xe8d0_0002, 0xe8d0_8000, 0xe1b0_f00e] {
        let mut cpu = Cpu::new(ROM_START);
        cpu.switch_mode(Mode::User);
        cpu.registers[0] = 0x0200_0000;
        let before = cpu.clone();
        assert!(cpu.step(&mut program(instruction)).is_err());
        assert_eq!(cpu, before);
    }
}

#[test]
fn failed_user_bank_load_preserves_hidden_registers() {
    let mut cpu = Cpu::at_reset();
    cpu.registers[15] = ROM_START;
    cpu.registers[0] = 0x0400_0054;
    let before = cpu.clone();
    assert!(cpu.step(&mut program(0xe8d0_6000)).is_err()); // LDMIA r0, {sp, lr}^
    assert_eq!(cpu, before);
}
