//! Original bus/CPU/DMA regressions for the disabled-device initialization subset.
use gba_core::{
    cpu::Cpu,
    dma::DMA_BASE,
    io::{JOYCNT, RCNT, SIOCNT, SIODATA32, SIODATA8, SOUNDBIAS, SOUNDCNT_H, SOUNDCNT_X, WAVE_RAM},
    machine::Machine,
    memory::{Memory, MemoryError, ROM_START},
};

fn memory() -> Memory {
    Memory::new(vec![0xfe, 0xff, 0xff, 0xea]).unwrap()
}

#[test]
fn disabled_sound_registers_masks_lanes_and_wave_ram() {
    let mut bus = memory();
    for address in (0x04000060..0x04000082).step_by(2) {
        bus.write16(address, 0xffff).unwrap();
        assert_eq!(bus.read16(address).unwrap(), 0);
    }
    bus.write16(SOUNDCNT_H, 0xffff).unwrap();
    assert_eq!(bus.read16(SOUNDCNT_H).unwrap(), 0x770f); // Reset strobes do not latch.
    bus.write8(SOUNDCNT_H, 0).unwrap();
    assert_eq!(bus.read16(SOUNDCNT_H).unwrap(), 0x7700);
    bus.write32(SOUNDBIAS, u32::MAX).unwrap();
    assert_eq!(bus.read32(SOUNDBIAS).unwrap(), 0xc3fe);
    for index in 0..16 {
        bus.write8(WAVE_RAM + index, index as u8 * 13).unwrap();
    }
    bus.write32(SOUNDCNT_X, 0x7f).unwrap(); // Status bits are read-only; master stays off.
    assert_eq!(bus.read32(SOUNDCNT_X).unwrap(), 0);
    for index in 0..16 {
        assert_eq!(bus.read8(WAVE_RAM + index).unwrap(), index as u8 * 13);
    }
    assert_eq!(bus.read16(SOUNDCNT_H).unwrap(), 0x7700);
    assert_eq!(bus.read16(SOUNDBIAS).unwrap(), 0xc3fe);
    // FIFOs are not ordinary register latches.
    bus.write32(0x040000a0, 0).unwrap();
    assert!(bus.read32(0x040000a0).is_err());
}

#[test]
fn disconnected_gpio_inputs_pull_high_and_outputs_follow_latches() {
    let mut bus = memory();
    bus.write16(RCNT, 0x8000).unwrap();
    assert_eq!(bus.read16(RCNT).unwrap(), 0x800f);
    bus.write16(RCNT, 0x80f5).unwrap();
    assert_eq!(bus.read16(RCNT).unwrap(), 0x80f5);
    bus.write8(RCNT, 0x20).unwrap(); // SD output low; SC/SI/SO pulled high.
    assert_eq!(bus.read16(RCNT).unwrap(), 0x802d);
    bus.write8(RCNT, 0x22).unwrap();
    assert_eq!(bus.read16(RCNT).unwrap(), 0x802f);
    bus.write16(RCNT, 0).unwrap();
    // Idle normal mode, as the mGBA suite measured: SC and SI high, SD and SO low.
    assert_eq!(bus.read16(RCNT).unwrap(), 0x0005);
}

#[test]
fn idle_normal_serial_data_and_control_have_distinct_widths() {
    let mut bus = memory();
    bus.write32(SIODATA32, 0x12345678).unwrap();
    assert_eq!(bus.read32(SIODATA32).unwrap(), 0); // Readable only in 32-bit mode.
    bus.write16(SIODATA8, 0xabcd).unwrap();
    assert_eq!(bus.read16(SIODATA8).unwrap(), 0xabcd);
    bus.write16(SIOCNT, 0x500b).unwrap();
    assert_eq!(bus.read16(SIOCNT).unwrap(), 0x500f); // Disconnected SI is high.
    assert_eq!(bus.read32(SIODATA32).unwrap(), 0x12345678);
    bus.write16(SIODATA32, 0).unwrap();
    assert_eq!(bus.read32(SIODATA32).unwrap(), 0x12340000);
    bus.advance_cycles(1000000);
    assert_eq!(bus.read16(SIOCNT).unwrap(), 0x500f); // No fabricated completion or IRQ.
    assert_eq!(bus.read16(0x04000202).unwrap() & 0x80, 0);
    bus.write16(JOYCNT, 7).unwrap(); // Clear empty status flags.
    assert_eq!(bus.read16(JOYCNT).unwrap(), 0);
}

#[test]
fn unsupported_operations_have_specific_retryable_diagnostics() {
    let mut bus = memory();
    // GPIO-interrupt configuration alone is accepted; SI is pulled high, so there is no
    // edge until it is driven low.
    bus.write16(RCNT, 0x8100).unwrap();
    let error = bus.write8(RCNT, 0x40).unwrap_err();
    assert!(matches!(error, MemoryError::UnsupportedIo { .. }));
    assert!(error
        .to_string()
        .contains("GPIO serial interrupt with SI low"));
    assert_eq!(bus.write8(RCNT, 0x40), Err(error));
    bus.write16(RCNT, 0).unwrap();
    bus.write16(RCNT, 0x8055).unwrap();
    assert!(bus.write16(RCNT, 0x8140).is_err());
    assert_eq!(bus.read16(RCNT).unwrap(), 0x805f); // Low byte was not committed.
    bus.write16(RCNT, 0).unwrap(); // RCNT bit 15 would override the UART selection.

    // A UART send has no partner: the byte is dropped, not a diagnostic.
    bus.write16(SIOCNT, 0x3400).unwrap();
    bus.write8(SIODATA8, 0x55).unwrap();
    assert_eq!(bus.read16(SIOCNT).unwrap(), 0x3420); // Receive buffer empty.
    assert_eq!(bus.read16(SIODATA8).unwrap(), 0);
}

#[test]
fn block_store_configures_and_triggers_64_sample_wave_playback() {
    let code: [u32; 7] = [
        0xe3a00301, 0xe2800070, 0xe59f1008, 0xe3a02902, 0xe8a00006, 0xeafffffe, 0x123400a0,
    ];
    let mut bus = Memory::new(code.into_iter().flat_map(u32::to_le_bytes).collect()).unwrap();
    bus.write16(SOUNDCNT_X, 0x80).unwrap();
    let mut machine = Machine::new(Cpu::new(ROM_START), bus);
    for _ in 0..4 {
        machine.step().unwrap();
    }
    machine.step().unwrap(); // STMIA: SOUND3CNT_L/H, then a trigger, in one store.
    assert_eq!(machine.memory().read16(0x04000070).unwrap(), 0xa0);
    assert_eq!(machine.memory().read16(SOUNDCNT_X).unwrap() & 4, 4);
}

#[test]
fn dma_activation_error_preserves_channel_progress_and_clocks() {
    let mut bus = memory();
    bus.write32(0x02000000, 0x0000_8140).unwrap(); // GPIO with SI driven low, interrupt on.
    bus.write32(DMA_BASE, 0x02000000).unwrap();
    bus.write32(DMA_BASE + 4, RCNT).unwrap();
    bus.write32(DMA_BASE + 8, 0x84000001).unwrap();
    let mut machine = Machine::new(Cpu::new(ROM_START), bus);
    let before = machine.cpu().clone();
    let error = machine.step().unwrap_err();
    assert!(error.to_string().contains("GPIO serial interrupt"));
    assert_eq!(machine.cycles(), 0);
    assert_eq!(machine.cpu(), &before);
    assert_eq!(machine.step(), Err(error));
    machine.memory_mut().write32(0x02000000, 0).unwrap();
    machine.step().unwrap();
    assert_eq!(machine.memory().read16(DMA_BASE + 10).unwrap() & 0x8000, 0);
}
