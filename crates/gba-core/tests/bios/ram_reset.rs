use super::*;
use gba_core::{
    dma::{DmaError, DMA_BASE, DMA_STRIDE},
    input::Buttons,
    io::{
        BG0CNT, BG2PA, BG2PD, BG3PA, BG3PD, BLDALPHA, BLDCNT, DISPCNT, GREENSWAP, KEYCNT, KEYINPUT,
        WAITCNT, WININ, WINOUT,
    },
    memory::{OAM_START, PALETTE_START},
    video::Framebuffer,
};

const REGIONS: [(u32, u32, u32); 5] = [
    (1, 0x0200_0000, 0x40000),
    (2, 0x0300_0000, 0x7e00),
    (4, PALETTE_START, 0x400),
    (8, VRAM_START, 0x18000),
    (16, OAM_START, 0x400),
];
const MARKER: u32 = 0xa55a_c33c;

#[test]
fn sound_reset_disables_running_psg_channels_only_when_selected() {
    use gba_core::io::SOUNDCNT_X;
    for thumb in [false, true] {
        for flags in [0, 0x40] {
            let (mut machine, return_pc) = prepare(flags, thumb, 0x1f);
            let bus = machine.memory_mut();
            bus.write16(SOUNDCNT_X, 0x80).unwrap();
            bus.write16(0x04000062, 0xf080).unwrap();
            bus.write16(0x04000064, 0x87ff).unwrap();
            bus.write16(0x04000068, 0xf080).unwrap();
            bus.write16(0x0400006c, 0x87fe).unwrap();
            bus.write16(0x04000078, 0xf000).unwrap();
            bus.write16(0x0400007c, 0x8000).unwrap();
            complete(&mut machine, return_pc);
            assert_eq!(
                machine.memory().read16(SOUNDCNT_X).unwrap(),
                if flags == 0 { 0x8b } else { 0 }
            );
        }
    }
}

#[test]
fn sound_reset_clears_disabled_fifo_queues_and_only_accessible_wave_bank() {
    use gba_core::io::{FIFO_A, SOUNDCNT_H, SOUNDCNT_X, TIMER_BASE, WAVE_RAM};
    for thumb in [false, true] {
        let (mut machine, return_pc) = prepare(0x40, thumb, 0x1f);
        let bus = machine.memory_mut();
        bus.write16(SOUNDCNT_X, 0x80).unwrap();
        bus.write16(0x04000070, 0x40).unwrap();
        bus.write32(WAVE_RAM, MARKER).unwrap(); // CPU bank 0.
        bus.write16(SOUNDCNT_X, 0).unwrap();
        bus.write32(WAVE_RAM, MARKER).unwrap(); // CPU bank 1.
        bus.write32(FIFO_A, 0x7f7f7f7f).unwrap();
        complete(&mut machine, return_pc);
        let bus = machine.memory_mut();
        assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0);
        assert_eq!(bus.read32(WAVE_RAM).unwrap(), 0);
        bus.write16(SOUNDCNT_X, 0x80).unwrap();
        bus.write16(0x04000070, 0x40).unwrap();
        assert_eq!(bus.read32(WAVE_RAM).unwrap(), MARKER);
        bus.write16(SOUNDCNT_H, 0x0304).unwrap();
        bus.write32(TIMER_BASE, 0x0080ffff).unwrap();
        bus.advance_cycles(1);
        assert_eq!(bus.audio_level().left, 0);
    }
}

fn literal(code: &mut Vec<u32>, register: u32, value: u32) {
    code.extend([0xe59f_0000 | register << 12, 0xea00_0000, value]);
}

fn prepare(flags: u32, thumb: bool, status: u32) -> (Machine, u32) {
    prepare_at(flags, thumb, status, None)
}

fn prepare_at(flags: u32, thumb: bool, status: u32, location: Option<u32>) -> (Machine, u32) {
    let mut code = Vec::new();
    literal(&mut code, 0, flags);
    for r in 1..=12 {
        code.push(0xe3a0_0080 | r << 12 | r);
    }
    code.extend([0xe328_f20f, 0xe321_f000 | status]);
    let swi_pc = if let Some(address) = location {
        literal(&mut code, 14, address | u32::from(thumb));
        code.push(0xe12f_ff1e);
        address
    } else {
        code.extend([
            if thumb { 0xe28f_e001 } else { 0xe1a0_e00f },
            if thumb { 0xe12f_ff1e } else { 0xe1a0_0000 },
        ]);
        let address = ROM_START + code.len() as u32 * 4;
        code.extend([if thumb { 0xe7fe_df01 } else { 0xef01_0000 }, 0xeaff_fffe]);
        address
    };
    let mut m = bios::boot(words(&code)).unwrap();
    if let Some(address) = location {
        m.memory_mut()
            .write32(address, if thumb { 0xe7fe_df01 } else { 0xef01_0000 })
            .unwrap();
        m.memory_mut().write32(address + 4, 0xeaff_fffe).unwrap();
    }
    reach(&mut m, swi_pc, 200);
    (m, swi_pc + if thumb { 2 } else { 4 })
}

fn complete(m: &mut Machine, pc: u32) {
    let before = m.cpu().clone();
    m.step().unwrap();
    reach(m, pc, 400_000);
    assert_eq!(&m.cpu().registers()[..15], &before.registers()[..15]);
    assert_eq!(m.cpu().cpsr(), before.cpsr());
    assert_eq!(m.memory().read16(DISPCNT).unwrap(), 0x80);
}

fn samples(start: u32, bytes: u32) -> [u32; 4] {
    [start, start + 4, start + bytes / 2, start + bytes - 4]
}

fn seed_samples(m: &mut Machine) {
    for (_, start, bytes) in REGIONS {
        for address in samples(start, bytes) {
            m.memory_mut().write32(address, MARKER).unwrap();
        }
    }
}

fn check_samples(m: &Machine, flags: u32) {
    for (bit, start, bytes) in REGIONS {
        for address in samples(start, bytes) {
            assert_eq!(
                m.memory().read32(address).unwrap(),
                if flags & bit == 0 { MARKER } else { 0 },
                "flags={flags:#x}, address={address:#x}"
            );
        }
    }
}

#[test]
fn each_memory_flag_clears_its_entire_region_and_no_other_region() {
    for thumb in [false, true] {
        for (flag, _, _) in REGIONS {
            let (mut m, pc) = prepare(flag, thumb, 0x1f);
            for (_, start, bytes) in REGIONS {
                for address in (start..start + bytes).step_by(4) {
                    m.memory_mut().write32(address, MARKER ^ address).unwrap();
                }
            }
            complete(&mut m, pc);
            for (bit, start, bytes) in REGIONS {
                for address in (start..start + bytes).step_by(4) {
                    assert_eq!(
                        m.memory().read32(address).unwrap(),
                        if bit == flag { 0 } else { MARKER ^ address },
                        "flag={flag}, address={address:#x}"
                    );
                }
            }
        }
    }
}

#[test]
fn all_memory_flag_combinations_are_independent_from_arm_and_thumb() {
    for thumb in [false, true] {
        for flags in 0..32 {
            let (mut m, pc) = prepare(flags, thumb, 0x1f);
            seed_samples(&mut m);
            complete(&mut m, pc);
            check_samples(&m, flags);
        }
    }
}

#[test]
fn zero_flags_still_force_blank_and_clear_other_display_control_bits() {
    for thumb in [false, true] {
        let (mut m, pc) = prepare(0, thumb, 0x1f);
        seed_samples(&mut m);
        m.memory_mut().write16(DISPCNT, 0x1403).unwrap();
        m.memory_mut().write16(BG0CNT, 0x1234).unwrap();
        complete(&mut m, pc);
        check_samples(&m, 0);
        assert_eq!(m.memory().read16(BG0CNT).unwrap(), 0x1204);
        let mut frame = Framebuffer::default();
        m.memory().render_frame(&mut frame).unwrap();
        assert!(frame.pixels().iter().all(|p| *p == 0xffffff));
    }
}

#[test]
fn reserved_high_bits_are_ignored_and_original_argument_is_restored() {
    for flags in [
        0x8000_0000,
        0xffff_ff00,
        0x1234_5604,
        0xffff_ff80,
        0xffff_ff9f,
    ] {
        for thumb in [false, true] {
            let (mut m, pc) = prepare(flags, thumb, 0x1f);
            seed_samples(&mut m);
            complete(&mut m, pc);
            check_samples(&m, flags);
        }
    }
}

#[test]
fn internal_ram_clear_preserves_bios_work_area_except_common_swi_frame() {
    for thumb in [false, true] {
        let (mut m, pc) = prepare(0x9f, thumb, 0x1f);
        for address in (0x0300_7e00..0x0300_8000).step_by(4) {
            m.memory_mut().write32(address, MARKER ^ address).unwrap();
        }
        complete(&mut m, pc);
        for address in (0x0300_7e00..0x0300_8000).step_by(4) {
            if (bios::SVC_STACK - 28..bios::SVC_STACK).contains(&address) {
                continue;
            }
            assert_eq!(
                m.memory().read32(address).unwrap(),
                MARKER ^ address,
                "{address:#x}"
            );
        }
        assert_eq!(m.memory().read32(0x0300_7dfc).unwrap(), 0);
    }
}

#[test]
fn serial_sound_reset_flags_are_selective_and_all_devices_return() {
    use gba_core::io::{
        RCNT, SIOCNT, SIODATA32, SIODATA8, SOUNDBIAS, SOUNDCNT_H, SOUNDCNT_X, WAVE_RAM,
    };
    for thumb in [false, true] {
        for flags in [
            0, 0x20, 0x40, 0x60, 0x80, 0xff, 0xffffff20, 0xffffff40, 0xffffffff,
        ] {
            let (mut m, pc) = prepare(flags, thumb, 0x1f);
            seed_samples(&mut m);
            let bus = m.memory_mut();
            bus.write32(SIODATA32, MARKER).unwrap();
            bus.write16(SIODATA8, 0x55).unwrap();
            bus.write16(SIOCNT, 0x500b).unwrap();
            bus.write16(RCNT, 0x80fa).unwrap();
            bus.write16(SOUNDCNT_H, 0xffff).unwrap();
            bus.write16(SOUNDBIAS, 0xffff).unwrap();
            for offset in 0..16 {
                bus.write8(WAVE_RAM + offset, 0x5a).unwrap();
            }
            complete(&mut m, pc);
            check_samples(&m, flags);
            let bus = m.memory();
            assert_eq!(bus.read32(SIODATA32).unwrap(), MARKER & 0xffff0000); // Unconditional low-halfword clear.
            assert_eq!(
                bus.read16(SIODATA8).unwrap(),
                if flags & 0x20 != 0 { 0 } else { 0x55 }
            );
            assert_eq!(
                bus.read16(SIOCNT).unwrap(),
                if flags & 0x20 != 0 { 4 } else { 0x500b }
            );
            assert_eq!(
                bus.read16(RCNT).unwrap(),
                if flags & 0x20 != 0 { 0x800f } else { 0x80fa }
            );
            assert_eq!(
                bus.read16(SOUNDCNT_H).unwrap(),
                if flags & 0x40 != 0 { 0 } else { 0x770f }
            );
            assert_eq!(
                bus.read16(SOUNDBIAS).unwrap(),
                if flags & 0x40 != 0 { 0x200 } else { 0xc3fe }
            );
            assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0);
            for offset in 0..16 {
                assert_eq!(
                    bus.read8(WAVE_RAM + offset).unwrap(),
                    if flags & 0x40 != 0 { 0 } else { 0x5a }
                );
            }
        }
    }
}

#[test]
fn serial_reset_cancels_an_external_clock_request_only_when_selected() {
    use gba_core::io::{RCNT, SIOCNT};
    for thumb in [false, true] {
        for flags in [0, 0x20, 0x40, 0x80, 0xff] {
            let (mut m, pc) = prepare(flags, thumb, 0x1f);
            m.memory_mut().write16(RCNT, 0).unwrap();
            m.memory_mut().write16(SIOCNT, 0x5080).unwrap();
            complete(&mut m, pc);
            assert_eq!(
                m.memory().read16(SIOCNT).unwrap(),
                if flags & 0x20 != 0 { 4 } else { 0x5084 }
            );
            assert_eq!(
                m.memory().read8(RCNT + 1).unwrap(),
                if flags & 0x20 != 0 { 0x80 } else { 0 }
            );
            assert_eq!(m.memory().read16(IF).unwrap() & 0x80, 0);
        }
    }
}

#[test]
fn caller_registers_flags_modes_and_interrupt_masks_are_preserved() {
    for thumb in [false, true] {
        for status in [0x10, 0x50, 0x90, 0xd0, 0x1f, 0x5f, 0x9f, 0xdf] {
            for flags in [0, 4, 0x80] {
                let (mut m, pc) = prepare(flags, thumb, status);
                assert!(m.cpu().registers()[1..13].iter().all(|r| *r != 0));
                complete(&mut m, pc);
            }
        }
    }
}

#[test]
fn io_reset_clears_readable_controls_but_keeps_input_postflg_and_green_swap() {
    for flags in [0x80, 0x9f] {
        let (mut m, pc) = prepare(flags, false, 0x9f);
        m.memory_mut().set_buttons(Buttons::from_bits(0x251));
        let keys = m.memory().read16(KEYINPUT).unwrap();
        m.memory_mut().write16(KEYCNT, 0x4001).unwrap();
        m.memory_mut().write16(GREENSWAP, 1).unwrap();
        for address in [
            BG0CNT,
            BG0CNT + 2,
            BG0CNT + 4,
            BG0CNT + 6,
            WININ,
            WINOUT,
            BLDCNT,
            BLDALPHA,
            WAITCNT,
            IE,
        ] {
            m.memory_mut().write16(address, 0xffff).unwrap();
        }
        m.memory_mut().write16(DISPSTAT, 0x7b38).unwrap();
        m.memory_mut().write16(IME, 1).unwrap();
        // Overflow queues IF, independently of CPU execution.
        m.memory_mut().write32(TIMER_BASE, 0x00c0_ffff).unwrap();
        m.memory_mut().advance_cycles(1);
        assert_ne!(m.memory().read16(IF).unwrap(), 0);
        let cycles = m.cycles();
        complete(&mut m, pc);
        for address in [
            BG0CNT,
            BG0CNT + 2,
            BG0CNT + 4,
            BG0CNT + 6,
            WININ,
            WINOUT,
            BLDCNT,
            BLDALPHA,
            WAITCNT,
            IE,
            IF,
            IME,
        ] {
            assert_eq!(m.memory().read16(address).unwrap(), 0, "{address:#x}");
        }
        assert_eq!(m.memory().read16(DISPSTAT).unwrap() & 0xff38, 0);
        assert_eq!(m.memory().read16(KEYINPUT).unwrap(), keys);
        assert_eq!(m.memory().read16(KEYCNT).unwrap(), 0x4001); // Current reset subset preserves KEYCNT.
        assert_eq!(m.memory().read16(POSTFLG).unwrap(), 1);
        assert_eq!(m.memory().read16(GREENSWAP).unwrap(), 1);
        assert!(m.cycles() > cycles);
        assert!(!m.halted());
    }
}

#[test]
fn io_reset_clears_write_only_display_state_and_restores_both_identity_matrices() {
    let (mut m, pc) = prepare(0x80, true, 0x1f);
    let mut reference = Memory::new(vec![]).unwrap();
    for (_, start, bytes) in [REGIONS[2], REGIONS[3]] {
        for offset in (0..bytes).step_by(2) {
            let value = ((offset * 37 + 1) ^ (offset >> 4)) as u16;
            m.memory_mut().write16(start + offset, value).unwrap();
            reference.write16(start + offset, value).unwrap();
        }
    }
    for address in (BG0CNT..0x0400_0058).step_by(2) {
        m.memory_mut().write16(address, 0x5353).unwrap();
    }
    complete(&mut m, pc);
    for address in [BG2PA, BG2PD, BG3PA, BG3PD] {
        reference.write16(address, 256).unwrap();
    }
    for control in [
        0x0100, 0x0200, 0x0400, 0x0800, 0x0402, 0x0802, 0x0403, 0x2403, 0x4403, 0x6403,
    ] {
        for bus in [m.memory_mut(), &mut reference] {
            bus.write16(DISPCNT, control).unwrap();
            // Enable mosaic in each background without altering its offset/matrix.
            for address in (BG0CNT..=BG0CNT + 6).step_by(2) {
                bus.write16(address, 0x40).unwrap();
            }
        }
        let mut actual = Framebuffer::default();
        let mut expected = Framebuffer::default();
        m.memory().render_frame(&mut actual).unwrap();
        reference.render_frame(&mut expected).unwrap();
        assert_eq!(actual.pixels(), expected.pixels(), "DISPCNT={control:#x}");
    }
}

#[test]
fn timer_reset_stops_counters_clears_reload_and_does_not_replace_counter_state() {
    let (mut m, pc) = prepare(0x80, false, 0x1f);
    for timer in 0..4 {
        m.memory_mut()
            .write32(TIMER_BASE + timer * 4, 0x0080_4321)
            .unwrap();
    }
    complete(&mut m, pc);
    for timer in 0..4 {
        let address = TIMER_BASE + timer * 4;
        assert_eq!(m.memory().read16(address + 2).unwrap(), 0);
        let counter = m.memory().read16(address).unwrap();
        assert!(counter >= 0x4321);
        m.memory_mut().advance_cycles(1000);
        assert_eq!(m.memory().read16(address).unwrap(), counter);
        m.memory_mut().write16(address + 2, 0x80).unwrap();
        assert_eq!(m.memory().read16(address).unwrap(), 0); // Reload latch was cleared.
        m.memory_mut().write16(address + 2, 0).unwrap();
    }
}

#[test]
fn dma_reset_disables_channels_and_clears_write_only_address_and_count_latches() {
    for channel in 0..4 {
        let (mut m, pc) = prepare(0x80, false, 0x1f);
        let base = DMA_BASE + channel as u32 * DMA_STRIDE;
        m.memory_mut().write32(base, SOURCE).unwrap();
        m.memory_mut().write32(base + 4, DEST).unwrap();
        m.memory_mut().write32(base + 8, 0x9200_0001).unwrap(); // Repeating VBlank, count one.
        complete(&mut m, pc);
        assert_eq!(m.memory().read16(base + 10).unwrap(), 0);
        // With only control re-enabled, the source must now be zero.
        m.memory_mut().write16(base + 10, 0x8000).unwrap();
        assert_eq!(
            m.step(),
            Err(MachineError::Dma(DmaError::UnsupportedSource {
                channel,
                address: 0
            }))
        );
        m.memory_mut().write16(base + 10, 0).unwrap();
        m.memory_mut().write32(base, SOURCE).unwrap();
        m.memory_mut().write32(SOURCE, MARKER).unwrap();
        m.memory_mut().write32(DEST, !MARKER).unwrap();
        let vector = m.memory().read32(0).unwrap();
        m.memory_mut().write16(base + 10, 0x8000).unwrap();
        assert_eq!(m.step().unwrap(), StepKind::Dma { channel });
        // The reset destination is BIOS: discard the transfer, not into stale DEST.
        assert_eq!(m.memory().read32(0).unwrap(), vector);
        assert_eq!(m.memory().read32(DEST).unwrap(), !MARKER);
        m.memory_mut().write16(base + 10, 0).unwrap();
        m.memory_mut().write32(base + 4, DEST).unwrap();
        m.memory_mut().write32(SOURCE, MARKER).unwrap();
        m.memory_mut().write16(base + 10, 0x8500).unwrap(); // Fixed source, words; count remains zero.
        assert_eq!(m.step().unwrap(), StepKind::Dma { channel });
        assert_eq!(m.step().unwrap(), StepKind::Dma { channel });
        assert_eq!(m.memory().read32(DEST).unwrap(), MARKER);
        assert_eq!(m.memory().read32(DEST + 4).unwrap(), MARKER);
        // Count zero expands to the channel maximum, not the stale count of one.
        assert_ne!(m.memory().read16(base + 10).unwrap() & 0x8000, 0);
        m.memory_mut().write16(base + 10, 0).unwrap();
    }
}

#[test]
fn memory_only_reset_keeps_io_and_allows_dma_timers_and_deferred_irq() {
    for thumb in [false, true] {
        let (mut m, pc) = prepare(4, thumb, 0x1f);
        let before = m.cpu().clone();
        m.step().unwrap();
        m.memory_mut().write16(BG0CNT, 0x41).unwrap();
        m.memory_mut().write16(WAITCNT, 0x4317).unwrap();
        m.memory_mut().write16(IE, 8).unwrap();
        m.memory_mut().write16(IME, 1).unwrap();
        m.memory_mut().write32(TIMER_BASE, 0x00c0_fff0).unwrap();
        m.memory_mut().write32(SOURCE, MARKER).unwrap();
        m.memory_mut().write32(DMA_BASE, SOURCE).unwrap();
        m.memory_mut().write32(DMA_BASE + 4, DEST).unwrap();
        m.memory_mut().write32(DMA_BASE + 8, 0x8400_0001).unwrap();
        let mut dma = 0;
        for _ in 0..4000 {
            if m.cpu().pc() == pc {
                break;
            }
            let step = m.step().unwrap();
            assert_ne!(step, StepKind::IrqEntry);
            if matches!(step, StepKind::Dma { channel: 0 }) {
                dma += 1;
            }
        }
        assert_eq!(m.cpu().pc(), pc);
        assert_eq!(&m.cpu().registers()[..15], &before.registers()[..15]);
        assert_eq!(m.cpu().cpsr(), before.cpsr());
        assert_eq!(dma, 1);
        assert_eq!(m.memory().read32(DEST).unwrap(), MARKER);
        for (address, value) in [
            (BG0CNT, 0x41),
            (WAITCNT, 0x4317),
            (IE, 8),
            (IME, 1),
            (TIMER_BASE + 2, 0xc0),
        ] {
            assert_eq!(m.memory().read16(address).unwrap(), value);
        }
        assert_ne!(m.memory().read16(IF).unwrap() & 8, 0);
        assert_eq!(m.step().unwrap(), StepKind::IrqEntry);
    }
}

#[test]
fn clear_is_visible_one_word_at_a_time_and_uses_normal_video_bus_stores() {
    for flag in [1, 2, 4, 8, 16] {
        let (mut m, pc) = prepare(flag, false, 0x1f);
        let (_, start, _) = REGIONS.into_iter().find(|r| r.0 == flag).unwrap();
        m.memory_mut().write32(start, MARKER).unwrap();
        m.memory_mut().write32(start + 4, MARKER).unwrap();
        m.step().unwrap();
        for _ in 0..1000 {
            if m.memory().read32(start).unwrap() == 0 {
                break;
            }
            m.step().unwrap();
        }
        assert_eq!(m.memory().read32(start).unwrap(), 0);
        assert_eq!(m.memory().read32(start + 4).unwrap(), MARKER);
        assert_eq!(m.memory().read16(DISPCNT).unwrap(), 0x80);
        assert!(m.cpu().pc() < BIOS_SIZE as u32);
        reach(&mut m, pc, 400_000);
        assert_eq!(m.memory().read32(start + 4).unwrap(), 0);
    }
}

#[test]
fn ram_callers_can_return_but_selected_ram_code_is_really_erased() {
    for thumb in [false, true] {
        for (location, clear_flag) in [(SOURCE + 0x100, 1), (0x0300_1000, 2)] {
            for flags in [4, clear_flag] {
                let (mut m, pc) = prepare_at(flags, thumb, 0x1f, Some(location));
                complete(&mut m, pc);
                assert_eq!(
                    m.memory().read32(location).unwrap(),
                    if flags == clear_flag {
                        0
                    } else if thumb {
                        0xe7fe_df01
                    } else {
                        0xef01_0000
                    }
                );
            }
        }
    }
}

#[test]
fn cleared_oam_is_zero_not_the_disabled_sprite_encoding() {
    let (mut m, pc) = prepare(16, false, 0x1f);
    m.memory_mut().write16(OAM_START, 0x200).unwrap();
    m.memory_mut().write16(PALETTE_START + 0x202, 31).unwrap();
    m.memory_mut()
        .write16(VRAM_START + 0x10000, 0x1111)
        .unwrap();
    complete(&mut m, pc);
    assert_eq!(m.memory().read16(OAM_START).unwrap(), 0);
    m.memory_mut().write16(DISPCNT, 0x1000).unwrap();
    let mut frame = Framebuffer::default();
    m.memory().render_frame(&mut frame).unwrap();
    assert_eq!(frame.pixels()[0], 0xff0000);
}
