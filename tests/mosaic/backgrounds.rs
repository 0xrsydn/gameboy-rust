use super::*;

fn bitmap(mode: u16, page: u16) -> Memory {
    let mut m = memory();
    m.write16(DISPCNT, 0x400 | mode | (page * 16)).unwrap();
    m.write16(BG2PA, 256).unwrap();
    m.write16(BG2PD, 256).unwrap();
    let (w, h) = if mode == 5 { (160, 128) } else { (240, 160) };
    for y in 0..h {
        for x in 0..w {
            let offset = (y * w + x) as u32;
            let index = (x * 3 + y * 7) as u16 & 255;
            if mode == 4 {
                if x % 2 == 0 {
                    let next = ((x + 1) * 3 + y * 7) as u16 & 255;
                    m.write16(VRAM + page as u32 * 0xa000 + offset, index | (next << 8))
                        .unwrap();
                }
            } else {
                let color =
                    (x as u16 & 31) | ((y as u16 & 31) << 5) | (((x + y) as u16 & 31) << 10);
                m.write16(VRAM + page as u32 * 0xa000 + offset * 2, color)
                    .unwrap();
            }
        }
    }
    m
}

#[test]
fn write_only_register_merges_bytes_and_ignores_upper_word_half() {
    let mut m = bitmap(3, 0);
    m.write16(BG2CNT, 0x40).unwrap();
    m.write32(MOSAIC, 0xffff_0034).unwrap(); // BG 5x4, OBJ1x1.
    assert_eq!(m.read32(MOSAIC).unwrap(), 0);
    m.write8(MOSAIC + 1, 0xff).unwrap(); // Must retain BG dimensions.
    let f = render(&m);
    assert_eq!(at(&f, 4, 3), at(&f, 0, 0));
    assert_ne!(at(&f, 5, 4), at(&f, 0, 0));
    m.write8(MOSAIC, 0).unwrap();
    assert_ne!(at(&render(&m), 1, 0), at(&f, 0, 0));
    assert_eq!(m.read16(MOSAIC + 2).unwrap(), 0);
    assert_eq!(
        m.read8(MOSAIC + 0x10000),
        Err(MemoryError::Unmapped(MOSAIC + 0x10000))
    );
    assert_eq!(
        m.write16(MOSAIC + 1, 0),
        Err(MemoryError::Unaligned(MOSAIC + 1))
    );
    assert_eq!(m.cycles(), 0);
}

#[test]
fn every_bg_size_and_bitmap_page_repeats_screen_grid_samples() {
    for mode in [3, 4, 5] {
        for page in 0..=u16::from(mode != 3) {
            let mut m = bitmap(mode, page);
            let base = render(&m);
            m.write16(BG2CNT, 0x40).unwrap();
            // Exercise every field value, including asymmetric blocks and partial LCD blocks.
            for w in 1..=16 {
                let h = 17 - w;
                m.write16(MOSAIC, ((h - 1) * 16 + w - 1) as u16).unwrap();
                let f = render(&m);
                for y in 0..HEIGHT {
                    for x in 0..WIDTH {
                        assert_eq!(
                            at(&f, x, y),
                            at(&base, x / w * w, y / h * h),
                            "mode={mode} page={page} size={w}x{h} at={x},{y}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn mosaic_flag_is_independent_of_dimensions_and_one_by_one_is_identity() {
    let mut m = bitmap(3, 0);
    let base = render(&m);
    m.write16(MOSAIC, 0xffff).unwrap();
    assert_eq!(render(&m).pixels(), base.pixels()); // Flag is off.
    m.write16(BG2CNT, 0x40).unwrap();
    m.write16(MOSAIC, 0xff00).unwrap(); // OBJ fields do not affect BG.
    assert_eq!(render(&m).pixels(), base.pixels());
    m.write16(MOSAIC, 0x000f).unwrap();
    let f = render(&m);
    assert_eq!(at(&f, 15, 1), at(&base, 0, 1));
    assert_ne!(at(&f, 15, 1), at(&base, 0, 0));
}

#[test]
fn text_mosaic_samples_before_scroll_and_tile_flips_in_both_depths() {
    for eight_bit in [false, true] {
        let mut m = memory();
        m.write16(DISPCNT, 0x100).unwrap();
        let control = 0x1000 | if eight_bit { 0x80 } else { 0 };
        m.write16(BG0CNT, control).unwrap();
        m.write16(BG0HOFS, 7).unwrap();
        m.write16(BG0VOFS, 5).unwrap();
        fill(&mut m, VRAM + 0x8000, 0x800, 0x0c00); // Both tile flips.
        for y in 0..8 {
            for x in (0..8).step_by(if eight_bit { 2 } else { 4 }) {
                let mut value = 0;
                for i in 0..if eight_bit { 2 } else { 4 } {
                    let index = ((x + i + y * 3) % 15) as u16;
                    value |= index << (i * if eight_bit { 8 } else { 4 });
                }
                m.write16(
                    VRAM + (y * if eight_bit { 8 } else { 4 } + x / if eight_bit { 1 } else { 2 })
                        as u32,
                    value,
                )
                .unwrap();
            }
        }
        let base = render(&m);
        m.write16(BG0CNT, control | 0x40).unwrap();
        m.write16(MOSAIC, 0x54).unwrap(); // 5x6 crosses tile boundaries.
        let f = render(&m);
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                assert_eq!(at(&f, x, y), at(&base, x / 5 * 5, y / 6 * 6));
            }
        }
    }
}

#[test]
fn affine_mosaic_samples_screen_positions_before_signed_transform() {
    for mode in [1, 2, 3, 4, 5] {
        let mut m = if mode >= 3 { bitmap(mode, 0) } else { memory() };
        m.write16(DISPCNT, 0x400 | mode).unwrap();
        let control = if mode <= 2 { 0x3000 } else { 0 };
        m.write16(BG2CNT, control).unwrap();
        if mode <= 2 {
            for offset in (0..64).step_by(2) {
                m.write16(VRAM + offset, (offset as u16 * 0x101).wrapping_add(0x100))
                    .unwrap();
            }
        }
        for (reg, value) in [
            (BG2PA, 181),
            (BG2PB, 91),
            (BG2PC, (-181i16) as u16),
            (BG2PD, 256),
        ] {
            m.write16(reg, value).unwrap();
        }
        m.write32(BG2X, (-769i32) as u32).unwrap();
        m.write32(BG2Y, 31 * 256 + 127).unwrap();
        let base = render(&m);
        m.write16(BG2CNT, control | 0x40).unwrap();
        m.write16(MOSAIC, 0x76).unwrap();
        let f = render(&m);
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                assert_eq!(
                    at(&f, x, y),
                    at(&base, x / 7 * 7, y / 8 * 8),
                    "mode={mode} pixel={x},{y}"
                );
            }
        }
    }
}

#[test]
fn independent_background_flags_preserve_transparency_and_priority() {
    let mut m = memory();
    m.write16(DISPCNT, 0x300).unwrap();
    m.write16(BG0CNT, 0x1040).unwrap();
    m.write16(BG1CNT, 0x1105).unwrap();
    m.write16(PAL + 2, 31).unwrap();
    m.write16(PAL + 4, 0x3e0).unwrap();
    fill(&mut m, VRAM, 32, 0x1010);
    fill(&mut m, VRAM + 0x4000, 32, 0x2222);
    m.write16(MOSAIC, 2).unwrap();
    let f = render(&m);
    for x in 0..18 {
        assert_eq!(at(&f, x, 0), rgb(if x / 3 % 2 == 0 { 0x3e0 } else { 31 }));
    }
    m.write16(BG0CNT, 0x1000).unwrap();
    m.write16(BG1CNT, 0x1145).unwrap();
    let f = render(&m);
    for x in 0..18 {
        assert_eq!(at(&f, x, 0), rgb(if x % 2 == 0 { 0x3e0 } else { 31 }));
    }
}

#[test]
fn windows_and_color_effects_use_output_coordinate_not_mosaic_sample() {
    let mut m = bitmap(3, 0);
    m.write16(BG2CNT, 0x40).unwrap();
    m.write16(MOSAIC, 0x77).unwrap();
    m.write16(DISPCNT, 0x2403).unwrap();
    m.write16(WIN0H, 0x0306).unwrap();
    m.write16(WIN0V, 0x0008).unwrap();
    m.write16(WININ, 4).unwrap();
    m.write16(WINOUT, 0x24).unwrap();
    m.write16(BLDCNT, 0x84).unwrap();
    m.write16(BLDY, 16).unwrap();
    let f = render(&m);
    for x in 0..8 {
        assert_eq!(at(&f, x, 0), if (3..6).contains(&x) { 0 } else { 0xffffff });
    }
    m.write16(DISPCNT, 0xffff).unwrap();
    assert!(render(&m).pixels().iter().all(|&p| p == 0xffffff));
}
