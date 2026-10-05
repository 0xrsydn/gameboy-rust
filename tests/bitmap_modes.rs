use gba_rust::{
    io::{BG2CNT, BG2PA, BG2X, BG2Y, DISPCNT, GREENSWAP},
    memory::{Memory, OAM_START as OAM, PALETTE_START as PAL, VRAM_START as VRAM},
    video::{rgb555_to_rgb888, Framebuffer, VideoError, WIDTH},
};

fn setup(mode: u16) -> Memory {
    let mut m = Memory::new(vec![]).unwrap();
    m.write16(DISPCNT, 0x400 | mode).unwrap();
    transform(&mut m, [256, 0, 0, 256], [0, 0]);
    m.write16(PAL, 0x4000).unwrap();
    for i in 1..256 {
        m.write16(PAL + i * 2, (i as u16 * 123) & 0x7fff).unwrap();
    }
    m
}
fn transform(m: &mut Memory, values: [i16; 4], origin: [i32; 2]) {
    for (i, v) in values.into_iter().enumerate() {
        m.write16(BG2PA + i as u32 * 2, v as u16).unwrap();
    }
    m.write32(BG2X, origin[0] as u32).unwrap();
    m.write32(BG2Y, origin[1] as u32).unwrap();
}
fn frame(m: &Memory) -> Framebuffer {
    let mut f = Framebuffer::default();
    m.render_frame(&mut f).unwrap();
    f
}
fn pixel(m: &Memory, x: usize, y: usize) -> u32 {
    frame(m).pixels()[y * WIDTH + x]
}
fn sprite(m: &mut Memory, mode: u16, priority: u16, affine: bool) {
    for i in 0..128 {
        m.write16(OAM + i * 8, 0x200).unwrap();
    }
    m.write16(OAM, if affine { 0x100 } else { 0 }).unwrap();
    m.write16(OAM + 4, 512 | (priority << 10)).unwrap();
    m.write16(OAM + 6, 256).unwrap();
    m.write16(OAM + 30, 256).unwrap();
    m.write16(VRAM + 0x14000, 0x1111).unwrap();
    m.write16(PAL + 0x202, 0x03e0).unwrap();
    m.write16(DISPCNT, 0x1440 | mode).unwrap();
}

#[test]
fn mode4_reads_adjacent_bytes_all_indices_and_both_page_boundaries() {
    let mut m = setup(4);
    for page in [0, 0xa000] {
        m.write16(VRAM + page, 0xff01).unwrap();
        m.write16(VRAM + page + 238, 0x0203).unwrap();
        m.write16(VRAM + page + 240, 0x0405).unwrap();
        m.write16(VRAM + page + 0x95fe, 0x0607).unwrap();
        m.write16(DISPCNT, 0x404 | if page == 0 { 0 } else { 0x10 })
            .unwrap();
        for (x, y, index) in [
            (0, 0, 1),
            (1, 0, 255),
            (239, 0, 2),
            (0, 1, 5),
            (239, 159, 6),
        ] {
            assert_eq!(pixel(&m, x, y), rgb555_to_rgb888((index * 123) & 0x7fff));
        }
    }
    for i in (0..256).step_by(2) {
        m.write16(VRAM + i, (i as u16) | ((i as u16 + 1) << 8))
            .unwrap();
    }
    m.write16(DISPCNT, 0x404).unwrap();
    let f = frame(&m);
    for i in 0..256 {
        assert_eq!(
            f.pixels()[i],
            rgb555_to_rgb888(if i == 0 {
                0x4000
            } else {
                (i as u16 * 123) & 0x7fff
            })
        );
    }
}

#[test]
fn mode4_page_stride_is_40k_and_inactive_page_writes_do_not_change_output() {
    let mut m = setup(4);
    m.write16(VRAM, 0x0101).unwrap();
    m.write16(VRAM + 0x9600, 0x0303).unwrap(); // Padding is not page1.
    let before = frame(&m);
    m.write16(VRAM + 0xa000, 0x0202).unwrap();
    assert_eq!(frame(&m).pixels(), before.pixels());
    m.write8(DISPCNT, 0x14).unwrap(); // Byte write selects page1 without losing BG2 enable.
    assert_eq!(pixel(&m, 0, 0), rgb555_to_rgb888(246));
    m.write16(PAL + 4, 0x001f).unwrap();
    assert_eq!(pixel(&m, 0, 0), 0xff0000); // Palette is live, not cached per page.
    m.write8(DISPCNT, 4).unwrap();
    assert_eq!(frame(&m).pixels(), before.pixels());
}

#[test]
fn mode5_uses_160_by_128_rgb555_pages_and_leaves_remainder_transparent() {
    let mut m = setup(5);
    for (page, color) in [(0, 0x801f), (0xa000, 0x83e0)] {
        m.write16(VRAM + page, color).unwrap();
        m.write16(VRAM + page + 318, 0x7c00).unwrap();
        m.write16(VRAM + page + 320, 0x7fff).unwrap();
        m.write16(VRAM + page + 0x9ffe, color).unwrap();
    }
    for (control, color) in [(0x405, 0xff0000), (0x415, 0x00ff00)] {
        m.write16(DISPCNT, control).unwrap();
        let f = frame(&m);
        assert_eq!(f.pixels()[0], color);
        assert_eq!(f.pixels()[159], 0x0000ff);
        assert_eq!(f.pixels()[WIDTH], 0xffffff);
        assert_eq!(f.pixels()[127 * WIDTH + 159], color);
        for y in 0..160 {
            for x in 0..240 {
                if x >= 160 || y >= 128 {
                    assert_eq!(f.pixels()[y * WIDTH + x], rgb555_to_rgb888(0x4000));
                }
            }
        }
    }
}

#[test]
fn mode4_zero_index_is_transparent_but_nonzero_black_is_opaque() {
    for affine in [false, true] {
        let mut m = setup(4);
        sprite(&mut m, 4, 3, affine);
        m.write16(VRAM, 0x0100).unwrap();
        m.write16(PAL + 2, 0).unwrap();
        assert_eq!(pixel(&m, 0, 0), 0x00ff00); // Index0 reveals lower-priority sprite.
        assert_eq!(pixel(&m, 1, 0), 0); // Black index1 still covers it.
        m.write16(BG2CNT, 3).unwrap();
        assert_eq!(pixel(&m, 1, 0), 0x00ff00); // OBJ wins equal priority.
    }
}

#[test]
fn mode5_black_is_opaque_and_ignores_palette_while_outside_pixels_reveal_objects() {
    let mut m = setup(5);
    sprite(&mut m, 5, 3, false);
    m.write16(VRAM, 0).unwrap();
    m.write16(PAL, 0x7fff).unwrap();
    assert_eq!(pixel(&m, 0, 0), 0);
    m.write32(BG2X, u32::MAX).unwrap(); // Source X=-1/256 => first output column outside.
    assert_eq!(pixel(&m, 0, 0), 0x00ff00);
    assert_eq!(pixel(&m, 1, 0), 0);
}

#[test]
fn bitmap_sprites_use_upper_obj_area_independent_of_page() {
    for mode in [4, 5] {
        let mut m = setup(mode);
        sprite(&mut m, mode, 0, false);
        m.write16(VRAM + 0x10000, 0x1111).unwrap();
        m.write16(OAM + 4, 0).unwrap();
        assert_ne!(pixel(&m, 0, 0), 0x00ff00); // Lower512 OBJ tiles unavailable.
        m.write16(OAM + 4, 512).unwrap();
        for page in [0, 0x10] {
            m.write16(DISPCNT, 0x1440 | mode | page).unwrap();
            assert_eq!(pixel(&m, 0, 0), 0x00ff00);
        }
        assert_eq!(m.read16(VRAM + 0x14000).unwrap(), 0x1111);
    }
}

#[test]
fn bitmap_byte_writes_duplicate_pixels_but_never_modify_obj_tiles() {
    for mode in [4, 5] {
        let mut m = setup(mode);
        for address in [VRAM, VRAM + 0xa001, VRAM + 0x13fff] {
            m.write8(address, 0x5a).unwrap();
            assert_eq!(m.read16(address & !1).unwrap(), 0x5a5a);
        }
        m.write16(VRAM + 0x14000, 0x1234).unwrap();
        m.write8(VRAM + 0x1c000, 0xff).unwrap(); // OBJ mirror.
        assert_eq!(m.read16(VRAM + 0x14000).unwrap(), 0x1234);
    }
}

#[test]
fn affine_bitmap_sampling_matches_wide_integer_reference_on_both_pages() {
    for mode in [4, 5] {
        let mut m = setup(mode);
        let (w, h) = if mode == 4 { (240, 160) } else { (160, 128) };
        for page in 0..2_u32 {
            if mode == 4 {
                for offset in (0..w * h).step_by(2) {
                    let lo = ((offset * 13 + page * 73) & 255) as u16;
                    let hi = (((offset + 1) * 13 + page * 73) & 255) as u16;
                    m.write16(VRAM + page * 0xa000 + offset, lo | (hi << 8))
                        .unwrap();
                }
            } else {
                for offset in 0..w * h {
                    m.write16(
                        VRAM + page * 0xa000 + offset * 2,
                        ((offset * 37 + page * 127) & 0xffff) as u16,
                    )
                    .unwrap();
                }
            }
        }
        for page in 0..2_u32 {
            m.write16(DISPCNT, 0x400 | mode | (page as u16 * 16))
                .unwrap();
            m.write16(BG2CNT, 0xff8f).unwrap(); // Ignored map/size/overflow/color fields; mosaic clear.
            for (coeff, origin) in [
                ([256, 0, 0, 256], [0, 0]),
                ([181, 181, -181, 181], [-173, 20001]),
                ([128, 0, 0, 128], [127, -129]),
                (
                    [-256, 0, 0, -256],
                    [(w as i32 - 1) * 256, (h as i32 - 1) * 256],
                ),
                ([256, 73, -129, 512], [-512, 127]),
                ([0; 4], [256, 256]),
            ] {
                transform(&mut m, coeff, origin);
                let f = frame(&m);
                for y in 0..160 {
                    for x in 0..240 {
                        let tx = (i64::from(origin[0])
                            + i64::from(coeff[0]) * x as i64
                            + i64::from(coeff[1]) * y as i64)
                            .div_euclid(256);
                        let ty = (i64::from(origin[1])
                            + i64::from(coeff[2]) * x as i64
                            + i64::from(coeff[3]) * y as i64)
                            .div_euclid(256);
                        let color = if tx < 0 || ty < 0 || tx >= i64::from(w) || ty >= i64::from(h)
                        {
                            0x4000
                        } else {
                            let offset = ty as u32 * w + tx as u32;
                            if mode == 4 {
                                let i = ((offset * 13 + page * 73) & 255) as u16;
                                if i == 0 {
                                    0x4000
                                } else {
                                    (i * 123) & 0x7fff
                                }
                            } else {
                                ((offset * 37 + page * 127) & 0x7fff) as u16
                            }
                        };
                        assert_eq!(
                            f.pixels()[y * WIDTH + x],
                            rgb555_to_rgb888(color),
                            "mode {mode}, page {page}, pixel {x},{y}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn mode5_can_scale_to_full_lcd_without_implicit_stretching() {
    let mut m = setup(5);
    for offset in 0..160 * 128 {
        m.write16(VRAM + offset * 2, 0x001f).unwrap();
    }
    assert_eq!(pixel(&m, 239, 159), rgb555_to_rgb888(0x4000));
    transform(&mut m, [170, 0, 0, 204], [0, 0]); // Approximate 160/240 and 128/160 sampling steps.
    assert!(frame(&m).pixels().iter().all(|&p| p == 0xff0000));
}

#[test]
fn bitmap_green_swap_disabled_bg_and_forced_blank_remain_consistent() {
    for mode in [4, 5] {
        let mut m = setup(mode);
        m.write16(PAL + 2, 0x001f).unwrap();
        m.write16(PAL + 4, 0x03e0).unwrap();
        if mode == 4 {
            m.write16(VRAM, 0x0201).unwrap();
        } else {
            m.write32(VRAM, 0x03e0_001f).unwrap();
        }
        m.write16(GREENSWAP, 1).unwrap();
        assert_eq!(&frame(&m).pixels()[..2], &[0xffff00, 0]);
        m.write16(DISPCNT, mode | 0x10).unwrap();
        assert!(frame(&m)
            .pixels()
            .iter()
            .all(|&p| p == rgb555_to_rgb888(0x4000)));
        m.write16(DISPCNT, 0xffff).unwrap();
        assert!(frame(&m).pixels().iter().all(|&p| p == 0xffffff));
    }
}

fn assert_demo_frame(
    f: &Framebuffer,
    mode: gba_rust::bitmap_demo::BitmapMode,
    pan: (i32, i32),
    rotated: bool,
    zoomed: bool,
    page: usize,
) {
    use gba_rust::bitmap_demo::BitmapMode;
    let (w, h) = if mode == BitmapMode::Mode4 {
        (240, 160)
    } else {
        (160, 128)
    };
    let a = f64::from(match (rotated, zoomed) {
        (false, false) => 256,
        (false, true) => 128,
        (true, false) => 181,
        (true, true) => 90,
    }) / 256.0;
    let b = if rotated { a } else { 0.0 };
    for y in 0..160 {
        for x in 0..240 {
            let dx = x as f64 - 120.0;
            let dy = y as f64 - 80.0;
            let tx = (f64::from(w / 2 + pan.0) + a * dx + b * dy).floor() as i32;
            let ty = (f64::from(h / 2 + pan.1) - b * dx + a * dy).floor() as i32;
            let color = if tx < 0 || ty < 0 || tx >= w || ty >= h {
                0x4000
            } else {
                let (tx, ty) = (tx as usize, ty as usize);
                if mode == BitmapMode::Mode4 {
                    let i = (tx / 8 + 3 * (ty / 8) + page * 64) % 256;
                    if i == 0 {
                        0x4000
                    } else {
                        ((i % 32) | ((i / 8 % 32) << 5) | ((i * 3 % 32) << 10)) as u16
                    }
                } else {
                    (((tx / 5 + page * 11) % 32)
                        | (((ty / 4 + page * 7) % 32) << 5)
                        | (((tx / 8 + ty / 8 + page * 5) % 32) << 10)) as u16
                }
            };
            assert_eq!(
                f.pixels()[y * WIDTH + x],
                rgb555_to_rgb888(color),
                "{mode:?} page{page}, pixel{x},{y}, pan={pan:?}"
            );
        }
    }
}

#[test]
fn cpu_demos_copy_both_pages_and_flip_during_vblank_every_32_updates() {
    use gba_rust::{
        bitmap_demo::{BitmapDemo, BitmapMode, BITMAP_STATE},
        input::Buttons,
    };
    for mode in [BitmapMode::Mode4, BitmapMode::Mode5] {
        let mut demo = BitmapDemo::new(mode).unwrap();
        let mut f = Framebuffer::default();
        assert_eq!(demo.machine().memory().read16(DISPCNT).unwrap(), 0);
        assert_eq!(demo.machine().memory().read16(VRAM + 0xa000).unwrap(), 0);
        for updates in 1..=65 {
            let before = demo.machine().memory().display_position();
            demo.frame(Buttons::default(), &mut f).unwrap();
            let page = (updates / 32) % 2;
            let m = demo.machine().memory();
            assert_eq!(
                m.read16(DISPCNT).unwrap(),
                0x400 | mode as u16 | (page as u16 * 16)
            );
            assert_eq!(m.read32(BITMAP_STATE).unwrap(), updates as u32);
            assert_eq!(m.display_position().scanline, 160);
            if updates > 1 {
                assert_eq!(m.display_position().vblanks, before.vblanks + 1);
            }
            assert_demo_frame(&f, mode, (0, 0), false, false, page);
        }
    }
}

#[test]
fn cpu_bitmap_controls_pan_rotate_zoom_force_page_and_reset() {
    use gba_rust::{
        bitmap_demo::{BitmapDemo, BitmapMode, BITMAP_STATE},
        input::{Button, Buttons},
    };
    for mode in [BitmapMode::Mode4, BitmapMode::Mode5] {
        let mut demo = BitmapDemo::new(mode).unwrap();
        let mut f = Framebuffer::default();
        demo.frame(
            Buttons::default()
                .with(Button::Left, true)
                .with(Button::Up, true),
            &mut f,
        )
        .unwrap();
        demo.frame(Buttons::from_bits(0xf0), &mut f).unwrap();
        for bits in 0..8 {
            let rotated = bits & 1 != 0;
            let zoomed = bits & 2 != 0;
            let page = usize::from(bits & 4 != 0);
            demo.frame(
                Buttons::default()
                    .with(Button::L, rotated)
                    .with(Button::R, zoomed)
                    .with(Button::A, page != 0),
                &mut f,
            )
            .unwrap();
            assert_demo_frame(&f, mode, (-2, -2), rotated, zoomed, page);
            assert_eq!(
                demo.machine().memory().read32(BITMAP_STATE + 4).unwrap() as i32,
                -2
            );
            assert_eq!(
                demo.machine().memory().read32(BITMAP_STATE + 8).unwrap() as i32,
                -2
            );
        }
        demo.frame(
            Buttons::default()
                .with(Button::Start, true)
                .with(Button::Right, true),
            &mut f,
        )
        .unwrap();
        assert_demo_frame(&f, mode, (0, 0), false, false, 0);
        assert_eq!(demo.machine().memory().read32(BITMAP_STATE + 4).unwrap(), 0);
        assert_eq!(demo.machine().memory().read32(BITMAP_STATE + 8).unwrap(), 0);
    }
}

#[test]
fn unsupported_bitmap_layers_leave_frame_unchanged() {
    for mode in [4, 5] {
        let mut m = setup(mode);
        let mut f = Framebuffer::default();
        f.clear(0x001f);
        for extra in [0x100, 0x200, 0x800] {
            let control = 0x410 | mode | extra;
            m.write16(DISPCNT, control).unwrap();
            assert_eq!(
                m.render_frame(&mut f),
                Err(VideoError::UnsupportedLayers(control))
            );
        }
        assert!(f.pixels().iter().all(|&p| p == 0xff0000));
        assert_eq!(m.cycles(), 0);
    }
}
