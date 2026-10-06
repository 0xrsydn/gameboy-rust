//! IRQ fetch history is distinct from executing a BIOS instruction.
use super::*;
use crate::{
    cpu::Cpu,
    dma::DMA_STRIDE,
    io::{IE, IME, TIMER_BASE},
    machine::{Machine, StepKind},
};

const RAM: u32 = 0x0300_0100;
const SEED: u32 = 0xa1b2_c3d4;

fn seeded_irq(memory: &mut Memory) {
    // DMA supplies known lanes after CPU setup, before the IRQ's discarded fetch.
    memory.write32(0x0200_1000, SEED).unwrap();
    let dma = DMA_BASE + 3 * DMA_STRIDE;
    memory.write32(dma, 0x0200_1000).unwrap();
    memory.write32(dma + 4, RAM + 0x100).unwrap();
    memory.write32(dma + 8, 0x8400_0001).unwrap();
    memory.step_dma().unwrap().unwrap();
    memory.write16(IE, 8).unwrap();
    memory.write16(IME, 1).unwrap();
    memory.write32(TIMER_BASE, 0x00c0_ffff).unwrap();
    memory.advance_cycles(1);
    memory.write16(TIMER_BASE + 2, 0).unwrap();
}

#[test]
fn irq_drives_only_the_discarded_iwram_fetch_in_incoming_state() {
    for thumb in [false, true] {
        for offset in [0, 2] {
            if !thumb && offset != 0 {
                continue;
            }
            let pc = RAM + offset;
            let mut memory = Memory::new(vec![]).unwrap();
            let cpu = if thumb {
                // Original EWRAM bootstrap: load Thumb target then BX.
                memory.write32(0x0200_0000, 0xe59f_0000).unwrap();
                memory.write32(0x0200_0004, 0xe12f_ff10).unwrap();
                memory.write32(0x0200_0008, pc | 1).unwrap();
                let mut cpu = Cpu::new(0x0200_0000);
                cpu.step(&mut memory).unwrap();
                cpu.step(&mut memory).unwrap();
                cpu
            } else {
                Cpu::new(pc) // Cold IRQ entry must not fill current/decode slots.
            };
            memory.write32(RAM, 0xffff_ffff).unwrap();
            memory.write32(RAM + 4, 0x5678_1234).unwrap();
            memory.write32(RAM + 8, 0x9abc_def0).unwrap();
            seeded_irq(&mut memory);
            let mut machine = Machine::new(cpu, memory);
            assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
            let expected = if thumb {
                if offset == 0 {
                    0xa1b2_1234
                } else {
                    0x5678_c3d4
                }
            } else {
                0x9abc_def0
            };
            assert_eq!(machine.memory().iwram_bus.committed_word(), Some(expected));
            // Missing BIOS vector fails next step, without reverting accepted IRQ history.
            let before = machine.cpu().clone();
            let cycles = machine.cycles();
            assert!(machine.step().is_err());
            assert_eq!(machine.cpu(), &before);
            assert_eq!(machine.cycles(), cycles);
            assert_eq!(machine.memory().iwram_bus.committed_word(), Some(expected));
        }
    }
}

#[test]
fn irq_fetches_do_not_commit_a_bios_instruction_snapshot_or_erase_local_history() {
    for pc in [0x100, ROM_START, 0x0e00_0000] {
        let mut memory = Memory::with_bios(vec![], vec![0; BIOS_SIZE]).unwrap();
        memory.bios_prefetch = Some(SEED);
        seeded_irq(&mut memory);
        let mut machine = Machine::new(Cpu::new(pc), memory);
        assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
        assert_eq!(machine.memory().bios_prefetch, Some(SEED));
        assert_eq!(machine.memory().iwram_bus.committed_word(), Some(SEED));
        // BIOS retained history still follows the successful-instruction policy.
        machine.step().unwrap();
        assert_eq!(machine.memory().bios_prefetch, Some(0));
    }
}
