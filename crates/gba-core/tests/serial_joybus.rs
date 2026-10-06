//! Original disconnected Joybus probes. No external commands or linked transfers.
use gba_core::{
    cpu::Cpu,
    dma::DMA_BASE,
    io::{HALTCNT, IE, IF, IME, JOYCNT, JOYSTAT, JOY_RECV, JOY_TRANS, RCNT, SIOCNT, SIODATA8},
    machine::{Machine, StepKind},
    memory::{Memory, MemoryError, ROM_START},
};

fn memory() -> Memory {
    Memory::new(0xeaff_fffe_u32.to_le_bytes().to_vec()).unwrap()
}

#[test]
fn local_register_masks_do_not_fabricate_remote_events_in_any_mode() {
    let mut bus = memory();
    for mode in [0, 0x8000, 0xc000, 0xc100, 0x4100] {
        bus.write16(RCNT, mode).unwrap();
        for value in 0..=255 {
            bus.write16(JOYCNT, 0xff00 | value).unwrap();
            bus.write16(JOYSTAT, 0xff00 | value).unwrap();
            assert_eq!(bus.read16(JOYCNT).unwrap(), value & 0x40);
            assert_eq!(bus.read16(JOYSTAT).unwrap(), value & 0x30);
            bus.write8(JOYCNT + 1, 0xff).unwrap();
            bus.write8(JOYSTAT + 1, 0xff).unwrap();
            assert_eq!(bus.read16(JOYCNT).unwrap(), value & 0x40);
            assert_eq!(bus.read16(JOYSTAT).unwrap(), value & 0x30);
        }
        bus.advance_cycles(1000000);
        assert_eq!(bus.read16(IF).unwrap(), 0);
    }
}

#[test]
fn data_lanes_retain_words_and_transmit_writes_set_pending_not_completion() {
    for mode in [0, 0x8000, 0xc000] {
        for base in [JOY_RECV, JOY_TRANS] {
            let mut bus = memory();
            bus.write16(RCNT, mode).unwrap();
            for lane in 0..4 {
                bus.write8(base + lane, (0x12 + lane * 0x22) as u8).unwrap();
            }
            assert_eq!(bus.read32(base).unwrap(), 0x78563412);
            bus.write16(base + 2, 0xabcd).unwrap();
            assert_eq!(bus.read32(base).unwrap(), 0xabcd3412);
            bus.write32(base, 0x98765432).unwrap();
            assert_eq!(bus.read16(base).unwrap(), 0x5432);
            assert_eq!(bus.read16(base + 2).unwrap(), 0x9876);
            let pending = if base == JOY_TRANS { 2 } else { 0 };
            assert_eq!(bus.read16(JOYSTAT).unwrap(), pending);
            bus.write16(JOYSTAT, 0xffff).unwrap();
            assert_eq!(bus.read16(JOYSTAT).unwrap(), pending | 0x30);
            bus.write16(JOYSTAT, 0).unwrap(); // Software cannot clear transmit pending.
            bus.write16(JOYCNT, 0x47).unwrap();
            bus.read32(JOY_RECV).unwrap();
            bus.advance_cycles(1000000);
            assert_eq!(bus.read16(JOYSTAT).unwrap(), pending);
            assert_eq!(bus.read16(JOYCNT).unwrap(), 0x40);
            assert_eq!(bus.read16(IF).unwrap(), 0);
            bus.write16(RCNT, 0x8000).unwrap();
            bus.write16(RCNT, 0xc100).unwrap();
            assert_eq!(bus.read32(base).unwrap(), 0x98765432);
            assert_eq!(bus.read16(JOYSTAT).unwrap(), pending);
        }
    }
    // Writing zero also makes a reply available; zero is not a reset strobe.
    let mut bus = memory();
    bus.write8(JOY_TRANS + 3, 0).unwrap();
    assert_eq!(bus.read16(JOYSTAT).unwrap(), 2);
}

#[test]
fn joybus_does_not_run_normal_clocks_or_wake_halt_or_stop() {
    for stop in [false, true] {
        let mut bus = memory();
        bus.write16(RCNT, 0xc100).unwrap(); // Bit 8 is inactive, not a GPIO IRQ enable.
        bus.write16(SIOCNT, 0x4083).unwrap(); // Unused in Joybus, even with internal start retained.
        bus.write8(SIODATA8, 0x12).unwrap();
        bus.write16(JOYCNT, 0x40).unwrap();
        bus.write32(JOY_TRANS, 0xaabbccdd).unwrap();
        bus.write16(IE, 0x80).unwrap();
        bus.write16(IME, 1).unwrap();
        bus.write8(HALTCNT, if stop { 0x80 } else { 0 }).unwrap();
        bus.advance_cycles(1000000);
        assert_eq!(bus.stopped(), stop);
        assert_eq!(bus.halted(), !stop);
        assert_eq!(bus.read8(SIODATA8).unwrap(), 0x12);
        assert_eq!(bus.read16(SIOCNT).unwrap(), 0x4083);
        assert_eq!(bus.read16(JOYCNT).unwrap(), 0x40);
        assert_eq!(bus.read16(JOYSTAT).unwrap(), 2);
        assert_eq!(bus.read16(IF).unwrap(), 0);
        assert_eq!(bus.read8(RCNT), Err(MemoryError::Unmapped(RCNT)));
    }
}

#[test]
fn live_normal_transfer_requires_completion_or_explicit_cancellation_before_joybus() {
    let mut bus = memory();
    bus.write16(SIOCNT, 0x4083).unwrap();
    for elapsed in [0, 8] {
        bus.advance_cycles(elapsed);
        let error = bus.write16(RCNT, 0xc100).unwrap_err();
        assert!(error.to_string().contains("serial reconfiguration"));
        assert_eq!(bus.write16(RCNT, 0xc100), Err(error));
        assert_eq!(bus.read8(RCNT + 1).unwrap(), 0);
    }
    bus.write16(SIOCNT, 0).unwrap();
    bus.write16(RCNT, 0xc100).unwrap();
    bus.advance_cycles(1000000);
    assert_eq!(bus.read16(IF).unwrap(), 0);
}

#[test]
fn joy_control_and_status_word_padding_fail_without_partial_side_effects() {
    let mut bus = memory();
    for address in [JOYCNT, JOYSTAT] {
        assert_eq!(
            bus.write32(address, u32::MAX),
            Err(MemoryError::Unmapped(address + 2))
        );
        assert_eq!(bus.read32(address), Err(MemoryError::Unmapped(address + 2)));
        assert_eq!(bus.read16(address).unwrap(), 0);
    }
}

#[test]
fn dma_transmit_write_has_only_dma_completion_irq() {
    let mut bus = memory();
    bus.write16(RCNT, 0xc000).unwrap();
    bus.write16(JOYCNT, 0x40).unwrap();
    bus.write32(0x02000000, 0x12345678).unwrap();
    bus.write32(DMA_BASE, 0x02000000).unwrap();
    bus.write32(DMA_BASE + 4, JOY_TRANS).unwrap();
    bus.write32(DMA_BASE + 8, 0xc4000001).unwrap();
    let mut machine = Machine::new(Cpu::new(ROM_START), bus);
    assert_eq!(machine.step().unwrap(), StepKind::Dma { channel: 0 });
    assert_eq!(machine.memory().read32(JOY_TRANS).unwrap(), 0x12345678);
    assert_eq!(machine.memory().read16(JOYSTAT).unwrap(), 2);
    assert_eq!(machine.memory().read16(IF).unwrap(), 0x100);
    machine.memory_mut().advance_cycles(1000000);
    assert_eq!(machine.memory().read16(IF).unwrap(), 0x100);
}
