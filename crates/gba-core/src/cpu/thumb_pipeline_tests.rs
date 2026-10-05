//! Original Thumb instruction retention tests; bus history and timing remain separate.
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
const NOP: u16 = 0x46c0;
const OLD: u16 = 0x2001; // MOV r0,#1.
const NEW: u16 = 0x2002; // MOV r0,#2.

fn program(pc: u32, code: &[u16]) -> (Cpu, Memory) {
    let mut bus = Memory::new(vec![]).unwrap();
    for (index, half) in code.iter().enumerate() {
        bus.write16(pc + index as u32 * 2, *half).unwrap();
    }
    let mut cpu = Cpu::new(pc);
    cpu.instruction_set = InstructionSet::Thumb;
    (cpu, bus)
}

#[test]
fn thumb_stores_retain_the_two_prefetched_halfwords_but_not_the_next_one() {
    for instruction in [0x800a, 0x700a] {
        // STRH/STRB r2,[r1].
        for pc in [CODE, CODE + 2] {
            for offset in [2, 4, 6] {
                let (mut cpu, mut bus) = program(pc, &[instruction, OLD, OLD, OLD]);
                cpu.registers[1] = pc + offset;
                cpu.registers[2] = u32::from(NEW);
                cpu.step(&mut bus).unwrap();
                assert_eq!(bus.read16(pc + offset).unwrap(), NEW);
                for _ in 0..offset / 2 {
                    cpu.step(&mut bus).unwrap();
                }
                assert_eq!(cpu.registers[0], if offset <= 4 { 1 } else { 2 });
            }
        }
    }
}

#[test]
fn host_writes_and_memory_mirrors_do_not_replace_buffered_halfwords() {
    for pc in [
        CODE,
        CODE + 2,
        0x0300_fffe,
        0x0203_fffe,
        0x0500_03fe,
        0x0600_fffe,
        0x0700_03fe,
    ] {
        let (mut cpu, mut bus) = program(pc, &[NOP, OLD, OLD, OLD]);
        cpu.step(&mut bus).unwrap();
        for offset in [2, 4, 6] {
            bus.write16(pc + offset, NEW).unwrap();
        }
        for expected in [1, 1, 2] {
            cpu.step(&mut bus).unwrap();
            assert_eq!(cpu.registers[0], expected);
        }
    }
}

#[test]
fn pc_writes_to_fallthrough_refill_the_thumb_target_pair() {
    for instruction in [
        0xe7ff, 0xd0ff, 0x468f, 0x448f, 0x4708, 0xbd00, 0xbc00, 0xc900, 0xf800,
    ] {
        // B, BEQ, MOV pc,r1, ADD pc,r1, BX r1, POP pc, empty POP/LDM, BL suffix.
        let (mut cpu, mut bus) = program(CODE, &[NOP, instruction, OLD, OLD]);
        cpu.flags.zero = true;
        cpu.registers[1] = match instruction {
            0x448f => u32::MAX - 1, // Visible PC=CODE+6, target=CODE+4.
            0xc900 => DATA,
            _ => (CODE + 4) | 1,
        };
        cpu.registers[13] = DATA;
        cpu.registers[14] = CODE + 4;
        bus.write32(DATA, CODE + 4).unwrap(); // POP keeps Thumb even with bit0 clear.
        cpu.step(&mut bus).unwrap();
        bus.write16(CODE + 4, NEW).unwrap();
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.pc(), CODE + 4);
        assert_eq!(cpu.instruction_set(), InstructionSet::Thumb);
        bus.write16(CODE + 4, OLD).unwrap();
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.registers[0], 2, "{instruction:#06x}");
    }
}

#[test]
fn untaken_branch_keeps_the_old_pair() {
    let (mut cpu, mut bus) = program(CODE, &[NOP, 0xd0ff, OLD]);
    cpu.step(&mut bus).unwrap();
    bus.write16(CODE + 4, NEW).unwrap();
    cpu.step(&mut bus).unwrap();
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.registers[0], 1);
}

#[test]
fn bl_prefix_retains_the_suffix_and_only_the_suffix_refills() {
    let (mut cpu, mut bus) = program(CODE, &[0xf000, 0xf800, OLD, OLD]);
    assert_eq!(cpu.step_timed(&mut bus).unwrap().code_cycles, 1);
    assert_eq!(cpu.registers[14], CODE + 4);
    bus.write16(CODE + 2, NOP).unwrap(); // Does not replace the retained suffix.
    bus.write16(CODE + 4, NEW).unwrap();
    assert_eq!(cpu.step_timed(&mut bus).unwrap().code_cycles, 3);
    assert_eq!(cpu.registers[14], CODE + 5);
    bus.write16(CODE + 4, OLD).unwrap(); // Suffix already captured the updated target.
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.registers[0], 2);
}

#[test]
fn arm_to_thumb_refill_captures_target_and_plus_two_but_not_plus_four() {
    for target in [TARGET, TARGET + 2] {
        let (mut cpu, mut bus) = program(CODE, &[0, 0]);
        cpu.instruction_set = InstructionSet::Arm;
        bus.write32(CODE, 0xe12f_ff11).unwrap(); // BX r1.
        cpu.registers[1] = target | 1;
        for offset in [0, 2, 4] {
            bus.write16(target + offset, OLD).unwrap();
        }
        cpu.step(&mut bus).unwrap();
        for offset in [0, 2, 4] {
            bus.write16(target + offset, NEW).unwrap();
        }
        for expected in [1, 1, 2] {
            cpu.step(&mut bus).unwrap();
            assert_eq!(cpu.registers[0], expected);
        }
    }
}

#[test]
fn dma_halfword_and_word_writes_do_not_replace_thumb_instruction_slots() {
    for word in [false, true] {
        let (cpu, mut bus) = program(CODE, &[NOP, OLD, OLD, OLD]);
        for offset in [0, 2, 4] {
            bus.write16(DATA + offset, NEW).unwrap();
        }
        let mut machine = Machine::new(cpu, bus);
        machine.step().unwrap();
        let before = machine.cpu().clone();
        let dma = DMA_BASE + 3 * DMA_STRIDE;
        machine.memory_mut().write32(dma, DATA).unwrap();
        machine
            .memory_mut()
            .write32(dma + 4, CODE + if word { 4 } else { 2 })
            .unwrap();
        machine
            .memory_mut()
            .write32(dma + 8, if word { 0x8400_0001 } else { 0x8000_0003 })
            .unwrap();
        for _ in 0..if word { 1 } else { 3 } {
            assert_eq!(machine.step().unwrap(), StepKind::Dma { channel: 3 });
            assert_eq!(machine.cpu(), &before);
        }
        for expected in [1, 1, 2] {
            machine.step().unwrap();
            assert_eq!(machine.cpu().registers()[0], expected);
        }
    }
}

#[test]
fn failed_thumb_load_rolls_back_buffer_progress_not_earlier_retention() {
    let (mut cpu, mut bus) = program(CODE, &[NOP, 0x680a, OLD, OLD]); // LDR r2,[r1].
    cpu.registers[1] = 0x0e00_0000;
    cpu.step(&mut bus).unwrap();
    let before = cpu.clone();
    assert!(cpu.step_timed(&mut bus).is_err());
    assert_eq!(cpu, before);
    bus.write16(CODE + 2, NOP).unwrap();
    bus.write16(CODE + 4, NEW).unwrap();
    bus.write16(CODE + 6, NEW).unwrap();
    bus.write32(DATA, 0x1234).unwrap();
    cpu.registers[1] = DATA;
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.registers[2], 0x1234);
    for expected in [1, 2] {
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.registers[0], expected);
    }
}

#[test]
fn short_rom_and_refill_errors_are_deferred_and_never_use_open_bus_instructions() {
    for half in [NOP, 0x4708, 0xf000] {
        // NOP, BX r1, BL prefix.
        let mut bus = Memory::new(half.to_le_bytes().to_vec()).unwrap();
        bus.write16(TARGET, OLD).unwrap();
        let mut cpu = Cpu::new(ROM_START);
        cpu.instruction_set = InstructionSet::Thumb;
        cpu.registers[1] = TARGET | 1;
        cpu.step(&mut bus).unwrap();
        if half == 0x4708 {
            bus.write16(TARGET, NEW).unwrap();
            cpu.step(&mut bus).unwrap();
            assert_eq!(cpu.registers[0], 1);
        } else {
            let before = cpu.clone();
            for _ in 0..2 {
                assert_eq!(
                    cpu.step(&mut bus),
                    Err(CpuError::Memory(MemoryError::Unmapped(ROM_START + 2)))
                );
                assert_eq!(cpu, before);
            }
        }
    }
    let (mut cpu, mut bus) = program(CODE, &[0x4708]);
    cpu.registers[1] = 0x8000_0001;
    cpu.step(&mut bus).unwrap();
    let before = cpu.clone();
    assert_eq!(
        cpu.step(&mut bus),
        Err(CpuError::Memory(MemoryError::Unmapped(0x8000_0000)))
    );
    assert_eq!(cpu, before);
}

#[test]
fn explicit_invalidation_repairs_buffered_thumb_code_and_clones_stay_independent() {
    let (mut cpu, mut bus) = program(CODE, &[NOP, 0xde00, OLD]);
    cpu.step(&mut bus).unwrap();
    let before = cpu.clone();
    let mut fork = cpu.clone();
    bus.write16(CODE + 2, NEW).unwrap();
    assert!(cpu.step(&mut bus).is_err());
    assert_eq!(cpu, before);
    cpu.invalidate_pipeline();
    assert_cpu_arch_eq!(cpu, before);
    assert_ne!(cpu, before);
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.registers[0], 2);
    assert!(fork.step(&mut bus).is_err());
}

#[test]
fn instruction_state_and_pc_both_select_the_retained_buffer() {
    let (mut cpu, mut bus) = program(CODE, &[NOP, OLD, OLD]);
    cpu.step(&mut bus).unwrap();
    // A PC discontinuity starts a new Thumb sequence from mapped bytes.
    cpu.registers[15] = TARGET + 2;
    bus.write16(TARGET + 2, NOP).unwrap();
    bus.write16(TARGET + 4, OLD).unwrap();
    cpu.step(&mut bus).unwrap();
    // Expected next Thumb PC is now TARGET+4. Keep that PC but change state without a refill.
    cpu.instruction_set = InstructionSet::Arm;
    bus.write32(TARGET + 4, 0xe3a0_0007).unwrap();
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.registers[0], 7); // Must not decode the old Thumb halfword as ARM.
    cpu.registers[15] = CODE + 1;
    cpu.instruction_set = InstructionSet::Thumb;
    let before = cpu.clone();
    assert_eq!(
        cpu.step(&mut bus),
        Err(CpuError::Memory(MemoryError::Unaligned(CODE + 1)))
    );
    assert_eq!(cpu, before);
}

#[test]
fn swi_and_irq_returns_refill_thumb_targets_using_saved_state() {
    for irq in [false, true] {
        let mut bios = vec![0; BIOS_SIZE];
        let vector = if irq { 0x18 } else { 8 };
        let instruction: u32 = if irq { 0xe25e_f004 } else { 0xe1b0_f00e };
        bios[vector..vector + 4].copy_from_slice(&instruction.to_le_bytes());
        let mut bus = Memory::with_bios(vec![], bios).unwrap();
        bus.write16(CODE, if irq { NOP } else { 0xdf00 }).unwrap();
        for offset in [2, 4] {
            bus.write16(CODE + offset, OLD).unwrap();
        }
        let mut cpu = Cpu::new(CODE);
        cpu.instruction_set = InstructionSet::Thumb;
        let mut machine = Machine::new(cpu, bus);
        machine.step().unwrap();
        if irq {
            machine.memory_mut().write16(IE, 8).unwrap();
            machine.memory_mut().write16(IME, 1).unwrap();
            machine
                .memory_mut()
                .write32(TIMER_BASE, 0x00c0_ffff)
                .unwrap();
            machine.memory_mut().advance_cycles(1);
            assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
            machine.memory_mut().write16(IME, 0).unwrap();
        }
        machine.memory_mut().write16(CODE + 2, NEW).unwrap();
        machine.step().unwrap(); // ARM exception return captures updated Thumb target.
        machine.memory_mut().write16(CODE + 2, OLD).unwrap();
        machine.step().unwrap();
        assert_eq!(machine.cpu().registers()[0], 2);
        assert_eq!(machine.cpu().instruction_set(), InstructionSet::Thumb);
    }
}

#[test]
fn timed_and_untimed_thumb_buffering_has_no_additional_nominal_costs() {
    let code = [NOP, 0xe7ff, OLD, NOP];
    let (mut timed, mut timed_bus) = program(CODE, &code);
    let (mut untimed, mut bus) = program(CODE, &code);
    for cost in [1, 3, 1] {
        let timing = timed.step_timed(&mut timed_bus).unwrap();
        untimed.step(&mut bus).unwrap();
        assert_eq!(timed, untimed);
        assert_eq!(timing.code_cycles, cost);
        assert_eq!(timing.data_cycles, 0);
        assert_eq!(timing.internal_cycles, 0);
        assert_eq!(timed_bus.cycles(), 0);
    }
}
