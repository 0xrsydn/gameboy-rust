use super::*;

#[test]
fn alpha_uses_five_bit_channels_clamps_coefficients_and_saturates_sums() {
    let mut m = setup();
    m.write16(BLDCNT, 0x0241).unwrap(); // BG0 with BG1.
    for (coefficients, expected) in [
        (0, 0),
        (0x0808, 0x01ef),
        (0x0010, 31),
        (0x1000, 0x3e0),
        (0x1f1f, 0x3ff),
    ] {
        m.write16(BLDALPHA, coefficients).unwrap();
        assert_eq!(pixel(&m, 0, 0), rgb(expected));
    }
    m.write16(PAL + 2, 0x7fff).unwrap();
    m.write16(PAL + 4, 0x7fff).unwrap();
    assert_eq!(pixel(&m, 0, 0), rgb(0x7fff));
    m.write16(PAL + 2, 1).unwrap();
    m.write16(PAL + 4, 1).unwrap();
    m.write16(BLDALPHA, 0x0808).unwrap();
    assert_eq!(pixel(&m, 0, 0), rgb(1)); // Sum fractional terms before rounding.
}

#[test]
fn alpha_requires_top_first_target_and_immediate_second_target() {
    let mut m = setup();
    m.write16(BLDALPHA, 0x0808).unwrap();
    for control in [0x0441, 0x0242, 0x0142, 0x2041, 0x0240] {
        m.write16(BLDCNT, control).unwrap();
        assert_eq!(pixel(&m, 0, 0), rgb(31), "control={control:x}");
    }
    m.write16(BLDCNT, 0x0441).unwrap();
    m.write16(VRAM + 0x4000, 0).unwrap(); // Transparent BG1 exposes BG2.
    assert_eq!(pixel(&m, 0, 0), rgb(0x3c0f));
    assert_eq!(pixel(&m, 4, 0), rgb(31));
}

#[test]
fn windows_remove_layers_before_selecting_blend_targets() {
    let mut m = setup();
    m.write16(BLDALPHA, 0x0808).unwrap();
    m.write16(BLDCNT, 0x0441).unwrap();
    window(&mut m, 0x0008, 0x0008, 0x25, 0x3f); // Hide BG1 only inside.
    assert_eq!(pixel(&m, 0, 0), rgb(0x3c0f));
    assert_eq!(pixel(&m, 8, 0), rgb(31));
}

#[test]
fn brightness_uses_selected_top_layer_and_clamped_write_only_coefficient() {
    let mut m = setup();
    m.write16(PAL + 2, 0x4210).unwrap();
    for (control, coefficient, expected) in [
        (0x81, 0, 0x4210),
        (0x81, 8, 0x5ef7),
        (0x81, 31, 0x7fff),
        (0xc1, 8, 0x2108),
        (0xc1, 16, 0),
        (0xc1, 31, 0),
        (0x82, 16, 0x4210),
        (0x01, 16, 0x4210),
    ] {
        m.write16(BLDCNT, control).unwrap();
        m.write32(BLDY, coefficient).unwrap();
        assert_eq!(m.read16(BLDY).unwrap(), 0);
        assert_eq!(pixel(&m, 0, 0), rgb(expected));
    }
    m.write16(BLDCNT, 0x81).unwrap();
    m.write8(BLDY, 8).unwrap();
    m.write8(BLDY + 1, 0xff).unwrap();
    assert_eq!(pixel(&m, 0, 0), rgb(0x5ef7));
}

#[test]
fn backdrop_is_a_target_but_cannot_blend_with_itself() {
    let mut m = setup();
    m.write16(DISPCNT, 0).unwrap();
    m.write16(BLDCNT, 0x2060).unwrap();
    assert_eq!(pixel(&m, 0, 0), rgb(0x4210));
    m.write16(BLDCNT, 0xa0).unwrap();
    m.write16(BLDY, 16).unwrap();
    assert_eq!(pixel(&m, 0, 0), rgb(0x7fff));
    m.write16(DISPCNT, 0x100).unwrap();
    m.write16(BLDCNT, 0x2041).unwrap();
    m.write16(BLDALPHA, 0x1000).unwrap();
    assert_eq!(pixel(&m, 0, 0), rgb(0x4210)); // Backdrop is not pre-brightened.
}

#[test]
fn object_can_be_first_or_second_target_but_never_blends_with_another_object() {
    let mut m = setup();
    m.write16(DISPCNT, 0x1100).unwrap();
    object(&mut m, 0, 0, 0, 0);
    object(&mut m, 1, 0, 0, 0xc00); // Lower priority; never a second OBJ target.
    m.write16(BLDALPHA, 0x0808).unwrap();
    m.write16(BLDCNT, 0x0150).unwrap();
    assert_eq!(pixel(&m, 0, 0), rgb(0x3dff)); // White OBJ + red BG0.
    m.write16(BLDCNT, 0x1050).unwrap();
    assert_eq!(pixel(&m, 0, 0), rgb(0x7fff)); // No OBJ-to-OBJ blend.
    object(&mut m, 0, 0, 0, 0x400); // OBJ0 below BG0; later OBJ still excluded.
    m.write16(BLDCNT, 0x1041).unwrap();
    assert_eq!(pixel(&m, 0, 0), rgb(0x3dff));
}

#[test]
fn semi_transparent_obj_overrides_mode_and_first_target_selection() {
    let mut m = setup();
    m.write16(DISPCNT, 0x1100).unwrap();
    object(&mut m, 0, 0x400, 0, 0);
    m.write16(BLDALPHA, 0x0808).unwrap();
    m.write16(BLDY, 16).unwrap();
    for mode in 0..4 {
        m.write16(BLDCNT, 0x100 | (mode << 6)).unwrap();
        assert_eq!(pixel(&m, 0, 0), rgb(0x3dff));
    }
    window(&mut m, 0x0004, 0x0008, 0x11, 0x31);
    assert_eq!(pixel(&m, 0, 0), rgb(0x7fff)); // Window can disable forced alpha.
    assert_eq!(pixel(&m, 4, 0), rgb(0x3dff));
}

#[test]
fn semi_transparent_obj_falls_back_to_selected_brightness_without_second_target() {
    let mut m = setup();
    m.write16(DISPCNT, 0x1100).unwrap();
    object(&mut m, 0, 0x400, 0, 0);
    m.write16(BLDY, 16).unwrap();
    for (control, expected) in [(0xc0, 0x7fff), (0xd0, 0), (0x10, 0x7fff), (0x1d0, 0)] {
        m.write16(BLDCNT, control).unwrap();
        // Last case has a second target; zero alpha coefficients produce black.
        assert_eq!(pixel(&m, 0, 0), rgb(expected));
    }
    m.write16(BLDALPHA, 0x0808).unwrap();
    assert_eq!(pixel(&m, 0, 0), rgb(0x3dff)); // Alpha takes precedence over brightness.
}

#[test]
fn color_effects_work_in_affine_and_all_bitmap_modes() {
    for mode in 1..=5 {
        let mut m = setup();
        m.write16(DISPCNT, 0x400 | mode).unwrap();
        m.write16(BG2CNT, 0).unwrap();
        m.write16(BG2PA, 256).unwrap();
        m.write16(BG2PD, 256).unwrap();
        if mode <= 2 {
            m.write16(BG2CNT, 0x1000).unwrap(); // Byte map at 0x8000, tile0 at zero.
            m.write16(VRAM + 0x8000, 0).unwrap();
            m.write16(VRAM, 0x0101).unwrap();
        } else {
            m.write16(VRAM, if mode == 4 { 0x0101 } else { 31 })
                .unwrap();
        }
        m.write16(BLDCNT, 0x2044).unwrap();
        m.write16(BLDALPHA, 0x1000).unwrap();
        assert_eq!(pixel(&m, 0, 0), rgb(0x4210), "mode={mode}");
        window(&mut m, 0x0001, 0x0001, 4, 0x24);
        assert_eq!(pixel(&m, 0, 0), rgb(31));
    }
}

#[test]
fn green_swap_follows_effects_and_forced_blank_bypasses_every_effect() {
    let mut m = setup();
    m.write16(PAL + 2, 0x3e0).unwrap();
    m.write16(BLDCNT, 0xc1).unwrap();
    m.write16(BLDY, 16).unwrap();
    window(&mut m, 0x0001, 0x0001, 1, 0x21);
    m.write16(GREENSWAP, 1).unwrap();
    assert_eq!(&render(&m).pixels()[..2], &[0, 0xff00]);
    m.write16(DISPCNT, 0xffff).unwrap();
    assert!(render(&m).pixels().iter().all(|&p| p == 0xffffff));
}
