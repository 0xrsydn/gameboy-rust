//! Original EEPROM bitstreams sent through DMA3; no game assets or save files.
use gba_core::{
    cartridge::{SaveDevice, EEPROM_WRITE_CYCLES},
    cpu::Cpu,
    dma::DMA_BASE,
    machine::{Machine, MachineError, StepKind},
    memory::{Memory, MemoryError, ROM_START},
};

const EEPROM: u32 = 0x0d00_0000;
const BUFFER: u32 = 0x0200_0000;
const DMA3: u32 = DMA_BASE + 0x24;

fn words(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

/// A ROM that polls EEPROM status with LDRH r1,[r0] and then loops.
fn machine(device: SaveDevice) -> Machine {
    let mut bus = Memory::new(words(&[0xe59f_0004, 0xe1d0_10b0, 0xeaff_fffe, EEPROM])).unwrap();
    bus.set_save_device(device);
    let mut cpu = Cpu::new(ROM_START);
    cpu.step(&mut bus).unwrap();
    Machine::new(cpu, bus)
}

fn address_bits(device: SaveDevice) -> u32 {
    if device == SaveDevice::Eeprom8k {
        14
    } else {
        6
    }
}

/// Run one immediate halfword DMA3 transfer to completion.
fn dma(machine: &mut Machine, source: u32, destination: u32, count: u32) {
    let bus = machine.memory_mut();
    bus.write32(DMA3, source).unwrap();
    bus.write32(DMA3 + 4, destination).unwrap();
    bus.write32(DMA3 + 8, 0x8000_0000 | count).unwrap();
    for _ in 0..count {
        assert_eq!(machine.step().unwrap(), StepKind::Dma { channel: 3 });
    }
    assert_eq!(machine.memory().read16(DMA3 + 10).unwrap() & 0x8000, 0);
}

/// Store `bits` MSB first as one halfword per bit and send them to the EEPROM.
fn send(machine: &mut Machine, bits: &[u8]) {
    for (index, &bit) in bits.iter().enumerate() {
        let value = u16::from(bit) | 0xfffe; // Bits 1-15 are don't care.
        machine
            .memory_mut()
            .write16(BUFFER + index as u32 * 2, value)
            .unwrap();
    }
    dma(machine, BUFFER, EEPROM, bits.len() as u32);
}

fn field(value: u64, width: u32) -> Vec<u8> {
    (0..width)
        .rev()
        .map(|shift| ((value >> shift) & 1) as u8)
        .collect()
}

fn write_request(device: SaveDevice, block: u64, data: u64) -> Vec<u8> {
    [
        vec![1, 0],
        field(block, address_bits(device)),
        field(data, 64),
        vec![0],
    ]
    .concat()
}

fn read_block(machine: &mut Machine, device: SaveDevice, block: u64) -> u64 {
    send(
        machine,
        &[vec![1, 1], field(block, address_bits(device)), vec![0]].concat(),
    );
    let output = BUFFER + 0x400;
    dma(machine, EEPROM, output, 68);
    let mut value = 0;
    for index in 0..68 {
        let bit = machine.memory().read16(output + index * 2).unwrap();
        assert!(bit <= 1);
        if index < 4 {
            assert_eq!(bit, 0, "dummy bit {index}");
        } else {
            value = (value << 1) | u64::from(bit);
        }
    }
    value
}

fn poll(machine: &mut Machine) -> u32 {
    let mut probe = machine.cpu().clone();
    probe.step(machine.memory_mut()).unwrap();
    probe.registers()[1]
}

#[test]
fn dma_writes_program_after_the_nominal_delay_and_reads_return_the_block() {
    for (device, block) in [(SaveDevice::Eeprom512, 0x3f), (SaveDevice::Eeprom8k, 0x3ff)] {
        let mut machine = machine(device);
        assert_eq!(poll(&mut machine), 1); // Idle reads as ready.
        send(
            &mut machine,
            &write_request(device, block, 0x0123_4567_89ab_cdef),
        );
        assert_eq!(poll(&mut machine), 0); // Busy.
        assert!(machine.memory().save_write_pending());
        machine.memory_mut().advance_cycles(EEPROM_WRITE_CYCLES);
        assert_eq!(poll(&mut machine), 1);
        assert!(!machine.memory().save_write_pending());
        assert!(machine.memory().save_modified());
        let offset = block as usize * 8;
        assert_eq!(
            machine.memory().save_image().unwrap()[offset..offset + 8],
            0x0123_4567_89ab_cdef_u64.to_be_bytes()
        );
        assert_eq!(
            read_block(&mut machine, device, block),
            0x0123_4567_89ab_cdef
        );
        assert_eq!(read_block(&mut machine, device, 0), u64::MAX); // Erased.
    }
}

#[test]
fn large_parts_decode_only_the_low_ten_address_bits() {
    let device = SaveDevice::Eeprom8k;
    let mut machine = machine(device);
    send(&mut machine, &write_request(device, 0x3c01, 7));
    machine.memory_mut().advance_cycles(EEPROM_WRITE_CYCLES);
    assert_eq!(read_block(&mut machine, device, 1), 7);
}

#[test]
fn cpu_stores_and_byte_or_word_accesses_are_diagnostics() {
    let mut machine = machine(SaveDevice::Eeprom512);
    let bus = machine.memory_mut();
    assert!(matches!(
        bus.read8(EEPROM),
        Err(MemoryError::UnsupportedCartridgeAccess { .. })
    ));
    assert!(matches!(
        bus.read32(EEPROM),
        Err(MemoryError::UnsupportedCartridgeAccess { .. })
    ));
    assert!(matches!(
        bus.write32(EEPROM, 1),
        Err(MemoryError::UnsupportedCartridgeAccess { .. })
    ));
    // STRH r0,[r0] from ROM code.
    let mut bus = Memory::new(words(&[0xe59f_0004, 0xe1c0_00b0, 0xeaff_fffe, EEPROM])).unwrap();
    bus.set_save_device(SaveDevice::Eeprom512);
    let mut cpu = Cpu::new(ROM_START);
    cpu.step(&mut bus).unwrap();
    let mut machine = Machine::new(cpu, bus);
    let before = machine.cpu().clone();
    assert!(matches!(
        machine.step(),
        Err(MachineError::Cpu(gba_core::cpu::CpuError::Memory(
            MemoryError::UnsupportedIo { .. }
        )))
    ));
    assert_eq!(machine.cpu(), &before);
    assert!(!machine.memory().save_write_pending());
}

#[test]
fn malformed_streams_fail_without_partial_state() {
    let mut machine = machine(SaveDevice::Eeprom512);
    machine.memory_mut().write16(BUFFER, 0).unwrap(); // Missing leading 1 bit.
    let bus = machine.memory_mut();
    bus.write32(DMA3, BUFFER).unwrap();
    bus.write32(DMA3 + 4, EEPROM).unwrap();
    bus.write32(DMA3 + 8, 0x8000_0001).unwrap();
    assert!(matches!(machine.step(), Err(MachineError::Dma(_))));
    assert!(!machine.memory().save_write_pending());
}

#[test]
fn roms_above_16_mib_map_eeprom_only_in_the_top_256_bytes() {
    let mut rom = vec![0; 16 * 1024 * 1024 + 4];
    rom[..4].copy_from_slice(&0xeaff_fffe_u32.to_le_bytes());
    let mut bus = Memory::new(rom).unwrap();
    bus.set_save_device(SaveDevice::Eeprom512);
    assert_eq!(bus.read16(0x0dff_ff00).unwrap(), 1);
    assert_eq!(bus.read16(0x0d00_0000).unwrap(), 0); // Ordinary ROM bytes.
    let mut machine = Machine::new(Cpu::new(ROM_START), bus);
    machine.memory_mut().write16(BUFFER, 1).unwrap();
    let bus = machine.memory_mut();
    bus.write32(DMA3, BUFFER).unwrap();
    bus.write32(DMA3 + 4, 0x0d00_0000).unwrap();
    bus.write32(DMA3 + 8, 0x8000_0001).unwrap();
    assert!(matches!(machine.step(), Err(MachineError::Dma(_))));
}

#[test]
fn save_states_restore_a_read_out_in_progress() {
    let device = SaveDevice::Eeprom512;
    let mut machine = machine(device);
    send(
        &mut machine,
        &write_request(device, 2, 0xa5a5_a5a5_a5a5_a5a5),
    );
    machine.memory_mut().advance_cycles(EEPROM_WRITE_CYCLES);
    send(&mut machine, &[vec![1, 1], field(2, 6), vec![0]].concat());
    dma(&mut machine, EEPROM, BUFFER + 0x400, 5); // Four dummy bits and the first data bit.
    let state = machine.save_state().unwrap();
    dma(&mut machine, EEPROM, BUFFER + 0x400, 63);
    let finished = machine.memory().read16(BUFFER + 0x400 + 62 * 2).unwrap();
    machine.load_state(&state).unwrap();
    assert!(machine.memory().save_write_pending());
    dma(&mut machine, EEPROM, BUFFER + 0x400, 63);
    assert_eq!(
        machine.memory().read16(BUFFER + 0x400 + 62 * 2).unwrap(),
        finished
    );
    assert!(!machine.memory().save_write_pending());
}
