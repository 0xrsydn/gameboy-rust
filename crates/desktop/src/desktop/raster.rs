use super::{create_window, read_buttons};
use gba_core::{
    display::{CPU_HZ, CYCLES_PER_FRAME},
    dma::DMA_BASE,
    input::{Button, Buttons},
    video::{rgb555_to_rgb888, Framebuffer, HEIGHT, WIDTH},
};
use gba_demos::raster_demo::{RasterDemo, RASTER_STATE};
use minifb::Key;
use std::{
    error::Error,
    io, thread,
    time::{Duration, Instant},
};

pub fn run(smoke_test: bool) -> Result<(), Box<dyn Error>> {
    let mut window = create_window(
        "GBA Rust | HBlank raster | Left/Right: move bands | Enter: reset | Esc: exit",
    )?;
    window.set_target_fps(0);
    let period = Duration::from_secs_f64(f64::from(CYCLES_PER_FRAME) / f64::from(CPU_HZ));
    let mut demo = RasterDemo::new()?;
    let mut frame = Framebuffer::default();
    let mut presented = 0;
    println!("Raster demo: HBlank DMA changes the backdrop palette after each captured row.");
    println!("Left/Right move the bands; Enter resets; Escape exits. Original CPU-driven content, not a game.");
    println!("Rows sample at HBlank and publish at VBlank. Pixel fetch timing is not yet modeled.");
    while window.is_open() && !window.is_key_down(Key::Escape) {
        let started = Instant::now();
        let buttons = if smoke_test {
            Buttons::default().with(
                if presented < 30 {
                    Button::Right
                } else {
                    Button::Left
                },
                true,
            )
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
            println!("Raster smoke test passed: 60 captured frames; CPU phase, HBlank DMA, VBlank publication, and every pixel verified.");
            return Ok(());
        }
        thread::sleep(period.saturating_sub(started.elapsed()));
    }
    if smoke_test {
        return Err(io::Error::other("window closed before raster smoke test finished").into());
    }
    Ok(())
}

fn validate(demo: &RasterDemo, frame: &Framebuffer, index: usize) -> Result<(), Box<dyn Error>> {
    let phase = if index < 30 { index + 1 } else { 59 - index };
    let memory = demo.machine().memory();
    let position = memory.display_position();
    if memory.read32(RASTER_STATE)? != index as u32 + 1
        || memory.read32(RASTER_STATE + 4)? != phase as u32
        || memory.read16(DMA_BASE + 10)? != 0xa240
        || position.scanline != 160
        || u32::from(position.line_cycle) >= demo.machine().last_timing().total()
        || memory.captured_vblank() != Some(position.vblanks)
    {
        return Err(io::Error::other(
            "raster smoke test: unexpected CPU state or capture position",
        )
        .into());
    }
    for y in 0..HEIGHT {
        let value = ((y + phase) % 32) as u16;
        let color = value + (31 - value) * 32 + (value / 2) * 1024;
        if frame.pixels()[y * WIDTH..(y + 1) * WIDTH]
            .iter()
            .any(|&p| p != rgb555_to_rgb888(color))
        {
            return Err(io::Error::other(format!(
                "raster smoke test: unexpected row {y} in frame {index}"
            ))
            .into());
        }
    }
    Ok(())
}
