mod desktop;

use std::{error::Error, ffi::OsString, io};

use gba_rust::{
    bitmap_demo::BitmapMode,
    cpu::Cpu,
    demo_bios, demo_program, exception_demo_program,
    io::{IF, TIMER_BASE},
    memory::{Memory, ROM_START},
    timer_demo::{timer_demo, TIMER_DEMO_STEPS},
    DEMO_STEPS, EXCEPTION_DEMO_STEPS,
};

#[derive(Debug, PartialEq, Eq)]
enum RunMode {
    Window,
    SmokeTest,
    CpuDemo,
    TimerDemo,
    GraphicsDemo,
    GraphicsSmokeTest,
    EffectsDemo,
    EffectsSmokeTest,
    MosaicDemo,
    MosaicSmokeTest,
    RasterDemo,
    RasterSmokeTest,
    TileDemo,
    TileSmokeTest,
    AffineDemo,
    AffineSmokeTest,
    AffineRasterDemo,
    AffineRasterSmokeTest,
    BitmapDemo(BitmapMode),
    BitmapSmokeTest(BitmapMode),
    Help,
}

fn parse_args(args: &[OsString]) -> Result<RunMode, io::Error> {
    match args {
        [] => Ok(RunMode::Window),
        [arg] if arg == "--smoke-test" => Ok(RunMode::SmokeTest),
        [arg] if arg == "--cpu-demo" => Ok(RunMode::CpuDemo),
        [arg] if arg == "--timer-demo" => Ok(RunMode::TimerDemo),
        [arg] if arg == "--graphics-demo" => Ok(RunMode::GraphicsDemo),
        [arg] if arg == "--graphics-smoke-test" => Ok(RunMode::GraphicsSmokeTest),
        [arg] if arg == "--raster-demo" => Ok(RunMode::RasterDemo),
        [arg] if arg == "--raster-smoke-test" => Ok(RunMode::RasterSmokeTest),
        [arg] if arg == "--mosaic-demo" => Ok(RunMode::MosaicDemo),
        [arg] if arg == "--mosaic-smoke-test" => Ok(RunMode::MosaicSmokeTest),
        [arg] if arg == "--effects-demo" => Ok(RunMode::EffectsDemo),
        [arg] if arg == "--effects-smoke-test" => Ok(RunMode::EffectsSmokeTest),
        [arg] if arg == "--tile-demo" => Ok(RunMode::TileDemo),
        [arg] if arg == "--tile-smoke-test" => Ok(RunMode::TileSmokeTest),
        [arg] if arg == "--affine-raster-demo" => Ok(RunMode::AffineRasterDemo),
        [arg] if arg == "--affine-raster-smoke-test" => Ok(RunMode::AffineRasterSmokeTest),
        [arg] if arg == "--affine-demo" => Ok(RunMode::AffineDemo),
        [arg] if arg == "--affine-smoke-test" => Ok(RunMode::AffineSmokeTest),
        [arg] if arg == "--bitmap4-demo" => Ok(RunMode::BitmapDemo(BitmapMode::Mode4)),
        [arg] if arg == "--bitmap5-demo" => Ok(RunMode::BitmapDemo(BitmapMode::Mode5)),
        [arg] if arg == "--bitmap4-smoke-test" => Ok(RunMode::BitmapSmokeTest(BitmapMode::Mode4)),
        [arg] if arg == "--bitmap5-smoke-test" => Ok(RunMode::BitmapSmokeTest(BitmapMode::Mode5)),
        [arg] if arg == "--help" || arg == "-h" => Ok(RunMode::Help),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: gba-rust [--cpu-demo | --timer-demo | --graphics-demo | --smoke-test | --graphics-smoke-test | --raster-demo | --raster-smoke-test | --mosaic-demo | --mosaic-smoke-test | --effects-demo | --effects-smoke-test | --tile-demo | --tile-smoke-test | --affine-demo | --affine-smoke-test | --affine-raster-demo | --affine-raster-smoke-test | --bitmap4-demo | --bitmap5-demo | --bitmap4-smoke-test | --bitmap5-smoke-test | --help]; ROM loading is not supported yet",
        )),
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    match parse_args(&std::env::args_os().skip(1).collect::<Vec<_>>())? {
        RunMode::Window => desktop::run(None),
        RunMode::SmokeTest => desktop::run(Some(60)),
        RunMode::CpuDemo => run_cpu_demo(),
        RunMode::TimerDemo => run_timer_demo(),
        RunMode::GraphicsDemo => desktop::run_graphics(false),
        RunMode::GraphicsSmokeTest => desktop::run_graphics(true),
        RunMode::RasterDemo => desktop::run_raster(false),
        RunMode::RasterSmokeTest => desktop::run_raster(true),
        RunMode::MosaicDemo => desktop::run_mosaic(false),
        RunMode::MosaicSmokeTest => desktop::run_mosaic(true),
        RunMode::EffectsDemo => desktop::run_effects(false),
        RunMode::EffectsSmokeTest => desktop::run_effects(true),
        RunMode::TileDemo => desktop::run_tiles(false),
        RunMode::TileSmokeTest => desktop::run_tiles(true),
        RunMode::AffineRasterDemo => desktop::run_affine_raster(false),
        RunMode::AffineRasterSmokeTest => desktop::run_affine_raster(true),
        RunMode::AffineDemo => desktop::run_affine(false),
        RunMode::AffineSmokeTest => desktop::run_affine(true),
        RunMode::BitmapDemo(mode) => desktop::run_bitmap(mode, false),
        RunMode::BitmapSmokeTest(mode) => desktop::run_bitmap(mode, true),
        RunMode::Help => {
            println!(
                "GBA Rust — emulator foundation, not game-compatible yet\n\
                No arguments   Open the native 240x160 display test at 4x scale\n\
                --cpu-demo     Run the terminal-only ARM/Thumb instruction demo\n\
                --timer-demo   Run the timer IRQ demo with nominal cycle costs\n\
                --graphics-demo        Open the CPU-driven Mode 3 demo\n\
                --graphics-smoke-test  Verify 60 CPU-driven frames with scripted input\n\
                --raster-demo          Open the HBlank DMA color-band demo\n\
                --raster-smoke-test    Verify 60 scanline-captured raster frames\n\
                --mosaic-demo          Open the CPU-driven mosaic demo\n\
                --mosaic-smoke-test    Verify 128 background and sprite mosaic frames\n\
                --effects-demo         Open the CPU-driven window and color-effects demo\n\
                --effects-smoke-test   Verify 60 window and color-effects frames\n\
                --tile-demo            Open the CPU-driven Mode 0 scrolling demo\n\
                --tile-smoke-test      Verify 60 tile frames with scripted input\n\
                --affine-raster-demo        Open the HBlank DMA affine distortion demo\n\
                --affine-raster-smoke-test  Verify 60 affine raster frames\n\
                --affine-demo          Open the CPU-driven Mode 2 background demo\n\
                --affine-smoke-test    Verify 60 transformed background frames\n\
                --bitmap4-demo        Open the indexed-color page-flipping demo\n\
                --bitmap5-demo        Open the 160x128 RGB555 page-flipping demo\n\
                --bitmap4-smoke-test  Verify 60 Mode 4 bitmap frames\n\
                --bitmap5-smoke-test  Verify 60 Mode 5 bitmap frames\n\
                --smoke-test   Open the host display test for 60 frames\n\
                --help, -h     Show this help\n\n\
                Display test: Arrows move; Space pauses; Escape exits.\n\
                CPU graphics: Arrows move; Z/X change color; Enter resets; Escape exits.\n\
                Raster: Left/Right move bands; Enter resets; Escape exits.\n\
                Mosaic: Arrows scroll; Z bypasses BG mosaic; X bypasses OBJ mosaic; Q/W transform; Enter resets; Escape exits.\n\
                Window effects: Arrows move; Z blends; X darkens; Enter resets; Escape exits.\n\
                Tile graphics: Arrows scroll; Q rotates; W zooms; Z flips without Q/W; X changes priority; Enter resets; Escape exits.\n\
                Affine raster: Arrows pan; Q rotates; W zooms; Z bypasses distortion; Enter resets; Escape exits.\n\
                Affine backgrounds: Arrows pan; Q rotates; W zooms; Z disables wrapping; Enter resets; Escape exits.\n\
                Bitmap pages: Arrows pan; Q rotates; W zooms; Z forces page1; Enter resets; Escape exits.\n\
                All demos use original test content. Game ROM loading is not supported."
            );
            Ok(())
        }
    }
}

fn run_cpu_demo() -> Result<(), Box<dyn Error>> {
    let mut memory = Memory::new(demo_program())?;
    let mut cpu = Cpu::new(ROM_START);

    println!("GBA core demo — not yet able to run games");
    // Fixed instruction count: the demo ends in an intentional infinite branch.
    for _ in 0..DEMO_STEPS {
        let pc = cpu.pc();
        let instruction_set = cpu.instruction_set();
        cpu.step(&mut memory)?;
        let flags = cpu.flags();
        println!(
            "{pc:#010x} {instruction_set:?}: r0={} r1={} r2={} r4={} r5={} r6={} r7={} sp={:#010x} NZCV={}{}{}{} next_pc={:#010x}",
            cpu.registers()[0],
            cpu.registers()[1],
            cpu.registers()[2],
            cpu.registers()[4],
            cpu.registers()[5],
            cpu.registers()[6],
            cpu.registers()[7],
            cpu.registers()[13],
            u8::from(flags.negative),
            u8::from(flags.zero),
            u8::from(flags.carry),
            u8::from(flags.overflow),
            cpu.pc(),
        );
    }

    println!("Exception demo — original test handler, not Nintendo BIOS services");
    let mut memory = Memory::with_bios(exception_demo_program(), demo_bios())?;
    let mut cpu = Cpu::new(ROM_START);
    for _ in 0..EXCEPTION_DEMO_STEPS {
        let pc = cpu.pc();
        let mode = cpu.mode();
        let state = cpu.instruction_set();
        cpu.step(&mut memory)?;
        println!(
            "{pc:#010x} {mode:?}/{state:?} -> {:?}/{:?} CPSR={:#010x} handler_calls={} next_pc={:#010x}",
            cpu.mode(), cpu.instruction_set(), cpu.cpsr(), cpu.registers()[10], cpu.pc(),
        );
    }
    Ok(())
}

fn run_timer_demo() -> Result<(), Box<dyn Error>> {
    let mut machine = timer_demo()?;
    println!(
        "Timer IRQ demo — nominal instruction/bus costs; no prefetch or sub-instruction timing"
    );
    for _ in 0..TIMER_DEMO_STEPS {
        let event = machine.step()?;
        println!(
            "cycle={} (+{}) {event:?} mode={:?} pc={:#010x} timer0={:#06x} IF={:#06x} handler_calls={}",
            machine.cycles(),
            machine.last_timing().total(),
            machine.cpu().mode(),
            machine.cpu().pc(),
            machine.memory().read16(TIMER_BASE)?,
            machine.memory().read16(IF)?,
            machine.cpu().registers()[10],
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_selects_window_demo_smoke_test_or_help() {
        assert_eq!(parse_args(&[]).unwrap(), RunMode::Window);
        for (argument, expected) in [
            ("--cpu-demo", RunMode::CpuDemo),
            ("--timer-demo", RunMode::TimerDemo),
            ("--bitmap4-demo", RunMode::BitmapDemo(BitmapMode::Mode4)),
            ("--bitmap5-demo", RunMode::BitmapDemo(BitmapMode::Mode5)),
            (
                "--bitmap4-smoke-test",
                RunMode::BitmapSmokeTest(BitmapMode::Mode4),
            ),
            (
                "--bitmap5-smoke-test",
                RunMode::BitmapSmokeTest(BitmapMode::Mode5),
            ),
            ("--affine-raster-demo", RunMode::AffineRasterDemo),
            ("--affine-raster-smoke-test", RunMode::AffineRasterSmokeTest),
            ("--affine-demo", RunMode::AffineDemo),
            ("--affine-smoke-test", RunMode::AffineSmokeTest),
            ("--raster-demo", RunMode::RasterDemo),
            ("--raster-smoke-test", RunMode::RasterSmokeTest),
            ("--mosaic-demo", RunMode::MosaicDemo),
            ("--mosaic-smoke-test", RunMode::MosaicSmokeTest),
            ("--effects-demo", RunMode::EffectsDemo),
            ("--effects-smoke-test", RunMode::EffectsSmokeTest),
            ("--tile-demo", RunMode::TileDemo),
            ("--tile-smoke-test", RunMode::TileSmokeTest),
            ("--graphics-demo", RunMode::GraphicsDemo),
            ("--graphics-smoke-test", RunMode::GraphicsSmokeTest),
            ("--smoke-test", RunMode::SmokeTest),
            ("--help", RunMode::Help),
            ("-h", RunMode::Help),
        ] {
            assert_eq!(parse_args(&[argument.into()]).unwrap(), expected);
        }
    }

    #[test]
    fn command_line_rejects_rom_paths_and_conflicting_options() {
        assert!(parse_args(&["emerald.gba".into()]).is_err());
        assert!(parse_args(&["--unknown".into()]).is_err());
        assert!(parse_args(&["--cpu-demo".into(), "--smoke-test".into()]).is_err());
    }
}
