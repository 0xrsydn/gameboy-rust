//! Original disconnected multiplayer register tests; no connected peer or completed transfer.
use gba_core::{
    cpu::Cpu,
    dma::DMA_BASE,
    io::{
        HALTCNT, IE, IF, IME, RCNT, SIOCNT, SIODATA32, SIODATA8, SIOMLT_SEND, SIOMULTI0, SIOMULTI1,
        SIOMULTI2, SIOMULTI3,
    },
    machine::{Machine, StepKind},
    memory::{Memory, MemoryError, ROM_START},
};

fn memory() -> Memory {
    Memory::new(0xeaff_fffe_u32.to_le_bytes().to_vec()).unwrap()
}

#[test]
fn all_multiplayer_control_values_mask_status_and_cannot_start_a_disconnected_child() {
    let mut bus = memory();
    bus.write16(IE, 0x80).unwrap();
    bus.write16(IME, 1).unwrap();
    for value in 0..=u16::MAX {
        if value & 0x3000 != 0x2000 {
            continue;
        }
        bus.write16(SIOCNT, value).unwrap();
        // SI high (child), SD high (local idle drive); no valid assigned ID yet.
        assert_eq!(bus.read16(SIOCNT).unwrap(), (value & 0x6f03) | 12);
        assert_eq!(bus.read8(SIOCNT).unwrap() & 0xf0, 0); // ID placeholder, no error or busy.
        assert_eq!(bus.read16(IF).unwrap(), 0);
    }
    bus.advance_cycles(u32::MAX);
    assert_eq!(bus.read16(IF).unwrap(), 0);
    assert!(!bus.irq_pending());
    assert_eq!(bus.read16(SIOCNT).unwrap(), 0x6f0f);
}

#[test]
fn receive_latches_have_independent_lanes_and_do_not_become_received_peer_data() {
    let mut bus = memory();
    bus.write16(SIOCNT, 0x2000).unwrap();
    for address in [SIOMULTI0, SIOMULTI1, SIOMULTI2, SIOMULTI3] {
        assert_eq!(bus.read16(address).unwrap(), 0); // Synthetic reset latches, not completed receives.
    }
    bus.write32(SIOMULTI0, 0x12345678).unwrap();
    bus.write32(SIOMULTI2, 0x9abcdef0).unwrap();
    bus.write8(SIOMULTI1 + 1, 0x55).unwrap();
    bus.write16(SIOMULTI3, 0xaabb).unwrap();
    assert_eq!(bus.read16(SIOMULTI0).unwrap(), 0x5678);
    assert_eq!(bus.read16(SIOMULTI1).unwrap(), 0x5534);
    assert_eq!(bus.read32(SIOMULTI2).unwrap(), 0xaabbdef0);
    bus.write16(SIOMLT_SEND, 0xcdef).unwrap();
    for rate in 0..4 {
        bus.write16(SIOCNT, 0x6080 | rate).unwrap(); // Start is read-only for this disconnected child.
        bus.advance_cycles(1000000);
        assert_eq!(bus.read32(SIOMULTI0).unwrap(), 0x55345678);
        assert_eq!(bus.read32(SIOMULTI2).unwrap(), 0xaabbdef0);
        assert_eq!(bus.read16(SIOMLT_SEND).unwrap(), 0xcdef);
        assert_eq!(bus.read16(IF).unwrap(), 0);
    }
}

#[test]
fn send_width_and_receive_aliases_follow_the_selected_format() {
    let mut bus = memory();
    bus.write32(SIOCNT, 0xabcd2000).unwrap(); // Final multiplayer format applies to all SEND lanes.
    assert_eq!(bus.read16(SIOMLT_SEND).unwrap(), 0xabcd);
    bus.write8(SIOMLT_SEND, 0x12).unwrap();
    bus.write8(SIOMLT_SEND + 1, 0x34).unwrap();
    assert_eq!(bus.read16(SIOMLT_SEND).unwrap(), 0x3412);
    bus.write32(SIOMULTI0, 0x12345678).unwrap();
    bus.write32(SIOMULTI2, 0x9abcdef0).unwrap();
    bus.write16(SIOCNT, 0x1000).unwrap();
    assert_eq!(bus.read32(SIODATA32).unwrap(), 0x12345678);
    assert_eq!(bus.read16(SIODATA8).unwrap(), 0x12);
    assert_eq!(bus.read32(SIOMULTI2), Err(MemoryError::Unmapped(SIOMULTI2)));
    assert!(bus
        .write32(SIOMULTI2, 0)
        .unwrap_err()
        .to_string()
        .contains("outside multiplayer mode"));
    bus.write16(SIODATA8, 0xee56).unwrap(); // Existing normal mode ignores the upper write lane.
    assert_eq!(bus.read16(SIODATA8).unwrap(), 0x56);
    bus.write16(SIOCNT, 0x83).unwrap();
    bus.advance_cycles(64); // Only the normal 8-bit register shifts.
    bus.write16(SIOCNT, 0x2000).unwrap();
    assert_eq!(bus.read16(SIOMLT_SEND).unwrap(), 0x34ff);
    assert_eq!(bus.read32(SIOMULTI0).unwrap(), 0x12345678);
    assert_eq!(bus.read32(SIOMULTI2).unwrap(), 0x9abcdef0);
}

#[test]
fn idle_multiplayer_neither_wakes_halt_nor_advances_stop() {
    for stop in [false, true] {
        let mut bus = memory();
        bus.write16(IE, 0x80).unwrap();
        bus.write16(IME, 1).unwrap();
        bus.write16(SIOCNT, 0x6083).unwrap();
        bus.write8(HALTCNT, if stop { 0x80 } else { 0 }).unwrap();
        let mut machine = Machine::new(Cpu::new(ROM_START), bus);
        for _ in 0..100 {
            assert_eq!(
                machine.step().unwrap(),
                if stop {
                    StepKind::StopIdle
                } else {
                    StepKind::HaltIdle
                }
            );
        }
        assert_eq!(machine.halted(), !stop);
        assert_eq!(machine.stopped(), stop);
        assert_eq!(machine.cycles() == 0, stop);
        assert_eq!(machine.memory().read16(SIOCNT).unwrap(), 0x600f);
        assert_eq!(machine.memory().read16(IF).unwrap(), 0);
    }
}

#[test]
fn gpio_status_uses_pin_latches_but_cannot_create_a_multiplayer_parent_transfer() {
    let mut bus = memory();
    bus.write16(RCNT, 0x80f0).unwrap(); // All GPIO outputs low.
    bus.write16(SIOCNT, 0x2083).unwrap();
    assert_eq!(bus.read16(SIOCNT).unwrap(), 0x2003);
    bus.write16(RCNT, 0x80f6).unwrap(); // SI and SD high.
    assert_eq!(bus.read16(SIOCNT).unwrap(), 0x200f);
    bus.write16(RCNT, 0).unwrap();
    assert_eq!(bus.read16(SIOCNT).unwrap(), 0x200f);
    assert_eq!(bus.read8(RCNT), Err(MemoryError::Unmapped(RCNT))); // Do not guess SC pin samples.
    bus.advance_cycles(1000000);
    assert_eq!(bus.read16(IF).unwrap(), 0);
}

#[test]
fn active_normal_mode_cannot_be_silently_cancelled_by_read_only_multiplayer_busy_masking() {
    let mut bus = memory();
    bus.write16(SIOCNT, 0x4083).unwrap();
    bus.advance_cycles(8);
    for (address, value) in [(SIOCNT + 1, 0x20), (SIOCNT + 1, 0x60)] {
        assert!(bus
            .write8(address, value)
            .unwrap_err()
            .to_string()
            .contains("serial reconfiguration"));
        assert_eq!(bus.read16(SIOCNT).unwrap(), 0x4087);
        assert_eq!(bus.read8(SIODATA8).unwrap(), 1);
    }
    assert!(bus.write16(SIOCNT, 0x2083).is_err());
    bus.write16(SIOCNT, 0x2000).unwrap(); // Explicitly clear start and select multiplayer together.
    bus.advance_cycles(1000000);
    assert_eq!(bus.read16(IF).unwrap(), 0);
    assert_eq!(bus.read16(SIOCNT).unwrap(), 0x200c);
}

#[test]
fn uart_rejection_preserves_multiplayer_control_and_send_lanes() {
    let mut bus = memory();
    bus.write32(SIOCNT, 0xabcd6001).unwrap();
    let error = bus.write32(SIOCNT, 0x12343083).unwrap_err();
    assert!(error.to_string().contains("UART serial mode"));
    assert_eq!(bus.write32(SIOCNT, 0x12343083), Err(error));
    assert_eq!(bus.read16(SIOCNT).unwrap(), 0x600d);
    assert_eq!(bus.read16(SIOMLT_SEND).unwrap(), 0xabcd);
}

#[test]
fn original_arm_and_thumb_word_stores_select_multiplayer_and_write_full_send_data() {
    for thumb in [false, true] {
        let code = if thumb {
            vec![
                0xe59f0010, 0xe59f1010, 0xe28f2001, 0xe12fff12, 0xe7fe6001, 0xeafffffe, SIOCNT,
                0xabcd6083,
            ]
        } else {
            vec![
                0xe59f0008, 0xe59f1008, 0xe5801000, 0xeafffffe, SIOCNT, 0xabcd6083,
            ]
        };
        let mut bus = Memory::new(code.into_iter().flat_map(u32::to_le_bytes).collect()).unwrap();
        let mut cpu = Cpu::new(ROM_START);
        for _ in 0..if thumb { 4 } else { 2 } {
            cpu.step(&mut bus).unwrap();
        }
        let mut machine = Machine::new(cpu, bus);
        assert_eq!(machine.step().unwrap(), StepKind::Instruction);
        assert_eq!(machine.memory().read16(SIOCNT).unwrap(), 0x600f);
        assert_eq!(machine.memory().read16(SIOMLT_SEND).unwrap(), 0xabcd);
        machine.memory_mut().advance_cycles(1000000);
        assert_eq!(machine.memory().read16(IF).unwrap(), 0);
    }
}

#[test]
fn dma_mode_and_send_write_only_requests_its_own_completion_irq() {
    let mut bus = memory();
    bus.write32(0x02000000, 0xabcd6083).unwrap();
    bus.write32(DMA_BASE, 0x02000000).unwrap();
    bus.write32(DMA_BASE + 4, SIOCNT).unwrap();
    bus.write32(DMA_BASE + 8, 0xc4000001).unwrap();
    let mut machine = Machine::new(Cpu::new(ROM_START), bus);
    assert_eq!(machine.step().unwrap(), StepKind::Dma { channel: 0 });
    assert_eq!(machine.memory().read16(SIOCNT).unwrap(), 0x600f);
    assert_eq!(machine.memory().read16(SIOMLT_SEND).unwrap(), 0xabcd);
    assert_eq!(machine.memory().read16(IF).unwrap(), 0x100);
    machine.memory_mut().advance_cycles(1000000);
    assert_eq!(machine.memory().read16(IF).unwrap(), 0x100);
}
