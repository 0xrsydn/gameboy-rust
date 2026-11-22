//! This emulator behind the [GBA Eval](https://github.com/mechanize-work/gba-eval) benchmark's
//! WebAssembly ABI (`spec/ABI.md` there): a `wasm32-unknown-unknown` cdylib, `gba_emu.wasm`,
//! with ten C exports plus the optional `emu_boot_frames`. See docs/gba-eval.md.
//!
//! The benchmark scores each frame's pixels and the audio against Mesen 2, configured as its
//! `reference/mesen_step.cpp` does. [`Session`] starts and runs the machine the same way:
//!
//! - Mesen skips the boot (`SkipBootScreen`); [`bios::skip_boot`] enters the cartridge in
//!   the same state, with the required firmware mapped for SWIs and IRQs.
//! - The reference burns 6 frames before frame 0 and exports `emu_boot_frames` = 6 when it
//!   runs as a candidate; this library does the same, which aligns the two frame for frame.
//! - A frame ends where Mesen's does, on entry to line 160.
//! - Mesen polls input at that VBlank, so keys set before a frame take effect as it ends.
//! - The save type is chosen as Mesen chooses it ([`save_device`]).
//! - Brightness decrease rounds down, as Mesen's does ([`DarkenRounding::Down`]); hardware's
//!   rounding is not known yet (docs/research/blend-rounding.md).
//!
//! [`Session`] is the whole emulator state; the exports wrap one instance. Native tools use
//! [`Session`] directly, so local scoring runs exactly the code the benchmark loads.

use std::cell::RefCell;

use gba_core::{
    audio::StereoLevel,
    bios,
    cartridge::SaveDevice,
    input::Buttons,
    machine::Machine,
    video::{DarkenRounding, Framebuffer, HEIGHT, WIDTH},
};

/// The firmware embedded at build time from `GBA_EVAL_BIOS` (see build.rs); empty if none.
static EMBEDDED_BIOS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/bios.bin"));

/// The ABI's minimum ROM buffer.
pub const ROM_BUFFER_SIZE: usize = 32 * 1024 * 1024;
/// Captured audio is 32768 Hz stereo.
pub const AUDIO_RATE: u32 = 32_768;
/// Frames the benchmark runs before frame 0, as its Mesen reference does.
pub const BOOT_FRAMES: u32 = 6;
/// Stereo pairs kept between drains: several frames (about 546 pairs each).
const AUDIO_PAIRS: usize = 8192;
/// Machine steps allowed for one frame before the run is declared stuck.
const FRAME_STEP_LIMIT: usize = 2_000_000;
/// Captured levels are signed 10-bit; this scales them to the i16 range.
const AUDIO_SCALE: i16 = 64;

/// The firmware the benchmark requires at address 0 when it was embedded, otherwise this
/// emulator's own.
pub fn firmware() -> Vec<u8> {
    if EMBEDDED_BIOS.is_empty() {
        bios::image()
    } else {
        EMBEDDED_BIOS.to_vec()
    }
}

/// Whether the benchmark's BIOS stub was embedded at build time.
pub fn has_embedded_bios() -> bool {
    !EMBEDDED_BIOS.is_empty()
}

/// The save device Mesen 2 chooses with `SaveType::AutoDetect` (`GbaConsole::InitSaveRam`):
/// three game codes are forced, then the first library marker found anywhere in the ROM,
/// checked in Mesen's order, and SRAM when there is none.
pub fn save_device(rom: &[u8]) -> SaveDevice {
    match rom.get(0xac..0xb0) {
        Some(b"A2YE") | Some(b"AI2E") => return SaveDevice::None,
        Some(b"AYGE") | Some(b"ALUE") | Some(b"ALUP") => return SaveDevice::Eeprom512,
        _ => {}
    }
    let contains = |marker: &[u8]| rom.windows(marker.len()).any(|window| window == marker);
    [
        (&b"SRAM_V"[..], SaveDevice::Sram),
        (b"EEPROM_V", SaveDevice::Eeprom),
        (b"FLASH_V", SaveDevice::Flash64),
        (b"FLASH512_V", SaveDevice::Flash64),
        (b"FLASH1M_V", SaveDevice::Flash128),
    ]
    .into_iter()
    .find(|(marker, _)| contains(marker))
    .map_or(SaveDevice::Sram, |(_, device)| device)
}

/// One emulator instance as the benchmark drives it.
pub struct Session {
    firmware: Vec<u8>,
    rom: Vec<u8>,
    machine: Option<Machine>,
    frame: Framebuffer,
    /// The last presented frame, 0xAABBGGRR with opaque alpha.
    pixels: Vec<u32>,
    /// Interleaved stereo i16 since the last drain. Its capacity is fixed, so the ABI's
    /// buffer pointer stays valid.
    audio: Vec<i16>,
    /// A drain happened: the next frame writes from the start.
    drained: bool,
    levels: Vec<StereoLevel>,
    /// Keys to apply at the end of the next frame.
    keys: u16,
    vblanks: u64,
    error: Option<String>,
}

impl Session {
    pub fn new(firmware: Vec<u8>) -> Self {
        Self {
            firmware,
            rom: Vec::new(),
            machine: None,
            frame: Framebuffer::default(),
            pixels: vec![0xff00_0000; WIDTH * HEIGHT],
            audio: Vec::with_capacity(AUDIO_PAIRS * 2),
            drained: false,
            levels: vec![StereoLevel::default(); AUDIO_PAIRS],
            keys: 0,
            vblanks: 0,
            error: None,
        }
    }

    /// Load a ROM and start it from the skipped boot. The previous state is discarded.
    pub fn load(&mut self, rom: Vec<u8>) -> Result<(), String> {
        self.rom = rom;
        self.reset()
    }

    /// Start the loaded ROM again, in the same state as `load`.
    pub fn reset(&mut self) -> Result<(), String> {
        self.machine = None;
        self.frame = Framebuffer::default();
        self.pixels.fill(0xff00_0000);
        self.audio.clear();
        self.drained = false;
        self.keys = 0;
        self.error = None;
        let mut machine = bios::skip_boot(self.rom.clone(), self.firmware.clone())
            .map_err(|error| format!("cannot start the ROM: {error:?}"))?;
        let memory = machine.memory_mut();
        memory.set_save_device(save_device(&self.rom));
        memory.set_darken_rounding(DarkenRounding::Down);
        memory.set_scanline_rendering(true);
        memory.set_audio_capture(true);
        self.vblanks = memory.display_position().vblanks;
        self.machine = Some(machine);
        Ok(())
    }

    /// Keys for the next frame, active-high in KEYINPUT bit order. Like Mesen's VBlank
    /// input poll, they take effect when that frame ends.
    pub fn set_keys(&mut self, keys: u16) {
        self.keys = keys & 0x3ff;
    }

    /// Run to the next entry to line 160 and present the frame. After a core diagnostic the
    /// machine stops, and the last frame and silence remain; see [`Session::error`].
    pub fn run_frame(&mut self) {
        let Some(machine) = self.machine.as_mut() else {
            return;
        };
        if self.error.is_some() {
            return;
        }
        if let Err(error) = machine.run_until_vblank(FRAME_STEP_LIMIT) {
            let cpu = machine.cpu();
            self.error = Some(format!(
                "{error:?} at PC={:#010x} after {} frames",
                cpu.pc(),
                self.vblanks
            ));
            return;
        }
        self.vblanks = machine.memory().display_position().vblanks;
        machine
            .memory_mut()
            .set_buttons(Buttons::from_bits(self.keys));
        if let Ok(true) = machine.memory().present_frame(&mut self.frame) {
            for (out, &pixel) in self.pixels.iter_mut().zip(self.frame.pixels()) {
                // 0x00RRGGBB to 0xAABBGGRR.
                *out = 0xff00_0000 | (pixel & 0xff) << 16 | (pixel & 0xff00) | (pixel >> 16 & 0xff);
            }
        }
        if self.drained {
            self.audio.clear();
            self.drained = false;
        }
        let count = machine.memory_mut().drain_audio_samples(&mut self.levels);
        let room = (AUDIO_PAIRS * 2 - self.audio.len()) / 2;
        for level in &self.levels[..count.min(room)] {
            self.audio.push(level.left.saturating_mul(AUDIO_SCALE));
            self.audio.push(level.right.saturating_mul(AUDIO_SCALE));
        }
    }

    /// The last presented frame: 240×160 pixels, 0xAABBGGRR.
    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }

    /// Interleaved stereo samples since the last call to [`Session::drain_audio`].
    pub fn audio(&self) -> &[i16] {
        &self.audio
    }

    /// Stereo pairs since the last drain; the next frame writes from the start again.
    /// The samples stay readable through [`Session::audio`] until then.
    pub fn drain_audio(&mut self) -> usize {
        self.drained = true;
        self.audio.len() / 2
    }

    fn audio_ptr(&self) -> *const i16 {
        self.audio.as_ptr()
    }

    /// The core diagnostic that stopped the machine, if any.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn machine(&self) -> Option<&Machine> {
        self.machine.as_ref()
    }
}

// The C ABI. The benchmark calls these from one thread; the state lives in a thread-local
// instance. Returned pointers address heap buffers that are allocated once and never
// reallocated, so they stay valid as the ABI requires.

struct Exports {
    session: Session,
    rom_buffer: Vec<u8>,
}

thread_local! {
    static EXPORTS: RefCell<Option<Exports>> = const { RefCell::new(None) };
}

fn with<R>(default: R, f: impl FnOnce(&mut Exports) -> R) -> R {
    EXPORTS.with_borrow_mut(|exports| exports.as_mut().map_or(default, f))
}

/// Set up the instance. Returns 1.
#[no_mangle]
pub extern "C" fn emu_init() -> i32 {
    EXPORTS.with_borrow_mut(|exports| {
        *exports = Some(Exports {
            session: Session::new(firmware()),
            rom_buffer: vec![0; ROM_BUFFER_SIZE],
        });
    });
    1
}

/// The buffer the host writes a ROM into before `emu_load_rom`.
#[no_mangle]
pub extern "C" fn emu_rom_buffer() -> *mut u8 {
    with(std::ptr::null_mut(), |exports| {
        exports.rom_buffer.as_mut_ptr()
    })
}

/// Load the first `len` bytes of the ROM buffer and start it. Returns 1, or 0 on failure.
#[no_mangle]
pub extern "C" fn emu_load_rom(len: i32) -> i32 {
    with(0, |exports| {
        let Ok(len) = usize::try_from(len) else {
            return 0;
        };
        if len == 0 || len > ROM_BUFFER_SIZE {
            return 0;
        }
        let rom = exports.rom_buffer[..len].to_vec();
        i32::from(exports.session.load(rom).is_ok())
    })
}

/// Start the loaded ROM again, in the state `emu_load_rom` gives. Returns 1, or 0.
#[no_mangle]
pub extern "C" fn emu_reset() -> i32 {
    with(0, |exports| i32::from(exports.session.reset().is_ok()))
}

/// Keys for the next frame: bit 0 A, 1 B, 2 Select, 3 Start, 4 Right, 5 Left, 6 Up,
/// 7 Down, 8 R, 9 L; set bits are pressed.
#[no_mangle]
pub extern "C" fn emu_set_keys(keys: u32) {
    with((), |exports| exports.session.set_keys(keys as u16));
}

/// Advance one frame.
#[no_mangle]
pub extern "C" fn emu_run_frame() {
    with((), |exports| exports.session.run_frame());
}

/// The last frame: 240×160 pixels, 0xAABBGGRR, alpha 0xff.
#[no_mangle]
pub extern "C" fn emu_framebuffer() -> *const u32 {
    with(std::ptr::null(), |exports| {
        exports.session.pixels().as_ptr()
    })
}

/// Interleaved stereo i16 samples; `emu_audio_samples` says how many pairs are valid.
#[no_mangle]
pub extern "C" fn emu_audio_buffer() -> *const i16 {
    with(std::ptr::null(), |exports| exports.session.audio_ptr())
}

/// Stereo pairs produced since the last call; the call drains them.
#[no_mangle]
pub extern "C" fn emu_audio_samples() -> i32 {
    with(0, |exports| exports.session.drain_audio() as i32)
}

/// The audio sample rate in Hz.
#[no_mangle]
pub extern "C" fn emu_audio_rate() -> i32 {
    AUDIO_RATE as i32
}

/// Frames the benchmark runs before frame 0, matching its Mesen reference's warmup.
#[no_mangle]
pub extern "C" fn emu_boot_frames() -> i32 {
    BOOT_FRAMES as i32
}
