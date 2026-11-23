//! Differential runner: execute a ROM in this emulator and in a reference emulator, mGBA
//! (through the `mgba-ref` shim) or Mesen 2 (through `mesen-ref`), with the same inputs, and
//! report where their state diverges. See docs/mgba-reference.md and docs/mesen-reference.md.

mod eval;
mod reference;
mod trace;

use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{self, ExitCode},
};

use gba_core::{
    bios,
    cartridge::{CartridgeHardware, RtcDateTime, SaveDevice},
    cpu::Cpu,
    input::{Button, Buttons},
    machine::Machine,
    memory::{Memory, BIOS_SIZE},
    video::{Framebuffer, HEIGHT, WIDTH},
};

use reference::{Kind, Reference};

const ROM_ENTRY: u32 = 0x0800_0000;
const DISPCNT: u32 = 0x0400_0000;
/// Machine steps allowed between VBlanks before the run is reported as stuck.
const FRAME_STEP_LIMIT: usize = 2_000_000;

const USAGE: &str = "usage: gba-diff --reference MGBA_REF|MESEN_REF --rom ROM [--frames N] [--save-type sram|flash64|flash128|eeprom|eeprom512|eeprom8k|none] [--rtc UNIX_SECONDS] [--input FRAME:BUTTON:HOLD[,...]] [--ignore START-END[,...]] [--max-report N] [--out DIR] [--images FRAME[,...]] [--detail-frame FRAME[,...]] [--no-align] [--trace INSTRUCTIONS] [--bios IMAGE] [--eval [--replay FILE]]";

/// Memory compared every frame. Video memory is compared after both frames render.
const REGIONS: [(&str, u32, usize); 5] = [
    ("EWRAM", 0x0200_0000, 0x4_0000),
    ("IWRAM", 0x0300_0000, 0x8000),
    ("palette", 0x0500_0000, 0x400),
    ("VRAM", 0x0600_0000, 0x1_8000),
    ("OAM", 0x0700_0000, 0x400),
];

/// Seconds from the Unix epoch to 2000-01-01, where the RTC calendar starts.
const UNIX_2000: u64 = 946_684_800;
/// Cycles per frame and per second, as mGBA's fake-epoch RTC clock counts them.
const FRAME_CYCLES: u64 = 280_896;
const CPU_HZ: u64 = 16_777_216;

/// Differing addresses listed per region for an ordinary report.
const SAMPLES: usize = 6;
/// Differing addresses listed per region for a `--detail-frame` frame.
const DETAIL_SAMPLES: usize = 256;

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
    /// The save device; detected from the ROM's save-library ID when not given.
    save: Option<SaveDevice>,
    rtc: Option<u64>,
    inputs: Vec<(u64, Button, u64)>,
    ignore: Vec<(u32, u32)>,
    max_report: usize,
    out: Option<PathBuf>,
    images: Vec<u64>,
    detail: Vec<u64>,
    align: bool,
    trace: Option<u64>,
    /// A 16 KiB firmware image run on both sides instead of this emulator's (Mesen only).
    bios: Option<PathBuf>,
    /// Score as GBA Eval lines frames up (mesen-ref and --bios only); see eval.rs.
    eval: bool,
    /// A GBA Eval replay (`<frame> <keys_hex>` lines) for --eval.
    replay: Option<PathBuf>,
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
        save: None,
        rtc: None,
        inputs: Vec::new(),
        ignore: Vec::new(),
        max_report: 8,
        out: None,
        images: Vec::new(),
        detail: Vec::new(),
        align: true,
        trace: None,
        bios: None,
        eval: false,
        replay: None,
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
            "--bios" => options.bios = Some(value()?.into()),
            "--eval" => options.eval = true,
            "--replay" => options.replay = Some(value()?.into()),
            "--frames" => options.frames = number(&value()?)?,
            "--max-report" => options.max_report = number(&value()?)? as usize,
            "--out" => options.out = Some(value()?.into()),
            "--save-type" => {
                options.save = Some(match value()?.as_str() {
                    "sram" => SaveDevice::Sram,
                    "flash64" => SaveDevice::Flash64,
                    "flash128" => SaveDevice::Flash128,
                    "eeprom" => SaveDevice::Eeprom,
                    "eeprom512" => SaveDevice::Eeprom512,
                    "eeprom8k" => SaveDevice::Eeprom8k,
                    "none" => SaveDevice::None,
                    other => return Err(format!("unknown save type: {other}")),
                })
            }
            "--rtc" => {
                let unix = number(&value()?)?;
                if unix < UNIX_2000 {
                    return Err(format!("--rtc must be at or after 2000-01-01: {unix}"));
                }
                options.rtc = Some(unix);
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
            "--detail-frame" => {
                for frame in value()?.split(',').filter(|frame| !frame.is_empty()) {
                    options.detail.push(number(frame)?);
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

/// The cartridge RTC on mGBA's fake-epoch clock (mgba-ref's `rtc` command): whole seconds of
/// `start + frames * FRAME_CYCLES / CPU_HZ`, counting frames from the cartridge entry.
struct RtcClock {
    start_ms: u64,
    entry_vblank: u64,
    seconds: u64,
}

impl RtcClock {
    fn seconds_after(&self, frames: u64) -> u64 {
        // Same integer order as mGBA: frames * (cycles * 1000) / frequency, in milliseconds.
        (self.start_ms + frames * (FRAME_CYCLES * 1000) / CPU_HZ) / 1000
    }
}

/// This emulator, run through its firmware's boot to the cartridge entry. mGBA starts there
/// itself (skip-BIOS); Mesen boots the same firmware, and both reach the same first VBlank.
struct Local {
    machine: Machine,
    frame: Framebuffer,
    vblank: u64,
    rtc: Option<RtcClock>,
}

impl Local {
    fn boot(
        rom: Vec<u8>,
        firmware: Option<Vec<u8>>,
        save: SaveDevice,
        rtc: Option<u64>,
    ) -> Result<Self, String> {
        let mut machine = match firmware {
            Some(image) => {
                Memory::with_bios(rom, image).map(|memory| Machine::new(Cpu::at_reset(), memory))
            }
            None => bios::boot(rom),
        }
        .map_err(|error| format!("{error:?}"))?;
        machine.memory_mut().set_save_device(save);
        machine.memory_mut().set_scanline_rendering(true);
        if let Some(unix) = rtc {
            let memory = machine.memory_mut();
            memory.set_cartridge_hardware(CartridgeHardware::Rtc);
            let date = RtcDateTime::from_seconds_since_2000(unix - UNIX_2000)
                .map_err(|error| format!("--rtc {unix}: {error:?}"))?;
            memory
                .set_rtc_datetime(date)
                .map_err(|error| format!("--rtc {unix}: {error:?}"))?;
        }
        for _ in 0..FRAME_STEP_LIMIT {
            if machine.cpu().pc() == ROM_ENTRY {
                let vblank = machine.memory().display_position().vblanks;
                return Ok(Self {
                    machine,
                    frame: Framebuffer::default(),
                    vblank,
                    rtc: rtc.map(|unix| RtcClock {
                        start_ms: unix * 1000,
                        entry_vblank: vblank,
                        seconds: unix,
                    }),
                });
            }
            machine.step().map_err(|error| format!("boot: {error:?}"))?;
        }
        Err("boot never reached the cartridge entry".to_owned())
    }

    /// Bring the RTC up to mGBA's clock for the frames run since the cartridge entry.
    fn sync_rtc(&mut self) -> Result<(), String> {
        let Some(rtc) = self.rtc.as_mut() else {
            return Ok(());
        };
        let frames = self.machine.memory().display_position().vblanks - rtc.entry_vblank;
        let seconds = rtc.seconds_after(frames);
        if seconds > rtc.seconds {
            self.machine
                .memory_mut()
                .advance_rtc_seconds(seconds - rtc.seconds)
                .map_err(|error| format!("RTC: {error:?}"))?;
            rtc.seconds = seconds;
        }
        Ok(())
    }

    fn run_frame(&mut self, buttons: Buttons) -> Result<(), String> {
        self.sync_rtc()?;
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
    samples: usize,
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
        if diff.samples.len() < samples {
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

    fn details(&self, out: &mut impl Write, label: &str) -> io::Result<()> {
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
                writeln!(out, "    {address:#010x}: ours {ours} {label} {theirs:02x}")?;
            }
            let listed = region.samples.len();
            if listed < region.differing + region.unreadable {
                writeln!(out, "    (first {listed} listed)")?;
            }
        }
        for (name, ours, theirs) in &self.io {
            let ours = ours.map_or("unreadable".to_owned(), |value| format!("{value:#06x}"));
            writeln!(out, "  {name}: ours {ours} {label} {theirs:#06x}")?;
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
    samples: usize,
) -> io::Result<(FrameReport, Vec<u16>, Vec<u16>)> {
    let mut regions = Vec::new();
    for (name, base, length) in REGIONS {
        let theirs = reference.read(base, length)?;
        let ours = local.read(base, length);
        regions.extend(compare_region(name, base, &ours, &theirs, ignore, samples));
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

fn write_images(
    dir: &Path,
    frame: u64,
    ours: &[u16],
    theirs: &[u16],
    label: &str,
) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    write_ppm(&dir.join(format!("frame{frame:05}-ours.ppm")), ours)?;
    let name = label.to_lowercase();
    write_ppm(&dir.join(format!("frame{frame:05}-{name}.ppm")), theirs)?;
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

/// Step this emulator up to `limit` instructions to the game's first RegisterRamReset and
/// return its instruction count and address. Without one, restore the state before the search,
/// so that both sides still start frame comparison from the same point.
fn find_ram_reset(local: &mut Local, limit: u64) -> Result<Option<(u64, u32)>, String> {
    let start = local.machine.save_state().map_err(|error| {
        format!("cannot save state before the RegisterRamReset search: {error:?}")
    })?;
    for count in 1..=limit {
        let record = trace::next_local(&mut local.machine)?;
        if is_ram_reset(local, record.address, record.thumb) {
            return Ok(Some((count, record.address)));
        }
    }
    local.machine.load_state(&start).map_err(|error| {
        format!("cannot restore state after the RegisterRamReset search: {error:?}")
    })?;
    Ok(None)
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
    let Some((count, address)) = find_ram_reset(local, RESET_SEARCH)? else {
        return writeln!(
            out,
            "no RegisterRamReset within {RESET_SEARCH} instructions; not aligned"
        )
        .map_err(report);
    };
    let theirs = reference.sync(count as usize).map_err(report)?;
    let last = theirs.last().map(|record| record.address);
    if last != Some(address) {
        return Err(format!(
            "cannot align after RegisterRamReset: ours at {address:#010x}, mGBA at {last:x?}"
        ));
    }
    let position = local.machine.memory().display_position();
    local.vblank = position.vblanks; // Frames count from here on both sides.
    reference
        .align_video(position.scanline, position.line_cycle)
        .map_err(report)?;
    writeln!(
        out,
        "aligned mGBA after RegisterRamReset at {address:#010x} (instruction {count}): VCOUNT {} cycle {}",
        position.scanline, position.line_cycle
    )
    .map_err(report)
}

/// The firmware both sides run, as a temporary file for a reference that runs it (Mesen).
struct BiosFile(PathBuf);

impl BiosFile {
    fn write(image: &[u8]) -> Result<Self, String> {
        let path = env::temp_dir().join(format!("gba-diff-bios-{}.bin", process::id()));
        fs::write(&path, image).map_err(|error| format!("{}: {error}", path.display()))?;
        Ok(Self(path))
    }
}

impl Drop for BiosFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Start mGBA from this emulator's cartridge-entry state. Both start states are
/// approximations of the real BIOS exit; neither is hardware-verified, so mGBA is moved to
/// ours and later differences are behavior.
fn align_mgba_start(
    local: &Local,
    reference: &mut Reference,
    align: bool,
    out: &mut impl Write,
) -> Result<(), String> {
    let report = |error: io::Error| error.to_string();
    let position = local.machine.memory().display_position();
    let theirs_vcount = reference.read(0x0400_0006, 2).map_err(report)?;
    writeln!(
        out,
        "cartridge entry: ours VCOUNT={} cycle={}, mGBA VCOUNT={}",
        position.scanline,
        position.line_cycle,
        u16::from_le_bytes([theirs_vcount[0], theirs_vcount[1]])
    )
    .map_err(report)?;
    if align {
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
    .map_err(report)
}

/// `--eval`: frames scored as GBA Eval lines them up. Passes when every frame matches.
fn run_eval(options: &Options) -> Result<bool, String> {
    let mut reference = Reference::spawn(&options.reference).map_err(|error| error.to_string())?;
    if reference.kind() != Kind::Mesen {
        return Err("--eval needs mesen-ref: the benchmark's reference is Mesen 2".to_owned());
    }
    let Some(bios) = &options.bios else {
        return Err("--eval needs --bios: the benchmark's BIOS stub".to_owned());
    };
    let replay = match &options.replay {
        Some(path) => eval::parse_replay(
            &fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?,
        )?,
        None => Vec::new(),
    };
    let mut out = io::stdout().lock();
    let images = eval::Images {
        frames: &options.images,
        dir: options.out.as_deref(),
    };
    let summary = eval::run(
        &mut reference,
        &options.rom,
        bios,
        options.frames,
        &replay,
        images,
        &mut out,
    )?;
    Ok(summary.identical == summary.frames && summary.error.is_none())
}

fn run(options: Options) -> Result<bool, String> {
    if options.eval {
        return run_eval(&options);
    }
    if options.replay.is_some() {
        return Err("--replay needs --eval".to_owned());
    }
    let rom =
        fs::read(&options.rom).map_err(|error| format!("{}: {error}", options.rom.display()))?;
    let firmware = match &options.bios {
        Some(path) => {
            let image = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
            if image.len() != BIOS_SIZE {
                return Err(format!(
                    "{}: a BIOS image is {} bytes, not {}",
                    path.display(),
                    BIOS_SIZE,
                    image.len()
                ));
            }
            Some(image)
        }
        None => None,
    };
    // As the desktop app does, the ROM's save-library ID picks the device unless one is given.
    let save = options.save.unwrap_or_else(|| SaveDevice::detect(&rom));
    let mut local = Local::boot(rom, firmware.clone(), save, options.rtc)?;
    let mut reference = Reference::spawn(&options.reference).map_err(|error| error.to_string())?;
    let report = |error: io::Error| error.to_string();
    let label = reference.label();
    let mut out = io::stdout().lock();
    match reference.kind() {
        Kind::Mgba => {
            if options.bios.is_some() {
                return Err(
                    "--bios needs mesen-ref: mGBA starts at its own approximation of the BIOS exit"
                        .to_owned(),
                );
            }
            reference.load(&options.rom, None).map_err(report)?;
            if let Some(unix) = options.rtc {
                reference.set_rtc(unix).map_err(report)?;
            }
            align_mgba_start(&local, &mut reference, options.align, &mut out)?;
            if options.align && options.trace.is_none() {
                align_after_ram_reset(&mut local, &mut reference, &mut out)?;
            }
        }
        Kind::Mesen => {
            if options.rtc.is_some() {
                return Err(
                    "--rtc needs mgba-ref: Mesen's RTC follows the host clock, so mesen-ref leaves it off"
                        .to_owned(),
                );
            }
            let image = firmware.unwrap_or_else(bios::image);
            let bios = BiosFile::write(&image)?;
            reference
                .load(&options.rom, Some(&bios.0))
                .map_err(report)?;
            drop(bios);
            // Both sides started at reset, so frames count from there: another firmware's
            // boot may pass a VBlank before the cartridge entry.
            local.vblank = 0;
            let firmware = match &options.bios {
                Some(path) => format!("{}", path.display()),
                None => "this emulator's firmware".to_owned(),
            };
            writeln!(out, "Mesen runs {firmware} from reset; no start alignment")
                .map_err(report)?;
        }
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
                registers_line(label, &reference.registers().map_err(report)?)
            )
            .map_err(report)?;
            return Ok(false);
        }
        let detailed = options.detail.contains(&frame);
        let samples = if detailed { DETAIL_SAMPLES } else { SAMPLES };
        let (report_frame, ours, theirs) =
            compare_frame(&local, &mut reference, &options.ignore, samples).map_err(report)?;
        if let (Some(dir), true) = (&options.out, options.images.contains(&frame)) {
            write_images(dir, frame, &ours, &theirs, label).map_err(report)?;
        }
        if report_frame.clean() {
            if detailed {
                writeln!(out, "frame {frame}: no divergence").map_err(report)?;
            }
            continue;
        }
        divergent += 1;
        if first.is_none() || detailed {
            if first.is_none() {
                first = Some(frame);
                writeln!(out, "first divergence at frame {frame}:").map_err(report)?;
            } else {
                writeln!(out, "frame {frame}:").map_err(report)?;
            }
            report_frame.details(&mut out, label).map_err(report)?;
            writeln!(out, "{}", registers_line("ours", &local.registers())).map_err(report)?;
            writeln!(
                out,
                "{}",
                registers_line(label, &reference.registers().map_err(report)?)
            )
            .map_err(report)?;
            if let (Some(dir), true) = (&options.out, first == Some(frame)) {
                write_images(dir, frame, &ours, &theirs, label).map_err(report)?;
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

    fn local_with(words: &[u32]) -> Local {
        let rom = words.iter().flat_map(|word| word.to_le_bytes()).collect();
        Local::boot(rom, None, SaveDevice::None, None).unwrap()
    }

    #[test]
    fn ram_reset_search_finds_the_first_call() {
        // mov r0, #0; swi 0x010000; b .
        let mut local = local_with(&[0xe3a0_0000, 0xef01_0000, 0xeaff_fffe]);
        assert_eq!(
            find_ram_reset(&mut local, 100).unwrap(),
            Some((2, 0x0800_0004))
        );
        assert_eq!(local.machine.cpu().pc(), 0x0800_0008);
    }

    #[test]
    fn ram_reset_search_without_a_call_leaves_the_machine_where_it_was() {
        // b .
        let mut local = local_with(&[0xeaff_fffe]);
        let before = (local.machine.cpu().pc(), local.machine.cycles());
        assert_eq!(find_ram_reset(&mut local, 1000).unwrap(), None);
        assert_eq!((local.machine.cpu().pc(), local.machine.cycles()), before);
    }

    #[test]
    fn options_parse_inputs_ranges_and_numbers() {
        let options = parse(&args(
            "--reference ref --rom game.gba --frames 0x10 --save-type sram \
             --rtc 1767225600 --input 5:a:2,9:start:1 --ignore 0x03007f00-0x03007fff --no-align --trace 100 \
             --detail-frame 7,0x20",
        ))
        .unwrap();
        assert_eq!(options.frames, 16);
        assert_eq!(options.save, Some(SaveDevice::Sram));
        assert_eq!(options.rtc, Some(1_767_225_600));
        assert_eq!(
            options.inputs,
            vec![(5, Button::A, 2), (9, Button::Start, 1)]
        );
        assert_eq!(options.ignore, vec![(0x0300_7f00, 0x0300_7fff)]);
        assert!(!options.align);
        assert_eq!(options.trace, Some(100));
        assert_eq!(options.detail, vec![7, 32]);
        for bad in [
            "--rom game.gba",
            "--reference ref --rom game.gba --input 5:x:1",
            "--reference ref --rom game.gba --ignore 5",
            "--reference ref --rom game.gba --frames ten",
            "--reference ref --rom game.gba --detail-frame last",
            "--reference ref --rom game.gba --save-type tape",
            "--reference ref --rom game.gba --rtc 946684799",
        ] {
            assert!(parse(&args(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn rtc_clock_counts_whole_seconds_of_emulated_frames_like_mgba() {
        let rtc = RtcClock {
            start_ms: 1_767_225_600_000,
            entry_vblank: 0,
            seconds: 1_767_225_600,
        };
        // 59.7275 frames per second: frame 59 is still second 0, frame 60 is second 1.
        assert_eq!(rtc.seconds_after(59), 1_767_225_600);
        assert_eq!(rtc.seconds_after(60), 1_767_225_601);
        assert_eq!(rtc.seconds_after(3584), 1_767_225_660);
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
        let diff = compare_region("test", 0x100, &ours, &theirs, &[], SAMPLES).unwrap();
        assert_eq!((diff.differing, diff.unreadable), (2, 1));
        assert_eq!(diff.samples[0], (0x101, Some(2), 9));
        assert_eq!(diff.samples.len(), 3);
        // A sample limit shortens the address list, never the counts.
        let diff = compare_region("test", 0x100, &ours, &theirs, &[], 1).unwrap();
        assert_eq!((diff.differing, diff.unreadable), (2, 1));
        assert_eq!(diff.samples, vec![(0x101, Some(2), 9)]);
        let diff =
            compare_region("test", 0x100, &ours, &theirs, &[(0x101, 0x102)], SAMPLES).unwrap();
        assert_eq!((diff.differing, diff.unreadable), (1, 0));
        assert!(
            compare_region("test", 0x100, &ours, &theirs, &[(0x100, 0x103)], SAMPLES).is_none()
        );
    }
}
