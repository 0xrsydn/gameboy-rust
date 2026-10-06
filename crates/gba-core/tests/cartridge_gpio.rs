//! Original GPIO and RTC transactions. No game code or host clock.
#[path = "cartridge_gpio/calendar.rs"]
mod calendar;
use gba_core::{
    cartridge::{CartridgeHardware, GPIO_CONTROL, GPIO_DATA, GPIO_DIRECTION},
    cpu::Cpu,
    dma::DMA_BASE,
    io::IF,
    machine::{Machine, StepKind},
    memory::{Memory, MemoryError, ROM_START},
};

fn memory() -> Memory {
    let mut rom = vec![0xa5; 0x200];
    rom[..4].copy_from_slice(&0xeafffffeu32.to_le_bytes());
    let mut bus = Memory::new(rom).unwrap();
    bus.set_cartridge_hardware(CartridgeHardware::Rtc);
    bus
}

fn start(bus: &mut Memory) {
    bus.write16(GPIO_DATA, 1).unwrap();
    bus.write16(GPIO_DIRECTION, 7).unwrap();
    bus.write16(GPIO_DATA, 5).unwrap();
}

fn send_bits(bus: &mut Memory, bits: impl IntoIterator<Item = bool>) -> Result<(), MemoryError> {
    for bit in bits {
        let data = 4 | (u16::from(bit) << 1);
        bus.write16(GPIO_DATA, data)?;
        bus.write16(GPIO_DATA, data | 1)?;
    }
    Ok(())
}

fn command(bus: &mut Memory, byte: u8, msb_first: bool) -> Result<(), MemoryError> {
    // GBATEK's Fwd and Rev representations describe the same physical wire sequence.
    let byte = if msb_first { byte } else { byte.reverse_bits() };
    send_bits(
        bus,
        (0..8).map(|i| byte & (1 << if msb_first { 7 - i } else { i }) != 0),
    )
}

fn parameter(bus: &mut Memory, byte: u8) -> Result<(), MemoryError> {
    send_bits(bus, (0..8).map(|i| byte & (1 << i) != 0))
}

fn control(bus: &mut Memory, msb_first: bool) -> u8 {
    start(bus);
    command(bus, 0x63, msb_first).unwrap();
    bus.write16(GPIO_DIRECTION, 5).unwrap();
    let mut value = 0;
    for bit in 0..8 {
        bus.write16(GPIO_DATA, 4).unwrap();
        let low = bus.read16(GPIO_DATA).unwrap();
        bus.write16(GPIO_DATA, 5).unwrap();
        let high = bus.read16(GPIO_DATA).unwrap();
        assert_eq!(low & 2, high & 2); // Output changes on falling, not rising, edges.
        assert_eq!(bus.read16(GPIO_DATA).unwrap(), high); // Reads do not shift.
        value |= ((high as u8 >> 1) & 1) << bit;
    }
    bus.write16(GPIO_DATA, 1).unwrap();
    value
}

#[test]
fn explicit_selection_overlay_and_disable_never_patch_rom_bytes() {
    let mut bus = memory();
    for address in [GPIO_DATA, GPIO_DIRECTION, GPIO_CONTROL] {
        assert_eq!(bus.read16(address).unwrap(), 0xa5a5);
    }
    bus.write16(GPIO_CONTROL, 0xffff).unwrap();
    assert_eq!(bus.read16(GPIO_CONTROL).unwrap(), 1);
    assert_eq!(bus.read16(GPIO_DATA).unwrap(), 2); // Idle SIO high; no host time.
    assert_eq!(bus.read16(GPIO_DIRECTION).unwrap(), 0);
    assert_eq!(bus.read32(GPIO_CONTROL).unwrap(), 0xa5a50001);
    assert_eq!(bus.read8(GPIO_DATA + 1).unwrap(), 0);
    for address in [
        GPIO_DATA + 0x02000000,
        GPIO_DATA + 0x04000000,
        GPIO_DATA + 0x01000000,
    ] {
        assert!(bus.write16(address, 0).is_err()); // No inferred mirror-write behavior.
    }
    for base in [0x0a000000, 0x0c000000] {
        assert_eq!(bus.read16(base + 0xc4).unwrap(), 0xa5a5);
    }
    bus.write16(GPIO_CONTROL, 0xfffe).unwrap();
    assert_eq!(bus.read16(GPIO_DATA).unwrap(), 0xa5a5);
    bus.set_cartridge_hardware(CartridgeHardware::None);
    assert_eq!(
        bus.write16(GPIO_CONTROL, 1),
        Err(MemoryError::ReadOnly(GPIO_CONTROL))
    );
    assert_eq!(bus.read16(GPIO_CONTROL).unwrap(), 0xa5a5);
}

#[test]
fn masks_directions_latches_and_access_boundaries() {
    let mut bus = memory();
    bus.write16(GPIO_CONTROL, 1).unwrap();
    bus.write16(GPIO_DATA, 0xfffb).unwrap(); // CS low; data writes latch even on inputs.
    assert_eq!(bus.read16(GPIO_DATA).unwrap(), 2);
    bus.write16(GPIO_DIRECTION, 0xfffb).unwrap();
    assert_eq!(bus.read16(GPIO_DIRECTION).unwrap(), 11);
    assert_eq!(bus.read16(GPIO_DATA).unwrap(), 11);
    bus.write32(GPIO_DATA, 0x00030002).unwrap(); // Two halfwords: data, then direction.
    assert_eq!(bus.read32(GPIO_DATA).unwrap(), 0x00030002);
    assert!(bus
        .write8(GPIO_DATA, 0)
        .unwrap_err()
        .to_string()
        .contains("GPIO byte write"));
    assert_eq!(
        bus.write16(GPIO_DATA + 1, 0),
        Err(MemoryError::Unaligned(GPIO_DATA + 1))
    );
    assert_eq!(
        bus.write32(GPIO_CONTROL, 0),
        Err(MemoryError::ReadOnly(GPIO_CONTROL + 2))
    );
    assert_eq!(bus.read16(GPIO_CONTROL).unwrap(), 1); // No partial disable.
    assert_eq!(
        bus.write16(GPIO_DATA - 2, 0),
        Err(MemoryError::ReadOnly(GPIO_DATA - 2))
    );
}

#[test]
fn control_read_write_reset_and_both_command_representations() {
    for msb in [false, true] {
        let mut bus = memory();
        bus.write16(GPIO_CONTROL, 1).unwrap();
        assert_eq!(control(&mut bus, msb), 0x40);
        for reset in [0x60, 0x61] {
            // Either access direction strobes reset.
            start(&mut bus);
            command(&mut bus, reset, msb).unwrap();
            bus.write16(GPIO_DATA, 1).unwrap();
            assert_eq!(control(&mut bus, msb), 0);
            start(&mut bus);
            command(&mut bus, 0x62, msb).unwrap();
            parameter(&mut bus, 0xd5).unwrap(); // Unused and read-only bits do not stick.
            bus.write16(GPIO_DATA, 1).unwrap();
            assert_eq!(control(&mut bus, msb), 0x40);
        }
        bus.advance_cycles(1000000);
        assert_eq!(bus.read16(IF).unwrap(), 0);
    }
}

#[test]
fn disabled_read_overlay_does_not_disable_serial_writes() {
    let mut bus = memory();
    start(&mut bus);
    command(&mut bus, 0x62, true).unwrap();
    parameter(&mut bus, 0).unwrap();
    bus.write16(GPIO_DATA, 1).unwrap();
    assert_eq!(bus.read16(GPIO_CONTROL).unwrap(), 0xa5a5);
    bus.write16(GPIO_CONTROL, 1).unwrap();
    assert_eq!(control(&mut bus, true), 0);
}

#[test]
fn dropping_select_aborts_partial_commands_and_parameters() {
    let mut bus = memory();
    bus.write16(GPIO_CONTROL, 1).unwrap();
    for bits in 0..8 {
        start(&mut bus);
        send_bits(&mut bus, (0..bits).map(|_| false)).unwrap();
        bus.write16(GPIO_DATA, 1).unwrap();
        assert_eq!(control(&mut bus, true), 0x40);
        start(&mut bus);
        command(&mut bus, 0x62, true).unwrap();
        send_bits(&mut bus, (0..bits).map(|_| false)).unwrap();
        bus.write16(GPIO_DATA, 1).unwrap();
        assert_eq!(control(&mut bus, true), 0x40);
    }
}

#[test]
fn unsupported_commands_and_control_bits_fail_at_the_last_edge_without_partial_writes() {
    for (byte, description) in [
        (0x6c, "force interrupt"),
        (0x68, "unused command"),
        (0xff, "command encoding"),
        (0xc6, "command encoding"), // Do not accept a reversed wire prefix as another format.
    ] {
        let mut bus = memory();
        bus.write16(GPIO_CONTROL, 1).unwrap();
        start(&mut bus);
        let error = command(&mut bus, byte, true).unwrap_err();
        assert!(error.to_string().contains(description), "{error}");
        let pins = bus.read16(GPIO_DATA).unwrap();
        assert_eq!(pins & 1, 0); // Failing rising edge was not committed.
        assert_eq!(bus.write16(GPIO_DATA, pins | 1), Err(error));
        bus.write16(GPIO_DATA, 1).unwrap();
        assert_eq!(control(&mut bus, true), 0x40);
        assert_eq!(bus.read16(IF).unwrap(), 0);
    }
    for value in [2, 8, 32, 0xff] {
        let mut bus = memory();
        bus.write16(GPIO_CONTROL, 1).unwrap();
        start(&mut bus);
        command(&mut bus, 0x62, true).unwrap();
        assert!(parameter(&mut bus, value)
            .unwrap_err()
            .to_string()
            .contains("control bits"));
        bus.write16(GPIO_DATA, 1).unwrap();
        assert_eq!(control(&mut bus, true), 0x40);
    }
}

#[test]
fn unsupported_edges_and_word_second_halfword_are_atomic() {
    let mut bus = memory();
    bus.write16(GPIO_CONTROL, 1).unwrap();
    // Data writes latch CS high while all pins are inputs. The following direction halfword
    // would select with SCK low, so neither halfword of this word may commit.
    assert!(bus.write32(GPIO_DATA, 0x00070004).is_err());
    assert_eq!(bus.read32(GPIO_DATA).unwrap(), 2);
    bus.write16(GPIO_DIRECTION, 7).unwrap();
    assert_eq!(bus.read16(GPIO_DATA).unwrap(), 0); // Failed word did not retain latch=4.
    bus.write16(GPIO_DATA, 1).unwrap();
    bus.write16(GPIO_DATA, 5).unwrap();
    bus.write16(GPIO_DATA, 4).unwrap();
    assert!(bus
        .write16(GPIO_DATA, 7)
        .unwrap_err()
        .to_string()
        .contains("sampling edge"));
    assert_eq!(bus.read16(GPIO_DATA).unwrap(), 4);
    bus.write16(GPIO_DATA, 5).unwrap(); // Same stable data is retryable.
}

#[test]
fn dma3_word_store_uses_gpio_without_modifying_rom_or_raising_a_cartridge_irq() {
    let mut bus = memory();
    bus.write16(GPIO_CONTROL, 1).unwrap();
    bus.write32(0x02000000, 0x00030002).unwrap();
    let dma = DMA_BASE + 36;
    bus.write32(dma, 0x02000000).unwrap();
    bus.write32(dma + 4, GPIO_DATA).unwrap();
    bus.write32(dma + 8, 0xc4000001).unwrap();
    let mut machine = Machine::new(Cpu::new(ROM_START), bus);
    assert_eq!(machine.step().unwrap(), StepKind::Dma { channel: 3 });
    assert_eq!(machine.memory().read32(GPIO_DATA).unwrap(), 0x00030002);
    assert_eq!(machine.memory().read16(IF).unwrap(), 0x800);
    machine.memory_mut().write16(GPIO_CONTROL, 0).unwrap();
    assert_eq!(machine.memory().read32(GPIO_DATA).unwrap(), 0xa5a5a5a5);
}
