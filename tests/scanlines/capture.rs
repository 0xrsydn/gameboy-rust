use super::*;

#[test]
fn capture_is_optional_and_snapshot_api_remains_independent() {
    let mut m = Memory::new(vec![]).unwrap();
    m.write16(PAL, 31).unwrap();
    m.advance_cycles(CYCLES_PER_FRAME * 2);
    let mut f = Framebuffer::default();
    f.clear(0x3e0);
    assert!(!m.present_frame(&mut f).unwrap());
    assert!(f.pixels().iter().all(|&p| p == rgb(0x3e0)));
    assert_eq!(m.captured_vblank(), None);
    m.render_frame(&mut f).unwrap();
    assert!(f.pixels().iter().all(|&p| p == rgb(31)));
    let cycles = m.cycles();
    m.set_scanline_rendering(true);
    assert_eq!(m.cycles(), cycles);
    m.advance_cycles(VBLANK_START);
    assert_eq!(m.captured_vblank(), Some(3));
    assert_eq!(present(&m).pixels(), f.pixels());
}

#[test]
fn hblank_captures_old_row_and_later_palette_writes_do_not_recolor_it() {
    let mut m = memory();
    m.write16(PAL, 31).unwrap();
    m.advance_cycles(HBLANK_START);
    m.write16(PAL, 0x3e0).unwrap();
    m.advance_cycles(VBLANK_START - HBLANK_START);
    let f = present(&m);
    for y in 0..HEIGHT {
        assert!(f.pixels()[y * WIDTH..(y + 1) * WIDTH]
            .iter()
            .all(|&p| p == rgb(if y == 0 { 31 } else { 0x3e0 })));
    }
    m.write16(PAL, 0x7c00).unwrap();
    assert_eq!(present(&m).pixels(), f.pixels());
    let cycles = m.cycles();
    let mut snapshot = Framebuffer::default();
    m.render_frame(&mut snapshot).unwrap();
    assert!(snapshot.pixels().iter().all(|&p| p == rgb(0x7c00)));
    assert_eq!(m.cycles(), cycles);
}

#[test]
fn last_row_capture_does_not_publish_before_vblank_entry() {
    let mut m = memory();
    m.write16(PAL, 31).unwrap();
    let mut f = Framebuffer::default();
    f.clear(0x3e0);
    m.advance_cycles(159 * CYCLES_PER_LINE + HBLANK_START);
    assert!(!m.present_frame(&mut f).unwrap());
    m.advance_cycles(CYCLES_PER_LINE - HBLANK_START - 1);
    assert!(!m.present_frame(&mut f).unwrap());
    assert!(f.pixels().iter().all(|&p| p == rgb(0x3e0)));
    m.advance_cycles(1);
    assert!(m.present_frame(&mut f).unwrap());
    assert_eq!(m.captured_vblank(), Some(1));
    assert!(f.pixels().iter().all(|&p| p == rgb(31)));
}

#[test]
fn enabling_mid_frame_discards_partial_frame_and_repeated_enable_preserves_progress() {
    for start in [HBLANK_START, 40 * CYCLES_PER_LINE, VBLANK_START - 1] {
        let mut m = Memory::new(vec![]).unwrap();
        m.advance_cycles(start);
        m.set_scanline_rendering(true);
        m.write16(PAL, 31).unwrap();
        m.advance_cycles(VBLANK_START - start);
        assert_eq!(m.captured_vblank(), None);
        m.advance_cycles(CYCLES_PER_FRAME - VBLANK_START + HBLANK_START);
        m.set_scanline_rendering(true); // Must not reset row0.
        m.advance_cycles(VBLANK_START - HBLANK_START);
        assert_eq!(m.captured_vblank(), Some(2));
        assert!(present(&m).pixels().iter().all(|&p| p == rgb(31)));
        m.set_scanline_rendering(false);
        assert_eq!(m.captured_vblank(), None);
        assert!(!m.present_frame(&mut Framebuffer::default()).unwrap());
    }
}

#[test]
fn enable_before_first_hblank_and_during_vblank_can_capture_next_complete_image() {
    for start in [0, HBLANK_START - 1, VBLANK_START, CYCLES_PER_FRAME - 1] {
        let mut m = Memory::new(vec![]).unwrap();
        m.advance_cycles(start);
        m.set_scanline_rendering(true);
        let target = if start < HBLANK_START {
            VBLANK_START
        } else {
            CYCLES_PER_FRAME + VBLANK_START
        };
        m.advance_cycles(target - start);
        assert_eq!(
            m.captured_vblank(),
            Some(if start < HBLANK_START { 1 } else { 2 })
        );
        assert!(m.present_frame(&mut Framebuffer::default()).unwrap());
    }
}

#[test]
fn large_batches_publish_latest_frame_and_zero_cycles_have_no_effect() {
    let mut m = memory();
    m.write16(PAL, 0x4210).unwrap();
    m.advance_cycles(CYCLES_PER_FRAME * 3 + VBLANK_START);
    assert_eq!(m.captured_vblank(), Some(4));
    let before = m.display_position();
    let f = present(&m);
    m.advance_cycles(0);
    m.write16(PAL, 0).unwrap();
    assert_eq!(m.display_position(), before);
    assert_eq!(present(&m).pixels(), f.pixels());
    assert!(f.pixels().iter().all(|&p| p == rgb(0x4210)));
}

#[test]
fn first_render_error_is_reported_at_publication_without_partial_output_then_recovers() {
    let mut m = memory();
    m.write16(PAL, 31).unwrap();
    m.advance_cycles(VBLANK_START);
    let mut f = present(&m);
    m.advance_cycles(CYCLES_PER_FRAME - VBLANK_START);
    m.write16(DISPCNT, 6).unwrap();
    m.advance_cycles(HBLANK_START);
    m.write16(DISPCNT, 7).unwrap();
    m.advance_cycles(CYCLES_PER_LINE);
    m.write16(DISPCNT, 0).unwrap();
    assert!(m.present_frame(&mut f).unwrap()); // Previous frame remains available while drawing.
    m.advance_cycles(VBLANK_START - HBLANK_START - CYCLES_PER_LINE);
    assert_eq!(m.captured_vblank(), Some(2));
    assert_eq!(m.present_frame(&mut f), Err(VideoError::UnsupportedMode(6)));
    assert!(f.pixels().iter().all(|&p| p == rgb(31)));
    m.write16(PAL, 0x3e0).unwrap();
    m.advance_cycles(CYCLES_PER_FRAME);
    assert_eq!(m.captured_vblank(), Some(3));
    assert!(m.present_frame(&mut f).unwrap());
    assert!(f.pixels().iter().all(|&p| p == rgb(0x3e0)));
}

#[test]
fn split_clock_advance_matches_single_cycle_reference_and_disabled_capture_devices() {
    let mut bulk = memory();
    let mut single = memory();
    let mut disabled = Memory::new(vec![]).unwrap();
    for m in [&mut bulk, &mut single, &mut disabled] {
        m.write16(DISPSTAT, 0x5038).unwrap();
        m.write32(TIMER_BASE, 0x00c0_fff1).unwrap();
        m.write16(IE, 15).unwrap();
        m.write8(HALTCNT, 0).unwrap();
        m.write16(PAL, 0x4210).unwrap();
    }
    let cycles = CYCLES_PER_FRAME + VBLANK_START + 73;
    bulk.advance_cycles(cycles);
    disabled.advance_cycles(cycles);
    for _ in 0..cycles {
        single.advance_cycles(1);
    }
    for m in [&single, &disabled] {
        assert_eq!(bulk.display_position(), m.display_position());
        assert_eq!(bulk.cycles(), m.cycles());
        assert_eq!(bulk.halted(), m.halted());
        for register in [DISPSTAT, IF, TIMER_BASE] {
            assert_eq!(bulk.read16(register), m.read16(register));
        }
    }
    assert_eq!(present(&bulk).pixels(), present(&single).pixels());
    assert_eq!(bulk.captured_vblank(), single.captured_vblank());
}
