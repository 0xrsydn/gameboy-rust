//! Native desktop presentation. No window-library types enter the emulator core.

mod affine;
mod affine_raster;
mod bitmap;
mod effects;
mod mosaic;
mod raster;
mod rom;
pub use affine::run as run_affine;
pub use affine_raster::run as run_affine_raster;
pub use bitmap::run as run_bitmap;
pub use effects::run as run_effects;
pub use mosaic::run as run_mosaic;
pub use raster::run as run_raster;
pub use rom::{run as run_rom, Playback as RomPlayback};

use std::{
    error::Error,
    io, thread,
    time::{Duration, Instant},
};

use gba_core::{
    display::{CPU_HZ, CYCLES_PER_FRAME, VISIBLE_LINES},
    input::{Button, Buttons},
    video::{rgb555_to_rgb888, Framebuffer, HEIGHT, WIDTH},
};
use gba_demos::{
    graphics_demo::{GraphicsDemo, BACKGROUND, DEMO_STATE, SQUARE_SIZE},
    tile_demo::{TileDemo, TILE_STATE},
};
use minifb::{Key, KeyRepeat, Scale, Window, WindowOptions};

const TITLE: &str =
    "GBA Rust | Display test (not a game) | Arrows: move | Space: pause | Esc: exit";
const CURSOR_SIZE: usize = 8;

#[derive(Debug, Default, PartialEq, Eq)]
struct Input {
    left: bool,
    right: bool,
    up: bool,
    down: bool,
    toggle_pause: bool,
}

/// minifb 0.28.0's macOS backend compares mfb_is_active() to zero,
/// although the native function returns true for an active window.
/// Keep this workaround tied to the exact dependency version in Cargo.toml.
fn is_focused(reported_active: bool) -> bool {
    if cfg!(target_os = "macos") {
        !reported_active
    } else {
        reported_active
    }
}

pub(super) fn read_buttons(reported_active: bool, key_down: impl Fn(Key) -> bool) -> Buttons {
    if !is_focused(reported_active) {
        return Buttons::default();
    }
    [
        (Key::Z, Button::A),
        (Key::X, Button::B),
        (Key::Backspace, Button::Select),
        (Key::Enter, Button::Start),
        (Key::Right, Button::Right),
        (Key::Left, Button::Left),
        (Key::Up, Button::Up),
        (Key::Down, Button::Down),
        (Key::W, Button::R),
        (Key::Q, Button::L),
    ]
    .into_iter()
    .fold(Buttons::default(), |buttons, (key, button)| {
        buttons.with(button, key_down(key))
    })
}

fn read_input(reported_active: bool, key_down: impl Fn(Key) -> bool, pause_pressed: bool) -> Input {
    if !is_focused(reported_active) {
        return Input::default();
    }
    Input {
        left: key_down(Key::Left),
        right: key_down(Key::Right),
        up: key_down(Key::Up),
        down: key_down(Key::Down),
        toggle_pause: pause_pressed,
    }
}

struct DisplayDemo {
    tick: usize,
    x: usize,
    y: usize,
    paused: bool,
}

impl Default for DisplayDemo {
    fn default() -> Self {
        Self {
            tick: 0,
            x: (WIDTH - CURSOR_SIZE) / 2,
            y: (HEIGHT - CURSOR_SIZE) / 2,
            paused: false,
        }
    }
}

impl DisplayDemo {
    fn update(&mut self, input: Input) {
        if input.toggle_pause {
            self.paused = !self.paused;
        }
        if !self.paused {
            self.tick = (self.tick + 1) % WIDTH;
        }
        // Opposite directions cancel. Movement remains available while paused.
        let dx = i32::from(input.right) - i32::from(input.left);
        let dy = i32::from(input.down) - i32::from(input.up);
        self.x = (self.x as i32 + dx * 2).clamp(0, (WIDTH - CURSOR_SIZE) as i32) as usize;
        self.y = (self.y as i32 + dy * 2).clamp(0, (HEIGHT - CURSOR_SIZE) as i32) as usize;
    }

    fn draw(&self, frame: &mut Framebuffer) {
        // Original host-generated test image, not output from the emulated CPU.
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let red = (x * 31 / (WIDTH - 1)) as u16;
                let green = (y * 31 / (HEIGHT - 1)) as u16;
                let blue = if (x / 16 + y / 16) % 2 == 0 { 8 } else { 12 };
                frame.set_pixel(x, y, red | (green << 5) | (blue << 10));
            }
        }
        // Color bars check RGB555 channel order and pixel scaling.
        let bars = [0x001f, 0x03e0, 0x7c00, 0x03ff, 0x7fe0, 0x7c1f, 0x7fff, 0];
        for y in 8..24 {
            for x in 8..WIDTH - 8 {
                frame.set_pixel(x, y, bars[(x - 8) * bars.len() / (WIDTH - 16)]);
            }
        }
        for y in 32..HEIGHT {
            frame.set_pixel(self.tick, y, 0x7fff);
        }
        for y in self.y..self.y + CURSOR_SIZE {
            for x in self.x..self.x + CURSOR_SIZE {
                let border = x == self.x
                    || y == self.y
                    || x == self.x + CURSOR_SIZE - 1
                    || y == self.y + CURSOR_SIZE - 1;
                frame.set_pixel(x, y, if border { 0 } else { 0x7fff });
            }
        }
        // Green status square while animating, red while paused.
        for y in 1..5 {
            for x in 1..5 {
                frame.set_pixel(x, y, if self.paused { 0x001f } else { 0x03e0 });
            }
        }
    }
}

/// Must run on the main thread for macOS AppKit.
fn create_window(title: &str) -> Result<Window, Box<dyn Error>> {
    let mut window = Window::new(
        title,
        WIDTH,
        HEIGHT,
        WindowOptions {
            scale: Scale::X4,
            resize: false,
            ..WindowOptions::default()
        },
    ).map_err(|error| io::Error::other(format!(
        "could not open the desktop window: {error}. Run from a logged-in macOS desktop session; use --cpu-demo for terminal-only output."
    )))?;
    // Default host-test cap. The graphics window replaces this with a GBA-rate cap.
    window.set_target_fps(60);
    Ok(window)
}

/// Original host-generated display test. Keep this separate from CPU graphics.
pub fn run(frame_limit: Option<usize>) -> Result<(), Box<dyn Error>> {
    let mut window = create_window(TITLE)?;
    let mut frame = Framebuffer::default();
    let mut demo = DisplayDemo::default();
    let mut presented = 0;
    println!("Display test: 240x160 at 4x scale. Arrows move; Space pauses; Escape exits.");

    while window.is_open() && !window.is_key_down(Key::Escape) {
        let reported_active = window.is_active();
        let input = read_input(
            reported_active,
            |key| window.is_key_down(key),
            window.is_key_pressed(Key::Space, KeyRepeat::No),
        );
        demo.update(input);
        demo.draw(&mut frame);
        window.update_with_buffer(frame.pixels(), WIDTH, HEIGHT)?;
        presented += 1;
        if frame_limit.is_some_and(|limit| presented >= limit) {
            println!("Window smoke test passed: {presented} frames submitted.");
            return Ok(());
        }
    }

    if frame_limit.is_some() {
        return Err(io::Error::other("window closed before the smoke test finished").into());
    }
    Ok(())
}

/// Original CPU-driven Mode 3 demo. The smoke test supplies scripted buttons;
/// normal execution reads physical keyboard input through the focus workaround.
pub fn run_graphics(smoke_test: bool) -> Result<(), Box<dyn Error>> {
    let mut window = create_window(
        "GBA Rust | CPU Mode 3 demo | Arrows: move | Z/X: color | Enter: reset | Esc: exit",
    )?;
    window.set_target_fps(0);
    let frame_period = Duration::from_secs_f64(f64::from(CYCLES_PER_FRAME) / f64::from(CPU_HZ));
    let mut demo = GraphicsDemo::new()?;
    let mut frame = Framebuffer::default();
    let mut presented = 0;
    println!("CPU Mode 3 demo: Arrows move; Z/X change color; Enter resets; Escape exits.");
    println!("BIOS VBlankIntrWait uses HALT and IRQ dispatch. Presentation targets 59.73 Hz; rows capture at HBlank and publish at VBlank.");
    while window.is_open() && !window.is_key_down(Key::Escape) {
        let frame_started = Instant::now();
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
                .with(Button::A, (20..40).contains(&presented))
        } else {
            read_buttons(window.is_active(), |key| window.is_key_down(key))
        };
        demo.frame(buttons, &mut frame)?;
        if smoke_test {
            validate_graphics_smoke(&demo, &frame, presented)?;
        }
        window.update_with_buffer(frame.pixels(), WIDTH, HEIGHT)?;
        presented += 1;
        if smoke_test && presented == 60 {
            println!("CPU graphics smoke test passed: 60 VBlank-synchronized frames; scripted movement and colors verified.");
            return Ok(());
        }
        // Slow hosts do not skip emulated frames. Sleeping only limits presentation;
        // it never advances the emulated display clock.
        thread::sleep(frame_period.saturating_sub(frame_started.elapsed()));
    }
    if smoke_test {
        return Err(
            io::Error::other("window closed before the graphics smoke test finished").into(),
        );
    }
    Ok(())
}

fn validate_graphics_smoke(
    demo: &GraphicsDemo,
    frame: &Framebuffer,
    index: usize,
) -> Result<(), Box<dyn Error>> {
    let x = if index < 30 {
        112 + 2 * (index + 1)
    } else {
        172 - 2 * (index - 29)
    };
    let color = if (20..40).contains(&index) {
        0x001f
    } else {
        0x7fff
    };
    let memory = demo.machine().memory();
    let position = memory.display_position();
    if u32::from(position.scanline) != VISIBLE_LINES
        || u32::from(position.line_cycle) >= demo.machine().last_timing().total()
    {
        return Err(
            io::Error::other("graphics smoke test: presentation missed VBlank entry").into(),
        );
    }
    if memory.read32(DEMO_STATE)? != index as u32 + 1
        || memory.read32(DEMO_STATE + 4)? != x as u32
        || memory.read32(DEMO_STATE + 8)? != 72
        || memory.read32(DEMO_STATE + 12)? != u32::from(color)
    {
        return Err(io::Error::other("graphics smoke test: unexpected CPU demo state").into());
    }
    for (offset, &pixel) in frame.pixels().iter().enumerate() {
        let inside = (x..x + SQUARE_SIZE).contains(&(offset % WIDTH))
            && (72..72 + SQUARE_SIZE).contains(&(offset / WIDTH));
        if pixel != rgb555_to_rgb888(if inside { color } else { BACKGROUND }) {
            return Err(io::Error::other(format!(
                "graphics smoke test: unexpected pixel {offset}"
            ))
            .into());
        }
    }
    Ok(())
}

/// Mode 0 uses the same native window and focus workaround as the Mode 3 demo.
pub fn run_tiles(smoke_test: bool) -> Result<(), Box<dyn Error>> {
    let mut window = create_window(
        "GBA Rust | Arrows: scroll | Q: rotate | W: zoom | Z: flip | X: behind | Enter: reset | Esc: exit",
    )?;
    window.set_target_fps(0);
    let frame_period = Duration::from_secs_f64(f64::from(CYCLES_PER_FRAME) / f64::from(CPU_HZ));
    let mut demo = TileDemo::new()?;
    let mut frame = Framebuffer::default();
    let mut presented = 0;
    println!("CPU Mode 0 demo: Arrows scroll; Q rotates 45 degrees; W zooms 2x; X puts the sprite behind gold crosses.");
    println!("Hold Q/W together to rotate and zoom. Z flips only while Q/W are released. Enter resets scrolling; Escape exits.");
    println!("A CPU-configured sprite stands over two scrolling backgrounds. Original content, not a game.");
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
                .with(
                    Button::A,
                    (5..15).contains(&presented) || (35..55).contains(&presented),
                )
                .with(Button::B, (30..50).contains(&presented))
                .with(Button::L, (10..30).contains(&presented))
                .with(Button::R, (20..50).contains(&presented))
        } else {
            read_buttons(window.is_active(), |key| window.is_key_down(key))
        };
        demo.frame(buttons, &mut frame)?;
        if smoke_test {
            validate_tile_smoke(&demo, &frame, presented)?;
        }
        window.update_with_buffer(frame.pixels(), WIDTH, HEIGHT)?;
        presented += 1;
        if smoke_test && presented == 60 {
            println!("Tile/sprite smoke test passed: 60 VBlank-synchronized frames; scroll, affine matrices, sprite attributes, and all pixels verified.");
            return Ok(());
        }
        thread::sleep(frame_period.saturating_sub(started.elapsed()));
    }
    if smoke_test {
        return Err(io::Error::other("window closed before the tile smoke test finished").into());
    }
    Ok(())
}

fn validate_tile_smoke(
    demo: &TileDemo,
    frame: &Framebuffer,
    index: usize,
) -> Result<(), Box<dyn Error>> {
    let sx = if index < 30 {
        2 * (index + 1)
    } else {
        60 - 2 * (index - 29)
    };
    let sy = 2 * (index + 1);
    let flipped = (5..15).contains(&index) || (35..55).contains(&index);
    let behind = (30..50).contains(&index);
    let rotated = (10..30).contains(&index);
    let zoomed = (20..50).contains(&index);
    let affine = rotated || zoomed;
    let coefficient: i16 = if rotated { 181 } else { 256 } / if zoomed { 2 } else { 1 };
    let off_diagonal = if rotated { coefficient } else { 0 };
    let matrix = [coefficient, off_diagonal, -off_diagonal, coefficient];
    let memory = demo.machine().memory();
    let position = memory.display_position();
    if memory.read32(TILE_STATE)? != index as u32 + 1
        || memory.read32(TILE_STATE + 4)? != sx as u32
        || memory.read32(TILE_STATE + 8)? != sy as u32
        || memory.read16(gba_core::memory::OAM_START)? != if affine { 0x340 } else { 72 }
        || memory.read16(gba_core::memory::OAM_START + 2)?
            != if affine {
                0x4068
            } else {
                0x4070 | if flipped { 0x1000 } else { 0 }
            }
        || memory.read16(gba_core::memory::OAM_START + 4)? != if behind { 0x400 } else { 0 }
        || u32::from(position.scanline) != VISIBLE_LINES
        || u32::from(position.line_cycle) >= demo.machine().last_timing().total()
    {
        return Err(
            io::Error::other("tile smoke test: unexpected CPU state or display position").into(),
        );
    }
    for (i, value) in matrix.into_iter().enumerate() {
        if memory.read16(gba_core::memory::OAM_START + 6 + i as u32 * 8)? != value as u16 {
            return Err(io::Error::other("tile smoke test: unexpected affine matrix").into());
        }
    }
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            // Independent scene formula, not a second call to the tile renderer.
            let fx = (x + sx / 2) % 256;
            let fy = (y + sy / 2) % 256;
            let cross = fx / 8 % 4 == 1 && fy / 8 % 4 == 1 && (fx % 8 == 3 || fy % 8 == 3);
            let bx = (x + sx) % 512;
            let by = (y + sy) % 512;
            let terrain = (bx / 32 + by / 32) % 2;
            let shade = (bx / 2 + by / 2) % 2;
            let mut color = if cross {
                0x03ff
            } else {
                [0x0260, 0x03a0, 0x7d20, 0x7e80][terrain * 2 + shade]
            };
            let source = if affine && (104..136).contains(&x) && (64..96).contains(&y) {
                let dx = x as f64 - 120.0;
                let dy = y as f64 - 80.0;
                Some((
                    ((f64::from(matrix[0]) * dx + f64::from(matrix[1]) * dy) / 256.0 + 8.0).floor()
                        as i32,
                    ((f64::from(matrix[2]) * dx + f64::from(matrix[3]) * dy) / 256.0 + 8.0).floor()
                        as i32,
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
            if frame.pixels()[y * WIDTH + x] != rgb555_to_rgb888(color) {
                return Err(
                    io::Error::other(format!("tile smoke test: unexpected pixel {x},{y}")).into(),
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focused_keyboard_maps_every_gba_button() {
        for (key, button) in [
            (Key::Z, Button::A),
            (Key::X, Button::B),
            (Key::Backspace, Button::Select),
            (Key::Enter, Button::Start),
            (Key::Right, Button::Right),
            (Key::Left, Button::Left),
            (Key::Up, Button::Up),
            (Key::Down, Button::Down),
            (Key::W, Button::R),
            (Key::Q, Button::L),
        ] {
            let buttons = read_buttons(!cfg!(target_os = "macos"), |pressed| pressed == key);
            assert_eq!(buttons.bits(), button as u16);
        }
        assert_eq!(
            read_buttons(!cfg!(target_os = "macos"), |_| true).bits(),
            0x3ff
        );
    }

    #[test]
    fn focus_loss_releases_all_gba_buttons_without_polling_keys() {
        let buttons = read_buttons(cfg!(target_os = "macos"), |_| {
            panic!("do not poll unfocused keys")
        });
        assert_eq!(buttons, Buttons::default());
    }

    #[test]
    fn focused_arrow_keys_reach_movement_on_this_platform() {
        // The macOS backend reports false for a focused window in minifb 0.28.0.
        let focused_report = !cfg!(target_os = "macos");
        for (pressed, dx, dy) in [
            (Key::Left, -2, 0),
            (Key::Right, 2, 0),
            (Key::Up, 0, -2),
            (Key::Down, 0, 2),
        ] {
            let mut demo = DisplayDemo::default();
            let (x, y) = (demo.x as i32, demo.y as i32);
            demo.update(read_input(focused_report, |key| key == pressed, false));
            assert_eq!((demo.x as i32, demo.y as i32), (x + dx, y + dy));
        }
    }

    #[test]
    fn unfocused_input_is_ignored_without_polling_keys() {
        let unfocused_report = cfg!(target_os = "macos");
        let input = read_input(
            unfocused_report,
            |_| panic!("do not poll movement keys while unfocused"),
            true,
        );
        assert_eq!(input, Input::default());
    }

    #[test]
    fn focused_space_press_reaches_pause() {
        let mut demo = DisplayDemo::default();
        demo.update(read_input(!cfg!(target_os = "macos"), |_| false, true));
        assert!(demo.paused);
    }

    #[test]
    fn movement_stays_inside_the_frame() {
        let mut demo = DisplayDemo::default();
        for _ in 0..WIDTH {
            demo.update(Input {
                left: true,
                up: true,
                ..Input::default()
            });
        }
        assert_eq!((demo.x, demo.y), (0, 0));
        for _ in 0..WIDTH {
            demo.update(Input {
                right: true,
                down: true,
                ..Input::default()
            });
        }
        assert_eq!(
            (demo.x, demo.y),
            (WIDTH - CURSOR_SIZE, HEIGHT - CURSOR_SIZE)
        );
    }

    #[test]
    fn opposing_directions_cancel() {
        let mut demo = DisplayDemo::default();
        let position = (demo.x, demo.y);
        demo.update(Input {
            left: true,
            right: true,
            up: true,
            down: true,
            ..Input::default()
        });
        assert_eq!((demo.x, demo.y), position);
    }

    #[test]
    fn pause_stops_animation_but_not_movement() {
        let mut demo = DisplayDemo::default();
        let x = demo.x;
        demo.update(Input {
            toggle_pause: true,
            right: true,
            ..Input::default()
        });
        assert!(demo.paused);
        assert_eq!(demo.tick, 0);
        assert_eq!(demo.x, x + 2);
        demo.update(Input::default());
        assert_eq!(demo.tick, 0);
        demo.update(Input {
            toggle_pause: true,
            ..Input::default()
        });
        assert!(!demo.paused);
        assert_eq!(demo.tick, 1);
    }

    #[test]
    fn animation_wraps_and_draws_a_changing_frame() {
        let mut demo = DisplayDemo::default();
        let mut frame = Framebuffer::default();
        demo.draw(&mut frame);
        let first = frame.pixels().to_vec();
        demo.update(Input::default());
        demo.draw(&mut frame);
        assert_ne!(frame.pixels(), first);
        for _ in 1..WIDTH {
            demo.update(Input::default());
        }
        assert_eq!(demo.tick, 0);
        demo.draw(&mut frame);
        assert_eq!(frame.pixels(), first);
    }
}
