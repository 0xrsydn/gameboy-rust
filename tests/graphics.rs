use gba_rust::{
    graphics_demo::{GraphicsDemo, BACKGROUND, DEMO_STATE, SQUARE_SIZE},
    input::{Button, Buttons},
    io::{BG2PA, BG2PD, DISPCNT, GREENSWAP, KEYINPUT},
    memory::{Memory, MemoryError, PALETTE_START, VRAM_START},
    timing::{bus_cycles, AccessKind, AccessWidth},
    video::{rgb555_to_rgb888, Framebuffer, VideoError, HEIGHT, WIDTH},
};

fn mode3() -> Memory {
    let mut memory = Memory::new(vec![]).unwrap();
    memory.write16(DISPCNT, 0x403).unwrap();
    memory.write16(BG2PA, 256).unwrap();
    memory.write16(BG2PD, 256).unwrap();
    memory
}

fn assert_demo_frame(demo: &GraphicsDemo, frame: &Framebuffer, x: usize, y: usize, color: u16) {
    let memory = demo.machine().memory();
    assert_eq!(memory.read32(DEMO_STATE + 4).unwrap(), x as u32);
    assert_eq!(memory.read32(DEMO_STATE + 8).unwrap(), y as u32);
    assert_eq!(memory.read32(DEMO_STATE + 12).unwrap(), u32::from(color));
    assert_eq!(memory.read16(DISPCNT).unwrap(), 0x403);
    for py in 0..HEIGHT {
        for px in 0..WIDTH {
            let expected =
                if (x..x + SQUARE_SIZE).contains(&px) && (y..y + SQUARE_SIZE).contains(&py) {
                    color
                } else {
                    BACKGROUND
                };
            assert_eq!(
                frame.pixels()[py * WIDTH + px],
                rgb555_to_rgb888(expected),
                "pixel {px},{py}"
            );
        }
    }
}

#[test]
fn cpu_program_draws_mode3_and_moves_from_keyinput_without_host_vram_writes() {
    let mut demo = GraphicsDemo::new().unwrap();
    let mut frame = Framebuffer::default();
    assert_eq!(demo.machine().memory().read16(DISPCNT).unwrap(), 0);
    assert_eq!(demo.machine().memory().read16(VRAM_START).unwrap(), 0);
    let startup_steps = demo.frame(Buttons::default(), &mut frame).unwrap();
    assert!(startup_steps > WIDTH * HEIGHT);
    assert_demo_frame(&demo, &frame, 112, 72, 0x7fff);
    let previous = demo.machine().memory().display_position();
    let previous_cycles = demo.machine().cycles();
    assert_eq!(previous.scanline, 160);
    let steps = demo
        .frame(
            Buttons::default()
                .with(Button::Right, true)
                .with(Button::Up, true),
            &mut frame,
        )
        .unwrap();
    assert!(steps < startup_steps);
    let position = demo.machine().memory().display_position();
    assert_eq!(position.scanline, 160);
    assert_eq!(position.vblanks, previous.vblanks + 1);
    assert_eq!(position.frames, previous.frames + 1);
    assert_eq!(
        demo.machine().cycles() - previous_cycles,
        u64::from(gba_rust::display::CYCLES_PER_FRAME) + u64::from(position.line_cycle)
            - u64::from(previous.line_cycle)
    );
    assert_demo_frame(&demo, &frame, 114, 70, 0x7fff);
    assert_eq!(demo.machine().memory().read32(DEMO_STATE).unwrap(), 2);
}

#[test]
fn cpu_program_clamps_edges_cancels_opposites_changes_color_and_resets() {
    let mut demo = GraphicsDemo::new().unwrap();
    let mut frame = Framebuffer::default();
    let upper_left = Buttons::default()
        .with(Button::Left, true)
        .with(Button::Up, true);
    for _ in 0..65 {
        demo.frame(upper_left, &mut frame).unwrap();
    }
    assert_demo_frame(&demo, &frame, 0, 0, 0x7fff);
    let all_directions = Buttons::from_bits(0xf0);
    demo.frame(all_directions, &mut frame).unwrap();
    assert_demo_frame(&demo, &frame, 0, 0, 0x7fff);
    let lower_right = Buttons::default()
        .with(Button::Right, true)
        .with(Button::Down, true);
    for _ in 0..120 {
        demo.frame(lower_right, &mut frame).unwrap();
    }
    assert_demo_frame(&demo, &frame, 224, 144, 0x7fff);
    demo.frame(all_directions, &mut frame).unwrap();
    assert_demo_frame(&demo, &frame, 224, 144, 0x7fff);
    for (buttons, color) in [
        (Buttons::from_bits(2), 0x3e0),
        (Buttons::from_bits(3), 0x1f),
        (Buttons::default(), 0x7fff),
    ] {
        demo.frame(buttons, &mut frame).unwrap();
        assert_demo_frame(&demo, &frame, 224, 144, color);
    }
    demo.frame(Buttons::default().with(Button::Start, true), &mut frame)
        .unwrap();
    assert_demo_frame(&demo, &frame, 112, 72, 0x7fff);
}

#[test]
fn mode3_renders_row_major_colors_and_ignores_page_and_pixel_top_bit() {
    let mut memory = mode3();
    let mut frame = Framebuffer::default();
    for (index, color) in [
        (0, 0x801f),
        (1, 0x03e0),
        (WIDTH, 0x7c00),
        (WIDTH * HEIGHT - 1, 0x7fff),
    ] {
        memory
            .write16(VRAM_START + index as u32 * 2, color)
            .unwrap();
    }
    memory.write16(DISPCNT, 0x413).unwrap(); // Page bit has no effect in Mode 3.
    memory.render_frame(&mut frame).unwrap();
    assert_eq!(frame.pixels()[0], 0xff0000);
    assert_eq!(frame.pixels()[1], 0x00ff00);
    assert_eq!(frame.pixels()[WIDTH], 0x0000ff);
    assert_eq!(frame.pixels()[WIDTH * HEIGHT - 1], 0xffffff);
    assert_eq!(frame.pixels()[2], 0);
    assert_eq!(memory.cycles(), 0);
}

#[test]
fn forced_blank_is_white_and_disabled_bg2_uses_palette_backdrop() {
    let mut memory = mode3();
    let mut frame = Framebuffer::default();
    memory.write16(PALETTE_START, 0x3e0).unwrap();
    memory.write16(DISPCNT, 3).unwrap();
    memory.render_frame(&mut frame).unwrap();
    assert!(frame.pixels().iter().all(|&pixel| pixel == 0x00ff00));
    memory.write16(DISPCNT, 0x403).unwrap();
    memory.render_frame(&mut frame).unwrap();
    assert!(frame.pixels().iter().all(|&pixel| pixel == 0)); // Mode 3 black is opaque.
    memory.write16(DISPCNT, 0xffff).unwrap();
    memory.render_frame(&mut frame).unwrap();
    assert!(frame.pixels().iter().all(|&pixel| pixel == 0xffffff));
}

#[test]
fn unsupported_modes_and_layers_report_errors_without_changing_output() {
    let mut memory = mode3();
    let mut frame = Framebuffer::default();
    frame.clear(0x001f);
    for mode in [6, 7] {
        memory.write16(DISPCNT, 0x400 | mode).unwrap();
        assert_eq!(
            memory.render_frame(&mut frame),
            Err(VideoError::UnsupportedMode(mode as u8))
        );
    }
    for layer in [0x100, 0x200, 0x800] {
        memory.write16(DISPCNT, 0x403 | layer).unwrap();
        assert_eq!(
            memory.render_frame(&mut frame),
            Err(VideoError::UnsupportedLayers(0x403 | layer))
        );
    }
    assert!(frame.pixels().iter().all(|&pixel| pixel == 0xff0000));
}

#[test]
fn display_control_masks_cgb_bit_and_green_swap_exchanges_only_green() {
    let mut memory = mode3();
    memory.write32(DISPCNT, u32::MAX).unwrap();
    assert_eq!(memory.read32(DISPCNT).unwrap(), 0x0001_fff7);
    memory.write8(DISPCNT, 3).unwrap();
    memory.write8(DISPCNT + 1, 4).unwrap();
    memory.write8(GREENSWAP + 1, 0xff).unwrap();
    assert_eq!(memory.read32(DISPCNT).unwrap(), 0x0001_0403);
    memory.write16(VRAM_START, 0x001f).unwrap();
    memory.write16(VRAM_START + 2, 0x7fe0).unwrap(); // Green + blue
    let mut frame = Framebuffer::default();
    memory.render_frame(&mut frame).unwrap();
    assert_eq!(frame.pixels()[0], 0xffff00);
    assert_eq!(frame.pixels()[1], 0x0000ff);
    memory.write8(GREENSWAP, 0).unwrap();
    memory.render_frame(&mut frame).unwrap();
    assert_eq!(frame.pixels()[0], 0xff0000);
    assert_eq!(frame.pixels()[1], 0x00ffff);
}

#[test]
fn palette_and_vram_mirrors_preserve_word_and_halfword_layouts() {
    let mut memory = mode3();
    memory.write32(PALETTE_START + 0x400, 0x1234_5678).unwrap();
    assert_eq!(memory.read32(PALETTE_START).unwrap(), 0x1234_5678);
    memory.write16(0x05ff_fffe, 0xabcd).unwrap();
    assert_eq!(memory.read16(PALETTE_START + 0x3fe).unwrap(), 0xabcd);
    for (mirror, physical) in [
        (0x0602_0000, VRAM_START),
        (0x0601_8000, VRAM_START + 0x10000),
        (0x06ff_fffc, VRAM_START + 0x17ffc),
    ] {
        memory.write32(mirror, 0x1234_5678).unwrap();
        assert_eq!(memory.read32(physical).unwrap(), 0x1234_5678);
        assert_eq!(memory.read8(mirror).unwrap(), 0x78);
        assert_eq!(memory.read16(mirror + 2).unwrap(), 0x1234);
    }
    memory.write32(VRAM_START + 0x17ffc, 0xabcd_1234).unwrap();
    assert_eq!(memory.read32(VRAM_START + 0x1fffc).unwrap(), 0xabcd_1234);
    assert_eq!(memory.read32(0x0700_0000).unwrap(), 0); // OAM starts cleared.
}

#[test]
fn video_byte_writes_duplicate_bg_and_palette_but_ignore_obj_area() {
    let mut memory = mode3();
    for address in [
        PALETTE_START,
        PALETTE_START + 0x3ff,
        VRAM_START,
        VRAM_START + 1,
        VRAM_START + 0x13fff,
    ] {
        memory.write8(address, 0x5a).unwrap();
        assert_eq!(memory.read16(address & !1).unwrap(), 0x5a5a);
    }
    for address in [
        VRAM_START + 0x14000,
        VRAM_START + 0x17fff,
        VRAM_START + 0x1ffff,
    ] {
        memory.write16(address & !1, 0x1234).unwrap();
        memory.write8(address, 0xff).unwrap();
        assert_eq!(memory.read16(address & !1).unwrap(), 0x1234);
    }
    memory.write16(DISPCNT, 0).unwrap(); // Tile-mode OBJ area starts at 64 KiB.
    memory.write16(VRAM_START + 0x10000, 0xabcd).unwrap();
    memory.write8(VRAM_START + 0x18000, 0).unwrap();
    assert_eq!(memory.read16(VRAM_START + 0x10000).unwrap(), 0xabcd);
}

#[test]
fn misaligned_video_writes_leave_all_bytes_unchanged() {
    let mut memory = mode3();
    for address in [PALETTE_START, VRAM_START] {
        memory.write32(address, 0x1234_5678).unwrap();
        assert_eq!(
            memory.write16(address + 1, 0),
            Err(MemoryError::Unaligned(address + 1))
        );
        assert_eq!(
            memory.write32(address + 2, 0),
            Err(MemoryError::Unaligned(address + 2))
        );
        assert_eq!(memory.read32(address).unwrap(), 0x1234_5678);
    }
}

#[test]
fn keyinput_exposes_all_ten_buttons_active_low_and_ignores_writes() {
    let mut memory = mode3();
    assert_eq!(memory.read16(KEYINPUT).unwrap(), 0x3ff);
    for bits in 0..=0x3ff {
        memory.set_buttons(Buttons::from_bits(bits));
        assert_eq!(memory.read16(KEYINPUT).unwrap(), !bits & 0x3ff);
        memory.write16(KEYINPUT, 0).unwrap();
        assert_eq!(memory.read16(KEYINPUT).unwrap(), !bits & 0x3ff);
    }
    let buttons = Buttons::from_bits(0xffff)
        .with(Button::A, false)
        .with(Button::L, false);
    assert_eq!(buttons.bits(), 0x1fe);
    memory.set_buttons(buttons);
    memory.write8(KEYINPUT + 1, 0).unwrap();
    assert_eq!(memory.read8(KEYINPUT).unwrap(), 1);
    assert_eq!(memory.read8(KEYINPUT + 1).unwrap(), 2);
    memory.set_buttons(Buttons::default());
    assert_eq!(memory.read16(KEYINPUT).unwrap(), 0x3ff);
    assert_eq!(memory.cycles(), 0);
    assert_eq!(
        memory.read16(KEYINPUT + 2),
        Err(MemoryError::Unmapped(KEYINPUT + 2))
    ); // KEYCNT deferred.
}

#[test]
fn video_bus_widths_have_nominal_16_bit_bus_costs() {
    for address in [PALETTE_START, VRAM_START, 0x06ff_fffc] {
        for kind in [AccessKind::Sequential, AccessKind::NonSequential] {
            assert_eq!(bus_cycles(0, address, AccessWidth::Byte, kind), 1);
            assert_eq!(bus_cycles(0, address, AccessWidth::Halfword, kind), 1);
            assert_eq!(bus_cycles(0, address, AccessWidth::Word, kind), 2);
        }
    }
}
