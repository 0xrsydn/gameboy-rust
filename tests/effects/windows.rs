use super::*;

#[test]
fn register_masks_byte_merges_and_write_only_reads() {
    let mut m = setup();
    for (address, mask) in [
        (WININ, 0x3f3f),
        (WINOUT, 0x3f3f),
        (BLDCNT, 0x3fff),
        (BLDALPHA, 0x1f1f),
    ] {
        assert_eq!(m.read16(address).unwrap(), 0);
        m.write16(address, 0xffff).unwrap();
        assert_eq!(m.read16(address).unwrap(), mask);
        m.write8(address, 0).unwrap();
        assert_eq!(m.read16(address).unwrap(), mask & 0xff00);
        m.write8(address + 1, 0).unwrap();
        assert_eq!(m.read16(address).unwrap(), 0);
    }
    for address in [WIN0H, WIN1H, WIN0V, WIN1V, BLDY] {
        m.write16(address, 0xffff).unwrap();
        assert_eq!(m.read16(address).unwrap(), 0);
    }
    m.write32(BLDY, u32::MAX).unwrap();
    assert_eq!(m.read32(BLDY).unwrap(), 0);
    assert_eq!(
        m.read8(0x0400_0058),
        Err(MemoryError::Unmapped(0x0400_0058))
    );
    assert_eq!(
        m.read8(0x0401_0048),
        Err(MemoryError::Unmapped(0x0401_0048))
    );
    assert_eq!(
        m.write16(WININ + 1, 1),
        Err(MemoryError::Unaligned(WININ + 1))
    );
}

#[test]
fn no_enabled_windows_ignore_zero_masks_but_apply_effects() {
    let mut m = setup();
    m.write16(BLDCNT, 0xc1).unwrap();
    m.write16(BLDY, 16).unwrap();
    assert_eq!(pixel(&m, 0, 0), 0);
    m.write16(DISPCNT, 0x2700).unwrap(); // Empty WIN0; outside mask is zero.
    assert_eq!(pixel(&m, 0, 0), rgb(0x4210));
}

#[test]
fn bounds_are_half_open_and_byte_writes_preserve_other_bound() {
    let mut m = setup();
    window(&mut m, 0, 0, 1, 2);
    m.write8(WIN0H, 30).unwrap();
    m.write8(WIN0H + 1, 10).unwrap();
    m.write8(WIN0V, 40).unwrap();
    m.write8(WIN0V + 1, 20).unwrap();
    let f = render(&m);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let inside = (10..30).contains(&x) && (20..40).contains(&y);
            assert_eq!(
                f.pixels()[y * WIDTH + x],
                rgb(if inside { 31 } else { 0x3e0 })
            );
        }
    }
}

#[test]
fn inverted_bounds_wrap_equal_bounds_are_empty_and_offscreen_bounds_clip() {
    let mut m = setup();
    for (h, v) in [
        (0xdc14, 0x960a),
        (0xfafa, 0x00ff),
        (0xf005, 0xa005),
        (0x00ff, 0x00ff),
    ] {
        window(&mut m, h, v, 1, 2);
        let f = render(&m);
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                // Independent modular-distance definition of a half-open interval.
                let inside = (x as u8).wrapping_sub((h >> 8) as u8)
                    < (h as u8).wrapping_sub((h >> 8) as u8)
                    && (y as u8).wrapping_sub((v >> 8) as u8)
                        < (v as u8).wrapping_sub((v >> 8) as u8);
                assert_eq!(
                    f.pixels()[y * WIDTH + x],
                    rgb(if inside { 31 } else { 0x3e0 })
                );
            }
        }
    }
}

#[test]
fn win0_overrides_win1_without_combining_masks() {
    let mut m = setup();
    window(&mut m, 0x0a1e, 0x0a1e, 0x0200, 4); // WIN0 hides all; WIN1 selects BG1.
    m.write16(WIN1H, 0x1428).unwrap();
    m.write16(WIN1V, 0x1428).unwrap();
    m.write16(DISPCNT, 0x6700).unwrap();
    assert_eq!(pixel(&m, 25, 25), rgb(0x4210));
    assert_eq!(pixel(&m, 35, 35), rgb(0x03e0));
    assert_eq!(pixel(&m, 45, 45), rgb(0x7c00));
    m.write16(DISPCNT, 0x6500).unwrap(); // Window cannot enable a globally disabled BG.
    assert_eq!(pixel(&m, 35, 35), rgb(0x4210));
}

#[test]
fn window_effect_bit_is_independent_of_layer_bits_and_backdrop_is_always_visible() {
    let mut m = setup();
    window(&mut m, 0x0008, 0x0008, 1, 0x21);
    m.write16(BLDCNT, 0xc1).unwrap();
    m.write16(BLDY, 16).unwrap();
    assert_eq!(pixel(&m, 0, 0), rgb(31));
    assert_eq!(pixel(&m, 8, 0), 0);
    m.write16(WINOUT, 0x20).unwrap();
    m.write16(BLDCNT, 0xa0).unwrap(); // Brighten backdrop.
    assert_eq!(pixel(&m, 8, 0), rgb(0x7fff));
}

#[test]
fn object_window_uses_nonzero_texels_not_color_priority_or_visible_obj_mask() {
    let mut m = setup();
    m.write16(DISPCNT, 0x9700).unwrap();
    m.write16(WINOUT, 0x0102).unwrap(); // OBJ region BG0 only, outside BG1 only.
    object(&mut m, 0, 0x800, 0, 0xfc00); // Palette15, priority3 are irrelevant.
    m.write16(VRAM + 0x10000, 0x1010).unwrap();
    assert_eq!(pixel(&m, 0, 0), rgb(0x3e0));
    assert_eq!(pixel(&m, 1, 0), rgb(31));
    assert_eq!(pixel(&m, 8, 0), rgb(0x3e0));
    m.write16(DISPCNT, 0x8700).unwrap(); // OBJ master enable is also required.
    assert_eq!(pixel(&m, 1, 0), rgb(0x3e0));
    m.write16(DISPCNT, 0x1700).unwrap(); // Window OBJ never becomes a visible sprite.
    assert_eq!(pixel(&m, 1, 0), rgb(31));
}

#[test]
fn visible_objects_do_not_occlude_object_window_and_rectangles_take_precedence() {
    let mut m = setup();
    m.write16(DISPCNT, 0x9700).unwrap();
    m.write16(WINOUT, 0x0102).unwrap();
    object(&mut m, 0, 0, 0, 0); // Occupies the same pixels as the later window OBJ.
    object(&mut m, 127, 0x800, 0, 0);
    assert_eq!(pixel(&m, 0, 0), rgb(31)); // OBJ mask hides color OBJ, not window coverage.
    window(&mut m, 0x0002, 0x0008, 4, 0x0102);
    assert_eq!(pixel(&m, 0, 0), rgb(0x7c00));
    assert_eq!(pixel(&m, 2, 0), rgb(31));
    m.write16(WIN1H, 0x0004).unwrap();
    m.write16(WIN1V, 0x0008).unwrap();
    m.write16(WININ, 0x0204).unwrap();
    m.write16(DISPCNT, 0xf700).unwrap();
    assert_eq!(pixel(&m, 2, 0), rgb(0x3e0));
    assert_eq!(pixel(&m, 4, 0), rgb(31));
}

#[test]
fn object_window_supports_regular_flips_and_affine_sampling() {
    let mut m = setup();
    m.write16(DISPCNT, 0x9700).unwrap();
    m.write16(WINOUT, 0x0102).unwrap();
    for offset in (0..32).step_by(2) {
        m.write16(VRAM + 0x10000 + offset, 0).unwrap();
    }
    m.write16(VRAM + 0x10000, 1).unwrap();
    object(&mut m, 0, 0x800, 0x1000, 0);
    assert_eq!(pixel(&m, 7, 0), rgb(31));
    assert_eq!(pixel(&m, 0, 0), rgb(0x3e0));
    object(&mut m, 0, 0x900, 0, 0);
    m.write16(OAM + 6, 256).unwrap();
    m.write16(OAM + 30, 256).unwrap();
    assert_eq!(pixel(&m, 0, 0), rgb(31));
    assert_eq!(pixel(&m, 1, 0), rgb(0x3e0));
}
