use gba_core::{
    cpu::Cpu,
    io::{IE, IF, IME, TIMER_BASE, WAITCNT},
    machine::{Machine, StepKind},
    memory::{Memory, MemoryError, ROM_START},
    timing::{bus_cycles, AccessKind, AccessWidth},
};

#[test]
fn waitcnt_masks_unused_bits_and_supports_all_bus_widths() {
    let mut memory = Memory::new(vec![]).unwrap();
    assert_eq!(memory.read32(WAITCNT).unwrap(), 0);
    memory.write32(WAITCNT, u32::MAX).unwrap();
    assert_eq!(memory.read32(WAITCNT).unwrap(), 0x5fff);
    memory.write8(WAITCNT, 0x17).unwrap();
    memory.write8(WAITCNT + 1, 0x43).unwrap();
    assert_eq!(memory.waitcnt(), 0x4317);
    memory.write16(WAITCNT + 2, 0xffff).unwrap();
    assert_eq!(memory.read32(WAITCNT).unwrap(), 0x4317);
    assert_eq!(memory.read16(IE).unwrap(), 0);
    assert_eq!(memory.read16(IF).unwrap(), 0);
    assert_eq!(memory.cycles(), 0);
    assert_eq!(
        memory.write32(WAITCNT + 2, 0),
        Err(MemoryError::Unaligned(WAITCNT + 2))
    );
    assert_eq!(memory.waitcnt(), 0x4317);
}

#[test]
fn every_rom_wait_setting_has_the_documented_halfword_and_word_costs() {
    for window in 0..3 {
        for first in 0..4 {
            for second in 0..2 {
                let waitcnt = (first << [2, 5, 8][window]) | (second << [4, 7, 10][window]);
                let n = [5, 4, 3, 9][first as usize];
                let s = if second == 1 { 2 } else { [3, 5, 9][window] };
                for mirror in [0, 0x0100_0000] {
                    let address = ROM_START + window as u32 * 0x0200_0000 + mirror + 0x100;
                    for (kind, initial) in
                        [(AccessKind::NonSequential, n), (AccessKind::Sequential, s)]
                    {
                        assert_eq!(
                            bus_cycles(waitcnt, address, AccessWidth::Byte, kind),
                            initial
                        );
                        assert_eq!(
                            bus_cycles(waitcnt, address, AccessWidth::Halfword, kind),
                            initial
                        );
                        assert_eq!(
                            bus_cycles(waitcnt, address, AccessWidth::Word, kind),
                            initial + s
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn rom_128_kib_boundaries_force_nonsequential_first_halfwords() {
    for address in [
        0x0800_0000,
        0x0802_0000,
        0x09fe_0000,
        0x0a02_0000,
        0x0c02_0000,
    ] {
        let window = ((address >> 25) - 4) as usize;
        let s = [3, 5, 9][window];
        assert_eq!(
            bus_cycles(0, address, AccessWidth::Halfword, AccessKind::Sequential),
            5
        );
        assert_eq!(
            bus_cycles(0, address + 1, AccessWidth::Byte, AccessKind::Sequential),
            5
        );
        assert_eq!(
            bus_cycles(0, address, AccessWidth::Word, AccessKind::Sequential),
            5 + s
        );
        assert_eq!(
            bus_cycles(
                0,
                address + 2,
                AccessWidth::Halfword,
                AccessKind::Sequential
            ),
            s
        );
    }
}

#[test]
fn internal_regions_and_ewram_use_fixed_width_specific_costs() {
    for kind in [AccessKind::NonSequential, AccessKind::Sequential] {
        for width in [AccessWidth::Byte, AccessWidth::Halfword, AccessWidth::Word] {
            for address in [0, 0x0300_0000, 0x03ff_fffc, WAITCNT] {
                assert_eq!(bus_cycles(0x5fff, address, width, kind), 1);
            }
            for address in [0x0200_0000, 0x02ff_fffc] {
                assert_eq!(
                    bus_cycles(0x5fff, address, width, kind),
                    if width == AccessWidth::Word { 6 } else { 3 }
                );
            }
        }
    }
}

#[test]
fn stored_prefetch_phi_and_sram_settings_do_not_change_supported_bus_costs() {
    // Explicit limitation: no prefetch queue, PHI output, or SRAM mapping yet.
    for width in [AccessWidth::Halfword, AccessWidth::Word] {
        for address in [ROM_START, ROM_START + 4, 0x0a00_0004, 0x0c00_0004] {
            for kind in [AccessKind::NonSequential, AccessKind::Sequential] {
                assert_eq!(
                    bus_cycles(0, address, width, kind),
                    bus_cycles(0x5803, address, width, kind)
                );
            }
        }
    }
}

#[test]
fn wait_states_change_when_a_timer_interrupt_is_sampled() {
    // 20-cycle timer: four 6-cycle fetches or five 4-cycle fetches.
    // IRQ entry adds one old-state ROM fetch and the two-cycle BIOS vector pair.
    for (waitcnt, entry_step, instructions, cycles, irq_cycles) in
        [(0, 5, 4, 32, 8), (0x18, 6, 5, 26, 6)]
    {
        let mut memory = Memory::new(0xe280_0001_u32.to_le_bytes().repeat(64)).unwrap();
        memory.write16(WAITCNT, waitcnt).unwrap();
        memory.write32(TIMER_BASE, 0x00c0_ffec).unwrap(); // 20 cycles
        memory.write16(IE, 8).unwrap();
        memory.write32(IME, 1).unwrap();
        let mut machine = Machine::new(Cpu::new(ROM_START), memory);
        assert_eq!(machine.last_timing().total(), 0);
        for step in 1..=entry_step {
            let event = machine.step().unwrap();
            assert_eq!(
                event,
                if step == entry_step {
                    StepKind::IrqEntry
                } else {
                    StepKind::Instruction
                }
            );
        }
        assert_eq!(machine.cpu().registers()[0], instructions);
        assert_eq!(machine.cycles(), cycles);
        assert_eq!(machine.last_timing().code_cycles, irq_cycles);
    }
}

#[test]
fn failed_machine_step_preserves_previous_timing_breakdown() {
    let mut memory = Memory::new(0xe1a0_0000_u32.to_le_bytes().to_vec()).unwrap();
    memory.write32(TIMER_BASE, 0x0080_0000).unwrap();
    let mut machine = Machine::new(Cpu::new(ROM_START), memory);
    machine.step().unwrap();
    let timing = machine.last_timing();
    let cycles = machine.cycles();
    let counter = machine.memory().read16(TIMER_BASE).unwrap();
    assert!(machine.step().is_err());
    assert_eq!(machine.last_timing(), timing);
    assert_eq!(machine.cycles(), cycles);
    assert_eq!(machine.memory().read16(TIMER_BASE).unwrap(), counter);
}
