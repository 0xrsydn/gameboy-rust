//! Differential runner: execute a ROM in this emulator and in mGBA (through the
//! `mgba-ref` shim) with the same inputs, and report where their state diverges.
//! See docs/mgba-reference.md.

mod reference;
mod trace;

use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use gba_core::{
    bios,
    cartridge::SaveDevice,
    input::{Button, Buttons},
    machine::Machine,
    video::{Framebuffer, HEIGHT, WIDTH},
};

use reference::Reference;

const ROM_ENTRY: u32 = 0x0800_0000;
const DISPCNT: u32 = 0x0400_0000;
/// Machine steps allowed between VBlanks before the run is reported as stuck.
const FRAME_STEP_LIMIT: usize = 2_000_000;

const USAGE: &str = "usage: gba-diff --reference MGBA_REF --rom ROM [--frames N] [--save-type sram|flash64|flash128|eeprom512|eeprom8k] [--input FRAME:BUTTON:HOLD[,...]] [--ignore START-END[,...]] [--max-report N] [--out DIR] [--images FRAME[,...]] [--no-align] [--trace INSTRUCTIONS]";

/// Memory compared every frame. Video memory is compared after both frames render.
const REGIONS: [(&str, u32, usize); 5] = [
    ("EWRAM", 0x0200_0000, 0x4_0000),
    ("IWRAM", 0x0300_0000, 0x8000),
    ("palette", 0x0500_0000, 0x400),
    ("VRAM", 0x0600_0000, 0x1_8000),
    ("OAM", 0x0700_0000, 0x400),
];

/// Readable I/O halfwords compared every frame.
const IO: [(&str, u32); 30] = [
    ("DISPCNT", 0x0400_0000),
    ("DISPSTAT", 0x0400_0004),
    ("VCOUNT", 0x0400_0006),
    ("BG0CNT", 0x0400_0008),
    ("BG1CNT", 0x0400_000a),
    ("BG2CNT", 0x0400_000c),
    ("BG3CNT", 0x0400_000e),
    ("WININ", 0x0400_0048),
    ("WINOUT", 0x0400_004a),
    ("BLDCNT", 0x0400_0050),
    ("BLDALPHA", 0x0400_0052),
    ("SOUNDCNT_L", 0x0400_0080),
    ("SOUNDCNT_H", 0x0400_0082),
    ("SOUNDCNT_X", 0x0400_0084),
    ("DMA0CNT_H", 0x0400_00ba),
    ("DMA1CNT_H", 0x0400_00c6),
    ("DMA2CNT_H", 0x0400_00d2),
    ("DMA3CNT_H", 0x0400_00de),
    ("TM0CNT_H", 0x0400_0102),
    ("TM1CNT_H", 0x0400_0106),
    ("TM2CNT_H", 0x0400_010a),
    ("TM3CNT_H", 0x0400_010e),
    ("KEYINPUT", 0x0400_0130),
    ("KEYCNT", 0x0400_0132),
    ("IE", 0x0400_0200),
    ("IF", 0x0400_0202),
    ("WAITCNT", 0x0400_0204),
    ("IME", 0x0400_0208),
    ("POSTFLG", 0x0400_0300),
    ("SOUNDBIAS", 0x0400_0088),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registers {
    pub gprs: [u32; 16],
    pub cpsr: u32,
    pub thumb: bool,
}

struct Options {
    reference: PathBuf,
    rom: PathBuf,
    frames: u64,
    save: SaveDevice,
    inputs: Vec<(u64, Button, u64)>,
    ignore: Vec<(u32, u32)>,
    max_report: usize,
    out: Option<PathBuf>,
    images: Vec<u64>,
    align: bool,
    trace: Option<u64>,
}

fn number(text: &str) -> Result<u64, String> {
    let parsed = match text.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16),
        None => text.parse(),
    };
    parsed.map_err(|_| format!("invalid number: {text}"))
}

fn button(name: &str) -> Result<Button, String> {
    Ok(match name {
        "a" => Button::A,
        "b" => Button::B,
        "select" => Button::Select,
        "start" => Button::Start,
        "right" => Button::Right,
        "left" => Button::Left,
        "up" => Button::Up,
        "down" => Button::Down,
        "r" => Button::R,
        "l" => Button::L,
        _ => return Err(format!("unknown button: {name}")),
    })
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        reference: PathBuf::new(),
        rom: PathBuf::new(),
        frames: 600,
        save: SaveDevice::None,
        inputs: Vec::new(),
        ignore: Vec::new(),
        max_report: 8,
        out: None,
        images: Vec::new(),
        align: true,
        trace: None,
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .cloned()
                .ok_or_else(|| format!("{arg} needs a value"))
        };
        match arg.as_str() {
            "--reference" => options.reference = value()?.into(),
            "--rom" => options.rom = value()?.into(),
            "--frames" => options.frames = number(&value()?)?,
            "--max-report" => options.max_report = number(&value()?)? as usize,
            "--out" => options.out = Some(value()?.into()),
            "--save-type" => {
                options.save = match value()?.as_str() {
                    "sram" => SaveDevice::Sram,
                    "flash64" => SaveDevice::Flash64,
                    "flash128" => SaveDevice::Flash128,
                    "eeprom512" => SaveDevice::Eeprom512,
                    "eeprom8k" => SaveDevice::Eeprom8k,
                    other => return Err(format!("unknown save type: {other}")),
                }
            }
            "--input" => {
                for entry in value()?.split(',').filter(|entry| !entry.is_empty()) {
                    let parts: Vec<&str> = entry.split(':').collect();
                    if parts.len() != 3 {
                        return Err(format!("input must be FRAME:BUTTON:HOLD, got {entry}"));
                    }
                    options
                        .inputs
                        .push((number(parts[0])?, button(parts[1])?, number(parts[2])?));
                }
            }
            "--ignore" => {
                for range in value()?.split(',').filter(|range| !range.is_empty()) {
                    let (start, end) = range
                        .split_once('-')
                        .ok_or_else(|| format!("range must be START-END, got {range}"))?;
                    options
                        .ignore
                        .push((number(start)? as u32, number(end)? as u32));
                }
            }
            "--images" => {
                for frame in value()?.split(',').filter(|frame| !frame.is_empty()) {
                    options.images.push(number(frame)?);
                }
            }
            "--no-align" => options.align = false,
            "--trace" => options.trace = Some(number(&value()?)?),
            "--help" | "-h" => return Err(USAGE.to_owned()),
            other => return Err(format!("unknown option: {other}\n{USAGE}")),
        }
    }
    if options.reference.as_os_str().is_empty() || options.rom.as_os_str().is_empty() {
        return Err(USAGE.to_owned());
    }
    Ok(options)
}

/// This emulator, started at the cartridge entry like mGBA's skip-BIOS path.
struct Local {
    machine: Machine,
    frame: Framebuffer,
    vblank: u64,
}

impl Local {
    fn boot(rom: Vec<u8>, save: SaveDevice) -> Result<Self, String> {
        let mut machine = bios::boot(rom).map_err(|error| format!("{error:?}"))?;
        machine.memory_mut().set_save_device(save);
        machine.memory_mut().set_scanline_rendering(true);
        for _ in 0..FRAME_STEP_LIMIT {
            if machine.cpu().pc() == ROM_ENTRY {
                let vblank = machine.memory().display_position().vblanks;
                return Ok(Self {
                    machine,
                    frame: Framebuffer::default(),
                    vblank,
                });
            }
            machine.step().map_err(|error| format!("boot: {error:?}"))?;
        }
        Err("boot never reached the cartridge entry".to_owned())
    }

    fn run_frame(&mut self, buttons: Buttons) -> Result<(), String> {
        self.machine.memory_mut().set_buttons(buttons);
        for _ in 0..FRAME_STEP_LIMIT {
            self.machine.step().map_err(|error| {
                let cpu = self.machine.cpu();
                format!(
                    "{error:?} at PC={:#010x} ({:?})",
                    cpu.pc(),
                    cpu.instruction_set()
                )
            })?;
            let vblank = self.machine.memory().display_position().vblanks;
            if vblank != self.vblank {
                self.vblank = vblank;
                return match self.machine.memory().present_frame(&mut self.frame) {
                    Ok(true) => Ok(()),
                    Ok(false) => Err("VBlank without a complete captured frame".to_owned()),
                    Err(error) => Err(format!("frame capture: {error:?}")),
                };
            }
        }
        Err(format!("no VBlank within {FRAME_STEP_LIMIT} steps"))
    }

    fn registers(&self) -> Registers {
        let cpu = self.machine.cpu();
        Registers {
            gprs: *cpu.registers(),
            cpsr: cpu.cpsr(),
            thumb: cpu.cpsr() & 0x20 != 0,
        }
    }

    /// Host inspection reads; unreadable bytes are reported as `None`.
    fn read(&self, address: u32, length: usize) -> Vec<Option<u8>> {
        let memory = self.machine.memory();
        (0..length as u32)
            .map(|offset| memory.read8(address + offset).ok())
            .collect()
    }

    fn rgb555(&self) -> Vec<u16> {
        self.frame
            .pixels()
            .iter()
            .map(|&pixel| {
                let red = (pixel >> 19) & 31;
                let green = (pixel >> 11) & 31;
                let blue = (pixel >> 3) & 31;
                (red | green << 5 | blue << 10) as u16
            })
            .collect()
    }
}

fn buttons_at(inputs: &[(u64, Button, u64)], frame: u64) -> Buttons {
    inputs
        .iter()
        .filter(|(start, _, hold)| frame >= *start && frame < start + hold)
        .fold(Buttons::default(), |held, (_, button, _)| {
            held.with(*button, true)
        })
}

/// One region's comparison: count of differing bytes and the first few of them.
struct RegionDiff {
    name: &'static str,
    differing: usize,
    unreadable: usize,
    samples: Vec<(u32, Option<u8>, u8)>,
}

fn compare_region(
    name: &'static str,
    base: u32,
    ours: &[Option<u8>],
    theirs: &[u8],
    ignore: &[(u32, u32)],
) -> Option<RegionDiff> {
    let mut diff = RegionDiff {
        name,
        differing: 0,
        unreadable: 0,
        samples: Vec::new(),
    };
    for (offset, (ours, theirs)) in ours.iter().zip(theirs).enumerate() {
        let address = base + offset as u32;
        if ignore
            .iter()
            .any(|&(start, end)| (start..=end).contains(&address))
        {
            continue;
        }
        if *ours == Some(*theirs) {
            continue;
        }
        if ours.is_none() {
            diff.unreadable += 1;
        } else {
            diff.differing += 1;
        }
        if diff.samples.len() < 6 {
            diff.samples.push((address, *ours, *theirs));
        }
    }
    (diff.differing + diff.unreadable > 0).then_some(diff)
}

struct FrameReport {
    regions: Vec<RegionDiff>,
    io: Vec<(&'static str, Option<u16>, u16)>,
    pixels: usize,
}

impl FrameReport {
    fn clean(&self) -> bool {
        self.regions.is_empty() && self.io.is_empty() && self.pixels == 0
    }

    fn summary(&self) -> String {
        let mut parts: Vec<String> = self
            .regions
            .iter()
            .map(|region| format!("{} {}B", region.name, region.differing + region.unreadable))
            .collect();
        if !self.io.is_empty() {
            parts.push(format!("I/O {}", self.io.len()));
        }
        if self.pixels > 0 {
            parts.push(format!("pixels {}", self.pixels));
        }
        parts.join(", ")
    }

    fn details(&self, out: &mut impl Write) -> io::Result<()> {
        for region in &self.regions {
            write!(
                out,
                "  {}: {} differing bytes",
                region.name, region.differing
            )?;
            if region.unreadable > 0 {
                write!(out, ", {} unreadable here", region.unreadable)?;
            }
            writeln!(out)?;
            for (address, ours, theirs) in &region.samples {
                let ours = ours.map_or("--".to_owned(), |value| format!("{value:02x}"));
                writeln!(out, "    {address:#010x}: ours {ours} mGBA {theirs:02x}")?;
            }
        }
        for (name, ours, theirs) in &self.io {
            let ours = ours.map_or("unreadable".to_owned(), |value| format!("{value:#06x}"));
            writeln!(out, "  {name}: ours {ours} mGBA {theirs:#06x}")?;
        }
        if self.pixels > 0 {
            writeln!(
                out,
                "  frame: {} of {} pixels differ",
                self.pixels,
                WIDTH * HEIGHT
            )?;
        }
        Ok(())
    }
}

fn compare_frame(
    local: &Local,
    reference: &mut Reference,
    ignore: &[(u32, u32)],
) -> io::Result<(FrameReport, Vec<u16>, Vec<u16>)> {
    let mut regions = Vec::new();
    for (name, base, length) in REGIONS {
        let theirs = reference.read(base, length)?;
        let ours = local.read(base, length);
        regions.extend(compare_region(name, base, &ours, &theirs, ignore));
    }
    let mut io = Vec::new();
    for (name, address) in IO {
        if ignore
            .iter()
            .any(|&(start, end)| (start..=end).contains(&address))
        {
            continue;
        }
        let theirs = reference.read(address, 2)?;
        let theirs = u16::from_le_bytes([theirs[0], theirs[1]]);
        let ours = local.machine.memory().read16(address).ok();
        if ours != Some(theirs) {
            io.push((name, ours, theirs));
        }
    }
    let ours_frame = local.rgb555();
    let theirs_frame = reference.frame()?;
    let pixels = ours_frame
        .iter()
        .zip(&theirs_frame)
        .filter(|(ours, theirs)| ours != theirs)
        .count();
    Ok((
        FrameReport {
            regions,
            io,
            pixels,
        },
        ours_frame,
        theirs_frame,
    ))
}

fn write_ppm(path: &Path, pixels: &[u16]) -> io::Result<()> {
    let mut bytes = format!("P6\n{WIDTH} {HEIGHT}\n255\n").into_bytes();
    for &pixel in pixels {
        for shift in [0, 5, 10] {
            let channel = ((pixel >> shift) & 31) as u8;
            bytes.push(channel << 3 | channel >> 2);
        }
    }
    fs::write(path, bytes)
}

fn write_images(dir: &Path, frame: u64, ours: &[u16], theirs: &[u16]) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    write_ppm(&dir.join(format!("frame{frame:05}-ours.ppm")), ours)?;
    write_ppm(&dir.join(format!("frame{frame:05}-mgba.ppm")), theirs)?;
    // Matching pixels are dimmed; differing pixels are shown in red.
    let mask: Vec<u16> = ours
        .iter()
        .zip(theirs)
        .map(|(&ours, &theirs)| {
            if ours == theirs {
                let gray = ((ours & 31) + ((ours >> 5) & 31) + ((ours >> 10) & 31)) / 9;
                gray | gray << 5 | gray << 10
            } else {
                31
            }
        })
        .collect();
    write_ppm(&dir.join(format!("frame{frame:05}-diff.ppm")), &mask)
}

fn registers_line(label: &str, registers: &Registers) -> String {
    let gprs: Vec<String> = registers
        .gprs
        .iter()
        .map(|value| format!("{value:08x}"))
        .collect();
    format!(
        "{label:5} cpsr={:08x} {} r0-r15: {}",
        registers.cpsr,
        if registers.thumb { "thumb" } else { "arm  " },
        gprs.join(" ")
    )
}

/// Instructions searched for the game's first RegisterRamReset.
const RESET_SEARCH: u64 = 1_000_000;

/// Whether the instruction at `address` is `SWI 0x01` (RegisterRamReset).
fn is_ram_reset(local: &Local, address: u32, thumb: bool) -> bool {
    let memory = local.machine.memory();
    if thumb {
        memory.read16(address) == Ok(0xdf01)
    } else {
        memory
            .read32(address)
            .is_ok_and(|op| op & 0x0f00_0000 == 0x0f00_0000 && (op >> 16) & 0xff == 1)
    }
}

/// mGBA runs RegisterRamReset as a high-level call of a few dozen cycles, while this
/// emulator's firmware clears RAM with real stores. Neither cost is hardware-verified, and the
/// gap shifts every later frame. Run both sides in lockstep to the first such call, then move
/// mGBA's display position to ours, as at cartridge entry.
fn align_after_ram_reset(
    local: &mut Local,
    reference: &mut Reference,
    out: &mut impl Write,
) -> Result<(), String> {
    let report = |error: io::Error| error.to_string();
    for count in 1..=RESET_SEARCH {
        let record = trace::next_local(&mut local.machine)?;
        if !is_ram_reset(local, record.address, record.thumb) {
            continue;
        }
        let theirs = reference.sync(count as usize).map_err(report)?;
        let last = theirs.last().map(|record| record.address);
        if last != Some(record.address) {
            return Err(format!(
                "cannot align after RegisterRamReset: ours at {:#010x}, mGBA at {last:x?}",
                record.address
            ));
        }
        let position = local.machine.memory().display_position();
        local.vblank = position.vblanks; // Frames count from here on both sides.
        reference
            .align_video(position.scanline, position.line_cycle)
            .map_err(report)?;
        writeln!(
            out,
            "aligned mGBA after RegisterRamReset at {:#010x} (instruction {count}): VCOUNT {} cycle {}",
            record.address, position.scanline, position.line_cycle
        )
        .map_err(report)?;
        return Ok(());
    }
    writeln!(
        out,
        "no RegisterRamReset within {RESET_SEARCH} instructions; not aligned"
    )
    .map_err(report)
}

fn run(options: Options) -> Result<bool, String> {
    let rom =
        fs::read(&options.rom).map_err(|error| format!("{}: {error}", options.rom.display()))?;
    let mut local = Local::boot(rom, options.save)?;
    let mut reference = Reference::spawn(&options.reference).map_err(|error| error.to_string())?;
    reference
        .load(&options.rom, None)
        .map_err(|error| error.to_string())?;

    let mut out = io::stdout().lock();
    let position = local.machine.memory().display_position();
    let theirs_vcount = reference
        .read(0x0400_0006, 2)
        .map_err(|error| error.to_string())?;
    let report = |error: io::Error| error.to_string();
    writeln!(
        out,
        "cartridge entry: ours VCOUNT={} cycle={}, mGBA VCOUNT={}",
        position.scanline,
        position.line_cycle,
        u16::from_le_bytes([theirs_vcount[0], theirs_vcount[1]])
    )
    .map_err(report)?;
    if options.align {
        // Both start states are approximations of the real BIOS exit; neither is
        // hardware-verified. Start mGBA from ours so later differences are behavior.
        reference
            .align_video(position.scanline, position.line_cycle)
            .map_err(report)?;
        let ours = local
            .machine
            .memory()
            .read16(DISPCNT)
            .map_err(|error| format!("{error:?}"))?;
        let theirs = reference.read(DISPCNT, 2).map_err(report)?;
        let theirs = u16::from_le_bytes([theirs[0], theirs[1]]);
        if ours != theirs {
            reference.write16(DISPCNT, ours).map_err(report)?;
        }
        writeln!(
            out,
            "aligned mGBA to this emulator's start: VCOUNT {} cycle {}, DISPCNT {theirs:#06x} -> {ours:#06x}",
            position.scanline, position.line_cycle
        )
        .map_err(report)?;
    }
    writeln!(out, "{}", registers_line("ours", &local.registers())).map_err(report)?;
    writeln!(
        out,
        "{}",
        registers_line("mGBA", &reference.registers().map_err(report)?)
    )
    .map_err(report)?;

    if options.align && options.trace.is_none() {
        align_after_ram_reset(&mut local, &mut reference, &mut out)?;
    }

    if let Some(count) = options.trace {
        return trace::run(
            &mut local.machine,
            &mut reference,
            count,
            options.max_report,
            &mut out,
        );
    }

    let mut first: Option<u64> = None;
    let mut reported = 0;
    let mut divergent = 0;
    for frame in 1..=options.frames {
        let buttons = buttons_at(&options.inputs, frame - 1);
        reference.set_keys(buttons.bits()).map_err(report)?;
        reference.run_frame().map_err(report)?;
        if let Err(error) = local.run_frame(buttons) {
            writeln!(out, "frame {frame}: this emulator stopped: {error}").map_err(report)?;
            writeln!(out, "{}", registers_line("ours", &local.registers())).map_err(report)?;
            writeln!(
                out,
                "{}",
                registers_line("mGBA", &reference.registers().map_err(report)?)
            )
            .map_err(report)?;
            return Ok(false);
        }
        let (report_frame, ours, theirs) =
            compare_frame(&local, &mut reference, &options.ignore).map_err(report)?;
        if let (Some(dir), true) = (&options.out, options.images.contains(&frame)) {
            write_images(dir, frame, &ours, &theirs).map_err(report)?;
        }
        if report_frame.clean() {
            continue;
        }
        divergent += 1;
        if first.is_none() {
            first = Some(frame);
            writeln!(out, "first divergence at frame {frame}:").map_err(report)?;
            report_frame.details(&mut out).map_err(report)?;
            writeln!(out, "{}", registers_line("ours", &local.registers())).map_err(report)?;
            writeln!(
                out,
                "{}",
                registers_line("mGBA", &reference.registers().map_err(report)?)
            )
            .map_err(report)?;
            if let Some(dir) = &options.out {
                write_images(dir, frame, &ours, &theirs).map_err(report)?;
                writeln!(out, "  images: {}", dir.display()).map_err(report)?;
            }
        } else if reported < options.max_report {
            reported += 1;
            writeln!(out, "frame {frame}: {}", report_frame.summary()).map_err(report)?;
        }
    }
    match first {
        None => writeln!(out, "{} frames: no divergence", options.frames),
        Some(frame) => writeln!(
            out,
            "{} frames: {divergent} divergent, first at frame {frame}",
            options.frames
        ),
    }
    .map_err(report)?;
    Ok(first.is_none())
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let options = match parse(&args) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    match run(options) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(message) => {
            eprintln!("gba-diff: {message}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn options_parse_inputs_ranges_and_numbers() {
        let options = parse(&args(
            "--reference ref --rom game.gba --frames 0x10 --save-type sram \
             --input 5:a:2,9:start:1 --ignore 0x03007f00-0x03007fff --no-align --trace 100",
        ))
        .unwrap();
        assert_eq!(options.frames, 16);
        assert_eq!(options.save, SaveDevice::Sram);
        assert_eq!(
            options.inputs,
            vec![(5, Button::A, 2), (9, Button::Start, 1)]
        );
        assert_eq!(options.ignore, vec![(0x0300_7f00, 0x0300_7fff)]);
        assert!(!options.align);
        assert_eq!(options.trace, Some(100));
        for bad in [
            "--rom game.gba",
            "--reference ref --rom game.gba --input 5:x:1",
            "--reference ref --rom game.gba --ignore 5",
            "--reference ref --rom game.gba --frames ten",
            "--reference ref --rom game.gba --save-type tape",
        ] {
            assert!(parse(&args(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn held_buttons_cover_their_frames_only() {
        let inputs = [(5, Button::A, 2), (6, Button::Start, 1)];
        assert_eq!(buttons_at(&inputs, 4), Buttons::default());
        assert_eq!(buttons_at(&inputs, 5).bits(), Button::A as u16);
        assert_eq!(
            buttons_at(&inputs, 6).bits(),
            Button::A as u16 | Button::Start as u16
        );
        assert_eq!(buttons_at(&inputs, 7), Buttons::default());
    }

    #[test]
    fn regions_count_differences_unreadable_bytes_and_skip_ignored_ranges() {
        let ours = [Some(1), Some(2), None, Some(4)];
        let theirs = [1, 9, 3, 8];
        let diff = compare_region("test", 0x100, &ours, &theirs, &[]).unwrap();
        assert_eq!((diff.differing, diff.unreadable), (2, 1));
        assert_eq!(diff.samples[0], (0x101, Some(2), 9));
        let diff = compare_region("test", 0x100, &ours, &theirs, &[(0x101, 0x102)]).unwrap();
        assert_eq!((diff.differing, diff.unreadable), (1, 0));
        assert!(compare_region("test", 0x100, &ours, &theirs, &[(0x100, 0x103)]).is_none());
    }
}
