//! Original ARM instruction-buffer tests. Bus timing remains nominal.
use super::*;
use crate::{
    dma::{DMA_BASE, DMA_STRIDE},
    io::{IE, IME, TIMER_BASE},
    machine::{Machine, StepKind},
    memory::{BIOS_SIZE, ROM_START},
};

const CODE: u32 = 0x0300_0100;
const TARGET: u32 = 0x0300_0200;
const DATA: u32 = 0x0200_1000;
const NOP: u32 = 0xe1a0_0000;
const OLD: u32 = 0xe3a0_0001; // MOV r0,#1.
const NEW: u32 = 0xe3a0_0002; // MOV r0,#2.

fn program(code: &[u32]) -> (Cpu, Memory) {
    let mut bus = Memory::new(vec![]).unwrap();
    for (index, word) in code.iter().enumerate() {
        bus.write32(CODE + index as u32 * 4, *word).unwrap();
    }
    (Cpu::new(CODE), bus)
}

#[test]
fn cpu_stores_do_not_replace_the_two_words_fetched_before_execution() {
    for offset in [4, 8, 12] {
        let (mut cpu, mut bus) = program(&[0xe581_2000, OLD, OLD, OLD]); // STR r2,[r1].
        cpu.registers[1] = CODE + offset;
        cpu.registers[2] = NEW;
        cpu.step(&mut bus).unwrap();
        assert_eq!(bus.read32(CODE + offset).unwrap(), NEW);
        for _ in 0..offset / 4 {
            cpu.step(&mut bus).unwrap();
        }
        assert_eq!(cpu.registers[0], if offset <= 8 { 1 } else { 2 });
    }
}

#[test]
fn host_writes_leave_buffered_words_but_future_fetches_observe_changes() {
    let (mut cpu, mut bus) = program(&[NOP, OLD, OLD, OLD]);
    cpu.step(&mut bus).unwrap();
    for offset in [4, 8, 12] {
        bus.write32(CODE + offset, NEW).unwrap();
    }
    assert_eq!(bus.read32(CODE + 4).unwrap(), NEW);
    for expected in [1, 1, 2] {
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.registers[0], expected);
    }
}

#[test]
fn branch_to_fallthrough_refills_even_when_the_pc_value_is_sequential() {
    for instruction in [0xeaff_ffff, 0xe1a0_f001, 0xe12f_ff11, 0xe591_f000] {
        // B P+4, MOV pc,r1, BX r1, LDR pc,[r1].
        let (mut cpu, mut bus) = program(&[NOP, instruction, OLD, OLD]);
        cpu.registers[1] = if instruction == 0xe591_f000 {
            DATA
        } else {
            CODE + 8
        };
        bus.write32(DATA, CODE + 8).unwrap();
        cpu.step(&mut bus).unwrap();
        bus.write32(CODE + 8, NEW).unwrap(); // Was fetched by NOP.
        cpu.step(&mut bus).unwrap();
        bus.write32(CODE + 8, OLD).unwrap(); // Was refetched by the branch.
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.registers[0], 2);
    }
}

#[test]
fn untaken_branch_keeps_buffered_instructions() {
    let (mut cpu, mut bus) = program(&[NOP, 0x0aff_ffff, OLD]); // BEQ, Z is clear.
    cpu.step(&mut bus).unwrap();
    bus.write32(CODE + 8, NEW).unwrap();
    cpu.step(&mut bus).unwrap();
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.registers[0], 1);
}

#[test]
fn refill_captures_target_pair_but_not_target_plus_eight() {
    for thumb_source in [false, true] {
        let (mut cpu, mut bus) = program(&[if thumb_source { 0x4708 } else { 0xe12f_ff11 }]);
        if thumb_source {
            cpu.instruction_set = InstructionSet::Thumb;
        }
        cpu.registers[1] = TARGET;
        for offset in [0, 4, 8] {
            bus.write32(TARGET + offset, OLD).unwrap();
        }
        cpu.step(&mut bus).unwrap();
        for offset in [0, 4, 8] {
            bus.write32(TARGET + offset, NEW).unwrap();
        }
        for expected in [1, 1, 2] {
            cpu.step(&mut bus).unwrap();
            assert_eq!(cpu.registers[0], expected);
        }
    }
}

#[test]
fn dma_writes_do_not_snoop_buffered_arm_instructions() {
    let (cpu, mut bus) = program(&[NOP, OLD, OLD, OLD]);
    bus.write32(DATA, NEW).unwrap();
    bus.write32(DATA + 4, NEW).unwrap();
    let mut machine = Machine::new(cpu, bus);
    machine.step().unwrap();
    let before = machine.cpu().clone();
    let dma = DMA_BASE + 3 * DMA_STRIDE;
    machine.memory_mut().write32(dma, DATA).unwrap();
    machine.memory_mut().write32(dma + 4, CODE + 4).unwrap();
    machine.memory_mut().write32(dma + 8, 0x8400_0002).unwrap();
    for _ in 0..2 {
        assert_eq!(machine.step().unwrap(), StepKind::Dma { channel: 3 });
        assert_eq!(machine.cpu(), &before);
    }
    for _ in 0..2 {
        machine.step().unwrap();
        assert_eq!(machine.cpu().registers()[0], 1);
    }
}

#[test]
fn speculative_errors_do_not_fail_a_valid_instruction_or_an_escaping_branch() {
    for first in [NOP, 0xe12f_ff11] {
        let mut bus = Memory::new(first.to_le_bytes().to_vec()).unwrap();
        bus.write32(TARGET, OLD).unwrap();
        let mut cpu = Cpu::new(ROM_START);
        cpu.registers[1] = TARGET;
        cpu.step(&mut bus).unwrap();
        if first == NOP {
            let before = cpu.clone();
            for _ in 0..2 {
                assert_eq!(
                    cpu.step(&mut bus),
                    Err(CpuError::Memory(MemoryError::Unmapped(ROM_START + 4)))
                );
                assert_eq!(cpu, before);
            }
        } else {
            cpu.step(&mut bus).unwrap();
            assert_eq!(cpu.registers[0], 1);
        }
    }
    let (mut cpu, mut bus) = program(&[0xe12f_ff11]);
    cpu.registers[1] = 0x1000_0000;
    cpu.step(&mut bus).unwrap(); // Refill failure is deferred until target execution.
    let before = cpu.clone();
    assert!(cpu.step(&mut bus).is_err());
    assert_eq!(cpu, before);
}

#[test]
fn failed_data_access_keeps_the_old_buffer_and_discards_speculative_progress() {
    let (mut cpu, mut bus) = program(&[NOP, 0xe591_2000, OLD, OLD]);
    cpu.registers[1] = 0x0e00_0000;
    cpu.step(&mut bus).unwrap();
    let before = cpu.clone();
    assert!(cpu.step(&mut bus).is_err());
    assert_eq!(cpu, before);
    // Current LDR and next MOV were buffered. P+8 sampled by the failed step was not committed.
    bus.write32(CODE + 4, NOP).unwrap();
    bus.write32(CODE + 8, NEW).unwrap();
    bus.write32(CODE + 12, NEW).unwrap();
    bus.write32(DATA, 0x1234).unwrap();
    cpu.registers[1] = DATA;
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.registers[2], 0x1234);
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.registers[0], 1);
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.registers[0], 2);
}

#[test]
fn swi_and_machine_irq_refill_arm_vectors_before_handler_execution() {
    for irq in [false, true] {
        let mut bios = vec![0; BIOS_SIZE];
        let vector = if irq { 0x18 } else { 8 };
        bios[vector..vector + 4].copy_from_slice(&0xe12f_ff11_u32.to_le_bytes()); // BX r1.
        let rom = if irq { NOP } else { 0xef00_0000_u32 }
            .to_le_bytes()
            .to_vec();
        let mut bus = Memory::with_bios(rom, bios).unwrap();
        bus.write32(TARGET, OLD).unwrap();
        let mut cpu = Cpu::new(ROM_START);
        cpu.registers[1] = TARGET;
        let mut machine = Machine::new(cpu, bus);
        if irq {
            machine.memory_mut().write16(IE, 8).unwrap();
            machine.memory_mut().write16(IME, 1).unwrap();
            machine
                .memory_mut()
                .write32(TIMER_BASE, 0x00c0_ffff)
                .unwrap();
            machine.memory_mut().advance_cycles(1);
        }
        assert_eq!(
            machine.step().unwrap(),
            if irq {
                StepKind::IrqEntry
            } else {
                StepKind::Instruction
            }
        );
        assert!(machine.cpu().pipeline.is_some()); // Entry itself captured the ARM vector pair.
        machine.step().unwrap(); // Vector BX refills target pair.
        machine.memory_mut().write32(TARGET, NEW).unwrap();
        machine.step().unwrap();
        assert_eq!(machine.cpu().registers()[0], 1);
    }
}

#[test]
fn explicit_invalidation_repairs_buffered_unsupported_code_without_changing_registers() {
    let (mut cpu, mut bus) = program(&[NOP, 0xffff_ffff, OLD]);
    cpu.step(&mut bus).unwrap();
    let before = cpu.clone();
    let mut fork = cpu.clone();
    bus.write32(CODE + 4, NEW).unwrap();
    for _ in 0..2 {
        assert!(cpu.step(&mut bus).is_err());
        assert_eq!(cpu, before); // Includes the retained unsupported word.
    }
    cpu.invalidate_pipeline();
    assert_cpu_arch_eq!(cpu, before);
    assert_ne!(cpu, before); // Full Cpu equality includes the buffer.
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.registers[0], 2);
    assert!(fork.step(&mut bus).is_err()); // Cloning owns an independent retained buffer.
}

#[test]
fn explicit_exception_and_pc_discontinuity_discard_the_old_execution_sequence() {
    let (mut cpu, mut bus) = program(&[NOP, OLD, OLD]);
    cpu.step(&mut bus).unwrap();
    bus.write32(TARGET, NEW).unwrap();
    cpu.registers[15] = TARGET;
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.registers[0], 2);
    cpu.enter_exception(Exception::Irq);
    assert!(cpu.pipeline.is_none()); // This API has no Memory to sample.
    let before = cpu.clone();
    assert!(cpu.step(&mut bus).is_err()); // Missing BIOS remains a strict error.
    assert_eq!(cpu, before);
}

#[test]
fn state_changes_replace_the_buffer_and_returning_refills_from_current_memory() {
    let (mut cpu, mut bus) = program(&[0xe12f_ff11, OLD, OLD]);
    bus.write16(TARGET, 0x4710).unwrap(); // Thumb BX r2.
    cpu.registers[1] = TARGET | 1;
    cpu.registers[2] = CODE + 4;
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.instruction_set(), InstructionSet::Thumb);
    assert!(cpu.pipeline.is_some()); // BX captured the Thumb target pair.
    bus.write32(CODE + 4, NEW).unwrap();
    cpu.step(&mut bus).unwrap();
    bus.write32(CODE + 4, OLD).unwrap();
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.registers[0], 2);
}

#[test]
fn arm_exception_return_refills_after_status_and_target_restore() {
    let (mut cpu, mut bus) = program(&[0xe1b0_f00e]); // MOVS pc,lr.
    cpu.enter_exception(Exception::SoftwareInterrupt);
    cpu.registers[15] = CODE;
    cpu.registers[14] = TARGET;
    bus.write32(TARGET, OLD).unwrap();
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.mode(), Mode::System);
    assert_eq!(cpu.pc(), TARGET);
    bus.write32(TARGET, NEW).unwrap();
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.registers[0], 1);
}

#[test]
fn ram_mirrors_and_word_alignment_apply_to_buffered_fetches() {
    for pc in [0x0300_7ffc, 0x0300_fffc, 0x0203_fffc] {
        let mut bus = Memory::new(vec![]).unwrap();
        bus.write32(pc, NOP).unwrap();
        bus.write32(pc + 4, OLD).unwrap();
        bus.write32(pc + 8, OLD).unwrap();
        let mut cpu = Cpu::new(pc);
        cpu.step(&mut bus).unwrap();
        bus.write32(pc + 4, NEW).unwrap();
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.registers[0], 1);
    }
    let (mut cpu, mut bus) = program(&[NOP]);
    cpu.registers[15] += 2;
    let before = cpu.clone();
    assert_eq!(
        cpu.step(&mut bus),
        Err(CpuError::Memory(MemoryError::Unaligned(CODE + 2)))
    );
    assert_eq!(cpu, before);
}

#[test]
fn timed_and_untimed_buffers_match_without_extra_fetch_or_refill_cycles() {
    let code = [NOP, 0xeaff_ffff, OLD, NOP];
    let (mut timed, mut bus_timed) = program(&code);
    let (mut untimed, mut bus_untimed) = program(&code);
    for expected_cost in [1, 3, 1] {
        let timing = timed.step_timed(&mut bus_timed).unwrap();
        untimed.step(&mut bus_untimed).unwrap();
        assert_eq!(timed, untimed);
        assert_eq!(timing.code_cycles, expected_cost);
        assert_eq!(timing.data_cycles, 0);
        assert_eq!(timing.internal_cycles, 0);
        assert_eq!(bus_timed.cycles(), 0);
    }
}
