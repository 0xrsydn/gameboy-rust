use gba_core::{
    io::*,
    memory::{Memory, MemoryError, PALETTE_START as PAL, VRAM_START as VRAM},
    video::{rgb555_to_rgb888, Framebuffer, VideoError, WIDTH},
};

fn setup() -> Memory {
    let mut m = Memory::new(vec![]).unwrap();
    m.write16(DISPCNT, 0x100).unwrap();
    m.write16(BG0CNT, 0x1000).unwrap(); // Map at 0x8000, tiles at zero.
    for (i, color) in [0x4210, 0x001f, 0x03e0, 0x7c00, 0x7fff]
        .into_iter()
        .enumerate()
    {
        m.write16(PAL + i as u32 * 2, color).unwrap();
    }
    m
}

fn pixel(m: &Memory, x: usize, y: usize) -> u32 {
    let mut frame = Framebuffer::default();
    m.render_frame(&mut frame).unwrap();
    frame.pixels()[y * WIDTH + x]
}

fn tile(m: &mut Memory, address: u32, value: u16, bytes: u32) {
    for i in (0..bytes).step_by(2) {
        m.write16(VRAM + address + i, value).unwrap();
    }
}

#[test]
fn controls_mask_unused_bits_and_merge_byte_and_word_writes() {
    let mut m = setup();
    m.write32(BG0CNT, u32::MAX).unwrap();
    m.write32(BG2CNT, u32::MAX).unwrap();
    assert_eq!(m.read32(BG0CNT).unwrap(), 0xdfcf_dfcf);
    assert_eq!(m.read32(BG2CNT).unwrap(), 0xffcf_ffcf);
    m.write8(BG0CNT, 0x81).unwrap();
    m.write8(BG0CNT + 1, 0x10).unwrap();
    assert_eq!(m.read16(BG0CNT).unwrap(), 0x1081);
    for reg in (BG0HOFS..=BG3VOFS).step_by(2) {
        m.write16(reg, 0xffff).unwrap();
        assert_eq!(m.read16(reg).unwrap(), 0); // Write-only placeholder, not open bus.
    }
    assert_eq!(
        m.read16(0x0400_0058),
        Err(MemoryError::Unmapped(0x0400_0058))
    );
}

#[test]
fn four_bit_nibble_order_palette_banks_and_transparent_zero() {
    let mut m = setup();
    m.write16(VRAM, 0x0321).unwrap();
    assert_eq!(pixel(&m, 0, 0), 0xff0000);
    assert_eq!(pixel(&m, 1, 0), 0x00ff00);
    assert_eq!(pixel(&m, 2, 0), 0x0000ff);
    assert_eq!(pixel(&m, 3, 0), rgb555_to_rgb888(0x4210));
    m.write16(VRAM + 0x8000, 0xf000).unwrap();
    m.write16(PAL + 241 * 2, 0x7fff).unwrap();
    m.write16(PAL + 240 * 2, 0x001f).unwrap(); // Bank zero is still transparent.
    assert_eq!(pixel(&m, 0, 0), 0xffffff);
    assert_eq!(pixel(&m, 3, 0), rgb555_to_rgb888(0x4210));
}

#[test]
fn eight_bit_tiles_use_full_index_and_ignore_palette_bank() {
    let mut m = setup();
    m.write16(BG0CNT, 0x1080).unwrap();
    m.write16(VRAM + 0x8000, 0xf001).unwrap();
    m.write16(VRAM + 64, 0x00ff).unwrap();
    m.write16(PAL + 510, 0x001f).unwrap();
    assert_eq!(pixel(&m, 0, 0), 0xff0000);
    assert_eq!(pixel(&m, 1, 0), rgb555_to_rgb888(0x4210));
}

#[test]
fn horizontal_and_vertical_flips_apply_in_both_color_depths() {
    for eight in [false, true] {
        let mut m = setup();
        m.write16(BG0CNT, 0x1000 | if eight { 0x80 } else { 0 })
            .unwrap();
        let (top, bottom) = if eight {
            (0x0201, 0x0403)
        } else {
            (0x21, 0x43)
        };
        m.write16(VRAM, top).unwrap();
        m.write16(VRAM + if eight { 56 } else { 28 }, bottom)
            .unwrap();
        for (flip, x, y, expected) in [
            (0, 0, 0, 0xff0000),
            (0x400, 7, 0, 0xff0000),
            (0x800, 0, 0, 0x0000ff),
            (0xc00, 6, 0, 0xffffff),
        ] {
            m.write16(VRAM + 0x8000, flip).unwrap();
            assert_eq!(pixel(&m, x, y), expected);
        }
    }
}

#[test]
fn all_map_sizes_use_screen_blocks_not_flat_rows_and_wrap() {
    for size in 0..4_u16 {
        let mut m = setup();
        for i in 1..=4 {
            tile(&mut m, i * 32, 0x1111 * i as u16, 32);
        }
        for block in 0..4 {
            tile(&mut m, 0x8000 + block * 0x800, block as u16 + 1, 0x800);
        }
        m.write16(BG0CNT, 0x1000 | (size << 14)).unwrap();
        let width = if size & 1 != 0 { 512 } else { 256 };
        let height = if size & 2 != 0 { 512 } else { 256 };
        for y in [0, 255, 256, 511] {
            for x in [0, 255, 256, 511] {
                m.write16(BG0HOFS, x).unwrap();
                m.write16(BG0VOFS, y).unwrap();
                let block = (x % width) / 256 + (y % height) / 256 * (width / 256);
                let colors = [0x001f, 0x03e0, 0x7c00, 0x7fff];
                assert_eq!(pixel(&m, 0, 0), rgb555_to_rgb888(colors[block as usize]));
            }
        }
    }
}

#[test]
fn scroll_byte_writes_merge_latches_mask_nine_bits_and_move_subtile_pixels() {
    let mut m = setup();
    m.write16(BG0CNT, 0x1080).unwrap();
    m.write16(VRAM + 2, 0x0201).unwrap();
    m.write16(BG0HOFS, 0xffff).unwrap();
    m.write8(BG0HOFS, 2).unwrap(); // latch becomes 0x102; 256-wide map wraps to 2.
    assert_eq!(pixel(&m, 0, 0), 0xff0000);
    m.write8(BG0HOFS + 1, 0xfe).unwrap(); // Only bit zero survives.
    assert_eq!(pixel(&m, 1, 0), 0x00ff00);
    m.write16(VRAM + 8 + 2, 0x0403).unwrap();
    m.write32(BG0HOFS, 0xfe01_fe02).unwrap(); // X=2, Y=1
    assert_eq!(pixel(&m, 0, 0), 0x0000ff);
    assert_eq!(m.cycles(), 0);
}

#[test]
fn each_background_uses_its_own_scroll_and_control() {
    for bg in 0..4 {
        let mut m = setup();
        m.write16(DISPCNT, 0x100 << bg).unwrap();
        m.write16(BG0CNT + bg * 2, 0x1080).unwrap();
        m.write16(VRAM + 10, 0x0201).unwrap();
        m.write16(BG0HOFS + bg * 4, 2).unwrap();
        m.write16(BG0VOFS + bg * 4, 1).unwrap();
        assert_eq!(pixel(&m, 0, 0), 0xff0000);
    }
}

#[test]
fn priorities_ties_transparency_and_opaque_black_composite_all_four_layers() {
    let mut m = setup();
    m.write16(DISPCNT, 0xf00).unwrap();
    for bg in 0..4 {
        m.write16(BG0CNT + bg * 2, ((16 + bg) << 8) as u16).unwrap();
        tile(&mut m, 0x8000 + bg * 0x800, bg as u16 + 1, 0x800);
        tile(&mut m, (bg + 1) * 32, 0x1111 * (bg as u16 + 1), 32);
    }
    for bg in 0..4 {
        let expected = [0xff0000, 0x00ff00, 0x0000ff, 0xffffff][bg as usize];
        assert_eq!(pixel(&m, 0, 0), expected);
        // Lower the winning layer's priority so the next wins the tie.
        m.write16(BG0CNT + bg * 2, ((16 + bg) << 8) as u16 | 3)
            .unwrap();
    }
    assert_eq!(pixel(&m, 0, 0), 0xff0000); // All priority 3, BG0 wins.
    m.write16(PAL + 2, 0).unwrap();
    assert_eq!(pixel(&m, 0, 0), 0); // Nonzero index with black palette color is opaque.
    tile(&mut m, 32, 0, 32);
    assert_eq!(pixel(&m, 0, 0), 0x00ff00);
    m.write16(DISPCNT, 0).unwrap();
    assert_eq!(pixel(&m, 0, 0), rgb555_to_rgb888(0x4210));
}

#[test]
fn character_base_and_high_tile_numbers_cross_character_blocks() {
    for eight in [false, true] {
        let mut m = setup();
        m.write16(BG0CNT, 0x100c | if eight { 0x80 } else { 0 })
            .unwrap();
        m.write16(VRAM + 0x8000, 1023).unwrap();
        let address = 0xc000 + 1023 * if eight { 64 } else { 32 };
        tile(
            &mut m,
            address,
            if eight { 0x0101 } else { 0x1111 },
            if eight { 64 } else { 32 },
        );
        assert_eq!(pixel(&m, 0, 0), 0xff0000);
    }
}

#[test]
fn wide_screen_block_at_map_base_31_reads_beyond_64k() {
    let mut m = setup();
    m.write16(BG0CNT, 0x5f04).unwrap(); // Map block31, wide; tile base1.
    m.write16(BG0HOFS, 256).unwrap(); // First pixel selects screen block1 at 0x10000.
    m.write16(VRAM + 0x18000, 1).unwrap(); // Bus mirror of the selected map entry.
    tile(&mut m, 0x4020, 0x1111, 32);
    assert_eq!(pixel(&m, 0, 0), 0xff0000);
}

#[test]
fn unsupported_modes_leave_output_unchanged_and_forced_blank_bypasses_them() {
    let mut m = setup();
    let mut frame = Framebuffer::default();
    frame.clear(0x001f);
    m.write16(DISPCNT, 0x106).unwrap();
    assert_eq!(
        m.render_frame(&mut frame),
        Err(VideoError::UnsupportedMode(6))
    );
    assert!(frame.pixels().iter().all(|&p| p == 0xff0000));
    m.write16(DISPCNT, 0xf180).unwrap();
    m.render_frame(&mut frame).unwrap();
    assert!(frame.pixels().iter().all(|&p| p == 0xffffff));
}

fn assert_scene(
    frame: &Framebuffer,
    sx: usize,
    sy: usize,
    flipped: bool,
    behind: bool,
    rotated: bool,
    zoomed: bool,
) {
    for y in 0..160 {
        for x in 0..240 {
            let fx = (x + sx / 2) % 256;
            let fy = (y + sy / 2) % 256;
            let bx = (x + sx) % 512;
            let by = (y + sy) % 512;
            let cross = fx / 8 % 4 == 1 && fy / 8 % 4 == 1 && (fx % 8 == 3 || fy % 8 == 3);
            let mut color = if cross {
                0x03ff
            } else {
                [0x0260, 0x03a0, 0x7d20, 0x7e80]
                    [((bx / 32 + by / 32) % 2) * 2 + (bx / 2 + by / 2) % 2]
            };
            let affine = rotated || zoomed;
            let source = if affine && (104..136).contains(&x) && (64..96).contains(&y) {
                let a = f64::from(if rotated {
                    if zoomed {
                        90
                    } else {
                        181
                    }
                } else {
                    128
                });
                let b = if rotated { a } else { 0.0 };
                let dx = x as f64 - 120.0;
                let dy = y as f64 - 80.0;
                Some((
                    ((a * dx + b * dy) / 256.0 + 8.0).floor() as i32,
                    ((-b * dx + a * dy) / 256.0 + 8.0).floor() as i32,
                ))
            } else if !affine && (112..128).contains(&x) && (72..88).contains(&y) {
                Some((
                    if flipped {
                        127 - x as i32
                    } else {
                        x as i32 - 112
                    },
                    y as i32 - 72,
                ))
            } else {
                None
            };
            if let Some((tx, ty)) = source.filter(|&(tx, ty)| {
                (2..14).contains(&tx) && (1..15).contains(&ty) && !(cross && behind)
            }) {
                color = if tx == 10 && ty == 7 {
                    0
                } else if tx == 12 && (10..13).contains(&ty) {
                    0x03ff
                } else if (5..10).contains(&ty) {
                    0x7fff
                } else {
                    0x001f
                };
            }
            assert_eq!(
                frame.pixels()[y * WIDTH + x],
                rgb555_to_rgb888(color),
                "pixel {x},{y} scroll {sx},{sy}"
            );
        }
    }
}

#[test]
fn cpu_tile_demo_copies_assets_scrolls_wraps_cancels_opposites_and_resets() {
    use gba_core::input::{Button, Buttons};
    use gba_demos::tile_demo::{TileDemo, TILE_STATE};
    let mut demo = TileDemo::new().unwrap();
    let mut frame = Framebuffer::default();
    assert_eq!(demo.machine().memory().read16(VRAM).unwrap(), 0);
    assert_eq!(demo.machine().memory().read16(DISPCNT).unwrap(), 0);
    let mut updates = 0;
    for (buttons, x, y) in [
        (Buttons::default(), 0, 0),
        (
            Buttons::default()
                .with(Button::Left, true)
                .with(Button::Up, true),
            510,
            510,
        ),
        (Buttons::from_bits(0xf0), 510, 510),
        (
            Buttons::default()
                .with(Button::Right, true)
                .with(Button::Down, true),
            0,
            0,
        ),
        (
            Buttons::default()
                .with(Button::Right, true)
                .with(Button::Down, true),
            2,
            2,
        ),
        (
            Buttons::default()
                .with(Button::Start, true)
                .with(Button::Right, true),
            0,
            0,
        ),
    ] {
        let before = demo.machine().memory().display_position();
        demo.frame(buttons, &mut frame).unwrap();
        updates += 1;
        assert_scene(&frame, x, y, false, false, false, false);
        let m = demo.machine().memory();
        assert_eq!(m.read32(TILE_STATE).unwrap(), updates);
        assert_eq!(m.read32(TILE_STATE + 4).unwrap(), x as u32);
        assert_eq!(m.read32(TILE_STATE + 8).unwrap(), y as u32);
        assert_eq!(m.read16(DISPCNT).unwrap(), 0x1340);
        assert_eq!(m.display_position().scanline, 160);
        if updates > 1 {
            assert_eq!(m.display_position().vblanks, before.vblanks + 1);
        }
    }
}

#[test]
fn cpu_sprite_controls_flip_and_change_priority_through_oam() {
    use gba_core::{
        input::{Button, Buttons},
        memory::OAM_START,
    };
    use gba_demos::tile_demo::TileDemo;
    let mut demo = TileDemo::new().unwrap();
    let mut frame = Framebuffer::default();
    for _ in 0..16 {
        demo.frame(Buttons::default().with(Button::Right, true), &mut frame)
            .unwrap();
    }
    for (flipped, behind) in [
        (false, false),
        (true, false),
        (true, true),
        (false, true),
        (false, false),
    ] {
        demo.frame(
            Buttons::default()
                .with(Button::A, flipped)
                .with(Button::B, behind),
            &mut frame,
        )
        .unwrap();
        assert_scene(&frame, 32, 0, flipped, behind, false, false);
        let m = demo.machine().memory();
        assert_eq!(m.read16(OAM_START).unwrap(), 72);
        assert_eq!(
            m.read16(OAM_START + 2).unwrap(),
            0x4070 | if flipped { 0x1000 } else { 0 }
        );
        assert_eq!(
            m.read16(OAM_START + 4).unwrap(),
            if behind { 0x400 } else { 0 }
        );
        for i in 1..128 {
            assert_eq!(m.read16(OAM_START + i * 8).unwrap(), 0x200);
        }
    }
}

#[test]
fn cpu_affine_controls_write_matrices_preserve_center_and_return_to_regular_mode() {
    use gba_core::{
        input::{Button, Buttons},
        memory::OAM_START,
    };
    use gba_demos::tile_demo::TileDemo;
    let mut demo = TileDemo::new().unwrap();
    let mut frame = Framebuffer::default();
    // Put foreground crosses over the sprite so X also tests affine composition.
    for _ in 0..16 {
        demo.frame(Buttons::default().with(Button::Right, true), &mut frame)
            .unwrap();
    }
    for bits in [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 0] {
        let flipped = bits & 1 != 0;
        let behind = bits & 2 != 0;
        let rotated = bits & 4 != 0;
        let zoomed = bits & 8 != 0;
        let buttons = Buttons::default()
            .with(Button::A, flipped)
            .with(Button::B, behind)
            .with(Button::L, rotated)
            .with(Button::R, zoomed);
        let before = demo.machine().memory().display_position();
        demo.frame(buttons, &mut frame).unwrap();
        assert_scene(&frame, 32, 0, flipped, behind, rotated, zoomed);
        let m = demo.machine().memory();
        let affine = rotated || zoomed;
        assert_eq!(
            m.read16(OAM_START).unwrap(),
            if affine { 0x340 } else { 72 }
        );
        assert_eq!(
            m.read16(OAM_START + 2).unwrap(),
            if affine {
                0x4068
            } else {
                0x4070 | if flipped { 0x1000 } else { 0 }
            }
        );
        let a: i16 = match (rotated, zoomed) {
            (false, false) => 256,
            (false, true) => 128,
            (true, false) => 181,
            (true, true) => 90,
        };
        let b = if rotated { a } else { 0 };
        for (i, value) in [a, b, -b, a].into_iter().enumerate() {
            assert_eq!(
                m.read16(OAM_START + 6 + i as u32 * 8).unwrap(),
                value as u16
            );
        }
        for i in 1..128 {
            assert_eq!(m.read16(OAM_START + i * 8).unwrap(), 0x200);
        }
        assert_eq!(m.display_position().vblanks, before.vblanks + 1);
        assert_eq!(m.display_position().scanline, 160);
    }
}

#[test]
fn green_swap_runs_after_tile_compositing() {
    let mut m = setup();
    m.write16(VRAM, 0x21).unwrap();
    m.write16(GREENSWAP, 1).unwrap();
    assert_eq!(pixel(&m, 0, 0), 0xffff00);
    assert_eq!(pixel(&m, 1, 0), 0);
}
