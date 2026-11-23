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
        // mGBA reads the array beyond the two ID bytes.
        assert_eq!(bus.read8(SAVE_START + 2).unwrap(), data[2]);
        assert_eq!(bus.read8(SAVE_END).unwrap(), data[0xffff]);
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
    // A 64 KiB chip accepts the bank command and bank 0. mGBA grows the chip on bank 1;
    // the selected image size is fixed here, so that stays diagnostic and retryable.
    let mut small = memory(SaveDevice::Flash64);
    command(&mut small, 0xb0).unwrap();
    let error = small.write8(SAVE_START, 1).unwrap_err();
    assert!(error.to_string().contains("bank 1 on 64 KiB"));
    assert_eq!(small.write8(SAVE_START, 1), Err(error));
    small.write8(SAVE_START, 0).unwrap();
    assert_eq!(small.read8(SAVE_START).unwrap(), 0xff);
    assert_eq!(small.save_image().unwrap().len(), 0x10000);
}

#[test]
fn unknown_commands_and_bad_bank_values_are_ignored_as_mgba_does() {
    for device in [SaveDevice::Flash64, SaveDevice::Flash128] {
        for byte in [0x42, 0x30, 0x10, 0xf0] {
            let mut bus = memory(device);
            let data = image(device);
            bus.load_save_image(&data).unwrap();
            command(&mut bus, byte).unwrap();
            assert_eq!(bus.save_image().unwrap(), data);
            assert_eq!(bus.read8(SAVE_START).unwrap(), data[0]);
            command(&mut bus, 0x90).unwrap(); // No unlock prefix was kept.
            assert_eq!(bus.read8(SAVE_START).unwrap(), 0xc2);
            assert_eq!(bus.read16(IF).unwrap(), 0);
        }
    }
    let mut bus = memory(SaveDevice::Flash128);
    let data = image(SaveDevice::Flash128);
    bus.load_save_image(&data).unwrap();
    for (address, value) in [(SAVE_START, 2), (SAVE_START, 255), (SAVE_START + 1, 1)] {
        command(&mut bus, 0xb0).unwrap();
        bus.write8(address, value).unwrap(); // Ignored; the bank command ends.
        assert_eq!(bus.read8(SAVE_START).unwrap(), data[0]);
    }
    bus.write8(SAVE_START, 1).unwrap(); // Plain array write without a command.
    assert_eq!(bus.read8(SAVE_START).unwrap(), data[0]);
    bank(&mut bus, 1);
    assert_eq!(bus.read8(SAVE_START).unwrap(), data[0x10000]);
    assert_eq!(bus.save_image().unwrap(), data);
}

#[test]
fn id_mode_ignores_commands_other_than_exit() {
    let mut bus = memory(SaveDevice::Flash128);
    let data = image(SaveDevice::Flash128);
    bus.load_save_image(&data).unwrap();
    command(&mut bus, 0x90).unwrap();
    for byte in [0xa0, 0x80, 0xb0, 0x90, 0x42] {
        command(&mut bus, byte).unwrap();
        bus.write8(SAVE_START, 1).unwrap(); // Neither program data nor a bank number.
        assert_eq!(bus.read8(SAVE_START).unwrap(), 0xc2);
    }
    command(&mut bus, 0xf0).unwrap();
    assert_eq!(bus.read8(SAVE_START).unwrap(), data[0]);
    assert!(!bus.save_write_pending());
    assert_eq!(bus.save_image().unwrap(), data);
}

#[test]
fn host_widths_stay_diagnostic_and_the_window_mirrors_through_the_save_region() {
    let mut bus = memory(SaveDevice::Flash128);
    assert_eq!(
        bus.read8(SAVE_START - 1),
        Err(MemoryError::Unmapped(SAVE_START - 1))
    );
    // Only A0-A15 reach the chip (jsmolka's Flash "Mirror" tests).
    for address in [SAVE_END + 1, 0x0f000000, 0x0eff5555, 0x0fff_ffff] {
        assert_eq!(
            bus.read8(address),
            bus.read8(SAVE_START | (address & 0xffff)),
            "{address:#x}"
        );
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
}

#[test]
fn broken_unlock_prefixes_are_ignored_and_start_over_as_mgba_does() {
    let mut bus = memory(SaveDevice::Flash128);
    for (address, value) in [(HI, 0x55), (LO, 0xaa)] {
        bus.write8(address, value).unwrap();
    }
    bus.write8(HI, 0xaa).unwrap();
    bus.write8(LO + 1, 0x55).unwrap(); // Breaks the prefix.
    bus.write8(LO, 0x55).unwrap();
    bus.write8(HI, 0x90).unwrap();
    assert_eq!(bus.read8(SAVE_START).unwrap(), 0xff);
    bus.write8(HI, 0xaa).unwrap();
    bus.write8(LO, 0x55).unwrap();
    bus.write8(HI + 1, 0x90).unwrap(); // Wrong command address.
    bus.write8(HI, 0x90).unwrap();
    assert_eq!(bus.read8(SAVE_START).unwrap(), 0xff);
    command(&mut bus, 0x90).unwrap();
    assert_eq!(bus.read8(SAVE_START).unwrap(), 0xc2);
    assert!(bus.save_image().unwrap().iter().all(|&b| b == 0xff));
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
    bus.write8(HI, 0xf0).unwrap(); // Ignored, as in mGBA: no direct ID exit on this device.
    assert_eq!(bus.read8(SAVE_START).unwrap(), 0xc2);
    command(&mut bus, 0xf0).unwrap();
    bus.write8(HI, 0xf0).unwrap(); // Redundant reset after documented unlocked ID exit.
    assert_eq!(bus.read8(SAVE_START).unwrap(), bytes[0x10000]);
    bus.write8(HI, 0xaa).unwrap();
    bus.write8(HI, 0xf0).unwrap(); // Breaks the partial unlock instead of resetting it.
    bus.write8(LO, 0x55).unwrap();
    bus.write8(HI, 0x90).unwrap();
    assert_eq!(bus.read8(SAVE_START).unwrap(), bytes[0x10000]);
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
        machine.step().unwrap();
        // DMA3 moves one byte over the 8-bit save bus. A source read repeats the erased
        // byte on both halfword lanes; a store of 0xaa outside a command is ignored.
        assert_eq!(
            machine.memory().read32(0x02000010).unwrap(),
            if source { 0xffff } else { 0 }
        );
        assert_eq!(machine.memory().read16(IF).unwrap(), 0x800);
        assert!(!machine.memory().save_write_pending());
        assert!(machine
            .memory()
            .save_image()
            .unwrap()
            .iter()
            .all(|&x| x == 255));
    }
}

/// Load r0.. from literals, run `body`, then loop. Returns the machine after `body`.
fn run_program(device: SaveDevice, image: &[u8], literals: &[u32], body: &[u32]) -> Machine {
    // Every `LDR rN, [pc, #imm]` reaches its literal with the same offset.
    let offset = 4 * (literals.len() + body.len() + 1) as u32 - 8;
    let mut rom: Vec<u32> = (0..literals.len() as u32)
        .map(|n| 0xe59f_0000 | n << 12 | offset)
        .collect();
    rom.extend(body);
    rom.push(0xeaff_fffe);
    rom.extend(literals);
    let bytes = rom.iter().flat_map(|w| w.to_le_bytes()).collect();
    let mut bus = Memory::new(bytes).unwrap();
    bus.set_save_device(device);
    bus.load_save_image(image).unwrap();
    let mut machine = Machine::new(Cpu::new(ROM_START), bus);
    for _ in 0..literals.len() + body.len() {
        machine.step().unwrap();
    }
    machine
}

// Flash shares SRAM's 8-bit save bus: wide CPU loads repeat the addressed byte, as
// mGBA 0.10.5's GBALoad16/GBALoad32 do for the save region.
#[test]
fn cpu_wide_loads_repeat_the_addressed_flash_byte_then_rotate() {
    let mut data = image(SaveDevice::Flash128);
    data[..4].copy_from_slice(b"Game");
    for (instruction, offset, value) in [
        (0xe591_2000, 0, 0x4747_4747), // LDR r2,[r1]
        (0xe591_2000, 1, 0x6161_6161),
        (0xe1d1_20b0, 0, 0x4747), // LDRH r2,[r1]
        (0xe1d1_20b0, 1, 0x6100_0061),
        (0xe1d1_20f0, 1, 0x61), // Odd LDRSH loads a signed byte.
    ] {
        let machine = run_program(
            SaveDevice::Flash128,
            &data,
            &[0, SAVE_START + offset],
            &[instruction],
        );
        assert_eq!(machine.cpu().registers()[2], value, "{instruction:#010x}");
    }
    // ID mode answers wide loads the same way.
    let machine = run_program(
        SaveDevice::Flash128,
        &data,
        &[HI, 0xaaaa, LO, 0x5555, 0x9090, SAVE_START + 1],
        &[
            0xe1c0_10b0, // STRH r1,[r0]: odd address, high byte 0xaa
            0xe1c2_30b0, // STRH r3,[r2]: even address, low byte 0x55
            0xe1c0_40b0, // STRH r4,[r0]: 0x90
            0xe595_6000, // LDR r6,[r5]
            0xe1d5_70b0, // LDRH r7,[r5]
        ],
    );
    assert_eq!(machine.cpu().registers()[6], 0x0909_0909);
    assert_eq!(machine.cpu().registers()[7], 0x0900_0009);
}

// Wide stores write the address's byte lane, as SRAM's do. mGBA instead issues a byte
// store of the low byte at each address of the halfword or word; for the aligned stores
// games use, the extra bytes are ignored writes and both program the same byte.
#[test]
fn cpu_wide_stores_program_the_address_byte_lane() {
    let data = vec![0xff; SaveDevice::Flash64.capacity()];
    let machine = run_program(
        SaveDevice::Flash64,
        &data,
        &[HI, 0xaaaa, LO, 0x5555, 0xa0a0, SAVE_START + 6, 0x1234_5678],
        &[
            0xe1c0_10b0, // STRH r1,[r0]
            0xe1c2_30b0, // STRH r3,[r2]
            0xe1c0_40b0, // STRH r4,[r0]
            0xe585_6000, // STR r6,[r5]: lane 2 of the word
        ],
    );
    // The program commits with the storing step, as mGBA applies it.
    assert!(!machine.memory().save_write_pending());
    let saved = machine.memory().save_image().unwrap();
    assert_eq!(&saved[4..8], &[0xff, 0xff, 0x34, 0xff]);
    assert!(saved[..4].iter().chain(&saved[8..]).all(|&b| b == 0xff));
}
