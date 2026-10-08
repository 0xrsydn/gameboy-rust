//! Save states must be indistinguishable from uninterrupted execution.
//! Original programs only; no game ROMs or recordings.
use gba_core::{
    audio::{StereoLevel, AUDIO_QUEUE_CAPACITY},
    bios,
    cartridge::{CartridgeHardware, SaveDevice, SAVE_START},
    input::{Button, Buttons},
    io::{SOUNDBIAS, SOUNDCNT_H, SOUNDCNT_X, SOUND_START, TIMER_BASE},
    machine::Machine,
    snapshot::SnapshotError,
    video::Framebuffer,
};
use gba_demos::{input_rom, tile_demo};

const FRAME_STEPS: usize = 400_000;

fn configured(rom: Vec<u8>, device: SaveDevice) -> Machine {
    let mut machine = bios::boot(rom).unwrap();
    let memory = machine.memory_mut();
    memory.set_cartridge_hardware(CartridgeHardware::Rtc);
    memory.set_save_device(device);
    memory.set_scanline_rendering(true);
    memory.set_audio_capture(true);
    machine
}

/// Free-running timers, a triggered pulse channel, and master sound enable, so the
/// state under test includes devices that the demo program itself never touches.
fn start_devices(machine: &mut Machine) {
    let memory = machine.memory_mut();
    memory.write16(SOUNDCNT_X, 0x80).unwrap();
    memory.write16(SOUNDBIAS, 0x200).unwrap();
    memory.write16(SOUNDCNT_H, 0x0302).unwrap();
    memory.write16(SOUND_START + 2, 0xf141).unwrap(); // Duty, envelope sweep.
    memory.write16(SOUND_START + 4, 0x8600).unwrap(); // Frequency and trigger.
    memory.write32(TIMER_BASE, 0x0081_f000).unwrap(); // /1 timer.
    memory.write32(TIMER_BASE + 4, 0x0084_ffff).unwrap(); // Cascaded timer.
}

fn buttons(frame: u64) -> Buttons {
    Buttons::default()
        .with(Button::Right, frame % 7 < 3)
        .with(Button::Down, frame % 11 < 4)
        .with(Button::A, frame % 5 == 0)
        .with(Button::L, frame % 13 < 6)
}

fn fold(hash: &mut u64, value: u64) {
    *hash = (*hash ^ value).wrapping_mul(0x0000_0100_0000_01b3);
}

/// One observation per frame: pixels, audio, registers, and the device clock.
fn run(machine: &mut Machine, frames: std::ops::Range<u64>) -> Vec<u64> {
    let mut frame = Framebuffer::default();
    let mut samples = vec![StereoLevel::default(); AUDIO_QUEUE_CAPACITY];
    let mut trace = Vec::new();
    for index in frames {
        machine.memory_mut().set_buttons(buttons(index));
        machine.run_until_vblank(FRAME_STEPS).unwrap();
        let mut hash = 0xcbf2_9ce4_8422_2325;
        if machine.memory().present_frame(&mut frame).unwrap() {
            frame
                .pixels()
                .iter()
                .for_each(|&p| fold(&mut hash, p.into()));
        }
        let count = machine.memory_mut().drain_audio_samples(&mut samples);
        for level in &samples[..count] {
            fold(&mut hash, level.left as u64);
            fold(&mut hash, level.right as u64);
        }
        machine
            .cpu()
            .registers()
            .iter()
            .for_each(|&r| fold(&mut hash, r.into()));
        fold(&mut hash, machine.cycles());
        trace.push(hash);
    }
    trace
}

#[test]
fn restored_machine_continues_exactly_like_the_original() {
    let mut original = configured(tile_demo::rom(), SaveDevice::Flash128);
    start_devices(&mut original);
    run(&mut original, 0..40);
    let state = original.save_state().unwrap();
    let expected = run(&mut original, 40..70);
    let expected_end = original.save_state().unwrap();

    let mut restored = configured(tile_demo::rom(), SaveDevice::Flash128);
    restored.load_state(&state).unwrap();
    assert_eq!(run(&mut restored, 40..70), expected);
    assert_eq!(restored.save_state().unwrap(), expected_end);
    assert_ne!(state, expected_end, "the run must change the state");
}

#[test]
fn saving_and_loading_the_same_machine_is_idempotent() {
    let mut machine = configured(tile_demo::rom(), SaveDevice::Flash64);
    start_devices(&mut machine);
    run(&mut machine, 0..25);
    let state = machine.save_state().unwrap();
    let expected = run(&mut machine, 25..35);
    machine.load_state(&state).unwrap();
    assert_eq!(machine.save_state().unwrap(), state);
    assert_eq!(run(&mut machine, 25..35), expected);
}

#[test]
fn flash_operation_in_flight_completes_identically_after_restore() {
    let command = |machine: &mut Machine, byte: u8| {
        let memory = machine.memory_mut();
        memory.write8(SAVE_START + 0x5555, 0xaa).unwrap();
        memory.write8(SAVE_START + 0x2aaa, 0x55).unwrap();
        memory.write8(SAVE_START + 0x5555, byte).unwrap();
    };
    let mut original = configured(input_rom(), SaveDevice::Flash128);
    command(&mut original, 0xa0);
    original
        .memory_mut()
        .write8(SAVE_START + 0x123, 0x42)
        .unwrap();
    for _ in 0..10 {
        original.step().unwrap();
    }
    assert!(
        original.memory().save_write_pending(),
        "operation must be in flight"
    );
    let state = original.save_state().unwrap();

    let mut restored = configured(input_rom(), SaveDevice::Flash128);
    restored.load_state(&state).unwrap();
    assert!(restored.memory().save_write_pending());
    for machine in [&mut original, &mut restored] {
        for _ in 0..200_000 {
            machine.step().unwrap();
        }
    }
    assert!(!original.memory().save_write_pending());
    assert_eq!(
        original.memory().save_image(),
        restored.memory().save_image()
    );
    assert_eq!(original.memory().save_image().unwrap()[0x123], 0x42);
    assert!(restored.memory().save_modified());
    assert_eq!(
        original.save_state().unwrap(),
        restored.save_state().unwrap()
    );
}

#[test]
fn rejected_states_leave_the_machine_untouched() {
    let mut source = configured(tile_demo::rom(), SaveDevice::Flash128);
    run(&mut source, 0..10);
    let state = source.save_state().unwrap();

    let mut target = configured(tile_demo::rom(), SaveDevice::Flash128);
    run(&mut target, 0..3);
    let before = target.save_state().unwrap();

    let mut wrong_rom = state.clone();
    // Same layout, different image: rebuild a source from another ROM.
    let other = configured(input_rom(), SaveDevice::Flash128);
    wrong_rom.splice(.., other.save_state().unwrap());
    let mut flipped = state.clone();
    let middle = flipped.len() / 2;
    flipped[middle] ^= 0x01;
    let mut version = state.clone();
    version[4..8].copy_from_slice(&99u32.to_le_bytes());
    let mut magic = state.clone();
    magic[0] = b'X';

    let cases = [
        (wrong_rom, SnapshotError::ConfigMismatch("ROM image")),
        (flipped, SnapshotError::Checksum),
        (version, SnapshotError::UnsupportedVersion(99)),
        (magic, SnapshotError::BadMagic),
        (state[..state.len() - 1].to_vec(), SnapshotError::Checksum),
        (Vec::new(), SnapshotError::Truncated),
    ];
    for (bytes, error) in cases {
        assert_eq!(target.load_state(&bytes), Err(error));
        assert_eq!(target.save_state().unwrap(), before);
    }

    let mut other_device = configured(tile_demo::rom(), SaveDevice::Flash64);
    let before = other_device.save_state().unwrap();
    assert_eq!(
        other_device.load_state(&state),
        Err(SnapshotError::ConfigMismatch("cartridge setup"))
    );
    assert_eq!(other_device.save_state().unwrap(), before);
}
