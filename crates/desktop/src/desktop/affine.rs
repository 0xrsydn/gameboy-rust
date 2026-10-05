use super::{create_window, read_buttons};
use gba_core::{
    display::{CPU_HZ, CYCLES_PER_FRAME, VISIBLE_LINES},
    input::{Button, Buttons},
    video::{rgb555_to_rgb888, Framebuffer, HEIGHT, WIDTH},
};
use gba_demos::affine_demo::{AffineDemo, AFFINE_STATE};
use minifb::Key;
use std::{
    error::Error,
    io, thread,
    time::{Duration, Instant},
};

pub fn run(smoke_test: bool) -> Result<(), Box<dyn Error>> {
    let mut window=create_window("GBA Rust | Affine BG | Arrows: pan | Q: rotate | W: zoom | Z: clip | Enter: reset | Esc: exit")?;
    window.set_target_fps(0);
    let period = Duration::from_secs_f64(f64::from(CYCLES_PER_FRAME) / f64::from(CPU_HZ));
    let mut demo = AffineDemo::new()?;
    let mut frame = Framebuffer::default();
    let mut presented = 0;
    println!("Mode 2 background: Arrows pan; hold Q to rotate 45 degrees; W zooms 2x; Z disables wrapping.");
    println!("Q/W can be combined. Enter resets panning; Escape exits. Original CPU-driven content, not a game.");
    while window.is_open() && !window.is_key_down(Key::Escape) {
        let started = Instant::now();
        let buttons = if smoke_test {
            Buttons::default()
                .with(
                    if presented < 30 {
                        Button::Right
                    } else {
                        Button::Left
                    },
                    true,
                )
                .with(Button::Down, true)
                .with(Button::L, (10..30).contains(&presented))
                .with(Button::R, (20..50).contains(&presented))
                .with(Button::A, (30..55).contains(&presented))
        } else {
            read_buttons(window.is_active(), |key| window.is_key_down(key))
        };
        demo.frame(buttons, &mut frame)?;
        if smoke_test {
            validate(&demo, &frame, presented)?;
        }
        window.update_with_buffer(frame.pixels(), WIDTH, HEIGHT)?;
        presented += 1;
        if smoke_test && presented == 60 {
            println!("Affine background smoke test passed: 60 VBlank-synchronized frames; CPU pan state and every pixel verified.");
            return Ok(());
        }
        thread::sleep(period.saturating_sub(started.elapsed()));
    }
    if smoke_test {
        return Err(
            io::Error::other("window closed before affine background smoke test finished").into(),
        );
    }
    Ok(())
}

fn validate(demo: &AffineDemo, frame: &Framebuffer, index: usize) -> Result<(), Box<dyn Error>> {
    let pan_x = if index < 30 {
        2 * (index + 1)
    } else {
        60 - 2 * (index - 29)
    };
    let pan_y = 2 * (index + 1);
    let rotated = (10..30).contains(&index);
    let zoomed = (20..50).contains(&index);
    let clipped = (30..55).contains(&index);
    let m = demo.machine().memory();
    let position = m.display_position();
    if m.read32(AFFINE_STATE)? != index as u32 + 1
        || m.read32(AFFINE_STATE + 4)? != pan_x as u32
        || m.read32(AFFINE_STATE + 8)? != pan_y as u32
        || m.read16(gba_core::io::BG2CNT)? != if clipped { 0x5000 } else { 0x7000 }
        || u32::from(position.scanline) != VISIBLE_LINES
        || u32::from(position.line_cycle) >= demo.machine().last_timing().total()
    {
        return Err(io::Error::other(
            "affine background smoke test: unexpected CPU state or display position",
        )
        .into());
    }
    let a = f64::from(match (rotated, zoomed) {
        (false, false) => 256,
        (false, true) => 128,
        (true, false) => 181,
        (true, true) => 90,
    }) / 256.0;
    let b = if rotated { a } else { 0.0 };
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let dx = x as f64 - 120.0;
            let dy = y as f64 - 80.0;
            let tx = (128.0 + pan_x as f64 + a * dx + b * dy).floor() as i32;
            let ty = (128.0 + pan_y as f64 - b * dx + a * dy).floor() as i32;
            let color = if clipped && (!(0..256).contains(&tx) || !(0..256).contains(&ty)) {
                0x4000
            } else {
                let tx = tx.rem_euclid(256) as usize;
                let ty = ty.rem_euclid(256) as usize;
                let terrain = (tx / 32 + 2 * (ty / 32)) % 4;
                let shade = (tx / 2 + ty / 2) % 2;
                [
                    0x0260, 0x03a0, 0x7d20, 0x7e80, 0x001f, 0x421f, 0x03ff, 0x7fff,
                ][terrain * 2 + shade]
            };
            if frame.pixels()[y * WIDTH + x] != rgb555_to_rgb888(color) {
                return Err(io::Error::other(format!(
                    "affine background smoke test: unexpected pixel {x},{y}"
                ))
                .into());
            }
        }
    }
    Ok(())
}
