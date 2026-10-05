use super::*;

#[test]
fn static_capture_matches_snapshots_in_every_mode_with_objects_mosaic_windows_and_effects() {
    for mode in 0..=5 {
        let mut m = memory();
        for i in 0..512 {
            m.write16(PAL + i * 2, (i as u16).wrapping_mul(0x421))
                .unwrap();
        }
        for i in 0..128 {
            m.write16(OAM + i * 8, 0x200).unwrap();
        }
        for offset in (0..0x14000).step_by(2) {
            let value = if mode < 3 {
                if offset < 0x4000 {
                    0x1111
                } else {
                    0
                }
            } else {
                (offset as u16).wrapping_mul(17)
            };
            m.write16(VRAM + offset, value).unwrap();
        }
        m.write16(DISPCNT, 0x3440 | mode).unwrap();
        m.write16(BG2CNT, 0x1041).unwrap();
        m.write16(BG2PA, 181).unwrap();
        m.write16(BG2PB, 91).unwrap();
        m.write16(BG2PC, (-91i16) as u16).unwrap();
        m.write16(BG2PD, 256).unwrap();
        m.write32(BG2X, 513).unwrap();
        m.write32(BG2Y, 7 * 256).unwrap();
        m.write16(OAM, 0x1003).unwrap();
        m.write16(OAM + 2, 5).unwrap();
        m.write16(OAM + 4, if mode >= 3 { 512 } else { 0 }).unwrap();
        let tiles = if mode >= 3 { 0x14000 } else { 0x10000 };
        for offset in (0..32).step_by(2) {
            m.write16(VRAM + tiles + offset, 0x1111).unwrap();
        }
        m.write16(MOSAIC, 0x3243).unwrap();
        m.write16(WIN0H, 0x0a3c).unwrap();
        m.write16(WIN0V, 0x0432).unwrap();
        m.write16(WININ, 0x1f).unwrap();
        m.write16(WINOUT, 0x3f).unwrap();
        m.write16(BLDCNT, 0xbf).unwrap();
        m.write16(BLDY, 5).unwrap();
        m.write16(GREENSWAP, 1).unwrap();
        let mut expected = Framebuffer::default();
        m.render_frame(&mut expected).unwrap();
        m.advance_cycles(VBLANK_START);
        assert_eq!(present(&m).pixels(), expected.pixels(), "mode={mode}");
    }
}

#[test]
fn page_palette_vram_and_oam_writes_only_change_rows_not_yet_captured() {
    let mut m = memory();
    m.write16(DISPCNT, 0x404).unwrap();
    m.write16(BG2PA, 256).unwrap();
    m.write16(BG2PD, 256).unwrap();
    for i in 0..128 {
        m.write16(OAM + i * 8, 0x200).unwrap();
    }
    for offset in (0..38400).step_by(2) {
        m.write16(VRAM + offset, 0x0101).unwrap();
        m.write16(VRAM + 0xa000 + offset, 0x0202).unwrap();
    }
    for (index, color) in [(1, 31), (2, 0x3e0), (3, 0x7fff)] {
        m.write16(PAL + index * 2, color).unwrap();
    }
    m.advance_cycles(HBLANK_START); // Row0 page0 red.
    m.write16(DISPCNT, 0x414).unwrap();
    m.advance_cycles(CYCLES_PER_LINE); // Row1 page1 green.
    m.write16(PAL + 4, 0x7c00).unwrap();
    m.advance_cycles(CYCLES_PER_LINE); // Row2 same index, now blue.
    m.write16(VRAM + 0xa000 + 3 * 240, 0x0303).unwrap();
    m.advance_cycles(CYCLES_PER_LINE); // Row3 first two pixels now white.
    m.write16(DISPCNT, 0x1454).unwrap();
    m.write16(OAM, 0).unwrap();
    m.write16(OAM + 4, 512).unwrap();
    for offset in (0..32).step_by(2) {
        m.write16(VRAM + 0x14000 + offset, 0x1111).unwrap();
    }
    m.write16(PAL + 0x202, 0).unwrap(); // Opaque black OBJ.
    m.advance_cycles(VBLANK_START - (3 * CYCLES_PER_LINE + HBLANK_START));
    let f = present(&m);
    // Row4's sprites were prepared before the row3 HBlank writes; row5 sees them.
    for (y, color) in [
        (0, 31),
        (1, 0x3e0),
        (2, 0x7c00),
        (3, 0x7fff),
        (4, 0x7c00),
        (5, 0),
    ] {
        assert_eq!(at(&f, 0, y), rgb(color));
    }
    assert_eq!(at(&f, 8, 4), rgb(0x7c00));
    assert_eq!(at(&f, 0, 8), rgb(0x7c00));
}

#[test]
fn scroll_mosaic_window_effect_and_forced_blank_changes_apply_per_row() {
    let mut m = memory();
    m.write16(DISPCNT, 0x100).unwrap();
    m.write16(BG0CNT, 0x1000).unwrap();
    m.write16(PAL, 0x4210).unwrap();
    m.write16(PAL + 2, 31).unwrap();
    m.write16(PAL + 4, 0x3e0).unwrap();
    for offset in (0..32).step_by(2) {
        m.write16(VRAM + offset, 0x2121).unwrap();
    }
    m.advance_cycles(HBLANK_START);
    m.write16(BG0HOFS, 1).unwrap();
    m.advance_cycles(CYCLES_PER_LINE);
    m.write16(BG0CNT, 0x1040).unwrap();
    m.write16(MOSAIC, 1).unwrap();
    m.advance_cycles(CYCLES_PER_LINE);
    m.write16(DISPCNT, 0x2100).unwrap();
    m.write16(WIN0H, 1).unwrap();
    // Set the next row's top edge; a missed row-zero edge cannot open the window.
    m.write16(WIN0V, 0x03a0).unwrap();
    m.write16(WININ, 0).unwrap();
    m.write16(WINOUT, 0x21).unwrap();
    m.write16(BLDCNT, 0xc1).unwrap();
    m.write16(BLDY, 8).unwrap();
    m.advance_cycles(CYCLES_PER_LINE);
    m.write16(DISPCNT, 0x80).unwrap();
    m.advance_cycles(VBLANK_START - (3 * CYCLES_PER_LINE + HBLANK_START));
    let f = present(&m);
    assert_eq!(&f.pixels()[..2], &[rgb(31), rgb(0x3e0)]);
    assert_eq!(&f.pixels()[WIDTH..WIDTH + 2], &[rgb(0x3e0), rgb(31)]);
    assert_eq!(&f.pixels()[2 * WIDTH..2 * WIDTH + 2], &[rgb(0x3e0); 2]);
    assert_eq!(at(&f, 0, 3), rgb(0x4210));
    assert_eq!(at(&f, 1, 3), rgb(0x200));
    assert!(f.pixels()[4 * WIDTH..].iter().all(|&p| p == 0xffffff));
}

#[test]
fn green_swap_setting_is_captured_per_row_after_color_composition() {
    let mut m = memory();
    m.write16(DISPCNT, 0x403).unwrap();
    m.write16(BG2PA, 256).unwrap();
    m.write16(BG2PD, 256).unwrap();
    for row in 0..2 {
        m.write32(VRAM + row * 480, 0x03e0_001f).unwrap();
    }
    m.advance_cycles(HBLANK_START);
    m.write16(GREENSWAP, 1).unwrap();
    m.advance_cycles(VBLANK_START - HBLANK_START);
    let f = present(&m);
    assert_eq!(&f.pixels()[..2], &[0xff0000, 0xff00]);
    assert_eq!(&f.pixels()[WIDTH..WIDTH + 2], &[0xffff00, 0]);
}
