//! Original nominal serial clocks and disconnected-input regressions.
use gba_core::{
    cpu::Cpu,
    input::Buttons,
    io::{HALTCNT, IE, IF, IME, KEYCNT, RCNT, SIOCNT, SIODATA32, SIODATA8},
    machine::{Machine, StepKind},
    memory::{Memory, ROM_START},
};

fn memory() -> Memory {
    Memory::new(0xeaff_fffe_u32.to_le_bytes().to_vec()).unwrap()
}
fn value(bus: &Memory, wide: bool) -> u32 {
    if wide {
        bus.read32(SIODATA32).unwrap()
    } else {
        u32::from(bus.read8(SIODATA8).unwrap())
    }
}

#[test]
fn every_shift_edge_samples_high_and_completion_occurs_after_exactly_eight_or_thirty_two_bits() {
    for wide in [false, true] {
        for fast in [false, true] {
            for irq in [false, true] {
                for seed in [0, u32::MAX, 0x12345678, 0x80000001] {
                    let mut bus = memory();
                    bus.write32(SIODATA32, seed).unwrap();
                    bus.write8(SIODATA8, seed as u8).unwrap();
                    let control = 0x81
                        | if wide { 0x1000 } else { 0 }
                        | if fast { 2 } else { 0 }
                        | if irq { 0x4000 } else { 0 };
                    bus.write16(SIOCNT, control).unwrap();
                    let width = if wide { 32 } else { 8 };
                    let period = if fast { 8 } else { 64 };
                    let mut bits: Vec<bool> =
                        (0..width).rev().map(|bit| seed & (1 << bit) != 0).collect();
                    for index in 0..width {
                        let expected = bits
                            .iter()
                            .fold(0u32, |word, bit| (word << 1) | u32::from(*bit));
                        bus.advance_cycles(period - 1);
                        assert_eq!(value(&bus, wide), expected);
                        assert_ne!(bus.read16(SIOCNT).unwrap() & 0x80, 0);
                        assert_eq!(bus.read16(IF).unwrap() & 0x80, 0);
                        bus.advance_cycles(1);
                        bits.remove(0);
                        bits.push(true);
                        let expected = bits
                            .iter()
                            .fold(0u32, |word, bit| (word << 1) | u32::from(*bit));
                        assert_eq!(value(&bus, wide), expected);
                        assert_eq!(bus.read16(SIOCNT).unwrap() & 0x80 != 0, index + 1 < width);
                    }
                    assert_eq!(bus.read16(IF).unwrap() & 0x80, if irq { 0x80 } else { 0 });
                    assert_eq!(bus.read16(SIOCNT).unwrap(), (control & !0x80) | 4);
                    if wide {
                        assert_eq!(bus.read8(SIODATA8).unwrap(), seed as u8);
                    } else {
                        assert_eq!(bus.read32(SIODATA32).unwrap(), seed);
                    }
                    bus.write16(IF, 0x80).unwrap();
                    bus.advance_cycles(u32::MAX);
                    assert_eq!(bus.read16(IF).unwrap() & 0x80, 0);
                }
            }
        }
    }
}

#[test]
fn bulk_and_single_cycle_advancement_match_at_every_phase_and_large_batches() {
    for control in [0x4081, 0x4083, 0x5081, 0x5083] {
        for cycles in [0, 1, 7, 8, 63, 64, 65, 255, 256, 511, 512, 2047, 2048, 4096] {
            let mut bulk = memory();
            let mut single = memory();
            for bus in [&mut bulk, &mut single] {
                bus.write32(SIODATA32, 0x12345678).unwrap();
                bus.write8(SIODATA8, 0x96).unwrap();
                bus.write16(SIOCNT, control).unwrap();
            }
            bulk.advance_cycles(cycles);
            for _ in 0..cycles {
                single.advance_cycles(1);
            }
            for address in [SIOCNT, SIODATA8, IF] {
                assert_eq!(bulk.read16(address), single.read16(address));
            }
            assert_eq!(bulk.read32(SIODATA32), single.read32(SIODATA32));
        }
    }
}

#[test]
fn external_to_internal_transition_starts_a_fresh_period_and_same_start_writes_preserve_phase() {
    let mut bus = memory();
    bus.write16(SIOCNT, 0x5080).unwrap();
    bus.advance_cycles(1000000);
    bus.write8(SIOCNT, 0x81).unwrap();
    bus.advance_cycles(63);
    assert_eq!(bus.read32(SIODATA32).unwrap(), 0);
    bus.write16(SIOCNT, 0x5085).unwrap(); // Read/modify/write includes read-only SI.
    bus.advance_cycles(1);
    assert_eq!(bus.read32(SIODATA32).unwrap(), 1);
    bus.advance_cycles(64 * 31 - 1);
    assert_ne!(bus.read16(SIOCNT).unwrap() & 0x80, 0);
    bus.advance_cycles(1);
    assert_eq!(bus.read32(SIODATA32).unwrap(), u32::MAX);
    assert_eq!(bus.read16(IF).unwrap() & 0x80, 0x80);
}

#[test]
fn cancellation_allows_reconfiguration_and_irq_enable_is_sampled_at_completion() {
    let mut bus = memory();
    bus.write16(SIOCNT, 0x81).unwrap();
    bus.advance_cycles(65);
    assert_eq!(bus.read8(SIODATA8).unwrap(), 1);
    let before = bus.read16(SIOCNT).unwrap();
    for (address, data) in [
        (SIOCNT, 0x83),
        (SIOCNT + 1, 0x10),
        (SIODATA8, 0x42),
        (RCNT + 1, 0x80),
    ] {
        let error = bus.write8(address, data).unwrap_err();
        assert!(error.to_string().contains("serial reconfiguration"));
        assert_eq!(bus.read16(SIOCNT).unwrap(), before);
        assert_eq!(bus.read8(SIODATA8).unwrap(), 1);
    }
    bus.write8(SIOCNT + 1, 0x40).unwrap(); // IRQ enable does not restart the current bit.
    bus.advance_cycles(447);
    assert_eq!(bus.read16(IF).unwrap() & 0x80, 0x80);
    bus.write16(IF, 0x80).unwrap();
    bus.write16(SIOCNT, 0x4083).unwrap();
    bus.advance_cycles(8);
    bus.write32(SIOCNT, 0x00aa0001).unwrap(); // Cancel and replace data atomically.
    bus.advance_cycles(1000000);
    assert_eq!(bus.read8(SIODATA8).unwrap(), 0xaa);
    assert_eq!(bus.read16(IF).unwrap() & 0x80, 0);
    bus.write16(SIOCNT, 0x4083).unwrap();
    bus.advance_cycles(63);
    bus.write8(SIOCNT + 1, 0).unwrap(); // Disable IRQ one cycle before completion.
    bus.advance_cycles(1);
    assert_eq!(bus.read8(SIODATA8).unwrap(), 0xff);
    assert_eq!(bus.read16(IF).unwrap() & 0x80, 0);
}

#[test]
fn gpio_gates_the_clock_and_leaving_normal_mode_requires_cancellation() {
    let mut bus = memory();
    bus.write16(RCNT, 0x8000).unwrap();
    bus.write16(SIOCNT, 0x4083).unwrap();
    bus.advance_cycles(1000000);
    assert_eq!(bus.read8(SIODATA8).unwrap(), 0);
    assert_eq!(bus.read16(IF).unwrap(), 0);
    bus.write16(RCNT, 0).unwrap();
    assert!(bus.write16(RCNT, 0x8000).is_err());
    bus.advance_cycles(7);
    assert_eq!(bus.read8(SIODATA8).unwrap(), 0);
    bus.advance_cycles(1);
    assert_eq!(bus.read8(SIODATA8).unwrap(), 1);
    bus.write16(SIOCNT, 0).unwrap();
    bus.write16(RCNT, 0x8000).unwrap();
    bus.advance_cycles(1000000);
    assert_eq!(bus.read8(SIODATA8).unwrap(), 1);
    assert_eq!(bus.read16(IF).unwrap(), 0);
}

#[test]
fn halt_stops_at_serial_completion_and_delivery_obeys_ime() {
    for ime in [0, 1] {
        let mut bus = memory();
        bus.write16(IE, 0x80).unwrap();
        bus.write16(IME, ime).unwrap();
        bus.write16(SIOCNT, 0x4083).unwrap();
        bus.write8(HALTCNT, 0).unwrap();
        let mut machine = Machine::new(Cpu::new(ROM_START), bus);
        assert_eq!(machine.step().unwrap(), StepKind::HaltIdle);
        assert_eq!(machine.cycles(), 64);
        assert!(!machine.halted());
        assert_eq!(machine.memory().read16(IF).unwrap(), 0x80);
        assert_eq!(
            machine.step().unwrap(),
            if ime == 0 {
                StepKind::Instruction
            } else {
                StepKind::IrqEntry
            }
        );
    }
}

#[test]
fn serial_and_timer_requests_latch_and_acknowledge_independently() {
    let mut bus = memory();
    bus.write32(gba_core::io::TIMER_BASE, 0x00c0ffc0).unwrap();
    bus.write16(SIOCNT, 0x4083).unwrap();
    bus.advance_cycles(64);
    assert_eq!(bus.read16(IF).unwrap(), 0x88);
    bus.write16(IF, 0x80).unwrap();
    assert_eq!(bus.read16(IF).unwrap(), 8);
    bus.write16(IF, 8).unwrap();
    assert_eq!(bus.read16(IF).unwrap(), 0);
    bus.write16(gba_core::io::TIMER_BASE + 2, 0).unwrap();
    bus.write16(SIOCNT, 0x5081).unwrap();
    bus.advance_cycles(u32::MAX);
    assert_eq!(bus.read32(SIODATA32).unwrap(), u32::MAX);
    assert_eq!(bus.read16(IF).unwrap(), 0x80);
}

#[test]
fn stop_freezes_a_partial_bit_until_keypad_wake() {
    let mut bus = memory();
    bus.write16(IE, 0x1080).unwrap();
    bus.write16(KEYCNT, 0x4001).unwrap();
    bus.write16(SIOCNT, 0x4083).unwrap();
    bus.advance_cycles(63);
    bus.write8(HALTCNT, 0x80).unwrap();
    let mut machine = Machine::new(Cpu::new(ROM_START), bus);
    assert_eq!(machine.step().unwrap(), StepKind::StopIdle);
    machine.memory_mut().advance_cycles(u32::MAX);
    assert_eq!(machine.cycles(), 63);
    assert_eq!(machine.memory().read8(SIODATA8).unwrap(), 0x7f);
    assert_eq!(machine.memory().read16(IF).unwrap() & 0x80, 0);
    machine.memory_mut().set_buttons(Buttons::from_bits(1));
    assert!(!machine.stopped());
    machine.memory_mut().advance_cycles(1);
    assert_eq!(machine.memory().read8(SIODATA8).unwrap(), 0xff);
    assert_eq!(machine.memory().read16(IF).unwrap() & 0x80, 0x80);
}
