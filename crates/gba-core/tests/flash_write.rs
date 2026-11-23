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
fn every_byte_value_programs_at_once_without_touching_neighbors_or_other_bank() {
    // Programs complete with their store, as mGBA applies them; host stores commit at once.
    let mut m = bus(SaveDevice::Flash128);
    for b in 0..2 {
        bank(&mut m, b);
        for value in 0..=255 {
            let address = BASE + u32::from(value);
            program(&mut m, address, value);
            assert!(!m.save_write_pending());
            assert_eq!(m.read8(address).unwrap(), value);
            assert_eq!(
                m.save_image().unwrap()[usize::from(b) * 65536 + usize::from(value)],
                value
            );
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
fn data_at_unlock_address_is_not_mistaken_for_reset_and_programs_as_written() {
    let mut m = bus(SaveDevice::Flash64);
    program(&mut m, HI, 0xf0);
    m.advance_cycles(PROGRAM);
    assert_eq!(m.read8(HI).unwrap(), 0xf0);
    program(&mut m, HI, 0x80);
    m.advance_cycles(PROGRAM);
    assert_eq!(m.read8(HI).unwrap(), 0x80);
    // mGBA stores the byte as written, even where hardware would need an erase first.
    program(&mut m, HI, 0x7f);
    assert_eq!(m.read8(HI).unwrap(), 0x7f); // A programmed byte reads back at once.
    m.advance_cycles(PROGRAM);
    assert_eq!(m.read8(HI).unwrap(), 0x7f);
    assert_eq!(m.save_image().unwrap()[0x5555], 0x7f);
}
#[test]
fn busy_erase_reads_and_commands_follow_mgba_and_64_reset_cancels() {
    for device in [SaveDevice::Flash64, SaveDevice::Flash128] {
        let mut m = bus(device);
        m.load_save_image(&vec![0; device.capacity()]).unwrap();
        erase(&mut m, BASE + 0x1000, 0x30);
        // Outside the erasing sector reads see the array; inside, mGBA's busy value.
        assert_eq!(m.read8(BASE + 0x2000).unwrap(), 0);
        assert_eq!(m.read8(BASE + 0x1001).unwrap(), 0x5f);
        if device == SaveDevice::Flash64 {
            // GBATEK's direct reset cancels the busy erase.
            m.write8(HI, 0xf0).unwrap();
            m.advance_cycles(ERASE);
            assert_eq!(m.read8(BASE + 0x1001).unwrap(), 0);
            assert!(!m.save_modified());
        } else {
            // Writes run the command state machine; a program waits for the erase to commit.
            command(&mut m, 0xa0);
            let error = m.write8(BASE + 7, 0x12).unwrap_err();
            assert!(error
                .to_string()
                .contains("before the previous one committed"));
            assert_eq!(m.write8(BASE + 7, 0x12), Err(error));
            m.advance_cycles(ERASE);
            assert_eq!(m.read8(BASE + 0x1001).unwrap(), 0xff);
            m.write8(BASE + 7, 0x12).unwrap(); // The armed program now proceeds.
            assert_eq!(m.read8(BASE + 7).unwrap(), 0x12);
        }
    }
}
#[test]
fn busy_sector_erase_reads_0x5f_inside_the_sector_as_mgba_does() {
    let mut m = bus(SaveDevice::Flash128);
    m.load_save_image(&vec![0x11; 131072]).unwrap();
    erase(&mut m, BASE + 0x3000, 0x30);
    assert_eq!(m.read8(BASE + 0x3000).unwrap(), 0); // DQ7 polling address.
    assert_eq!(m.read8(BASE + 0x3001).unwrap(), 0x5f);
    assert_eq!(m.read8(BASE + 0x3fff).unwrap(), 0x5f);
    assert_eq!(m.read8(BASE + 0x2fff).unwrap(), 0x11);
    assert_eq!(m.read8(BASE + 0x4000).unwrap(), 0x11);
    m.advance_cycles(ERASE);
    assert_eq!(m.read8(BASE + 0x3001).unwrap(), 0xff);
}
#[test]
fn invalid_erase_confirmations_are_ignored_or_stay_armed_as_mgba_does() {
    let mut m = bus(SaveDevice::Flash128);
    m.load_save_image(&vec![0; 131072]).unwrap();
    command(&mut m, 0x80);
    m.write8(LO, 0x55).unwrap(); // Broken prefix: the erase stays armed.
    m.write8(HI, 0xaa).unwrap();
    m.write8(LO, 0x55).unwrap();
    // A bad value away from the unlock address keeps the erase armed.
    for (at, val) in [(BASE, 0x10), (BASE + 1, 0x90)] {
        m.write8(at, val).unwrap();
        assert!(m.save_image().unwrap().iter().all(|&b| b == 0));
        m.write8(HI, 0xaa).unwrap();
        m.write8(LO, 0x55).unwrap();
    }
    // A sector erase at an unaligned address erases the sector holding it.
    m.write8(BASE + 0x1234, 0x30).unwrap();
    assert_eq!(m.read8(BASE + 0x1000).unwrap(), 0); // DQ7 polls the sector base.
    m.advance_cycles(ERASE);
    let image = m.save_image().unwrap();
    assert!(image[0x1000..0x2000].iter().all(|&b| b == 255));
    assert!(image[..0x1000]
        .iter()
        .chain(&image[0x2000..])
        .all(|&b| b == 0));
    // A bad value at the unlock address drops the erase.
    command(&mut m, 0x80);
    m.write8(HI, 0xaa).unwrap();
    m.write8(LO, 0x55).unwrap();
    m.write8(HI, 0x90).unwrap();
    m.write8(HI, 0xaa).unwrap();
    m.write8(LO, 0x55).unwrap();
    m.write8(BASE, 0x30).unwrap(); // Ignored: no erase is armed.
    assert!(!m.save_write_pending());
    assert_eq!(m.read8(BASE).unwrap(), 0);
}
#[test]
fn halt_clocks_nominal_operations_stop_freezes_them_and_setup_resets_dirty_state() {
    let mut m = bus(SaveDevice::Flash128);
    m.load_save_image(&vec![0; 131072]).unwrap();
    erase(&mut m, BASE, 0x30);
    m.write8(HALTCNT, 0).unwrap();
    m.advance_cycles(ERASE);
    assert!(m.save_modified());
    assert_eq!(m.read8(BASE).unwrap(), 0xff);
    m.load_save_image(&vec![0; 131072]).unwrap();
    assert!(!m.save_modified());
    erase(&mut m, BASE, 0x30);
    m.write8(HALTCNT, 0x80).unwrap();
    m.advance_cycles(ERASE);
    assert!(m.save_write_pending());
    assert!(!m.save_modified());
    assert_eq!(m.read8(BASE + 1).unwrap(), 0x5f); // Frozen mid-erase.
}
