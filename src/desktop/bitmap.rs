use super::{create_window, read_buttons};
use gba_rust::{
    bitmap_demo::{BitmapDemo, BitmapMode, BITMAP_STATE},
    display::{CPU_HZ, CYCLES_PER_FRAME, VISIBLE_LINES},
    input::{Button, Buttons},
    io::DISPCNT,
    video::{rgb555_to_rgb888, Framebuffer, HEIGHT, WIDTH},
};
use minifb::Key;
use std::{
    error::Error,
    io, thread,
    time::{Duration, Instant},
};

pub fn run(mode: BitmapMode, smoke_test: bool) -> Result<(), Box<dyn Error>> {
    let mut window=create_window(&format!("GBA Rust | Bitmap {} | Arrows: pan | Q: rotate | W: zoom | Z: page1 | Enter: reset | Esc: exit",mode as u16))?;
    window.set_target_fps(0);
    let period = Duration::from_secs_f64(f64::from(CYCLES_PER_FRAME) / f64::from(CPU_HZ));
    let mut demo = BitmapDemo::new(mode)?;
    let mut frame = Framebuffer::default();
    let mut presented = 0;
    println!(
        "Mode {} bitmap: Pages alternate every 32 updates. Hold Z to force page1.",
        mode as u16
    );
    println!("Arrows pan; Q rotates 45 degrees; W zooms 2x; Enter resets panning; Escape exits.");
    println!(
        "The CPU copies two original images and switches DISPCNT during VBlank. No game content."
    );
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
                .with(Button::A, (10..20).contains(&presented))
        } else {
            read_buttons(window.is_active(), |key| window.is_key_down(key))
        };
        demo.frame(buttons, &mut frame)?;
        if smoke_test {
            validate(&demo, &frame, mode, presented)?;
        }
        window.update_with_buffer(frame.pixels(), WIDTH, HEIGHT)?;
        presented += 1;
        if smoke_test && presented == 60 {
            println!("Mode {} bitmap smoke test passed: 60 VBlank-synchronized frames; CPU state, page selection, and every pixel verified.",mode as u16);
            return Ok(());
        }
        thread::sleep(period.saturating_sub(started.elapsed()));
    }
    if smoke_test {
        return Err(io::Error::other("window closed before bitmap smoke test finished").into());
    }
    Ok(())
}

fn validate(
    demo: &BitmapDemo,
    frame: &Framebuffer,
    mode: BitmapMode,
    index: usize,
) -> Result<(), Box<dyn Error>> {
    let sx = if index < 30 {
        2 * (index + 1)
    } else {
        60 - 2 * (index - 29)
    };
    let sy = 2 * (index + 1);
    let rotated = (10..30).contains(&index);
    let zoomed = (20..50).contains(&index);
    let page = usize::from((index + 1) & 32 != 0 || (10..20).contains(&index));
    let m = demo.machine().memory();
    let position = m.display_position();
    if m.read32(BITMAP_STATE)? != index as u32 + 1
        || m.read32(BITMAP_STATE + 4)? != sx as u32
        || m.read32(BITMAP_STATE + 8)? != sy as u32
        || m.read16(DISPCNT)? != (0x400 | mode as u16 | (page as u16 * 16))
        || u32::from(position.scanline) != VISIBLE_LINES
        || u32::from(position.line_cycle) >= demo.machine().last_timing().total()
    {
        return Err(io::Error::other(
            "bitmap smoke test: unexpected CPU state, page, or display position",
        )
        .into());
    }
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
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let dx = x as f64 - 120.0;
            let dy = y as f64 - 80.0;
            let tx = (f64::from(w / 2) + sx as f64 + a * dx + b * dy).floor() as i32;
            let ty = (f64::from(h / 2) + sy as f64 - b * dx + a * dy).floor() as i32;
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
            if frame.pixels()[y * WIDTH + x] != rgb555_to_rgb888(color) {
                return Err(io::Error::other(format!(
                    "bitmap smoke test: unexpected pixel {x},{y}"
                ))
                .into());
            }
        }
    }
    Ok(())
}
