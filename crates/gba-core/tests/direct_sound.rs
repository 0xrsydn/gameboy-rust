//! Original Direct Sound tests. No game data or upstream test programs.
use gba_core::{
    audio::StereoLevel,
    cpu::Cpu,
    dma::{DMA_BASE, DMA_STRIDE},
    io::{FIFO_A, FIFO_B, HALTCNT, IF, SOUNDBIAS, SOUNDCNT_H, SOUNDCNT_X, TIMER_BASE, WAVE_RAM},
    machine::{Machine, StepKind},
    memory::{Memory, ROM_START},
};

fn bus() -> Memory {
    let mut bus = Memory::new(0xeaff_fffeu32.to_le_bytes().to_vec()).unwrap();
    bus.write16(SOUNDCNT_X, 0x80).unwrap();
    bus.write16(SOUNDBIAS, 0x200).unwrap();
    bus.write16(SOUNDCNT_H, 0x0304).unwrap(); // A, both sides, 100%, timer 0.
    bus
}

fn timer(bus: &mut Memory, index: u32, period: u16, control: u16) {
    bus.write32(
        TIMER_BASE + index * 4,
        u32::from(0u16.wrapping_sub(period)) | u32::from(control) << 16,
    )
    .unwrap();
}

fn stereo(value: i16) -> StereoLevel {
    StereoLevel {
        left: value,
        right: value,
    }
}

fn sound_dma(bus: &mut Memory, channel: u32, control: u16) {
    let base = DMA_BASE + channel * DMA_STRIDE;
    bus.write32(base, 0x0200_0000).unwrap();
    bus.write32(base + 4, FIFO_A + (channel - 1) * 4).unwrap();
    bus.write32(base + 8, u32::from(control | 0xb000) << 16 | 1)
        .unwrap();
}

#[test]
fn signed_little_endian_samples_hold_between_edges_then_underflow_to_zero() {
    let mut bus = bus();
    bus.write32(FIFO_A, 0x7f80ff01).unwrap();
    timer(&mut bus, 0, 10, 0x80);
    assert_eq!(bus.audio_level(), stereo(0));
    for sample in [4, -4, -512, 508, 0, 0] {
        bus.advance_cycles(9);
        let previous = bus.audio_level();
        bus.advance_cycles(0);
        assert_eq!(bus.audio_level(), previous);
        bus.advance_cycles(1);
        assert_eq!(bus.audio_level(), stereo(sample));
    }
    assert!(bus.read32(FIFO_A).is_err());
}

#[test]
fn independent_timer_selection_cascade_routing_volume_and_clipping() {
    let mut bus = bus();
    bus.write16(SOUNDCNT_H, 0x610c).unwrap(); // A right/timer0, B left/timer1, both 100%.
    bus.write32(FIFO_A, 0x7f7f7f7f).unwrap();
    bus.write32(FIFO_B, 0x80808080).unwrap();
    timer(&mut bus, 0, 2, 0x80);
    timer(&mut bus, 1, 2, 0x84); // Cascade: one B sample per two A samples.
    bus.advance_cycles(2);
    assert_eq!(
        bus.audio_level(),
        StereoLevel {
            left: 0,
            right: 508
        }
    );
    bus.advance_cycles(2);
    assert_eq!(
        bus.audio_level(),
        StereoLevel {
            left: -512,
            right: 508
        }
    );
    bus.write16(SOUNDCNT_H, 0x3300).unwrap(); // Both sides, both 50%, retain held samples.
    assert_eq!(bus.audio_level(), stereo(-2));
    bus.write16(SOUNDBIAS, 0).unwrap();
    assert_eq!(bus.audio_level(), stereo(-512));
    bus.write16(SOUNDBIAS, 0x3fe).unwrap();
    bus.write16(SOUNDCNT_H, 0x0304).unwrap();
    assert_eq!(bus.audio_level(), stereo(511));
}

#[test]
fn muted_routes_still_consume_but_master_disable_freezes_playback() {
    let mut bus = bus();
    bus.write16(SOUNDCNT_H, 4).unwrap();
    bus.write32(FIFO_A, 0x04030201).unwrap();
    timer(&mut bus, 0, 1, 0x80);
    bus.advance_cycles(1);
    assert_eq!(bus.audio_level(), stereo(0));
    bus.write16(SOUNDCNT_H, 0x0304).unwrap();
    assert_eq!(bus.audio_level(), stereo(4));
    bus.write16(SOUNDCNT_X, 0).unwrap();
    bus.advance_cycles(100);
    assert_eq!(bus.audio_level(), stereo(0));
    bus.write16(SOUNDCNT_X, 0x80).unwrap();
    bus.advance_cycles(1);
    assert_eq!(bus.audio_level(), stereo(8)); // In-flight word is separate from the reset queue.
}

#[test]
fn reset_strobes_clear_queues_not_in_flight_word_or_held_level() {
    let mut bus = bus();
    bus.write32(FIFO_A, 0x04030201).unwrap();
    bus.write32(FIFO_A, 0x7f7f7f7f).unwrap();
    timer(&mut bus, 0, 1, 0x80);
    bus.advance_cycles(1);
    bus.write16(SOUNDCNT_H, 0x8b04).unwrap();
    assert_eq!(bus.read16(SOUNDCNT_H).unwrap(), 0x0304);
    assert_eq!(bus.audio_level(), stereo(4));
    for sample in [8, 12, 16, 0] {
        bus.advance_cycles(1);
        assert_eq!(bus.audio_level(), stereo(sample));
    }
}

#[test]
fn partial_writes_enqueue_words_and_preserve_other_lanes_on_reused_slots() {
    let mut bus = bus();
    timer(&mut bus, 0, 1, 0x80);
    // Visit each of the seven queue slots. Reuse the first with a byte write.
    for _ in 0..7 {
        bus.write32(FIFO_A, 0x04030201).unwrap();
        bus.advance_cycles(4);
    }
    bus.write8(FIFO_A + 1, 9).unwrap();
    for sample in [4, 36, 12, 16] {
        bus.advance_cycles(1);
        assert_eq!(bus.audio_level(), stereo(sample));
    }
    bus.write16(FIFO_A + 2, 0x0807).unwrap();
    for sample in [4, 8, 28, 32] {
        bus.advance_cycles(1);
        assert_eq!(bus.audio_level(), stereo(sample));
    }
}

#[test]
fn full_queue_overflow_clears_queued_words() {
    let mut bus = bus();
    for _ in 0..8 {
        bus.write32(FIFO_A, 0x7f7f7f7f).unwrap();
    }
    timer(&mut bus, 0, 1, 0x80);
    bus.advance_cycles(1);
    assert_eq!(bus.audio_level(), stereo(0));
    bus.write32(FIFO_A, 1).unwrap();
    bus.advance_cycles(1);
    assert_eq!(bus.audio_level(), stereo(4));
}

#[test]
fn wave_banks_and_idle_channel_masks_preserve_explicit_limits() {
    let mut bus = bus();
    bus.write16(0x04000062, 0xffff).unwrap();
    assert_eq!(bus.read16(0x04000062).unwrap(), 0xffc0);
    bus.write32(WAVE_RAM, 0x11223344).unwrap(); // CPU bank 1.
    bus.write16(0x04000070, 0x40).unwrap();
    assert_eq!(bus.read32(WAVE_RAM).unwrap(), 0);
    bus.write32(WAVE_RAM, 0xaabbccdd).unwrap();
    let previous = bus.read16(0x04000074).unwrap();
    assert!(bus
        .write16(0x04000074, 0xffff)
        .unwrap_err()
        .to_string()
        .contains("PSG channel trigger"));
    assert_eq!(bus.read16(0x04000074).unwrap(), previous);
    bus.write16(SOUNDCNT_X, 0).unwrap();
    assert_eq!(bus.read32(WAVE_RAM).unwrap(), 0x11223344);
    assert_eq!(bus.read16(0x04000062).unwrap(), 0);
    bus.write16(0x04000064, 0xffff).unwrap(); // Disabled PSG writes are ignored.
    bus.write16(SOUNDCNT_X, 0xff).unwrap();
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x80);
    bus.write16(0x04000070, 0x40).unwrap();
    assert_eq!(bus.read32(WAVE_RAM).unwrap(), 0xaabbccdd);
}

#[test]
fn sound_dma_forces_four_words_and_fixed_destination_then_repeats_and_interrupts() {
    for channel in [1, 2] {
        let mut bus = bus();
        let fifo = FIFO_A + (channel - 1) * 4;
        let route = if channel == 1 { 0x0304 } else { 0x3008 };
        bus.write16(SOUNDCNT_H, route).unwrap();
        for index in 0..8 {
            bus.write32(0x02000000 + index * 4, 0x01010101 * (index + 1))
                .unwrap();
        }
        sound_dma(&mut bus, channel, 0x4200); // IRQ/repeat, programmed count1, halfword/increment destination.
        timer(&mut bus, 0, 100, 0x80);
        bus.advance_cycles(100); // Empty playback, request.
        let mut machine = Machine::new(Cpu::new(ROM_START), bus);
        for _ in 0..4 {
            assert_eq!(
                machine.step().unwrap(),
                StepKind::Dma {
                    channel: channel as usize
                }
            );
        }
        assert_ne!(
            machine.memory().read16(IF).unwrap() & (1 << (8 + channel)),
            0
        );
        assert_ne!(
            machine
                .memory()
                .read16(DMA_BASE + channel * DMA_STRIDE + 10)
                .unwrap()
                & 0x8000,
            0
        );
        // No fifth unit. Four queued words suppress the next request BEFORE the playback word loads.
        assert!(matches!(machine.step().unwrap(), StepKind::Instruction));
        let remaining = 100 - (machine.cycles() as u32 % 100);
        machine.memory_mut().advance_cycles(remaining);
        assert_eq!(machine.memory().audio_level(), stereo(4));
        assert!(matches!(machine.step().unwrap(), StepKind::Instruction));
        let remaining = 100 - (machine.cycles() as u32 % 100);
        machine.memory_mut().advance_cycles(remaining);
        for _ in 0..4 {
            assert_eq!(
                machine.step().unwrap(),
                StepKind::Dma {
                    channel: channel as usize
                }
            );
        }
        // Drain without servicing further requests. The repeated block must continue the source,
        // use four-byte strides, and never advance the FIFO destination into the other channel.
        for offset in 0..30 {
            let remaining = 100 - (machine.cycles() as u32 % 100);
            machine.memory_mut().advance_cycles(remaining);
            assert_eq!(
                machine.memory().audio_level(),
                stereo(((offset + 2) / 4 + 1) * 4)
            );
        }
        assert!(machine.memory().read32(fifo).is_err());
    }
}

#[test]
fn nonrepeat_sound_dma_disables_after_four_words() {
    let mut bus = bus();
    sound_dma(&mut bus, 1, 0);
    timer(&mut bus, 0, 1000, 0x80);
    bus.advance_cycles(1000);
    let mut machine = Machine::new(Cpu::new(ROM_START), bus);
    for _ in 0..4 {
        assert_eq!(machine.step().unwrap(), StepKind::Dma { channel: 1 });
    }
    assert_eq!(
        machine.memory().read16(DMA_BASE + DMA_STRIDE + 10).unwrap() & 0x8000,
        0
    );
}

#[test]
fn halt_clocks_samples_and_stop_freezes_them() {
    for stop in [false, true] {
        let mut bus = bus();
        bus.write32(FIFO_A, 0x04030201).unwrap();
        timer(&mut bus, 0, 10, 0x80);
        bus.write8(HALTCNT, if stop { 0x80 } else { 0 }).unwrap();
        let mut machine = Machine::new(Cpu::new(ROM_START), bus);
        machine.step().unwrap();
        assert_eq!(
            machine.memory().audio_level(),
            stereo(if stop { 0 } else { 4 })
        );
        if stop {
            machine.memory_mut().advance_cycles(u32::MAX);
            assert_eq!(machine.cycles(), 0);
        }
    }
}

#[test]
fn large_device_batches_match_single_clocks_with_and_without_row_capture() {
    for capture in [false, true] {
        let mut bulk = bus();
        let mut small = bus();
        for bus in [&mut bulk, &mut small] {
            if capture {
                bus.set_scanline_rendering(true);
            }
            bus.write16(SOUNDCNT_H, 0x730c).unwrap();
            for index in 1..=7 {
                bus.write32(FIFO_A, index * 0x01010101).unwrap();
                bus.write32(FIFO_B, (index + 8) * 0x01010101).unwrap();
            }
            timer(bus, 0, 2, 0x80);
            timer(bus, 1, 3, 0x80);
        }
        for cycles in [1, 2, 3, 17, 39, 1000] {
            bulk.advance_cycles(cycles);
            for _ in 0..cycles {
                small.advance_cycles(1);
            }
            assert_eq!(bulk.audio_level(), small.audio_level());
        }
        bulk.set_scanline_rendering(false);
        bulk.advance_cycles(u32::MAX);
        assert_eq!(bulk.audio_level(), stereo(0));
    }
}
