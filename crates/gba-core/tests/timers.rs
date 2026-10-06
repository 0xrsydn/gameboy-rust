use gba_core::{
    io::{IE, IF, IME, TIMER_BASE},
    memory::{Memory, MemoryError},
};

fn memory() -> Memory {
    Memory::new(vec![]).unwrap()
}

#[test]
fn io_reset_values_masks_and_unused_bytes() {
    let mut bus = memory();
    for address in (TIMER_BASE..TIMER_BASE + 16)
        .chain(IE..IE + 4)
        .chain(IME..IME + 4)
    {
        assert_eq!(bus.read8(address).unwrap(), 0);
    }
    bus.write16(IE, 0xffff).unwrap();
    assert_eq!(bus.read16(IE).unwrap(), 0x3fff);
    bus.write32(IME, u32::MAX).unwrap();
    assert_eq!(bus.read32(IME).unwrap(), 1);
    bus.write32(IME, 0xffff_fffe).unwrap();
    assert_eq!(bus.read32(IME).unwrap(), 0);
    for index in 0..4 {
        let address = TIMER_BASE + index * 4 + 2;
        bus.write16(address, 0xffff).unwrap();
        assert_eq!(
            bus.read16(address).unwrap(),
            if index == 0 { 0xc3 } else { 0xc7 }
        );
    }
}

#[test]
fn timer_data_reads_counter_not_reload_and_start_loads_reload() {
    let mut bus = memory();
    bus.write16(TIMER_BASE, 0x1234).unwrap();
    assert_eq!(bus.read16(TIMER_BASE).unwrap(), 0);
    bus.advance_cycles(100);
    assert_eq!(bus.read16(TIMER_BASE).unwrap(), 0);
    bus.write16(TIMER_BASE + 2, 0x80).unwrap();
    assert_eq!(bus.read16(TIMER_BASE).unwrap(), 0x1234);
    bus.advance_cycles(5);
    assert_eq!(bus.read16(TIMER_BASE).unwrap(), 0x1239);
    bus.write16(TIMER_BASE, 0xabcd).unwrap();
    assert_eq!(bus.read16(TIMER_BASE).unwrap(), 0x1239);
    bus.write16(TIMER_BASE + 2, 0x80).unwrap(); // No rising enable edge
    assert_eq!(bus.read16(TIMER_BASE).unwrap(), 0x1239);
    bus.write16(TIMER_BASE + 2, 0).unwrap();
    bus.advance_cycles(100);
    assert_eq!(bus.read16(TIMER_BASE).unwrap(), 0x1239);
    bus.write16(TIMER_BASE + 2, 0x80).unwrap();
    assert_eq!(bus.read16(TIMER_BASE).unwrap(), 0xabcd);
}

#[test]
fn combined_word_write_uses_new_reload_before_enabling() {
    let mut bus = memory();
    for index in 0..4 {
        let address = TIMER_BASE + index * 4;
        bus.write32(address, 0x0080_abcd).unwrap();
        assert_eq!(bus.read32(address).unwrap(), 0x0080_abcd);
    }
    bus.advance_cycles(1);
    for index in 0..4 {
        assert_eq!(bus.read16(TIMER_BASE + index * 4).unwrap(), 0xabce);
    }
}

#[test]
fn reload_byte_writes_merge_with_hidden_latch_not_counter() {
    let mut bus = memory();
    bus.write16(TIMER_BASE, 0x1234).unwrap();
    bus.write8(TIMER_BASE, 0x56).unwrap();
    bus.write8(TIMER_BASE + 2, 0x80).unwrap();
    assert_eq!(bus.read16(TIMER_BASE).unwrap(), 0x1256);
    bus.advance_cycles(1000);
    bus.write8(TIMER_BASE + 1, 0xab).unwrap();
    bus.write8(TIMER_BASE + 2, 0).unwrap();
    bus.write8(TIMER_BASE + 2, 0x80).unwrap();
    assert_eq!(bus.read16(TIMER_BASE).unwrap(), 0xab56);
}

#[test]
fn all_prescalers_retain_fractional_cycles_between_advances() {
    for (selection, divisor) in [1, 64, 256, 1024].into_iter().enumerate() {
        let mut bus = memory();
        bus.write32(TIMER_BASE, ((0x80 | selection as u32) << 16) | 123)
            .unwrap();
        bus.advance_cycles(divisor - 1);
        assert_eq!(bus.read16(TIMER_BASE).unwrap(), 123);
        bus.advance_cycles(1);
        assert_eq!(bus.read16(TIMER_BASE).unwrap(), 124);
        bus.advance_cycles(divisor * 3 + divisor - 1);
        assert_eq!(bus.read16(TIMER_BASE).unwrap(), 127);
        bus.advance_cycles(1);
        assert_eq!(bus.read16(TIMER_BASE).unwrap(), 128);
    }
}

#[test]
fn clock_source_changes_and_restart_keep_the_shared_phase() {
    let mut bus = memory();
    bus.write32(TIMER_BASE, 0x0081_0000).unwrap(); // /64
    bus.advance_cycles(63);
    bus.write16(TIMER_BASE + 2, 0xc1).unwrap(); // IRQ change preserves phase
    bus.advance_cycles(1);
    assert_eq!(bus.read16(TIMER_BASE).unwrap(), 1);
    bus.advance_cycles(63);
    bus.write16(TIMER_BASE + 2, 0x82).unwrap(); // t=127: next shared /256 edge is t=256.
    bus.advance_cycles(128);
    assert_eq!(bus.read16(TIMER_BASE).unwrap(), 1);
    bus.advance_cycles(1);
    assert_eq!(bus.read16(TIMER_BASE).unwrap(), 2);
    bus.write16(TIMER_BASE + 2, 0).unwrap();
    bus.advance_cycles(255); // t=511; disabled time still advances the divider.
    bus.write16(TIMER_BASE + 2, 0x82).unwrap();
    assert_eq!(bus.read16(TIMER_BASE).unwrap(), 0);
    bus.advance_cycles(1);
    assert_eq!(bus.read16(TIMER_BASE).unwrap(), 1);
}

#[test]
fn overflow_reloads_and_latches_request_without_delivery_enables() {
    let mut bus = memory();
    for index in 0..4 {
        bus.write32(TIMER_BASE + index * 4, 0x00c0_fffe).unwrap();
    }
    bus.advance_cycles(1);
    assert_eq!(bus.read16(IF).unwrap(), 0);
    bus.advance_cycles(1);
    assert_eq!(bus.read16(IF).unwrap(), 0x78);
    assert!(!bus.irq_pending()); // IE and IME remain zero
    for index in 0..4 {
        assert_eq!(bus.read16(TIMER_BASE + index * 4).unwrap(), 0xfffe);
    }
    bus.advance_cycles(5);
    assert_eq!(bus.read16(IF).unwrap(), 0x78);
    assert_eq!(bus.read16(TIMER_BASE).unwrap(), 0xffff);
}

#[test]
fn overflow_uses_updated_reload_without_resetting_current_counter() {
    let mut bus = memory();
    bus.write32(TIMER_BASE, 0x00c0_fffe).unwrap();
    bus.write16(TIMER_BASE, 0x1234).unwrap();
    bus.advance_cycles(1);
    assert_eq!(bus.read16(TIMER_BASE).unwrap(), 0xffff);
    bus.advance_cycles(1);
    assert_eq!(bus.read16(TIMER_BASE).unwrap(), 0x1234);
    assert_eq!(bus.read16(IF).unwrap(), 8);
}

#[test]
fn overflow_with_local_irq_disabled_does_not_set_request() {
    let mut bus = memory();
    bus.write32(TIMER_BASE, 0x0080_ffff).unwrap();
    bus.advance_cycles(100);
    assert_eq!(bus.read16(IF).unwrap(), 0);
    bus.write16(TIMER_BASE + 2, 0xc0).unwrap();
    assert_eq!(bus.read16(IF).unwrap(), 0); // No retroactive request
    bus.advance_cycles(1);
    assert_eq!(bus.read16(IF).unwrap(), 8);
}

#[test]
fn cascades_count_all_overflows_including_multiple_levels() {
    let mut bus = memory();
    bus.write32(TIMER_BASE, 0x00c0_fffe).unwrap(); // Overflow every 2 cycles
    for index in 1..4 {
        bus.write32(TIMER_BASE + index * 4, 0x00c7_fffe).unwrap(); // /1024 ignored
    }
    bus.advance_cycles(15);
    assert_eq!(bus.read16(IF).unwrap(), 0x38);
    for index in 0..4 {
        assert_eq!(bus.read16(TIMER_BASE + index * 4).unwrap(), 0xffff);
    }
    bus.advance_cycles(1);
    assert_eq!(bus.read16(IF).unwrap(), 0x78);
    for index in 0..4 {
        assert_eq!(bus.read16(TIMER_BASE + index * 4).unwrap(), 0xfffe);
    }
}

#[test]
fn disabled_cascade_stage_blocks_downstream_pulses() {
    let mut bus = memory();
    bus.write32(TIMER_BASE, 0x0080_ffff).unwrap();
    bus.write32(TIMER_BASE + 4, 0x0004_ffff).unwrap(); // Disabled timer 1
    bus.write32(TIMER_BASE + 8, 0x00c4_1234).unwrap();
    bus.advance_cycles(100_000);
    assert_eq!(bus.read16(TIMER_BASE + 8).unwrap(), 0x1234);
    assert_eq!(bus.read16(IF).unwrap(), 0);
}

#[test]
fn normal_timer_breaks_cascade_dependency_and_timer_zero_ignores_count_up() {
    let mut bus = memory();
    bus.write32(TIMER_BASE, 0x0084_ffff).unwrap(); // Bit 2 ignored
    bus.write32(TIMER_BASE + 8, 0x0080_fffe).unwrap(); // Independent timer 2
    bus.write32(TIMER_BASE + 12, 0x00c4_ffff).unwrap();
    bus.advance_cycles(2);
    assert_eq!(bus.read16(TIMER_BASE + 2).unwrap(), 0x80);
    assert_eq!(bus.read16(TIMER_BASE).unwrap(), 0xffff);
    assert_eq!(bus.read16(IF).unwrap(), 0x40);
}

#[test]
fn bulk_advance_handles_maximum_cycle_count_and_period_one() {
    let mut bus = memory();
    bus.write32(TIMER_BASE, 0x00c0_ffff).unwrap();
    for index in 1..4 {
        bus.write32(TIMER_BASE + index * 4, 0x00c4_ffff).unwrap();
    }
    bus.advance_cycles(u32::MAX);
    assert_eq!(bus.cycles(), u64::from(u32::MAX));
    for index in 0..4 {
        assert_eq!(bus.read16(TIMER_BASE + index * 4).unwrap(), 0xffff);
    }
    assert_eq!(bus.read16(IF).unwrap(), 0x78);
    bus.advance_cycles(u32::MAX);
    assert_eq!(bus.cycles(), 2 * u64::from(u32::MAX));
}

#[test]
fn bulk_advance_matches_independent_cycle_by_cycle_reference() {
    // A small literal reference: count prescaler ticks and propagate one pulse
    // per cycle. Unlike the implementation, this does not use overflow division.
    for selection in 0..4 {
        for cascade_mask in 0..8 {
            let mut bus = memory();
            let reloads = [0xfffd_u16, 0xfffe, 0xffff, 0xfffc];
            let mut counters = reloads;
            let mut phases = [0_u32; 4];
            let mut pending = 0_u16;
            for (index, reload) in reloads.into_iter().enumerate() {
                let cascade = index > 0 && cascade_mask & (1 << (index - 1)) != 0;
                let control = 0xc0 | selection | if cascade { 4 } else { 0 };
                bus.write32(
                    TIMER_BASE + index as u32 * 4,
                    (control << 16) | u32::from(reload),
                )
                .unwrap();
            }
            for chunk in [0, 1, 17, 63, 1000, 8193] {
                bus.advance_cycles(chunk);
                for _ in 0..chunk {
                    let mut overflow = false;
                    for index in 0..4 {
                        let cascade = index > 0 && cascade_mask & (1 << (index - 1)) != 0;
                        let tick = if cascade {
                            overflow
                        } else {
                            phases[index] += 1;
                            if phases[index] == [1, 64, 256, 1024][selection as usize] {
                                phases[index] = 0;
                                true
                            } else {
                                false
                            }
                        };
                        overflow = tick && counters[index] == u16::MAX;
                        if overflow {
                            counters[index] = reloads[index];
                            pending |= 1 << (index + 3);
                        } else if tick {
                            counters[index] += 1;
                        }
                    }
                }
                for (index, counter) in counters.into_iter().enumerate() {
                    assert_eq!(bus.read16(TIMER_BASE + index as u32 * 4).unwrap(), counter);
                }
                assert_eq!(bus.read16(IF).unwrap(), pending);
            }
        }
    }
}

#[test]
fn interrupt_delivery_requires_both_ie_and_ime_without_clearing_if() {
    let mut bus = memory();
    bus.write32(TIMER_BASE, 0x00c0_ffff).unwrap();
    bus.advance_cycles(1);
    for (enable, master, expected) in [
        (0, 0, false),
        (8, 0, false),
        (0, 1, false),
        (16, 1, false),
        (8, 1, true),
    ] {
        bus.write16(IE, enable).unwrap();
        bus.write32(IME, master).unwrap();
        assert_eq!(bus.irq_pending(), expected);
        assert_eq!(bus.read16(IF).unwrap(), 8);
    }
}

#[test]
fn interrupt_acknowledgement_is_write_one_to_clear_for_all_widths() {
    let mut bus = memory();
    for index in 0..4 {
        bus.write32(TIMER_BASE + index * 4, 0x00c0_ffff).unwrap();
    }
    bus.advance_cycles(1);
    bus.write16(IF, 0).unwrap();
    assert_eq!(bus.read16(IF).unwrap(), 0x78);
    bus.write8(IF + 1, 0xff).unwrap(); // High byte must not clear low flags
    assert_eq!(bus.read16(IF).unwrap(), 0x78);
    bus.write8(IF, 8).unwrap();
    assert_eq!(bus.read16(IF).unwrap(), 0x70);
    bus.write16(IF, 0x20).unwrap();
    assert_eq!(bus.read16(IF).unwrap(), 0x50);
    bus.write32(IE, 0x0010_0078).unwrap(); // IE=0x78, clear only Timer 1
    assert_eq!(bus.read32(IE).unwrap(), 0x0040_0078);
    bus.write16(IF, 0xffff).unwrap();
    assert_eq!(bus.read16(IF).unwrap(), 0);
    bus.advance_cycles(1);
    assert_eq!(bus.read16(IF).unwrap(), 0x78); // Later events relatch
}

#[test]
fn high_byte_writes_do_not_reset_control_or_modify_low_enable_bits() {
    let mut bus = memory();
    bus.write32(TIMER_BASE, 0x0080_1234).unwrap();
    bus.write8(TIMER_BASE + 3, 0xff).unwrap();
    bus.write8(IE, 8).unwrap();
    bus.write8(IE + 1, 0xff).unwrap();
    assert_eq!(bus.read16(IE).unwrap(), 0x3f08);
    bus.write8(IME, 1).unwrap();
    for offset in 1..4 {
        bus.write8(IME + offset, 0xff).unwrap();
    }
    assert_eq!(bus.read32(IME).unwrap(), 1);
    bus.advance_cycles(1);
    assert_eq!(bus.read32(TIMER_BASE).unwrap(), 0x0080_1235);
}

#[test]
fn unknown_io_and_mirrors_stay_unmapped_and_unaligned_writes_have_no_effect() {
    let mut bus = memory();
    for address in [0x0400_0058, TIMER_BASE + 16, IE + 12, IME + 4, 0x0401_0100] {
        assert_eq!(bus.read8(address), Err(MemoryError::Unmapped(address)));
        assert_eq!(
            bus.write32(address, u32::MAX),
            Err(MemoryError::Unmapped(address))
        );
    }
    assert_eq!(
        bus.write32(TIMER_BASE + 2, u32::MAX),
        Err(MemoryError::Unaligned(TIMER_BASE + 2))
    );
    assert_eq!(
        bus.write16(IF + 1, u16::MAX),
        Err(MemoryError::Unaligned(IF + 1))
    );
    assert_eq!(bus.read32(TIMER_BASE).unwrap(), 0);
    assert_eq!(bus.read32(IE).unwrap(), 0);
    assert_eq!(bus.cycles(), 0);
}
