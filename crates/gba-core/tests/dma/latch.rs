//! Original tests of per-channel retained data, not CPU open-bus handoff.
use super::*;

const DATA: u32 = 0x9a73_b5e1;
const OTHER: u32 = 0x2468_1357;

fn seed(machine: &mut Machine, channel: usize, value: u32) {
    machine.memory_mut().write32(SOURCE, value).unwrap();
    configure(machine.memory_mut(), channel, SOURCE, DEST, 1, 0x400);
    units(machine, channel, 1);
}

fn blocked(machine: &mut Machine, channel: usize, destination: u32, control: u16) {
    configure(machine.memory_mut(), channel, 0, destination, 1, control);
    units(machine, channel, 1);
}

#[test]
fn blocked_sources_reuse_each_channels_word_without_reading_bios() {
    for channel in 0..4 {
        for source in [0, 0x3ffc, 0x4000, 0x0100_0000, 0x01ff_fffc] {
            let mut bios = vec![0x62; BIOS_SIZE];
            bios[..4].copy_from_slice(&OTHER.to_le_bytes());
            let mut machine = Machine::new(
                Cpu::new(ROM_START),
                Memory::with_bios(words(&[0xeaff_fffe]), bios).unwrap(),
            );
            seed(&mut machine, channel, DATA);
            machine.memory_mut().write32(SOURCE, 0).unwrap();
            assert_eq!(machine.memory().read32(0).unwrap(), OTHER);
            configure(machine.memory_mut(), channel, source, DEST + 4, 1, 0x400);
            units(&mut machine, channel, 1);
            assert_eq!(machine.memory().read32(DEST + 4).unwrap(), DATA);
            assert!(!enabled(&machine, channel));
        }
    }
}

#[test]
fn blocked_halfwords_select_destination_lane_and_do_not_duplicate_retained_word() {
    for channel in 0..4 {
        let mut machine = machine();
        seed(&mut machine, channel, DATA);
        // Neither source alignment nor halfword width may replace the old full word.
        for source in [0, 2, 0x3ffe, 0x01ff_fffe] {
            for lane in [0, 2] {
                configure(machine.memory_mut(), channel, source, DEST + 4 + lane, 1, 0);
                units(&mut machine, channel, 1);
            }
            assert_eq!(machine.memory().read32(DEST + 4).unwrap(), DATA);
        }
        blocked(&mut machine, channel, DEST + 8, 0x400);
        assert_eq!(machine.memory().read32(DEST + 8).unwrap(), DATA);
    }
}

#[test]
fn mapped_halfword_sources_duplicate_either_source_lane_in_the_channel_latch() {
    for channel in 0..4 {
        for source_lane in [0, 2] {
            for destination_lane in [0, 2] {
                let mut machine = machine();
                seed(&mut machine, channel, OTHER);
                machine.memory_mut().write32(SOURCE, DATA).unwrap();
                configure(
                    machine.memory_mut(),
                    channel,
                    SOURCE + source_lane,
                    DEST + destination_lane,
                    1,
                    0,
                );
                units(&mut machine, channel, 1);
                let half = (DATA >> (source_lane * 8)) as u16;
                assert_eq!(
                    machine.memory().read16(DEST + destination_lane).unwrap(),
                    half
                );
                blocked(&mut machine, channel, DEST + 4, 0x400);
                assert_eq!(
                    machine.memory().read32(DEST + 4).unwrap(),
                    u32::from(half) * 0x10001
                );
            }
        }
    }
}

#[test]
fn latches_are_independent_and_survive_completion_disable_and_host_access() {
    let mut machine = machine();
    for channel in 0..4 {
        seed(&mut machine, channel, DATA ^ (0x1111_1111 * channel as u32));
    }
    machine.memory_mut().write32(SOURCE, OTHER).unwrap();
    assert_eq!(machine.memory().read32(SOURCE).unwrap(), OTHER);
    for channel in (0..4).rev() {
        machine.memory_mut().write16(base(channel) + 10, 0).unwrap();
        blocked(&mut machine, channel, DEST + channel as u32 * 4, 0x400);
        assert_eq!(
            machine.memory().read32(DEST + channel as u32 * 4).unwrap(),
            DATA ^ (0x1111_1111 * channel as u32)
        );
    }
}

#[test]
fn cold_channel_stays_unknown_despite_other_channels_and_host_reads() {
    for channel in 0..4 {
        let mut machine = machine();
        seed(&mut machine, (channel + 1) % 4, DATA);
        assert_eq!(machine.memory().read32(DEST).unwrap(), DATA);
        configure(machine.memory_mut(), channel, 0, DEST + 4, 1, 0x4400);
        let cycles = machine.cycles();
        let timing = machine.last_timing();
        let cpu = machine.cpu().clone();
        for _ in 0..2 {
            assert_eq!(
                machine.step(),
                Err(MachineError::Dma(DmaError::UnsupportedSource {
                    channel,
                    address: 0
                }))
            );
            assert_eq!(machine.cpu(), &cpu);
            assert_eq!(machine.cycles(), cycles);
            assert_eq!(machine.last_timing(), timing);
            assert_eq!(machine.memory().read32(DEST + 4).unwrap(), 0);
            assert_eq!(machine.memory().read16(IF).unwrap(), 0);
            assert!(enabled(&machine, channel));
        }
    }
}

#[test]
fn zero_is_known_data_not_an_unknown_latch() {
    let mut machine = machine();
    seed(&mut machine, 3, 0);
    machine.memory_mut().write32(DEST + 4, u32::MAX).unwrap();
    blocked(&mut machine, 3, DEST + 4, 0x400);
    assert_eq!(machine.memory().read32(DEST + 4).unwrap(), 0);
}

#[test]
fn failed_units_preserve_the_old_latch_without_new_data_or_irq() {
    for (source, destination, control) in [
        (0x0e00_0000, DEST, 0x4400), // Unmapped reads do not use a known latch.
        (SOURCE, ROM_START, 0x4400),
        (SOURCE, 0x0400_0058, 0x4400),
        (SOURCE, DMA_BASE, 0x4400),
        (SOURCE, 0x0400_0300, 0x4400),
        (SOURCE, DEST, 0x4580), // Unsupported source mode.
        (0, ROM_START, 0x4400), // A blocked source cannot bypass destination checks.
    ] {
        let mut machine = machine();
        seed(&mut machine, 3, DATA);
        machine.memory_mut().write32(SOURCE, OTHER).unwrap();
        configure(machine.memory_mut(), 3, source, destination, 1, control);
        let cycles = machine.cycles();
        let timing = machine.last_timing();
        for _ in 0..2 {
            assert!(machine.step().is_err());
            assert_eq!(machine.cycles(), cycles);
            assert_eq!(machine.last_timing(), timing);
            assert_eq!(machine.memory().read16(IF).unwrap(), 0);
            assert_eq!(machine.memory().read32(DEST).unwrap(), DATA);
        }
        machine.memory_mut().write16(base(3) + 10, 0).unwrap();
        blocked(&mut machine, 3, DEST + 4, 0x400);
        assert_eq!(machine.memory().read32(DEST + 4).unwrap(), DATA);
    }
}

#[test]
fn failed_destination_does_not_seed_a_cold_channel() {
    let mut machine = machine();
    machine.memory_mut().write32(SOURCE, DATA).unwrap();
    configure(machine.memory_mut(), 3, SOURCE, ROM_START, 1, 0x400);
    assert!(machine.step().is_err());
    machine.memory_mut().write16(base(3) + 10, 0).unwrap();
    configure(machine.memory_mut(), 3, 0, DEST, 1, 0x400);
    assert_eq!(
        machine.step(),
        Err(MachineError::Dma(DmaError::UnsupportedSource {
            channel: 3,
            address: 0
        }))
    );
    assert_eq!(machine.cycles(), 0);
}

#[test]
fn source_counter_crossing_into_blocked_region_retains_the_last_completed_unit() {
    let mut machine = machine();
    machine.memory_mut().write32(SOURCE, DATA).unwrap();
    configure(machine.memory_mut(), 3, SOURCE, DEST, 3, 0x4480); // Decrement word source, IRQ.
    units(&mut machine, 3, 3);
    for offset in [0, 4, 8] {
        assert_eq!(machine.memory().read32(DEST + offset).unwrap(), DATA);
    }
    assert_eq!(machine.cycles(), 13); // 6+1+2, then two 1+1 units.
    assert_eq!(machine.memory().read16(IF).unwrap(), 1 << 11);
    assert!(!enabled(&machine, 3));
}

#[test]
fn failed_later_read_retains_last_successful_halfword_not_partial_bytes() {
    let mut machine = Machine::new(
        Cpu::new(ROM_START),
        Memory::new(vec![0x37, 0x92, 0x6a]).unwrap(),
    );
    configure(machine.memory_mut(), 3, ROM_START, DEST, 2, 0);
    units(&mut machine, 3, 1);
    assert!(machine.step().is_err());
    machine.memory_mut().write16(base(3) + 10, 0).unwrap();
    blocked(&mut machine, 3, DEST + 4, 0x400);
    assert_eq!(machine.memory().read32(DEST + 4).unwrap(), 0x9237_9237);
}

#[test]
fn cancellation_keeps_the_last_completed_unit_not_programmed_source_bytes() {
    let mut machine = machine();
    machine.memory_mut().write32(SOURCE, DATA).unwrap();
    machine.memory_mut().write32(SOURCE + 4, OTHER).unwrap();
    configure(machine.memory_mut(), 3, SOURCE, DEST, 3, 0x400);
    units(&mut machine, 3, 2);
    assert!(enabled(&machine, 3));
    machine.memory_mut().write16(base(3) + 10, 0).unwrap();
    machine.memory_mut().write32(SOURCE + 4, 0).unwrap();
    blocked(&mut machine, 3, DEST + 8, 0x400);
    assert_eq!(machine.memory().read32(DEST + 8).unwrap(), OTHER);
}

#[test]
fn blocked_dma_does_not_read_or_replace_the_protected_cpu_bios_word() {
    let mut bios = vec![0; BIOS_SIZE];
    bios[..16].copy_from_slice(&words(&[
        0xe59f_0000, // LDR r0,[pc]: ROM destination.
        0xe12f_ff10, // BX r0: retained BIOS word comes from address 12.
        ROM_START,
        OTHER,
    ]));
    let mut machine = Machine::new(
        Cpu::new(0),
        Memory::with_bios(words(&[0xe592_1000, 0xeaff_fffe, 0]), bios).unwrap(),
    ); // ROM LDR r1,[r2], with r2=0.
    for _ in 0..2 {
        assert_eq!(machine.step().unwrap(), StepKind::Instruction);
    }
    seed(&mut machine, 3, DATA);
    blocked(&mut machine, 3, DEST + 4, 0x400);
    assert_eq!(machine.memory().read32(DEST + 4).unwrap(), DATA);
    assert_eq!(machine.memory().read32(0).unwrap(), 0xe59f_0000);
    assert_eq!(machine.step().unwrap(), StepKind::Instruction);
    assert_eq!(machine.cpu().registers()[1], OTHER);
}

#[test]
fn repeated_hblank_blocks_and_priority_keep_channel_history_and_nominal_clocks() {
    let mut machine = machine();
    seed(&mut machine, 0, OTHER);
    seed(&mut machine, 3, DATA);
    machine
        .memory_mut()
        .write32(TIMER_BASE, 0x0080_0000)
        .unwrap();
    let start = machine.cycles();
    for channel in [3, 0] {
        configure(
            machine.memory_mut(),
            channel,
            0,
            DEST + 8 + channel as u32 * 8,
            1,
            0x6340,
        );
    } // Halfword, fixed source, increment/reload destination, HBlank repeat, IRQ.
    for cycle in [
        u64::from(HBLANK_START),
        u64::from(HBLANK_START + CYCLES_PER_LINE),
    ] {
        advance_to(&mut machine, cycle);
        for channel in [0, 3] {
            units(&mut machine, channel, 1);
            assert_eq!(machine.last_timing().data_cycles, 2);
            assert_eq!(machine.last_timing().internal_cycles, 2);
            assert!(enabled(&machine, channel));
        }
        assert_eq!(machine.memory().read16(DEST + 8).unwrap(), OTHER as u16);
        assert_eq!(machine.memory().read16(DEST + 32).unwrap(), DATA as u16);
        assert_eq!(machine.memory().read16(IF).unwrap() & 0x900, 0x900);
    }
    assert_eq!(
        u64::from(machine.memory().read16(TIMER_BASE).unwrap()),
        machine.cycles() - start
    );
}
