//! The benchmark ABI's behavior, through `Session` and the C exports, on original ROMs.
use gba_core::{bios, cartridge::SaveDevice, io::KEYINPUT};
use gba_emu::{save_device, Session, AUDIO_RATE};

fn words(code: &[u32]) -> Vec<u8> {
    code.iter().flat_map(|word| word.to_le_bytes()).collect()
}

/// B . — a cartridge that idles; the display and sound clocks still run.
fn idle_rom() -> Vec<u8> {
    words(&[0xeaff_fffe])
}

fn session(rom: Vec<u8>) -> Session {
    let mut session = Session::new(bios::image());
    session.load(rom).unwrap();
    session
}

fn keyinput(session: &Session) -> u16 {
    session
        .machine()
        .unwrap()
        .memory()
        .read16(KEYINPUT)
        .unwrap()
}

#[test]
fn frames_end_at_vblank_and_present_opaque_abgr_pixels() {
    let mut session = session(idle_rom());
    let start = session.machine().unwrap().memory().display_position();
    assert_eq!(start.scanline, bios::SKIP_BOOT_LINE as u16);
    session.run_frame();
    let position = session.machine().unwrap().memory().display_position();
    assert_eq!(position.scanline, 160);
    assert_eq!(position.vblanks, start.vblanks + 1);
    // Forced blank after the skipped boot: white, 0xAABBGGRR with alpha 0xff.
    assert_eq!(session.pixels().len(), 240 * 160);
    assert!(session.pixels().iter().all(|&pixel| pixel == 0xffff_ffff));
    assert!(session.error().is_none());
}

#[test]
fn keys_take_effect_when_the_frame_ends_like_mesens_vblank_poll() {
    let mut session = session(idle_rom());
    session.run_frame();
    assert_eq!(keyinput(&session), 0x03ff);
    session.set_keys(0x0009); // A and Start
    assert_eq!(keyinput(&session), 0x03ff, "latched, not applied at once");
    session.run_frame();
    assert_eq!(keyinput(&session), 0x03ff & !0x0009);
    session.set_keys(0xffff); // Bits above L are ignored.
    session.run_frame();
    assert_eq!(keyinput(&session), 0);
}

#[test]
fn audio_drains_per_frame_at_the_capture_rate() {
    let mut session = session(idle_rom());
    session.run_frame(); // From line 225: 163 lines.
    session.drain_audio();
    for _ in 0..3 {
        session.run_frame();
        let pairs = session.drain_audio();
        // 280,896 cycles per frame at 512 cycles per 32,768 Hz sample: 548.6.
        assert!((548..=549).contains(&pairs), "{pairs}");
        assert_eq!(
            session.audio().len(),
            pairs * 2,
            "readable until the next frame"
        );
    }
    // Undrained frames accumulate.
    session.run_frame();
    session.run_frame();
    assert!((1097..=1098).contains(&session.drain_audio()));
    assert_eq!(AUDIO_RATE, 32_768);
}

#[test]
fn reset_repeats_the_loaded_start_exactly() {
    // A cartridge that counts frames into r0 by polling VCOUNT.
    let rom = words(&[
        0xe3a0_1301, // MOV r1,#0x04000000
        0xe1d1_20b6, // LDRH r2,[r1,#6] (VCOUNT)
        0xe352_00a0, // CMP r2,#160
        0x1aff_fffc, // BNE back to the LDRH
        0xe280_0001, // ADD r0,r0,#1
        0xe1d1_20b6, // LDRH r2,[r1,#6]
        0xe352_00a0, // CMP r2,#160
        0x0aff_fffc, // BEQ back to the LDRH
        0xeaff_fff7, // B to the first LDRH
    ]);
    let mut session = session(rom);
    for _ in 0..5 {
        session.run_frame();
    }
    let after_five = session.machine().unwrap().cpu().registers()[0];
    assert!(after_five >= 4, "{after_five}");
    session.set_keys(1);
    session.reset().unwrap();
    assert_eq!(session.machine().unwrap().cpu().registers()[0], 0);
    for _ in 0..5 {
        session.run_frame();
    }
    assert_eq!(session.machine().unwrap().cpu().registers()[0], after_five);
    assert_eq!(keyinput(&session), 0x03ff, "reset clears the latched keys");
}

#[test]
fn a_core_diagnostic_stops_the_machine_and_keeps_the_last_frame() {
    let mut session = session(words(&[0xe7f0_00f0])); // An undefined instruction.
    session.run_frame();
    let error = session.error().expect("diagnostic").to_owned();
    assert!(error.contains("0x08000000"), "{error}");
    let pixels = session.pixels().to_vec();
    session.run_frame();
    assert_eq!(session.pixels(), &pixels[..]);
    assert_eq!(session.error(), Some(&error[..]));
}

#[test]
fn save_device_follows_mesens_auto_detection() {
    let rom = |text: &[u8], code: &[u8; 4]| {
        let mut rom = vec![0; 0x200];
        rom[0xac..0xb0].copy_from_slice(code);
        rom[0x101..0x101 + text.len()].copy_from_slice(text); // Unaligned is fine for Mesen.
        rom
    };
    assert_eq!(save_device(&rom(b"", b"TEST")), SaveDevice::Sram);
    assert_eq!(
        save_device(&rom(b"EEPROM_V124", b"TEST")),
        SaveDevice::Eeprom
    );
    assert_eq!(
        save_device(&rom(b"FLASH512_V131", b"TEST")),
        SaveDevice::Flash64
    );
    assert_eq!(
        save_device(&rom(b"FLASH1M_V103", b"TEST")),
        SaveDevice::Flash128
    );
    // Mesen checks SRAM_V before the others.
    assert_eq!(
        save_device(&rom(b"FLASH1M_V103 SRAM_V113", b"TEST")),
        SaveDevice::Sram
    );
    assert_eq!(save_device(&rom(b"SRAM_V113", b"AI2E")), SaveDevice::None);
    assert_eq!(save_device(&rom(b"", b"ALUE")), SaveDevice::Eeprom512);
}

#[test]
fn the_c_exports_load_run_and_report_through_stable_buffers() {
    use gba_emu::*;
    assert_eq!(emu_init(), 1);
    let rom = idle_rom();
    let buffer = emu_rom_buffer();
    // SAFETY: the buffer holds ROM_BUFFER_SIZE bytes.
    unsafe { std::ptr::copy_nonoverlapping(rom.as_ptr(), buffer, rom.len()) };
    assert_eq!(emu_load_rom(rom.len() as i32), 1);
    assert_eq!(emu_load_rom(0), 0);
    assert_eq!(emu_load_rom(rom.len() as i32), 1);
    let framebuffer = emu_framebuffer();
    let audio = emu_audio_buffer();
    for _ in 0..BOOT_FRAMES {
        emu_run_frame();
        emu_audio_samples();
    }
    emu_set_keys(1);
    emu_run_frame();
    assert!((548..=549).contains(&emu_audio_samples()));
    assert_eq!(emu_framebuffer(), framebuffer, "stable framebuffer");
    assert_eq!(emu_audio_buffer(), audio, "stable audio buffer");
    // SAFETY: 240×160 pixels.
    let pixels = unsafe { std::slice::from_raw_parts(framebuffer, 240 * 160) };
    assert!(pixels.iter().all(|&pixel| pixel >> 24 == 0xff));
    assert_eq!(emu_reset(), 1);
    assert_eq!(emu_audio_rate(), 32_768);
    assert_eq!(emu_boot_frames(), 6);
    assert_eq!(emu_rom_buffer(), buffer, "stable ROM buffer");
}

#[test]
fn brightness_decrease_rounds_down_like_the_mesen_reference() {
    use gba_demos::blend_rounding::{blend_rounding_rom, block_center, Side, BANDS};
    let mut session = session(blend_rounding_rom());
    for _ in 0..3 {
        session.run_frame();
    }
    let level = |(x, y): (usize, usize)| session.pixels()[y * 240 + x] & 0xff;
    for (band_index, band) in BANDS.iter().enumerate() {
        for (group_index, group) in band.groups().iter().enumerate() {
            let expected = match group.mesen {
                Side::Lower => group.lower,
                Side::Higher => group.higher,
            };
            let sample = level(block_center(band_index, group_index, 1)) >> 3;
            assert_eq!(sample, expected, "{band:?} {group:?}");
        }
    }
}
