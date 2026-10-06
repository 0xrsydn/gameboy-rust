//! Original local-bus tests. Expected lanes are independent of the CPU instruction buffer.
use super::*;
use crate::{
    cpu::Cpu,
    dma::DMA_STRIDE,
    io::{IE, IME, TIMER_BASE},
    machine::{Machine, StepKind},
};

const CODE: u32 = 0x0200_0100;
const IWRAM: u32 = 0x0300_0000;
const WORD: u32 = 0x9abc_1234;
const NOP: u32 = 0xe1a0_0000;

fn arm(code: &[u32]) -> (Cpu, Memory) {
    let mut memory = Memory::new(vec![]).unwrap();
    for (i, word) in code.iter().enumerate() {
        memory.write32(CODE + i as u32 * 4, *word).unwrap();
    }
    memory.write32(IWRAM, WORD).unwrap();
    (Cpu::new(CODE), memory)
}

#[test]
fn arm_data_accesses_from_other_code_regions_drive_only_addressed_iwram_lanes() {
    for (instruction, expected) in [
        (0xe5d0_1000, None),       // LDRB low byte, not a complete word.
        (0xe1d0_10b2, None),       // LDRH high half, not a complete word.
        (0xe590_1000, Some(WORD)), // LDR word.
    ] {
        let (mut cpu, mut memory) = arm(&[0xe3a0_0403, instruction, NOP, NOP]); // MOV r0,#0x03000000.
        cpu.step(&mut memory).unwrap();
        assert_eq!(memory.iwram_bus.committed_word(), None);
        cpu.step(&mut memory).unwrap();
        assert_eq!(memory.iwram_bus.committed_word(), expected);
        cpu.step(&mut memory).unwrap();
        assert_eq!(memory.iwram_bus.committed_word(), expected); // EWRAM code does not erase the local latch.
    }
}

#[test]
fn arm_byte_and_halfword_reads_and_stores_preserve_other_lanes_across_nonlocal_code() {
    for (instruction, expected) in [
        (0xe5d0_1001, 0x9abc_5634), // LDRB r1,[r0,#1].
        (0xe1d0_10b2, 0x5678_1234), // LDRH r1,[r0,#2].
        (0xe5c0_2001, 0x9abc_0034), // STRB r2,[r0,#1], r2=0.
        (0xe1c0_20b2, 0x0000_1234), // STRH r2,[r0,#2].
    ] {
        let (mut cpu, mut memory) = arm(&[0xe3a0_0403, 0xe590_1000, NOP, instruction, NOP, NOP]);
        cpu.step(&mut memory).unwrap();
        cpu.step(&mut memory).unwrap();
        memory.write32(IWRAM, 0x5678_5678).unwrap(); // Host writes do not drive the latch.
        cpu.step(&mut memory).unwrap();
        assert_eq!(memory.iwram_bus.committed_word(), Some(WORD));
        cpu.step(&mut memory).unwrap();
        assert_eq!(memory.iwram_bus.committed_word(), Some(expected));
    }
}

#[test]
fn arm_and_thumb_target_pairs_drive_the_same_local_bus_in_fetch_order() {
    // EWRAM ARM -> IWRAM ARM -> IWRAM Thumb -> unavailable BIOS ARM.
    let (mut cpu, mut memory) = arm(&[0xe3a0_0403, 0xe12f_ff10, NOP, NOP]);
    memory.write32(IWRAM, 0xe280_1011).unwrap(); // ADD r1,r0,#0x11.
    memory.write32(IWRAM + 4, 0xe12f_ff11).unwrap(); // BX r1.
    memory.write32(IWRAM + 8, WORD).unwrap();
    memory.write32(IWRAM + 12, 0x8765_4321).unwrap();
    memory.write16(IWRAM + 16, 0x46c0).unwrap(); // Thumb NOP.
    memory.write16(IWRAM + 18, 0x4710).unwrap(); // BX r2 (zero -> BIOS; target error deferred).
    memory.write32(IWRAM + 20, 0x5678_1234).unwrap();
    cpu.step(&mut memory).unwrap();
    cpu.step(&mut memory).unwrap();
    assert_eq!(memory.iwram_bus.committed_word(), Some(0xe12f_ff11));
    cpu.step(&mut memory).unwrap();
    assert_eq!(memory.iwram_bus.committed_word(), Some(WORD));
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.instruction_set(), InstructionSet::Thumb);
    assert_eq!(memory.iwram_bus.committed_word(), Some(0x4710_46c0));
    cpu.step(&mut memory).unwrap();
    assert_eq!(memory.iwram_bus.committed_word(), Some(0x4710_1234));
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.instruction_set(), InstructionSet::Arm);
    assert_eq!(memory.iwram_bus.committed_word(), Some(0x5678_1234));
    let before = cpu.clone();
    assert!(cpu.step(&mut memory).is_err());
    assert_eq!(cpu, before);
    assert_eq!(memory.iwram_bus.committed_word(), Some(0x5678_1234));
}

#[test]
fn thumb_data_access_from_ewram_drives_local_iwram_history() {
    let (mut cpu, mut memory) = arm(&[
        0xe3a0_0403, // MOV r0,#0x03000000.
        0xe28f_1001, // ADD r1,pc,#1 -> CODE+13.
        0xe12f_ff11, // BX r1.
        0x46c0_6802, // Thumb LDR r2,[r0]; NOP.
        0x46c0_46c0,
    ]);
    for _ in 0..3 {
        cpu.step(&mut memory).unwrap();
    }
    assert_eq!(cpu.instruction_set(), InstructionSet::Thumb);
    assert_eq!(memory.iwram_bus.committed_word(), None);
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers()[2], WORD);
    assert_eq!(memory.iwram_bus.committed_word(), Some(WORD));
    memory.write32(IWRAM, 0).unwrap();
    cpu.step(&mut memory).unwrap();
    assert_eq!(memory.iwram_bus.committed_word(), Some(WORD));
}

#[test]
fn dma_can_establish_lanes_before_any_cpu_instruction() {
    for word in [false, true] {
        let (_, mut memory) = arm(&[]);
        let dma = DMA_BASE + 3 * DMA_STRIDE;
        memory.write32(dma, IWRAM).unwrap();
        memory.write32(dma + 4, CODE).unwrap();
        memory
            .write32(dma + 8, if word { 0x8400_0001 } else { 0x8000_0002 })
            .unwrap();
        memory.step_dma().unwrap().unwrap();
        assert_eq!(
            memory.iwram_bus.committed_word(),
            if word { Some(WORD) } else { None }
        );
        if !word {
            memory.step_dma().unwrap().unwrap();
            assert_eq!(memory.iwram_bus.committed_word(), Some(WORD));
        }
    }
}

#[test]
fn accepted_irq_and_bios_fetches_preserve_local_lanes_until_actual_iwram_access() {
    let mut bios = vec![0; BIOS_SIZE];
    bios[0x18..0x1c].copy_from_slice(&NOP.to_le_bytes());
    bios[0x1c..0x20].copy_from_slice(&0xe590_1000_u32.to_le_bytes()); // LDR r1,[r0].
    let mut memory = Memory::with_bios(vec![], bios).unwrap();
    for (i, word) in [0xe3a0_0403, 0xe590_1000, NOP, NOP].into_iter().enumerate() {
        memory.write32(CODE + i as u32 * 4, word).unwrap();
    }
    memory.write32(IWRAM, WORD).unwrap();
    let mut machine = Machine::new(Cpu::new(CODE), memory);
    machine.step().unwrap();
    machine.step().unwrap();
    assert_eq!(machine.memory().iwram_bus.committed_word(), Some(WORD));
    machine.memory_mut().write32(IWRAM, 0x8765_4321).unwrap();
    machine.memory_mut().write16(IE, 8).unwrap();
    machine.memory_mut().write16(IME, 1).unwrap();
    machine
        .memory_mut()
        .write32(TIMER_BASE, 0x00c0_ffff)
        .unwrap();
    machine.memory_mut().advance_cycles(1);
    assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
    assert_eq!(machine.memory().iwram_bus.committed_word(), Some(WORD));
    machine.step().unwrap();
    assert_eq!(machine.memory().iwram_bus.committed_word(), Some(WORD));
    machine.step().unwrap();
    assert_eq!(
        machine.memory().iwram_bus.committed_word(),
        Some(0x8765_4321)
    );
}

#[test]
fn failed_instruction_discards_its_iwram_fetch_and_failed_dma_preserves_committed_lanes() {
    let (mut cpu, mut memory) = arm(&[0xe3a0_0403, 0xe590_1000, 0xe12f_ff10, NOP, NOP]);
    cpu.step(&mut memory).unwrap();
    cpu.step(&mut memory).unwrap();
    memory.write32(IWRAM, 0xe7f0_00f0).unwrap(); // Unsupported ARM instruction.
    memory.write32(IWRAM + 4, WORD).unwrap();
    memory.write32(IWRAM + 8, 0x8765_4321).unwrap();
    cpu.step(&mut memory).unwrap(); // Target pair ends with WORD.
    let before = cpu.clone();
    assert!(cpu.step_timed(&mut memory).is_err());
    assert_eq!(cpu, before);
    assert_eq!(memory.iwram_bus.committed_word(), Some(WORD));
    // A failing cold entry also discards its current/decode/lookahead lane updates.
    let mut cold = Cpu::new(IWRAM);
    let before = cold.clone();
    assert!(cold.step(&mut memory).is_err());
    assert_eq!(cold, before);
    assert_eq!(memory.iwram_bus.committed_word(), Some(WORD));
    let dma = DMA_BASE + 3 * DMA_STRIDE;
    memory.write32(dma, IWRAM + 8).unwrap();
    memory.write32(dma + 4, ROM_START).unwrap(); // Read-only destination.
    memory.write32(dma + 8, 0x8400_0001).unwrap();
    assert!(memory.step_dma().is_err());
    assert_eq!(memory.iwram_bus.committed_word(), Some(WORD));
}
