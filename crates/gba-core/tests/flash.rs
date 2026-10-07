//! Original Flash command sequences and supplied save bytes; no game assets or save files.
use gba_core::{
    cartridge::{CartridgeHardware, SaveDevice, SaveError, SAVE_END, SAVE_START},
    cpu::Cpu,
    dma::DMA_BASE,
    io::{HALTCNT, IE, IF, IME},
    machine::Machine,
    memory::{Memory, MemoryError, ROM_START},
};

const HI: u32 = SAVE_START + 0x5555;
const LO: u32 = SAVE_START + 0x2aaa;
fn memory(device: SaveDevice) -> Memory {
    let mut bus = Memory::new(0xeafffffeu32.to_le_bytes().to_vec()).unwrap();
    bus.set_save_device(device);
    bus
}
fn command(bus: &mut Memory, byte: u8) -> Result<(), MemoryError> {
    bus.write8(HI, 0xaa)?;
    bus.write8(LO, 0x55)?;
    bus.write8(HI, byte)
}
fn bank(bus: &mut Memory, number: u8) {
    command(bus, 0xb0).unwrap();
    bus.write8(SAVE_START, number).unwrap();
}
fn image(device: SaveDevice) -> Vec<u8> {
    (0..device.capacity())
        .map(|i| ((i >> 16) * 73 + i * 19 + (i >> 8)) as u8)
        .collect()
}

#[test]
fn selection_erased_state_and_exact_image_loading_are_explicit() {
    let mut bus = memory(SaveDevice::None);
    assert_eq!(bus.save_image(), None);
    assert_eq!(
        bus.read8(SAVE_START),
        Err(MemoryError::Unmapped(SAVE_START))
    );
    assert_eq!(bus.write8(HI, 0xaa), Err(MemoryError::Unmapped(HI)));
    assert_eq!(bus.load_save_image(&[]), Err(SaveError::NoDevice));
    for device in [SaveDevice::Flash64, SaveDevice::Flash128] {
        bus.set_save_device(device);
        assert_eq!(bus.save_image().unwrap(), vec![255; device.capacity()]);
        assert_eq!(bus.read8(SAVE_START).unwrap(), 255);
        assert_eq!(bus.read8(SAVE_END).unwrap(), 255);
        let bytes = image(device);
        bus.load_save_image(&bytes).unwrap();
        command(&mut bus, 0x90).unwrap();
        for len in [0, device.capacity() - 1, device.capacity() + 1] {
            assert_eq!(
                bus.load_save_image(&vec![0; len]),
                Err(SaveError::InvalidSize {
                    expected: device.capacity(),
                    actual: len
                })
            );
            assert_eq!(bus.save_image().unwrap(), bytes);
            assert_eq!(bus.read8(SAVE_START).unwrap(), 0xc2); // Invalid load did not reset ID state.
        }
        bus.load_save_image(&bytes).unwrap(); // Valid host load resets command/bank state.
        assert_eq!(bus.read8(SAVE_START).unwrap(), bytes[0]);
        assert_eq!(bus.read32(ROM_START).unwrap(), 0xeafffffe);
    }
}

#[test]
fn manufacturer_and_device_id_are_separate_bytes_and_exit_preserves_data() {
    for (device, id) in [(SaveDevice::Flash64, 0x1c), (SaveDevice::Flash128, 9)] {
        let mut bus = memory(device);
        let data = image(device);
        bus.load_save_image(&data).unwrap();
        command(&mut bus, 0x90).unwrap();
        assert_eq!(bus.read8(SAVE_START).unwrap(), 0xc2);
        assert_eq!(bus.read8(SAVE_START + 1).unwrap(), id);
        assert!(bus
            .read8(SAVE_START + 2)
            .unwrap_err()
            .to_string()
            .contains("ID read outside"));
        command(&mut bus, 0xf0).unwrap();
        assert_eq!(bus.read8(SAVE_START).unwrap(), data[0]);
        assert_eq!(bus.read8(SAVE_START + 1).unwrap(), data[1]);
        assert_eq!(bus.save_image().unwrap(), data);
        assert_eq!(bus.read16(IF).unwrap(), 0);
    }
}

#[test]
fn bank_selection_covers_every_byte_and_survives_id_mode() {
    let mut bus = memory(SaveDevice::Flash128);
    let data = image(SaveDevice::Flash128);
    bus.load_save_image(&data).unwrap();
    for selected in [1, 0, 1] {
        bank(&mut bus, selected);
        for offset in 0..=0xffff {
            assert_eq!(
                bus.read8(SAVE_START + offset).unwrap(),
                data[selected as usize * 0x10000 + offset as usize]
            );
        }
        command(&mut bus, 0x90).unwrap();
        assert_eq!(bus.read8(SAVE_START).unwrap(), 0xc2);
        assert_eq!(bus.read8(SAVE_START + 1).unwrap(), 9);
        command(&mut bus, 0xf0).unwrap();
        assert_eq!(
            bus.read8(SAVE_START).unwrap(),
            data[selected as usize * 0x10000]
        );
    }
    let mut small = memory(SaveDevice::Flash64);
    assert!(command(&mut small, 0xb0)
        .unwrap_err()
        .to_string()
        .contains("64 KiB"));
    small.write8(HI, 0xf0).unwrap();
    assert_eq!(small.save_image().unwrap().len(), 0x10000);
}

#[test]
fn unsupported_commands_and_bad_bank_values_are_retryable_without_data_changes() {
    for device in [SaveDevice::Flash64, SaveDevice::Flash128] {
        for (byte, message) in [
            (0x42, "Flash command"),
            (0x30, "Flash command"),
            (0x10, "Flash command"),
        ] {
            let mut bus = memory(device);
            let data = image(device);
            bus.load_save_image(&data).unwrap();
            let error = command(&mut bus, byte).unwrap_err();
            assert!(error.to_string().contains(message));
            assert_eq!(bus.write8(HI, byte), Err(error));
            assert_eq!(bus.save_image().unwrap(), data);
            bus.write8(HI, 0xf0).unwrap(); // Finish the already accepted unlock sequence.
            assert_eq!(bus.read8(SAVE_START).unwrap(), data[0]);
            assert_eq!(bus.read16(IF).unwrap(), 0);
        }
    }
    let mut bus = memory(SaveDevice::Flash128);
    let data = image(SaveDevice::Flash128);
    bus.load_save_image(&data).unwrap();
    command(&mut bus, 0xb0).unwrap();
    for (address, value) in [(SAVE_START, 2), (SAVE_START, 255), (SAVE_START + 1, 1)] {
        let error = bus.write8(address, value).unwrap_err();
        assert_eq!(bus.write8(address, value), Err(error));
        assert_eq!(bus.read8(SAVE_START).unwrap(), data[0]);
    }
    bus.write8(SAVE_START, 1).unwrap();
    assert_eq!(bus.read8(SAVE_START).unwrap(), data[0x10000]);
}

#[test]
fn widths_aliases_and_invalid_unlocks_remain_diagnostic() {
    let mut bus = memory(SaveDevice::Flash128);
    for address in [SAVE_START - 1, SAVE_END + 1, 0x0f000000, 0x0eff5555] {
        assert_eq!(bus.read8(address), Err(MemoryError::Unmapped(address)));
    }
    assert!(bus
        .read16(SAVE_START)
        .unwrap_err()
        .to_string()
        .contains("non-byte read"));
    assert!(bus
        .read32(SAVE_START)
        .unwrap_err()
        .to_string()
        .contains("non-byte read"));
    assert!(bus
        .write16(HI & !1, 0xaa)
        .unwrap_err()
        .to_string()
        .contains("non-byte write"));
    assert!(bus
        .write32(SAVE_START, 0)
        .unwrap_err()
        .to_string()
        .contains("non-byte write"));
    for (address, value) in [(HI, 0x55), (LO, 0xaa)] {
        let error = bus.write8(address, value).unwrap_err();
        assert_eq!(bus.write8(address, value), Err(error));
    }
    bus.write8(HI, 0xaa).unwrap();
    assert!(bus.write8(LO + 1, 0x55).is_err());
    bus.write8(LO, 0x55).unwrap();
    assert!(bus.write8(HI + 1, 0x90).is_err());
    bus.write8(HI, 0x90).unwrap();
    assert_eq!(bus.read8(SAVE_START).unwrap(), 0xc2);
}

#[test]
fn redundant_128_reset_preserves_array_bank_but_does_not_exit_id_or_partial_unlock() {
    let mut bus = memory(SaveDevice::Flash128);
    let bytes = image(SaveDevice::Flash128);
    bus.load_save_image(&bytes).unwrap();
    bank(&mut bus, 1);
    bus.write8(HI, 0xf0).unwrap(); // Already idle in array mode.
    assert_eq!(bus.read8(SAVE_START).unwrap(), bytes[0x10000]);
    command(&mut bus, 0x90).unwrap();
    assert!(bus.write8(HI, 0xf0).is_err()); // Direct ID exit for this device is not established.
    assert_eq!(bus.read8(SAVE_START).unwrap(), 0xc2);
    command(&mut bus, 0xf0).unwrap();
    bus.write8(HI, 0xf0).unwrap(); // Redundant reset after documented unlocked ID exit.
    assert_eq!(bus.read8(SAVE_START).unwrap(), bytes[0x10000]);
    bus.write8(HI, 0xaa).unwrap();
    assert!(bus.write8(HI, 0xf0).is_err());
    bus.write8(LO, 0x55).unwrap();
    bus.write8(HI, 0x90).unwrap();
    assert_eq!(bus.save_image().unwrap(), bytes);
}

#[test]
fn macronix_64_direct_reset_and_gpio_selection_are_independent() {
    let mut bus = memory(SaveDevice::Flash64);
    bus.set_cartridge_hardware(CartridgeHardware::Rtc);
    bus.advance_rtc_seconds(42).unwrap();
    let rtc = bus.rtc_datetime();
    command(&mut bus, 0x90).unwrap();
    bus.write8(HI, 0xf0).unwrap();
    assert_eq!(bus.read8(SAVE_START).unwrap(), 255);
    bus.write8(HI, 0xaa).unwrap();
    bus.write8(HI, 0xf0).unwrap(); // Abort partial unlock for this documented device.
    command(&mut bus, 0x90).unwrap();
    bus.set_cartridge_hardware(CartridgeHardware::None);
    assert_eq!(bus.read8(SAVE_START).unwrap(), 0xc2);
    bus.set_cartridge_hardware(CartridgeHardware::Rtc);
    bus.set_rtc_datetime(rtc.unwrap()).unwrap();
    bus.set_save_device(SaveDevice::Flash128);
    assert_eq!(bus.rtc_datetime(), rtc);
    bus.set_save_device(SaveDevice::None);
    assert_eq!(bus.save_image(), None);
    assert_eq!(bus.rtc_datetime(), rtc);
}

#[test]
fn halt_stop_and_dma_do_not_create_a_flash_completion_or_change_data() {
    for stop in [false, true] {
        let mut bus = memory(SaveDevice::Flash128);
        command(&mut bus, 0x90).unwrap();
        bus.write16(IE, 0x2000).unwrap();
        bus.write16(IME, 1).unwrap();
        bus.write8(HALTCNT, if stop { 128 } else { 0 }).unwrap();
        let mut machine = Machine::new(Cpu::new(ROM_START), bus);
        for _ in 0..20 {
            machine.step().unwrap();
        }
        assert_eq!(machine.memory().read8(SAVE_START).unwrap(), 0xc2);
        assert_eq!(machine.memory().read16(IF).unwrap(), 0);
        assert_eq!(machine.halted(), !stop);
        assert_eq!(machine.stopped(), stop);
    }
    for source in [false, true] {
        let mut bus = memory(SaveDevice::Flash128);
        let dma = DMA_BASE + 36;
        bus.write32(0x02000000, 0xaa).unwrap();
        bus.write32(dma, if source { SAVE_START } else { 0x02000000 })
            .unwrap();
        bus.write32(dma + 4, if source { 0x02000010 } else { SAVE_START })
            .unwrap();
        bus.write32(dma + 8, 0xc0000001).unwrap();
        let mut machine = Machine::new(Cpu::new(ROM_START), bus);
        let error = machine.step().unwrap_err();
        assert!(error.to_string().contains("Flash non-byte"));
        assert_eq!(machine.step(), Err(error));
        assert_eq!(machine.cycles(), 0);
        assert_eq!(machine.memory().read32(0x02000010).unwrap(), 0);
        assert_eq!(machine.memory().read16(IF).unwrap(), 0);
        assert!(machine
            .memory()
            .save_image()
            .unwrap()
            .iter()
            .all(|&x| x == 255));
    }
}
