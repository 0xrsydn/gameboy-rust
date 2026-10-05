use gba_core::memory::{Memory, MemoryError, ROM_START};

#[test]
fn halfword_and_word_writes_are_little_endian() {
    let mut memory = Memory::new(vec![]).unwrap();
    for address in [0x0200_0000, 0x0300_0000] {
        memory.write32(address, 0x1234_5678).unwrap();
        assert_eq!(memory.read8(address).unwrap(), 0x78);
        assert_eq!(memory.read16(address).unwrap(), 0x5678);
        assert_eq!(memory.read16(address + 2).unwrap(), 0x1234);
        memory.write16(address + 2, 0xabcd).unwrap();
        assert_eq!(memory.read32(address).unwrap(), 0xabcd_5678);
    }
}

#[test]
fn multi_byte_writes_respect_ram_mirrors_and_region_ends() {
    let mut memory = Memory::new(vec![]).unwrap();
    for (base, size, end) in [
        (0x0200_0000, 0x40000, 0x02ff_ffff),
        (0x0300_0000, 0x8000, 0x03ff_ffff),
    ] {
        memory.write32(end - 3, 0xdead_beef).unwrap();
        assert_eq!(memory.read32(base + size - 4).unwrap(), 0xdead_beef);
        memory.write16(base + size, 0x1234).unwrap();
        assert_eq!(memory.read16(base).unwrap(), 0x1234);
    }
}

#[test]
fn unaligned_bus_writes_fail_without_modifying_memory() {
    let mut memory = Memory::new(vec![]).unwrap();
    let address = 0x0200_0000;
    memory.write32(address, 0xdead_beef).unwrap();
    assert_eq!(
        memory.write16(address + 1, 0),
        Err(MemoryError::Unaligned(address + 1))
    );
    for offset in 1..4 {
        assert_eq!(
            memory.write32(address + offset, 0),
            Err(MemoryError::Unaligned(address + offset))
        );
    }
    assert_eq!(memory.read32(address).unwrap(), 0xdead_beef);
    assert_eq!(
        memory.read16(address + 1),
        Err(MemoryError::Unaligned(address + 1))
    );
}

#[test]
fn rom_and_unmapped_writes_return_errors_without_partial_changes() {
    let mut memory = Memory::new(vec![0x12, 0x34, 0x56, 0x78]).unwrap();
    assert_eq!(
        memory.write16(ROM_START, 0),
        Err(MemoryError::ReadOnly(ROM_START))
    );
    assert_eq!(
        memory.write32(ROM_START, 0),
        Err(MemoryError::ReadOnly(ROM_START))
    );
    assert_eq!(memory.read32(ROM_START).unwrap(), 0x7856_3412);
    for address in [0, 0x0400_0058, 0xffff_fffc] {
        assert_eq!(
            memory.write32(address, 0),
            Err(MemoryError::Unmapped(address))
        );
        assert_eq!(
            memory.write16(address, 0),
            Err(MemoryError::Unmapped(address))
        );
    }
}

#[test]
fn truncated_rom_halfword_read_returns_an_error() {
    let memory = Memory::new(vec![0x12]).unwrap();
    assert_eq!(
        memory.read16(ROM_START),
        Err(MemoryError::Unmapped(ROM_START + 1))
    );
    assert_eq!(
        memory.read16(0xffff_fffe),
        Err(MemoryError::Unmapped(0xffff_fffe))
    );
}
