use super::*;
use gba_core::{
    cartridge::{RtcDateTime, RtcError},
    io::{HALTCNT, IE, IME},
};

fn setup(parts: [u8; 7]) -> Memory {
    let mut bus = memory();
    bus.write16(GPIO_CONTROL, 1).unwrap();
    bus.set_rtc_datetime(RtcDateTime::new(parts).unwrap())
        .unwrap();
    bus
}
fn read_byte(bus: &mut Memory) -> u8 {
    let mut byte = 0;
    for bit in 0..8 {
        bus.write16(GPIO_DATA, 4).unwrap();
        let data = bus.read16(GPIO_DATA).unwrap() & 2;
        bus.write16(GPIO_DATA, 5).unwrap();
        assert_eq!(bus.read16(GPIO_DATA).unwrap() & 2, data);
        byte |= (data as u8 >> 1) << bit;
    }
    byte
}
fn read(bus: &mut Memory, cmd: u8, len: usize) -> Vec<u8> {
    start(bus);
    command(bus, cmd, true).unwrap();
    bus.write16(GPIO_DIRECTION, 5).unwrap();
    let bytes = (0..len).map(|_| read_byte(bus)).collect();
    bus.write16(GPIO_DATA, 1).unwrap();
    bytes
}
fn write(bus: &mut Memory, cmd: u8, bytes: &[u8]) -> Result<(), MemoryError> {
    start(bus);
    command(bus, cmd, true)?;
    for &byte in bytes {
        parameter(bus, byte)?;
    }
    bus.write16(GPIO_DATA, 1)
}

#[test]
fn calendar_and_time_reads_use_bcd_and_gba_pm_bit() {
    let mut bus = setup([24, 2, 29, 4, 23, 58, 59]);
    assert_eq!(
        read(&mut bus, 0x65, 7),
        [0x24, 2, 0x29, 4, 0xa3, 0x58, 0x59]
    );
    assert_eq!(read(&mut bus, 0x67, 3), [0xa3, 0x58, 0x59]);
    write(&mut bus, 0x62, &[0]).unwrap(); // 12-hour presentation preserves the actual hour.
    assert_eq!(read(&mut bus, 0x67, 3), [0x91, 0x58, 0x59]);
    bus.advance_rtc_seconds(61).unwrap();
    assert_eq!(read(&mut bus, 0x65, 7), [0x24, 3, 1, 5, 0, 0, 0]);
    assert_eq!(bus.read16(IF).unwrap(), 0);
}

#[test]
fn complete_writes_commit_calendar_or_time_and_mask_unused_bits() {
    let mut bus = setup([0, 1, 1, 0, 0, 0, 0]);
    write(&mut bus, 0x64, &[0x24, 0xe2, 0xe9, 0xfc, 0x63, 0xd8, 0xd9]).unwrap();
    assert_eq!(
        bus.rtc_datetime().unwrap().components(),
        [24, 2, 29, 4, 23, 58, 59]
    );
    write(&mut bus, 0x66, &[0x80, 0x12, 0x34]).unwrap(); // PM ignored in 24-hour writes.
    assert_eq!(
        bus.rtc_datetime().unwrap().components(),
        [24, 2, 29, 4, 0, 12, 34]
    );
    write(&mut bus, 0x62, &[0]).unwrap();
    write(&mut bus, 0x66, &[0x80, 0, 0]).unwrap(); // Noon is 00 with PM, not 12.
    assert_eq!(
        bus.rtc_datetime().unwrap().components(),
        [24, 2, 29, 4, 12, 0, 0]
    );
    assert_eq!(read(&mut bus, 0x67, 3), [0x80, 0, 0]);
}

#[test]
fn read_snapshot_survives_mid_transfer_tick_and_host_reseed() {
    let mut bus = setup([24, 12, 31, 2, 23, 59, 59]);
    start(&mut bus);
    command(&mut bus, 0x65, true).unwrap();
    bus.write16(GPIO_DIRECTION, 5).unwrap();
    assert_eq!(read_byte(&mut bus), 0x24);
    bus.advance_rtc_seconds(1).unwrap();
    assert_eq!(
        bus.rtc_datetime().unwrap().components(),
        [25, 1, 1, 3, 0, 0, 0]
    );
    bus.set_rtc_datetime(RtcDateTime::new([50, 6, 7, 1, 8, 9, 10]).unwrap())
        .unwrap();
    let tail: Vec<_> = (0..6).map(|_| read_byte(&mut bus)).collect();
    assert_eq!(tail, [0x12, 0x31, 2, 0xa3, 0x59, 0x59]);
    bus.write16(GPIO_DATA, 1).unwrap();
    assert_eq!(read(&mut bus, 0x65, 7), [0x50, 6, 7, 1, 8, 9, 0x10]);
}

#[test]
fn partial_payloads_abort_without_calendar_changes_and_time_writes_preserve_live_date() {
    let original = [24, 2, 28, 6, 23, 59, 59];
    for bits in 0..56 {
        let mut bus = setup(original);
        start(&mut bus);
        command(&mut bus, 0x64, true).unwrap();
        send_bits(&mut bus, (0..bits).map(|_| false)).unwrap();
        bus.write16(GPIO_DATA, 1).unwrap();
        assert_eq!(bus.rtc_datetime().unwrap().components(), original);
    }
    let mut bus = setup(original);
    start(&mut bus);
    command(&mut bus, 0x66, true).unwrap();
    parameter(&mut bus, 0x12).unwrap();
    bus.advance_rtc_seconds(1).unwrap();
    parameter(&mut bus, 0x34).unwrap();
    parameter(&mut bus, 0x56).unwrap();
    bus.write16(GPIO_DATA, 1).unwrap();
    assert_eq!(
        bus.rtc_datetime().unwrap().components(),
        [24, 2, 29, 0, 12, 34, 56]
    );
}

#[test]
fn invalid_calendar_payload_is_diagnostic_retryable_and_cannot_commit() {
    for payload in [
        [0xfa, 1, 1, 0, 0, 0, 0],
        [0x23, 2, 0x29, 0, 0, 0, 0],
        [0, 0, 1, 0, 0, 0, 0],
        [0, 1, 1, 7, 0, 0, 0],
        [0, 1, 1, 0, 0x24, 0, 0],
        [0, 1, 1, 0, 0, 0x60, 0],
        [0, 1, 1, 0, 0, 0, 0x6a],
    ] {
        let mut bus = setup([24, 5, 6, 3, 7, 8, 9]);
        let before = bus.rtc_datetime();
        let error = write(&mut bus, 0x64, &payload).unwrap_err();
        assert!(error.to_string().contains("invalid calendar data"));
        let pins = bus.read16(GPIO_DATA).unwrap();
        assert_eq!(pins & 1, 0);
        assert_eq!(bus.write16(GPIO_DATA, pins | 1), Err(error));
        assert_eq!(bus.rtc_datetime(), before);
        bus.write16(GPIO_DATA, 1).unwrap();
        assert_eq!(read(&mut bus, 0x65, 7), [0x24, 5, 6, 3, 7, 8, 9]);
    }
}

#[test]
fn reset_affects_calendar_and_mode_but_not_the_gpio_overlay() {
    for cmd in [0x60, 0x61] {
        let mut bus = setup([99, 12, 31, 6, 23, 59, 59]);
        write(&mut bus, cmd, &[]).unwrap();
        assert_eq!(bus.rtc_datetime(), Some(RtcDateTime::default()));
        assert_eq!(control(&mut bus, true), 0);
        assert_eq!(read(&mut bus, 0x65, 7), [0, 1, 1, 0, 0, 0, 0]);
        assert_eq!(bus.read16(GPIO_CONTROL).unwrap(), 1);
        bus.advance_rtc_seconds(43200).unwrap();
        assert_eq!(read(&mut bus, 0x67, 3), [0x80, 0, 0]);
    }
}

#[test]
fn battery_clock_is_independent_of_cpu_cycles_halt_stop_and_device_presence() {
    let mut absent = Memory::new(vec![0; 4]).unwrap();
    assert_eq!(absent.rtc_datetime(), None);
    assert_eq!(
        absent.set_rtc_datetime(RtcDateTime::default()),
        Err(RtcError::NotAttached)
    );
    assert_eq!(absent.advance_rtc_seconds(0), Err(RtcError::NotAttached));
    for stop in [false, true] {
        let mut bus = setup([0, 1, 1, 0, 0, 0, 0]);
        bus.write16(IE, 0x2080).unwrap();
        bus.write16(IME, 1).unwrap();
        bus.write8(HALTCNT, if stop { 128 } else { 0 }).unwrap();
        let mut machine = Machine::new(Cpu::new(ROM_START), bus);
        for _ in 0..20 {
            machine.step().unwrap();
        }
        machine.memory_mut().advance_cycles(16777216);
        assert_eq!(
            machine.memory().rtc_datetime(),
            Some(RtcDateTime::default())
        );
        let cycles = machine.cycles();
        machine.memory_mut().advance_rtc_seconds(86401).unwrap();
        assert_eq!(
            machine.memory().rtc_datetime().unwrap().components(),
            [0, 1, 2, 1, 0, 0, 1]
        );
        assert_eq!(machine.cycles(), cycles);
        assert_eq!(machine.memory().read16(IF).unwrap(), 0);
        assert_eq!(machine.halted(), !stop);
        assert_eq!(machine.stopped(), stop);
    }
}
