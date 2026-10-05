use super::{create_window, read_buttons};
use gba_rust::{
    display::{CPU_HZ, CYCLES_PER_FRAME, VISIBLE_LINES},
    effects_demo::{EffectsDemo, EFFECTS_STATE},
    input::{Button, Buttons},
    io::{BLDALPHA, BLDCNT, DISPCNT, WININ, WINOUT},
    video::{rgb555_to_rgb888, Framebuffer, HEIGHT, WIDTH},
};
use minifb::Key;
use std::{
    error::Error,
    io, thread,
    time::{Duration, Instant},
};

pub fn run(smoke_test: bool) -> Result<(), Box<dyn Error>> {
    let mut window = create_window(
        "GBA Rust | Effects | Arrows: move | Z: blend | X: darken | Enter: reset | Esc: exit",
    )?;
    window.set_target_fps(0);
    let period = Duration::from_secs_f64(f64::from(CYCLES_PER_FRAME) / f64::from(CPU_HZ));
    let mut demo = EffectsDemo::new()?;
    let mut frame = Framebuffer::default();
    let mut presented = 0;
    let mut vertical_active = false;
    println!("Window/effects demo: Arrows scroll and move the unchanged rectangle. Outside it, colors become brighter.");
    println!("Hold Z to blend or X to darken; Z takes precedence. Enter resets; Escape exits.");
    println!(
        "Tile controls also remain active: Q rotates, W zooms, Z flips, X lowers sprite priority."
    );
    println!("The CPU updates WIN0 and color registers during VBlank. Original test content, not a game.");
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
                .with(Button::A, (20..40).contains(&presented))
                .with(Button::B, presented >= 30)
        } else {
            read_buttons(window.is_active(), |key| window.is_key_down(key))
        };
        demo.frame(buttons, &mut frame)?;
        if smoke_test {
            validate(&demo, &frame, presented, &mut vertical_active)?;
        }
        window.update_with_buffer(frame.pixels(), WIDTH, HEIGHT)?;
        presented += 1;
        if smoke_test && presented == 60 {
            println!("Window/effects smoke test passed: 60 VBlank-synchronized frames; CPU state, effect registers, and every pixel verified.");
            return Ok(());
        }
        thread::sleep(period.saturating_sub(started.elapsed()));
    }
    if smoke_test {
        return Err(io::Error::other("window closed before effects smoke test finished").into());
    }
    Ok(())
}

fn validate(
    demo: &EffectsDemo,
    frame: &Framebuffer,
    index: usize,
    vertical_active: &mut bool,
) -> Result<(), Box<dyn Error>> {
    let sx = if index < 30 {
        2 * (index + 1)
    } else {
        60 - 2 * (index - 29)
    };
    let sy = 2 * (index + 1);
    let alpha = (20..40).contains(&index);
    let behind = index >= 30;
    let control = if alpha {
        0x251
    } else if behind {
        0xff
    } else {
        0xbf
    };
    let m = demo.machine().memory();
    let position = m.display_position();
    if m.read32(EFFECTS_STATE)? != index as u32 + 1
        || m.read32(EFFECTS_STATE + 4)? != sx as u32
        || m.read32(EFFECTS_STATE + 8)? != sy as u32
        || m.read16(DISPCNT)? != 0x3340
        || m.read16(WININ)? != 0x1f
        || m.read16(WINOUT)? != 0x3f
        || m.read16(BLDCNT)? != control
        || m.read16(BLDALPHA)? != 0x808
        || u32::from(position.scanline) != VISIBLE_LINES
        || u32::from(position.line_cycle) >= demo.machine().last_timing().total()
    {
        return Err(
            io::Error::other("effects smoke test: unexpected CPU state or registers").into(),
        );
    }
    // The scripted ARM update writes WIN0V during row160, after its comparison.
    // Track every subsequent line independently of the renderer, including VBlank.
    // The previous call retained state after row160's old-bounds comparison.
    let top = (sy + 40) & 255;
    let bottom = (top + 80) & 255;
    let compare = |active: &mut bool, row| {
        if row == top {
            *active = true;
        }
        if row == bottom {
            *active = false;
        }
    };
    for row in 161..228 {
        compare(vertical_active, row);
    }
    for y in 0..HEIGHT {
        compare(vertical_active, y);
        for x in 0..WIDTH {
            // Independent scene and five-bit arithmetic, without invoking the renderer.
            let fx = (x + sx / 2) % 256;
            let fy = (y + sy / 2) % 256;
            let cross = fx / 8 % 4 == 1 && fy / 8 % 4 == 1 && (fx % 8 == 3 || fy % 8 == 3);
            let bx = (x + sx) % 512;
            let by = (y + sy) % 512;
            let terrain: u16 = [0x260, 0x3a0, 0x7d20, 0x7e80]
                [((bx / 32 + by / 32) % 2) * 2 + (bx / 2 + by / 2) % 2];
            let tx = if alpha {
                127 - x as i32
            } else {
                x as i32 - 112
            };
            let ty = y as i32 - 72;
            let object = (2..14).contains(&tx) && (1..15).contains(&ty);
            let obj_color = if tx == 10 && ty == 7 {
                0
            } else if tx == 12 && (10..13).contains(&ty) {
                0x3ff
            } else if (5..10).contains(&ty) {
                0x7fff
            } else {
                31
            };
            let raw = if object && !(cross && behind) {
                obj_color
            } else if cross {
                0x3ff
            } else {
                terrain
            };
            let inside = x.wrapping_sub(sx + 64) < 112 && *vertical_active;
            let blend = alpha && (cross || object) && !(cross && object);
            let mut color = 0;
            for shift in [0, 5, 10] {
                let c = (raw >> shift) & 31;
                let channel = if inside {
                    c
                } else if alpha {
                    if blend {
                        (c + ((terrain >> shift) & 31)) / 2
                    } else {
                        c
                    }
                } else if behind {
                    c - c / 2
                } else {
                    c + (31 - c) / 2
                };
                color |= channel << shift;
            }
            if frame.pixels()[y * WIDTH + x] != rgb555_to_rgb888(color) {
                return Err(io::Error::other(format!(
                    "effects smoke test: unexpected pixel {x},{y} in frame {index}"
                ))
                .into());
            }
        }
    }
    compare(vertical_active, 160); // Publication follows entry to row160.
    Ok(())
}
