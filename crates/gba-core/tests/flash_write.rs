//! Original byte-program/erase probes. Nominal delays are not physical chip timings.
use gba_core::{
    cartridge::{
        SaveDevice, FLASH_ERASE_CYCLES as ERASE, FLASH_PROGRAM_CYCLES as PROGRAM,
        SAVE_START as BASE,
    },
    io::{HALTCNT, IF},
    memory::Memory,
};
const HI: u32 = BASE + 0x5555;
const LO: u32 = BASE + 0x2aaa;
fn bus(device: SaveDevice) -> Memory {
    let mut m = Memory::new(vec![]).unwrap();
    m.set_save_device(device);
    m
}
fn command(m: &mut Memory, command: u8) {
    m.write8(HI, 0xaa).unwrap();
    m.write8(LO, 0x55).unwrap();
    m.write8(HI, command).unwrap();
}
fn program(m: &mut Memory, address: u32, value: u8) {
    command(m, 0xa0);
    m.write8(address, value).unwrap();
}
fn erase(m: &mut Memory, address: u32, confirm: u8) {
    command(m, 0x80);
    m.write8(HI, 0xaa).unwrap();
    m.write8(LO, 0x55).unwrap();
    m.write8(address, confirm).unwrap();
}
fn bank(m: &mut Memory, index: u8) {
    command(m, 0xb0);
    m.write8(BASE, index).unwrap();
}
#[test]
fn every_byte_value_programs_after_delay_without_touching_neighbors_or_other_bank() {
    let mut m = bus(SaveDevice::Flash128);
    for b in 0..2 {
        bank(&mut m, b);
        for value in 0..=255 {
            let address = BASE + u32::from(value);
            program(&mut m, address, value);
            assert!(m.save_write_pending());
            assert_eq!(m.read8(address).unwrap(), (value ^ 0x80) & 0x80);
            assert_eq!(
                m.save_image().unwrap()[usize::from(b) * 65536 + usize::from(value)],
                255
            );
            m.advance_cycles(PROGRAM - 1);
            assert_eq!(m.read8(address).unwrap(), (value ^ 0x80) & 0x80);
            m.advance_cycles(1);
            assert_eq!(m.read8(address).unwrap(), value);
            assert!(!m.save_write_pending());
        }
    }
    let image = m.save_image().unwrap();
    for b in 0..2 {
        for (i, &v) in image[b * 65536..(b + 1) * 65536].iter().enumerate() {
            assert_eq!(v, if i < 256 { i as u8 } else { 255 });
        }
    }
    assert!(m.save_modified());
    assert_eq!(m.read16(IF).unwrap(), 0);
}
#[test]
fn sector_and_chip_erases_have_exact_scope_and_wait_for_completion() {
    for device in [SaveDevice::Flash64, SaveDevice::Flash128] {
        let mut m = bus(device);
        m.load_save_image(&vec![0; device.capacity()]).unwrap();
        if device == SaveDevice::Flash128 {
            bank(&mut m, 1);
        }
        erase(&mut m, BASE + 0xf000, 0x30);
        assert_eq!(m.read8(BASE + 0xf000).unwrap(), 0);
        m.advance_cycles(ERASE - 1);
        assert!(m.save_image().unwrap().iter().all(|&b| b == 0));
        m.advance_cycles(1);
        let start = device.capacity() - 4096;
        assert!(m.save_image().unwrap()[..start].iter().all(|&b| b == 0));
        assert!(m.save_image().unwrap()[start..].iter().all(|&b| b == 255));
        erase(&mut m, HI, 0x10);
        m.advance_cycles(ERASE);
        assert!(m.save_image().unwrap().iter().all(|&b| b == 255));
    }
}
#[test]
fn data_at_unlock_address_is_not_mistaken_for_reset_and_zero_to_one_requires_erase() {
    let mut m = bus(SaveDevice::Flash64);
    program(&mut m, HI, 0xf0);
    m.advance_cycles(PROGRAM);
    assert_eq!(m.read8(HI).unwrap(), 0xf0);
    program(&mut m, HI, 0x80);
    m.advance_cycles(PROGRAM);
    assert_eq!(m.read8(HI).unwrap(), 0x80);
    command(&mut m, 0xa0);
    let error = m.write8(HI, 0xff).unwrap_err();
    assert!(error.to_string().contains("requires erase"));
    assert_eq!(m.write8(HI, 0xff), Err(error));
    m.write8(HI, 0).unwrap();
    m.advance_cycles(PROGRAM);
    assert_eq!(m.read8(HI).unwrap(), 0);
}
#[test]
fn busy_access_limits_and_64_reset_are_atomic_without_fabricated_completion() {
    for device in [SaveDevice::Flash64, SaveDevice::Flash128] {
        let mut m = bus(device);
        program(&mut m, BASE + 123, 0x12);
        assert!(m.read8(BASE + 124).is_err());
        let error = m.write8(HI, 0xaa).unwrap_err();
        assert_eq!(m.write8(HI, 0xaa), Err(error));
        if device == SaveDevice::Flash64 {
            m.write8(HI, 0xf0).unwrap();
            m.advance_cycles(PROGRAM);
            assert_eq!(m.read8(BASE + 123).unwrap(), 255);
            assert!(!m.save_modified());
        } else {
            assert!(m.write8(HI, 0xf0).is_err());
            m.advance_cycles(PROGRAM);
            assert_eq!(m.read8(BASE + 123).unwrap(), 0x12);
        }
    }
}
#[test]
fn invalid_erase_sequences_preserve_data_and_can_be_retried() {
    let mut m = bus(SaveDevice::Flash128);
    m.load_save_image(&vec![0; 131072]).unwrap();
    command(&mut m, 0x80);
    assert!(m.write8(LO, 0x55).is_err());
    m.write8(HI, 0xaa).unwrap();
    m.write8(LO, 0x55).unwrap();
    for (at, val) in [(BASE + 1, 0x30), (BASE, 0x10), (HI, 0x90)] {
        let err = m.write8(at, val).unwrap_err();
        assert_eq!(m.write8(at, val), Err(err));
        assert!(m.save_image().unwrap().iter().all(|&b| b == 0));
    }
    m.write8(BASE, 0x30).unwrap();
    m.advance_cycles(ERASE);
    assert!(m.save_image().unwrap()[..4096].iter().all(|&b| b == 255));
}
#[test]
fn halt_clocks_nominal_operations_stop_freezes_them_and_setup_resets_dirty_state() {
    let mut m = bus(SaveDevice::Flash128);
    program(&mut m, BASE, 0);
    m.write8(HALTCNT, 0).unwrap();
    m.advance_cycles(PROGRAM);
    assert!(m.save_modified());
    assert_eq!(m.read8(BASE).unwrap(), 0);
    m.load_save_image(&vec![255; 131072]).unwrap();
    assert!(!m.save_modified());
    program(&mut m, BASE, 1);
    m.write8(HALTCNT, 0x80).unwrap();
    m.advance_cycles(PROGRAM);
    assert!(m.save_write_pending());
    assert!(!m.save_modified());
    assert_eq!(m.read8(BASE).unwrap(), 0x80);
}
