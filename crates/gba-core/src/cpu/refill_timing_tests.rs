//! Original tests with independent source-fetch and target-pair cost tables.
use super::*;
use crate::{
    dma::{DMA_BASE, DMA_STRIDE},
    io::{IE, IME, TIMER_BASE, WAITCNT},
    machine::{Machine, StepKind},
    memory::{BIOS_SIZE, ROM_START},
    timing::StepTiming,
};

const RAM: u32 = 0x0300_0100;
const ARM_NOP: u32 = 0xe1a0_0000;

fn bx(thumb: bool, pc: u32, target: u32, memory: &mut Memory) -> Cpu {
    let mut cpu = Cpu::new(pc);
    cpu.registers[0] = target;
    if thumb {
        cpu.instruction_set = InstructionSet::Thumb;
        memory.write16(pc, 0x4700).unwrap();
    } else {
        memory.write32(pc, 0xe12f_ff10).unwrap();
    }
    cpu
}

fn pending_irq(memory: &mut Memory) {
    memory.write16(IE, 8).unwrap();
    memory.write16(IME, 1).unwrap();
    memory.write32(TIMER_BASE, 0x00c0_ffff).unwrap();
    memory.advance_cycles(1);
    memory.write16(TIMER_BASE + 2, 0).unwrap();
}

fn resume(memory: &mut Memory) {
    let dma = DMA_BASE + 3 * DMA_STRIDE;
    memory.write32(dma, 0x0200_0000).unwrap();
    memory.write32(dma + 4, 0x0300_1000).unwrap();
    memory.write32(dma + 8, 0x8400_0001).unwrap();
    memory.step_dma().unwrap().unwrap();
}

#[test]
fn bx_target_pairs_use_outgoing_width_and_charge_the_third_fetch_only_on_execution() {
    for source_thumb in [false, true] {
        for target_thumb in [false, true] {
            let width = if target_thumb { 2 } else { 4 };
            for window in 0..3 {
                let boundary = ROM_START + window as u32 * 0x0200_0000 + 0x20000;
                // Boundary in first, second, or third target slot, or no boundary.
                for slot in 0..4 {
                    let target = boundary - slot * width;
                    let pattern = if target_thumb { 0x46c0_46c0 } else { ARM_NOP };
                    let mut memory =
                        Memory::new(pattern.to_le_bytes().repeat(0x20020 / 4)).unwrap();
                    let cpu = bx(
                        source_thumb,
                        RAM,
                        target | u32::from(target_thumb),
                        &mut memory,
                    );
                    for first in 0..4 {
                        for second in 0..2 {
                            for prefetch in [0, 0x4000] {
                                memory
                                    .write16(
                                        WAITCNT,
                                        (first << [2, 5, 8][window])
                                            | (second << [4, 7, 10][window])
                                            | prefetch,
                                    )
                                    .unwrap();
                                let n = [5, 4, 3, 9][first as usize];
                                let s = if second == 1 { 2 } else { [3, 5, 9][window] };
                                let extra_half = if target_thumb { 0 } else { s };
                                let mut cpu = cpu.clone();
                                let target_pair =
                                    n + extra_half + if slot == 1 { n } else { s } + extra_half;
                                assert_eq!(
                                    cpu.step_timed(&mut memory).unwrap(),
                                    StepTiming {
                                        code_cycles: 1 + target_pair,
                                        ..StepTiming::default()
                                    }
                                );
                                assert_eq!(cpu.pc(), target);
                                assert_eq!(
                                    cpu.step_timed(&mut memory).unwrap().code_cycles,
                                    if slot == 2 {
                                        n + extra_half
                                    } else {
                                        s + extra_half
                                    }
                                );
                                assert_eq!(memory.cycles(), 0);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn rom_source_fetch_uses_incoming_width_boundary_and_dma_override_before_ram_refill() {
    for thumb in [false, true] {
        let width = if thumb { 2 } else { 4 };
        for window in 0..3 {
            for boundary in [false, true] {
                for resumed in [false, true] {
                    let pc = ROM_START
                        + window as u32 * 0x0200_0000
                        + if boundary { 0x20000 - 2 * width } else { 0x100 };
                    let mut rom = vec![0; 0x20020];
                    let instruction: u32 = if thumb { 0x4700 } else { 0xe12f_ff10 };
                    let offset = (pc & 0x01ff_ffff) as usize;
                    rom[offset..offset + width as usize]
                        .copy_from_slice(&instruction.to_le_bytes()[..width as usize]);
                    let mut memory = Memory::new(rom).unwrap();
                    memory.write32(RAM, ARM_NOP).unwrap();
                    memory.write32(RAM + 4, ARM_NOP).unwrap();
                    for first in 0..4 {
                        for second in 0..2 {
                            memory
                                .write16(
                                    WAITCNT,
                                    (first << [2, 5, 8][window]) | (second << [4, 7, 10][window]),
                                )
                                .unwrap();
                            let mut cpu = Cpu::new(pc);
                            if thumb {
                                cpu.instruction_set = InstructionSet::Thumb;
                            }
                            cpu.registers[0] = RAM;
                            if resumed {
                                resume(&mut memory);
                            }
                            let n = [5, 4, 3, 9][first as usize];
                            let s = if second == 1 { 2 } else { [3, 5, 9][window] };
                            let source =
                                if boundary || resumed { n } else { s } + if thumb { 0 } else { s };
                            assert_eq!(
                                cpu.step_timed(&mut memory).unwrap().code_cycles,
                                source + 2
                            );
                            assert_eq!(cpu.step_timed(&mut memory).unwrap().code_cycles, 1);
                            // Resume is consumed by the branch, not delayed until another ROM fetch.
                            let mut probe = Cpu::new(pc);
                            if thumb {
                                probe.instruction_set = InstructionSet::Thumb;
                            }
                            probe.registers[0] = RAM;
                            assert_eq!(
                                probe.step_timed(&mut memory).unwrap().code_cycles,
                                if boundary { n } else { s } + if thumb { 0 } else { s } + 2
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn irq_charges_discarded_incoming_fetch_even_when_rom_is_missing() {
    for thumb in [false, true] {
        let width = if thumb { 2 } else { 4 };
        for (pc, normal, resumed) in [
            (
                0x0200_0100,
                if thumb { 3 } else { 6 },
                if thumb { 3 } else { 6 },
            ),
            (RAM, 1, 1),
            (
                ROM_START - width,
                if thumb { 3 } else { 6 },
                if thumb { 5 } else { 8 },
            ),
            (
                ROM_START + 0x100,
                if thumb { 3 } else { 6 },
                if thumb { 5 } else { 8 },
            ),
            (
                ROM_START + 0x20000 - 2 * width,
                if thumb { 5 } else { 8 },
                if thumb { 5 } else { 8 },
            ),
            (
                0x0c00_0000 - 2 * width,
                if thumb { 5 } else { 14 },
                if thumb { 5 } else { 14 },
            ),
        ] {
            for dma in [false, true] {
                let mut bios = vec![0; BIOS_SIZE];
                bios[0x18..0x1c].copy_from_slice(&ARM_NOP.to_le_bytes());
                let mut memory = Memory::with_bios(vec![], bios).unwrap();
                let mut cpu = Cpu::new(pc);
                if thumb {
                    cpu.instruction_set = InstructionSet::Thumb;
                }
                let cpsr = cpu.cpsr();
                if dma {
                    resume(&mut memory);
                }
                pending_irq(&mut memory);
                let mut machine = Machine::new(cpu, memory);
                let start = machine.cycles();
                assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
                let expected = if dma { resumed } else { normal } + 2;
                assert_eq!(
                    machine.last_timing(),
                    StepTiming {
                        code_cycles: expected,
                        ..StepTiming::default()
                    }
                );
                assert_eq!(machine.cycles() - start, u64::from(expected));
                assert_eq!(machine.cpu().pc(), 0x18);
                assert_eq!(machine.cpu().registers()[14], pc + 4);
                assert_eq!(machine.cpu().spsr(), Some(cpsr));
                assert_eq!(machine.cpu().instruction_set(), InstructionSet::Arm);
                // No old-PC opcode executes, even when its ROM bytes are missing.
                assert_eq!(machine.step().unwrap(), StepKind::Instruction);
                assert_eq!(machine.last_timing().code_cycles, 1);
            }
        }
    }
}

#[test]
fn target_pair_crossing_a_wait_window_uses_each_fetch_region() {
    for thumb in [false, true] {
        let width = if thumb { 2 } else { 4 };
        for (boundary, pair) in [
            (0x0a00_0000, if thumb { 10 } else { 18 }),
            (0x0c00_0000, if thumb { 10 } else { 24 }),
        ] {
            let mut memory = Memory::new(vec![]).unwrap();
            let mut cpu = bx(
                false,
                RAM,
                (boundary - width) | u32::from(thumb),
                &mut memory,
            );
            assert_eq!(cpu.step_timed(&mut memory).unwrap().code_cycles, 1 + pair);
            let before = cpu.clone();
            assert!(cpu.step_timed(&mut memory).is_err()); // Missing target deferred until current.
            assert_eq!(cpu, before);
            assert_eq!(memory.cycles(), 0);
        }
    }
}

#[test]
fn masked_irq_does_not_sample_or_change_cpu_or_consume_dma_resume() {
    for cpu_mask in [false, true] {
        let mut memory = Memory::new(vec![]).unwrap();
        pending_irq(&mut memory);
        let mut cpu = Cpu::new(ROM_START);
        cpu.irq_disabled = cpu_mask;
        if !cpu_mask {
            memory.write16(IME, 0).unwrap();
        }
        resume(&mut memory);
        let before = cpu.clone();
        let cycles = memory.cycles();
        assert_eq!(cpu.take_irq_timed(&mut memory), None);
        assert_eq!(cpu, before);
        assert_eq!(memory.cycles(), cycles);
        assert_eq!(
            memory.cpu_code_kind(crate::timing::AccessKind::Sequential),
            crate::timing::AccessKind::NonSequential
        );
    }
}
