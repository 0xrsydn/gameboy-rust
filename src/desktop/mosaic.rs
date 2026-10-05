use super::{create_window, read_buttons};
use gba_rust::{
    display::{CPU_HZ, CYCLES_PER_FRAME, VISIBLE_LINES},
    input::{Button, Buttons},
    io::{BG0CNT, BG1CNT, DISPCNT, MOSAIC},
    memory::OAM_START,
    mosaic_demo::{MosaicDemo, MOSAIC_STATE},
    video::{rgb555_to_rgb888, Framebuffer, HEIGHT, WIDTH},
};
use minifb::Key;
use std::{
    error::Error,
    io, thread,
    time::{Duration, Instant},
};

pub fn run(smoke_test: bool) -> Result<(), Box<dyn Error>> {
    let mut window = create_window("GBA Rust | Mosaic | Arrows: scroll | Z: clear BG | X: clear OBJ | Q/W: transform | Esc: exit")?;
    window.set_target_fps(0);
    let period = Duration::from_secs_f64(f64::from(CYCLES_PER_FRAME) / f64::from(CPU_HZ));
    let mut demo = MosaicDemo::new()?;
    let mut frame = Framebuffer::default();
    let mut presented = 0;
    println!("Mosaic demo: The CPU cycles block sizes from 1x1 to 16x16, changing every eight VBlank updates.");
    println!("Arrows scroll; hold Z to bypass background mosaic, X to bypass sprite mosaic, or both to bypass all mosaic.");
    println!("Q rotates the sprite; W zooms; Enter resets scrolling; Escape exits. Original test content, not a game.");
    while window.is_open() && !window.is_key_down(Key::Escape) {
        let started = Instant::now();
        let buttons = if smoke_test {
            Buttons::default()
                .with(
                    if presented < 64 {
                        Button::Right
                    } else {
                        Button::Left
                    },
                    true,
                )
                .with(Button::Down, true)
                .with(Button::A, (32..64).contains(&presented))
                .with(Button::B, (64..96).contains(&presented))
        } else {
            read_buttons(window.is_active(), |key| window.is_key_down(key))
        };
        demo.frame(buttons, &mut frame)?;
        if smoke_test {
            validate(&demo, &frame, presented)?;
        }
        window.update_with_buffer(frame.pixels(), WIDTH, HEIGHT)?;
        presented += 1;
        if smoke_test && presented == 128 {
            println!("Mosaic smoke test passed: 128 VBlank-synchronized frames; CPU state, independent BG/OBJ controls, size cycling, and every pixel verified.");
            return Ok(());
        }
        thread::sleep(period.saturating_sub(started.elapsed()));
    }
    if smoke_test {
        return Err(io::Error::other("window closed before mosaic smoke test finished").into());
    }
    Ok(())
}

fn validate(demo: &MosaicDemo, frame: &Framebuffer, index: usize) -> Result<(), Box<dyn Error>> {
    let sx = if index < 64 {
        2 * (index + 1)
    } else {
        128 - 2 * (index - 63)
    };
    let sy = 2 * (index + 1);
    let size = 1 + ((index + 1) / 8) % 16;
    let bg_size = if (32..64).contains(&index) { 1 } else { size };
    let obj_size = if (64..96).contains(&index) { 1 } else { size };
    let m = demo.machine().memory();
    let position = m.display_position();
    if m.read32(MOSAIC_STATE)? != index as u32 + 1
        || m.read32(MOSAIC_STATE + 4)? != sx as u32
        || m.read32(MOSAIC_STATE + 8)? != sy as u32
        || m.read16(DISPCNT)? != 0x1340
        || m.read16(BG0CNT)? != 0x1844
        || m.read16(BG1CNT)? != 0xd0c1
        || m.read16(OAM_START)? != 0x1048
        || m.read16(OAM_START + 2)? != 0x4070
        || m.read16(OAM_START + 4)? != 0
        || m.read32(MOSAIC)? != 0
        || u32::from(position.scanline) != VISIBLE_LINES
        || u32::from(position.line_cycle) >= demo.machine().last_timing().total()
    {
        return Err(
            io::Error::other("mosaic smoke test: unexpected CPU state or registers").into(),
        );
    }
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            // Independent scene formula: sample the screen grid before scrolling.
            let mx = x / bg_size * bg_size;
            let my = y / bg_size * bg_size;
            let fx = (mx + sx / 2) % 256;
            let fy = (my + sy / 2) % 256;
            let cross = fx / 8 % 4 == 1 && fy / 8 % 4 == 1 && (fx % 8 == 3 || fy % 8 == 3);
            let bx = (mx + sx) % 512;
            let by = (my + sy) % 512;
            let mut color = if cross {
                0x3ff
            } else {
                [0x260, 0x3a0, 0x7d20, 0x7e80]
                    [((bx / 32 + by / 32) % 2) * 2 + (bx / 2 + by / 2) % 2]
            };
            if (112..128).contains(&x) && (72..88).contains(&y) {
                // All texels in this regular sprite's canvas carry its mosaic flag,
                // including transparent ones. Its first partial blocks clamp.
                let tx = (x / obj_size * obj_size).max(112) - 112;
                let ty = (y / obj_size * obj_size).max(72) - 72;
                if (2..14).contains(&tx) && (1..15).contains(&ty) {
                    color = if tx == 10 && ty == 7 {
                        0
                    } else if tx == 12 && (10..13).contains(&ty) {
                        0x3ff
                    } else if (5..10).contains(&ty) {
                        0x7fff
                    } else {
                        31
                    };
                }
            }
            if frame.pixels()[y * WIDTH + x] != rgb555_to_rgb888(color) {
                return Err(io::Error::other(format!(
                    "mosaic smoke test: unexpected pixel {x},{y} in frame {index}"
                ))
                .into());
            }
        }
    }
    Ok(())
}
