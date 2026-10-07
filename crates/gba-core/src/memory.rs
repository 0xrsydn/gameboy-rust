use std::{cell::Cell, error::Error, fmt};

#[cfg(test)]
mod audio_capture_tests;
#[cfg(test)]
mod audio_step_tests;
#[cfg(test)]
mod cartridge_step_tests;
mod fetch;
#[cfg(test)]
mod fetch_tests;
#[cfg(test)]
mod flash_step_tests;
#[cfg(test)]
mod flash_write_tests;
#[cfg(test)]
mod irq_fetch_tests;
mod iwram_bus;
#[cfg(test)]
mod iwram_history_tests;
#[cfg(test)]
mod joybus_step_tests;
#[cfg(test)]
mod multiplayer_step_tests;
#[cfg(test)]
mod prefetch_tests;
#[cfg(test)]
mod serial_step_tests;
#[cfg(test)]
mod timer_step_tests;
#[cfg(test)]
mod timing_event_tests;
#[cfg(test)]
mod wave_step_tests;
pub(crate) use fetch::InstructionFetch;
use iwram_bus::IwramBus;

use crate::{
    audio::{Audio, StereoLevel},
    cartridge::{Cartridge, CartridgeHardware, RtcDateTime, RtcError, SaveDevice, SaveError},
    cpu::InstructionSet,
    display::{DisplayPosition, CYCLES_PER_LINE, HBLANK_START, VISIBLE_LINES},
    dma::{DmaError, DMA_BASE, DMA_END},
    input::Buttons,
    io::{Io, Serial, TimerStep, HALTCNT, KEYINPUT, POSTFLG},
    timing::{AccessKind, AccessWidth, CpuTiming, Prefetch, StepTiming},
    video::{self, sprites::pipeline, Framebuffer, VideoError},
};

pub const ROM_START: u32 = 0x0800_0000;
/// Maximum supplied cartridge size, shared by all three Game Pak ROM windows.
pub const ROM_CAPACITY: usize = 32 * 1024 * 1024;
pub const BIOS_SIZE: usize = 16 * 1024;
pub const PALETTE_START: u32 = 0x0500_0000;
pub const VRAM_START: u32 = 0x0600_0000;
pub const OAM_START: u32 = 0x0700_0000;
const VRAM_SIZE: usize = 96 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryError {
    Unmapped(u32),
    ReadOnly(u32),
    Unaligned(u32),
    RomTooLarge(usize),
    InvalidBiosSize(usize),
    UnsupportedCartridgeAccess {
        address: u32,
        operation: &'static str,
    },
    UnsupportedIo {
        address: u32,
        value: u8,
        operation: &'static str,
    },
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedCartridgeAccess { address, operation } => {
                write!(f, "unsupported {operation} at {address:#010x}")
            }
            Self::UnsupportedIo {
                address,
                value,
                operation,
            } => write!(
                f,
                "unsupported {operation}: write {value:#04x} at {address:#010x}"
            ),
            Self::Unmapped(address) => write!(f, "unmapped memory at {address:#010x}"),
            Self::ReadOnly(address) => write!(f, "read-only memory at {address:#010x}"),
            Self::Unaligned(address) => write!(f, "unaligned memory access at {address:#010x}"),
            Self::RomTooLarge(size) => write!(f, "ROM size {size} exceeds 32 MiB"),
            Self::InvalidBiosSize(size) => {
                write!(f, "BIOS size {size} must be exactly {BIOS_SIZE} bytes")
            }
        }
    }
}

impl Error for MemoryError {}

fn vram_index(address: u32) -> usize {
    let offset = (address & 0x1ffff) as usize;
    if offset >= 0x18000 {
        offset - 0x8000
    } else {
        offset
    }
}

/// Host setup rejects read-only destinations; CPU and DMA use bus semantics.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WriteAccess {
    Host,
    Emulated,
}

#[derive(Clone, Copy)]
struct CpuAccess {
    pc: u32,
    prefetch: Option<u32>,
}

/// Work/video/palette RAM, OAM, cartridge ROM, optional BIOS, and supported I/O.
/// Unused-memory reads use ARM PC+8 or supported region-dependent Thumb snapshots.
/// Protected BIOS reads separately retain the snapshot from BIOS execution.
/// ARM/Thumb IWRAM fetches and data accesses retain local bus lanes transactionally.
/// Cold fills and target pairs drive the same latch, independently of executing region.
/// Successful DMA accesses drive local lanes even before the first CPU instruction.
/// Channel data remains separate; general and sub-instruction CPU bus handoff is unmodeled.
/// Instruction buffering and supported bus history consume shared fetch samples.
/// Timer, sound, and serial accesses use staged machine-step time; other per-access device timing remains unmodeled.
pub struct Memory {
    external_ram: Vec<u8>,
    internal_ram: Vec<u8>,
    palette_ram: Vec<u8>,
    video_ram: Vec<u8>,
    oam: Vec<u8>,
    rom: Vec<u8>,
    save_image: Vec<u8>,
    save_modified: bool,
    cartridge_elapsed: Cell<u32>,
    cartridge: Cartridge,
    cartridge_step: Cell<Option<Cartridge>>,
    bios: Option<Vec<u8>>,
    io: Io,
    cycles: u64,
    cpu_timing: Cell<Option<CpuTiming>>,
    timer_step: Cell<Option<TimerStep>>,
    gamepak_prefetch: Prefetch,
    #[cfg(test)]
    last_cpu_timing: Option<CpuTiming>,
    // A completed DMA unit breaks the CPU's next nominal code-access sequence.
    cpu_resume_nonsequential: bool,
    cpu_access: Option<CpuAccess>,
    bios_prefetch: Option<u32>,
    iwram_bus: IwramBus,
    scanline_capture: Option<video::capture::Capture>,
    sprite_pipeline: pipeline::SpritePipeline,
}

impl Memory {
    pub fn new(rom: Vec<u8>) -> Result<Self, MemoryError> {
        if rom.len() > ROM_CAPACITY {
            return Err(MemoryError::RomTooLarge(rom.len()));
        }
        Ok(Self {
            external_ram: vec![0; 256 * 1024],
            internal_ram: vec![0; 32 * 1024],
            palette_ram: vec![0; 1024],
            video_ram: vec![0; VRAM_SIZE],
            oam: vec![0; 1024],
            rom,
            save_image: Vec::new(),
            save_modified: false,
            cartridge_elapsed: Cell::new(0),
            cartridge: Cartridge::default(),
            cartridge_step: Cell::new(None),
            bios: None,
            io: Io::default(),
            cycles: 0,
            cpu_timing: Cell::new(None),
            timer_step: Cell::new(None),
            gamepak_prefetch: Prefetch::default(),
            #[cfg(test)]
            last_cpu_timing: None,
            cpu_resume_nonsequential: false,
            cpu_access: None,
            bios_prefetch: None,
            iwram_bus: IwramBus::default(),
            scanline_capture: None,
            sprite_pipeline: pipeline::SpritePipeline::default(),
        })
    }

    /// Map a caller-supplied 16 KiB image at the ARM exception vectors.
    /// No Nintendo BIOS is included. The image is read-only and is not mirrored.
    pub fn with_bios(rom: Vec<u8>, bios: Vec<u8>) -> Result<Self, MemoryError> {
        if bios.len() != BIOS_SIZE {
            return Err(MemoryError::InvalidBiosSize(bios.len()));
        }
        let mut memory = Self::new(rom)?;
        memory.bios = Some(bios);
        Ok(memory)
    }

    /// Replace GPIO/RTC hardware with its initial state. Call between machine steps.
    /// Save hardware/data are preserved. This never modifies ROM bytes or enables a host clock.
    pub fn set_cartridge_hardware(&mut self, hardware: CartridgeHardware) {
        assert!(
            self.cartridge_step.get().is_none(),
            "active cartridge transaction"
        );
        self.cartridge.set_hardware(hardware);
    }

    /// Replace save hardware and its array with erased bytes. GPIO/RTC state is unchanged.
    /// Call between machine steps; no file is loaded or written by the core.
    pub fn set_save_device(&mut self, device: SaveDevice) {
        assert!(
            self.cartridge_step.get().is_none(),
            "active cartridge transaction"
        );
        self.save_image = vec![0xff; device.capacity()];
        self.save_modified = false;
        self.cartridge.set_save_device(device);
    }

    /// Inspect the complete save array, independently of ID mode and selected bank.
    pub fn save_image(&self) -> Option<&[u8]> {
        (self.cartridge.save_device() != SaveDevice::None).then_some(self.save_image.as_slice())
    }

    /// Replace the entire array and reset Flash command/bank state. RTC state is unchanged.
    /// A wrong length changes nothing. This setup API does not emulate programming.
    pub fn load_save_image(&mut self, bytes: &[u8]) -> Result<(), SaveError> {
        assert!(
            self.cartridge_step.get().is_none(),
            "active cartridge transaction"
        );
        let device = self.cartridge.save_device();
        if device == SaveDevice::None {
            return Err(SaveError::NoDevice);
        }
        if bytes.len() != device.capacity() {
            return Err(SaveError::InvalidSize {
                expected: device.capacity(),
                actual: bytes.len(),
            });
        }
        self.save_image.copy_from_slice(bytes);
        self.save_modified = false;
        self.cartridge.set_save_device(device);
        Ok(())
    }

    /// Inspect the live calendar, not an in-progress serial read snapshot.
    pub fn rtc_datetime(&self) -> Option<RtcDateTime> {
        self.cartridge_state().rtc_datetime()
    }

    /// Set the RTC calendar between machine steps. Existing serial read snapshots stay latched.
    pub fn set_rtc_datetime(&mut self, date: RtcDateTime) -> Result<(), RtcError> {
        assert!(
            self.cartridge_step.get().is_none(),
            "active cartridge transaction"
        );
        self.cartridge.set_rtc_datetime(date)
    }

    /// Supply battery-clock elapsed seconds independently of CPU cycles, HALT, and STOP.
    /// Call between machine steps. This requests no IRQ and never wakes the CPU.
    pub fn advance_rtc_seconds(&mut self, seconds: u64) -> Result<(), RtcError> {
        assert!(
            self.cartridge_step.get().is_none(),
            "active cartridge transaction"
        );
        self.cartridge.advance_rtc_seconds(seconds)
    }

    /// True after a completed Flash operation changed stored bytes since setup/load.
    pub fn save_modified(&self) -> bool {
        self.save_modified
    }

    /// A command or operation is incomplete. Hosts should not persist on exit in this state.
    pub fn save_write_pending(&self) -> bool {
        self.cartridge.save_unfinished()
    }

    fn commit_save_mutation(&mut self) {
        if let Some(effect) = self.cartridge.take_save_mutation() {
            let target = &mut self.save_image[effect.start..effect.end];
            self.save_modified |= target.iter().any(|&b| b != effect.value);
            target.fill(effect.value);
        }
    }

    fn cartridge_state(&self) -> Cartridge {
        self.cartridge_step.get().unwrap_or(self.cartridge)
    }

    /// Current committed audio mixer level. This is not a host sample stream.
    pub fn audio_level(&self) -> StereoLevel {
        self.io.audio.level()
    }

    /// Enable optional 32768 Hz stereo capture. A toggle clears queued frames,
    /// resets its synthetic phase/drop counter, and does not reset sound hardware.
    /// Call between machine steps. Repeating the current setting preserves state.
    pub fn set_audio_capture(&mut self, enabled: bool) {
        assert!(
            self.timer_step.get().is_none(),
            "audio capture configuration during a step"
        );
        self.io.audio_capture.set_enabled(enabled);
    }

    /// Drain committed frames, oldest first. Failed steps never publish samples.
    /// Each sample is a signed ten-bit digital level, not normalized host PCM.
    pub fn drain_audio_samples(&mut self, output: &mut [StereoLevel]) -> usize {
        self.io.audio_capture.drain(output)
    }

    /// Frames discarded because the bounded capture buffers were full.
    pub fn audio_dropped_samples(&self) -> u64 {
        self.io.audio_capture.dropped
    }

    fn serial_state(&self) -> Serial {
        self.timer_step
            .get()
            .map_or(self.io.serial, |step| step.serial)
    }

    fn audio_state(&self) -> Audio {
        self.timer_step
            .get()
            .map_or(self.io.audio, |step| step.audio)
    }

    /// Total elapsed emulated clock cycles, wrapping at u64::MAX. Reads/writes
    /// alone consume no cycles. STOP discards supplied GBA-cycle advances, not independent RTC time.
    pub fn cycles(&self) -> u64 {
        self.cycles
    }

    /// Advance devices and optional row capture. Multiple unserviced DMA requests
    /// coalesce. No CPU or DMA work is executed by this method. STOP freezes GBA-clock
    /// progress: supplied cycles are ignored until an external wake condition.
    /// The independently supplied RTC battery clock is not advanced by this method.
    /// This device-only API does not advance opcode prefetch. CPU/DMA/HALT steps
    /// account for their own cartridge-bus time before updating device clocks.
    pub fn advance_cycles(&mut self, cycles: u32) {
        if !self.stopped() {
            self.advance_running_cycles(cycles);
        }
    }

    /// Finish the instruction/IRQ entry that started while running. A STOP store
    /// still pays its full nominal cost before later steps freeze the system.
    pub(crate) fn complete_cpu_step(&mut self, cycles: u32) {
        // IRQ entry consumes the resume too, without executing an instruction.
        self.cpu_resume_nonsequential = false;
        self.complete_timer_step(cycles);
    }

    pub(crate) fn begin_timer_step(&self) {
        debug_assert!(self.timer_step.get().is_none());
        self.timer_step.set(Some(self.io.timer_step()));
        self.cartridge_step.set(Some(self.cartridge));
        self.cartridge_elapsed.set(0);
    }

    pub(crate) fn discard_timer_step(&self) {
        self.timer_step.set(None);
        self.cartridge_step.set(None);
        self.cartridge_elapsed.set(0);
    }

    fn advance_timer_step(&self, cycles: u32) {
        if let Some(mut cartridge) = self.cartridge_step.get() {
            cartridge.advance(cycles - self.cartridge_elapsed.get());
            self.cartridge_elapsed.set(cycles);
            self.cartridge_step.set(Some(cartridge));
        }
        if let Some(mut step) = self.timer_step.get() {
            step.advance_to(cycles);
            self.timer_step.set(Some(step));
        }
    }

    fn complete_timer_step(&mut self, cycles: u32) {
        self.advance_timer_step(cycles);
        let step = self
            .timer_step
            .take()
            .expect("machine timer transaction is active");
        self.io.commit_timer_step(step);
        self.cartridge = self
            .cartridge_step
            .take()
            .expect("active cartridge transaction");
        self.commit_save_mutation();
        self.cartridge_elapsed.set(0);
        // Staged devices already reached this boundary. Advance remaining devices exactly once.
        self.advance_running_cycles_with_timers(cycles, false);
    }

    fn advance_running_cycles(&mut self, cycles: u32) {
        self.cartridge.advance(cycles);
        self.commit_save_mutation();
        self.advance_running_cycles_with_timers(cycles, true);
    }

    fn advance_running_cycles_with_timers(&mut self, cycles: u32, advance_timers: bool) {
        if cycles != 0 && !self.sprite_pipeline.started {
            // Synthetic reset begins on row0, with no preceding row227 to prepare it.
            self.prepare_sprite_row(0);
            self.sprite_pipeline.started = true;
        }
        if self.scanline_capture.is_none() {
            let position = self.io.display_position();
            let phase =
                u32::from(position.scanline) * CYCLES_PER_LINE + u32::from(position.line_cycle);
            let mut advanced = 0;
            for (offset, row) in pipeline::latest_events(phase, cycles).into_iter().flatten() {
                self.io.advance(offset - advanced, advance_timers);
                self.prepare_sprite_row(row);
                advanced = offset;
            }
            self.io.advance(cycles - advanced, advance_timers);
        } else {
            // Split at display and sprite-preparation edges. Video writes have
            // already committed. Staged devices skip their bulk clock on machine steps.
            let mut remaining = cycles;
            while remaining != 0 {
                let position = self.io.display_position();
                let cycle = u32::from(position.line_cycle);
                let display_edge = if cycle < HBLANK_START {
                    HBLANK_START - cycle
                } else {
                    CYCLES_PER_LINE - cycle
                };
                let until =
                    display_edge.min(pipeline::next_event(position.scanline, position.line_cycle));
                let step = remaining.min(until);
                self.io.advance(step, advance_timers);
                remaining -= step;
                if step == until {
                    let position = self.io.display_position();
                    if u32::from(position.line_cycle) == pipeline::PREPARE_CYCLE {
                        if let Some(row) = pipeline::target(position.scanline) {
                            self.prepare_sprite_row(row);
                        }
                    } else if u32::from(position.line_cycle) == HBLANK_START
                        && u32::from(position.scanline) < VISIBLE_LINES
                    {
                        let row = usize::from(position.scanline);
                        if self.scanline_capture.as_ref().unwrap().wants_row(row) {
                            let mut settings = self.io.video_settings(true);
                            settings.prepared_objects = Some(self.sprite_pipeline.row(row));
                            let pixels = video::render_rows(
                                settings,
                                &self.video_ram,
                                &self.palette_ram,
                                &self.oam,
                                row..row + 1,
                            );
                            self.scanline_capture.as_mut().unwrap().record(row, pixels);
                        }
                    } else if position.line_cycle == 0
                        && u32::from(position.scanline) == VISIBLE_LINES
                    {
                        self.scanline_capture
                            .as_mut()
                            .unwrap()
                            .publish(position.vblanks);
                    }
                }
            }
        }
        self.cycles = self.cycles.wrapping_add(u64::from(cycles));
    }

    fn prepare_sprite_row(&mut self, row: usize) {
        let settings = self.io.video_settings(true);
        let prepared = video::sprites::prepare(
            settings.control,
            settings.mosaic,
            settings.vertical_mosaic.map(|v| v.object),
            &self.oam,
            &self.video_ram,
            row..row + 1,
        );
        self.sprite_pipeline.store(row, prepared);
    }

    /// Enable row capture without changing clocks or hardware registers. Disabled
    /// by default to keep CPU-only tests and large clock batches inexpensive.
    /// Repeating the current setting preserves captured data. Disabling drops it.
    pub fn set_scanline_rendering(&mut self, enabled: bool) {
        if enabled && self.scanline_capture.is_none() {
            self.scanline_capture = Some(video::capture::Capture::default());
        } else if !enabled {
            self.scanline_capture = None;
        }
    }

    /// VBlank number of the latest full captured frame, including failed frames.
    /// None means capture is disabled or no full frame has reached VBlank yet.
    pub fn captured_vblank(&self) -> Option<u64> {
        self.scanline_capture
            .as_ref()
            .and_then(video::capture::Capture::vblank)
    }

    /// Copy the latest captured frame. False means no complete frame is available.
    /// Rendering errors are reported here, not as CPU/DMA failures. On errors or
    /// false, the output is unchanged. Presenting does not advance time or consume
    /// the frame. Writes after capture cannot change its pixels.
    pub fn present_frame(&self, output: &mut Framebuffer) -> Result<bool, VideoError> {
        match &self.scanline_capture {
            Some(capture) => capture.present(output),
            None => Ok(false),
        }
    }

    /// Execute at most one DMA unit. Validate before side effects, then advance
    /// device clocks with the CPU paused. Earlier units remain committed on error.
    pub(crate) fn step_dma(&mut self) -> Result<Option<(usize, StepTiming)>, DmaError> {
        let Some(transfer) = self.io.next_dma()? else {
            return Ok(None);
        };
        let channel = transfer.channel;
        if (POSTFLG..=POSTFLG + 3).contains(&transfer.destination) {
            return Err(DmaError::PowerControlDestination {
                channel,
                address: transfer.destination,
            });
        }
        if transfer.source < 0x0200_0000 && transfer.data_latch.is_none() {
            return Err(DmaError::UnsupportedSource {
                channel,
                address: transfer.source,
            });
        }
        if (DMA_BASE..=DMA_END).contains(&transfer.destination) {
            return Err(DmaError::RegisterDestination {
                channel,
                address: transfer.destination,
            });
        }
        let map_error = |error| DmaError::Memory { channel, error };
        // Validate before reading or committing channel data. Failed units are atomic.
        match transfer.width {
            AccessWidth::Halfword => {
                self.write_index::<2>(transfer.destination, WriteAccess::Emulated)
            }
            AccessWidth::Word => self.write_index::<4>(transfer.destination, WriteAccess::Emulated),
            AccessWidth::Byte => unreachable!("DMA is never byte-wide"),
        }
        .map_err(map_error)?;
        let kind = if transfer.first {
            AccessKind::NonSequential
        } else {
            AccessKind::Sequential
        };
        let mut prefetch = self.gamepak_prefetch;
        prefetch.configure(self.waitcnt());
        let internal_cycles = if transfer.first { 2 } else { 0 };
        // Keep the existing whole-unit DMA startup model; no sub-instruction arbitration.
        prefetch.advance(internal_cycles);
        let source_cycles = prefetch.data(transfer.source, transfer.width, kind);
        let destination_cycles = prefetch.data(transfer.destination, transfer.width, kind);
        let timing = StepTiming {
            code_cycles: 0,
            data_cycles: source_cycles + destination_cycles,
            internal_cycles,
            idle_cycles: 0,
        };
        self.begin_timer_step();
        let result = (|| {
            self.advance_timer_step(internal_cycles + source_cycles);
            let data_latch = if transfer.source < 0x0200_0000 {
                // Blocked reads retain channel data, not firmware or another channel's value.
                transfer
                    .data_latch
                    .expect("unknown blocked source rejected above")
            } else {
                match transfer.width {
                    AccessWidth::Halfword => u32::from(self.read16(transfer.source)?) * 0x0001_0001,
                    AccessWidth::Word => self.read32(transfer.source)?,
                    AccessWidth::Byte => unreachable!("DMA is never byte-wide"),
                }
            };
            // Blocked halfword reads retain both lanes; the destination selects a lane.
            let value = match transfer.width {
                AccessWidth::Halfword => data_latch >> ((transfer.destination & 2) * 8),
                AccessWidth::Word => data_latch,
                AccessWidth::Byte => unreachable!("DMA is never byte-wide"),
            };
            self.advance_timer_step(timing.total());
            match transfer.width {
                AccessWidth::Halfword => self.write_aligned(
                    transfer.destination,
                    (value as u16).to_le_bytes(),
                    WriteAccess::Emulated,
                ),
                AccessWidth::Word => self.write_aligned(
                    transfer.destination,
                    value.to_le_bytes(),
                    WriteAccess::Emulated,
                ),
                AccessWidth::Byte => unreachable!("DMA is never byte-wide"),
            }?;
            Ok::<_, MemoryError>((data_latch, value))
        })();
        let (data_latch, value) = match result {
            Ok(result) => result,
            Err(error) => {
                self.discard_timer_step();
                return Err(map_error(error));
            }
        };
        // Only a successful unit commits queue progress. A WAITCNT destination can reset it.
        prefetch.configure(self.waitcnt());
        self.gamepak_prefetch = prefetch;
        // DMA runs before the resumed CPU fetch. Only actual IWRAM accesses drive
        // its local lanes, in read/write order. Channel halfword duplication is not
        // a full IWRAM bus write. Failed units never reach these history commits.
        self.iwram_bus
            .dma_access(transfer.source, transfer.width, data_latch);
        self.iwram_bus
            .dma_access(transfer.destination, transfer.width, value);
        // Keep this channel active during its cycles: another edge must not
        // queue a second block while the current block is still transferring.
        self.complete_timer_step(timing.total());
        self.io.complete_dma_unit(channel, data_latch);
        self.cpu_resume_nonsequential = true;
        Ok(Some((channel, timing)))
    }

    /// Whether HALT is waiting for IE & IF to become nonzero.
    /// IME and the CPU's interrupt mask do not affect this state.
    pub fn halted(&self) -> bool {
        self.io.halted()
    }

    /// Whether STOP is waiting for an enabled live keypad wake condition.
    pub fn stopped(&self) -> bool {
        self.io.stopped()
    }

    pub(crate) fn next_event_cycles(&self) -> u32 {
        self.io.next_event_cycles()
    }

    pub(crate) fn begin_cpu_access(
        &mut self,
        pc: u32,
        instruction_set: InstructionSet,
        fill: Option<&[InstructionFetch; 2]>,
        fetch: &InstructionFetch,
    ) {
        debug_assert!(self.cpu_access.is_none());
        debug_assert_eq!(fetch.state, instruction_set);
        debug_assert_eq!(fetch.address, pc.wrapping_add(2 * instruction_set.width()));
        // Cold fills drive current/decode fetches; retained instructions never replay them.
        // Stage the new lookahead last, before data accesses and the entry snapshot.
        self.iwram_bus.begin();
        if let Some(fill) = fill {
            for (index, sample) in fill.iter().enumerate() {
                debug_assert_eq!(sample.state, instruction_set);
                debug_assert_eq!(
                    sample.address,
                    pc.wrapping_add(index as u32 * instruction_set.width())
                );
                self.drive_iwram_fetch(sample);
            }
        }
        self.drive_iwram_fetch(fetch);
        // The newly accessed bus supplies the snapshot, even across a region boundary.
        // Unknown local lanes and unavailable/unsupported fetch observations stay diagnostic.
        let prefetch = match instruction_set {
            InstructionSet::Arm => fetch.bus_word,
            InstructionSet::Thumb if fetch.instruction.is_err() => None,
            InstructionSet::Thumb if fetch.address >> 24 == 3 => self.iwram_bus.snapshot(),
            InstructionSet::Thumb => fetch.bus_word,
        };
        self.cpu_access = Some(CpuAccess { pc, prefetch });
    }

    pub(crate) fn end_cpu_access(&mut self, succeeded: bool) {
        if succeeded {
            // Timed and untimed CPU execution both consume a resume. Errors do not.
            self.cpu_resume_nonsequential = false;
        }
        self.iwram_bus.finish(succeeded);
        if let Some(access) = self.cpu_access.take() {
            if succeeded && access.pc < BIOS_SIZE as u32 {
                // Commit only successful instruction snapshots under our diagnostic policy.
                // Missing lookahead invalidates history rather than exposing stale bytes.
                self.bios_prefetch = access.prefetch;
            }
        }
    }

    fn drive_iwram_fetch(&self, fetch: &InstructionFetch) {
        if let Ok(value) = fetch.instruction {
            let width = match fetch.state {
                InstructionSet::Arm => AccessWidth::Word,
                InstructionSet::Thumb => AccessWidth::Halfword,
            };
            self.iwram_bus.access(fetch.address, width, value);
        }
    }

    /// IRQ's discarded source fetch drives only its actual IWRAM lanes.
    /// Do not invent cold fills or commit a BIOS instruction snapshot: none executed.
    pub(crate) fn discarded_cpu_fetch(&mut self, fetch: &InstructionFetch) {
        self.iwram_bus.begin();
        self.drive_iwram_fetch(fetch);
        self.iwram_bus.finish(true);
    }

    /// Successful IWRAM target fetches drive their lanes in order, in either CPU state.
    /// Other-region and unavailable target fetches leave the local latch unchanged.
    pub(crate) fn refill_cpu_bus_history(&mut self, fetches: &[InstructionFetch; 2]) {
        self.iwram_bus.begin();
        let [first, second] = fetches;
        debug_assert_eq!(first.state, second.state);
        debug_assert_eq!(
            second.address,
            first.address.wrapping_add(first.state.width())
        );
        for fetch in fetches {
            self.drive_iwram_fetch(fetch);
        }
        self.iwram_bus.finish(true);
    }

    fn can_write_power_control(&self) -> bool {
        // Bare memory writes are host/debug setup. CPU code must execute in BIOS.
        self.cpu_access
            .is_none_or(|access| access.pc < BIOS_SIZE as u32)
    }

    /// Current scanline position and frame/VBlank event counters.
    pub fn display_position(&self) -> DisplayPosition {
        self.io.display_position()
    }

    /// The GBA IRQ line after IE/IF/IME gating, before the CPU's interrupt mask.
    pub fn irq_pending(&self) -> bool {
        self.io.irq_pending()
    }

    /// Replace the current pressed-button state. This does not advance time.
    /// While running, sample KEYCNT and latch a matching IRQ without cycles.
    /// This can wake HALT. In STOP, input can wake the system without setting IF.
    pub fn set_buttons(&mut self, buttons: Buttons) {
        self.io.set_buttons(buttons);
    }

    /// Render a Mode 0–5 snapshot. No emulated cycles are consumed.
    /// Uses programmed affine origins, geometric window bounds, and screen-grid mosaic.
    /// Ignores captured history. Use present_frame for latched scanline state.
    pub fn render_frame(&self, frame: &mut Framebuffer) -> Result<(), VideoError> {
        video::render(
            self.io.video_settings(false),
            &self.video_ram,
            &self.palette_ram,
            &self.oam,
            frame,
        )
    }

    /// Current Game Pak wait-state control, including nominal opcode prefetch enable.
    pub fn waitcnt(&self) -> u16 {
        self.io.waitcnt()
    }

    /// Resolve the next nominal code access without consuming pending DMA history.
    /// Successful instruction/IRQ completion consumes it; host reads and idle do not.
    pub(crate) fn cpu_code_kind(&self, kind: AccessKind) -> AccessKind {
        if self.cpu_resume_nonsequential {
            AccessKind::NonSequential
        } else {
            kind
        }
    }

    pub(crate) fn begin_cpu_timing(&self, cold: bool) {
        debug_assert!(self.cpu_timing.get().is_none());
        let mut prefetch = self.gamepak_prefetch;
        if cold {
            prefetch.clear();
        }
        self.cpu_timing
            .set(Some(CpuTiming::new(self.waitcnt(), prefetch)));
    }

    pub(crate) fn advance_halt_cycles(&mut self, cycles: u32) {
        if !self.stopped() {
            self.gamepak_prefetch.advance(cycles);
        }
        self.advance_cycles(cycles);
    }

    pub(crate) fn end_cpu_timing(&mut self, succeeded: bool) -> StepTiming {
        let trace = self
            .cpu_timing
            .take()
            .expect("CPU timing transaction is active");
        if !succeeded {
            return StepTiming::default();
        }
        self.gamepak_prefetch = trace.prefetch;
        #[cfg(test)]
        {
            self.last_cpu_timing = Some(trace);
        }
        trace.total
    }

    pub(crate) fn record_code_access(&self, address: u32, width: AccessWidth, kind: AccessKind) {
        if let Some(mut trace) = self.cpu_timing.get() {
            trace.code(address, width, kind);
            self.cpu_timing.set(Some(trace));
            self.advance_timer_step(trace.total.total());
        }
    }

    pub(crate) fn record_internal_cycles(&self, cycles: u32) {
        if cycles == 0 {
            return;
        }
        if let Some(mut trace) = self.cpu_timing.get() {
            trace.internal(cycles);
            self.cpu_timing.set(Some(trace));
            self.advance_timer_step(trace.total.total());
        }
    }

    fn record_access(&self, address: u32, width: AccessWidth) {
        if let Some(mut trace) = self.cpu_timing.get() {
            trace.data(self.waitcnt(), address, width);
            self.cpu_timing.set(Some(trace));
            self.advance_timer_step(trace.total.total());
        }
    }

    pub fn read8(&self, address: u32) -> Result<u8, MemoryError> {
        self.record_access(address, AccessWidth::Byte);
        let value = self.read_byte(address)?;
        self.iwram_bus
            .access(address, AccessWidth::Byte, u32::from(value));
        Ok(value)
    }

    // Multi-byte reads use this helper to avoid charging each byte as a bus access.
    fn read_byte(&self, address: u32) -> Result<u8, MemoryError> {
        if self.cartridge_state().save_mapped(address)
            && self
                .cpu_access
                .is_some_and(|access| !matches!(access.pc >> 24, 0x02 | 0x03))
        {
            return Err(MemoryError::UnsupportedCartridgeAccess {
                address,
                operation: "Flash CPU read outside work RAM",
            });
        }
        if address >> 24 == 0x04 {
            if let Some(step) = self.timer_step.get() {
                if Serial::mapped(address) {
                    return step
                        .serial
                        .read8(address)
                        .ok_or(MemoryError::Unmapped(address));
                }
                if let Some(value) = step.read8(address, self.io.read8(address).unwrap_or(0)) {
                    return Ok(value);
                }
            }
        }
        if address < BIOS_SIZE as u32
            && self.bios.is_some()
            && self
                .cpu_access
                .is_some_and(|access| access.pc >= BIOS_SIZE as u32)
        {
            // Protect CPU data reads, not host inspection or BIOS instruction fetches.
            return self
                .bios_prefetch
                .map(|word| (word >> ((address & 3) * 8)) as u8)
                .ok_or(MemoryError::Unmapped(address));
        }
        if matches!(address, 0x0000_4000..=0x01ff_ffff | 0x1000_0000..=0xffff_ffff) {
            if let Some(word) = self.cpu_access.and_then(|access| access.prefetch) {
                return Ok((word >> ((address & 3) * 8)) as u8);
            }
        }
        self.read_mapped_byte(address)
    }

    // Strict lookup for host access, instruction fetches, and prefetch snapshots.
    fn read_mapped_byte(&self, address: u32) -> Result<u8, MemoryError> {
        let cartridge = self.cartridge_state();
        if cartridge.save_mapped(address) {
            return cartridge.read_save8(address, &self.save_image);
        }
        if let Some(value) = cartridge.read8(address) {
            return Ok(value);
        }
        match address >> 24 {
            0x00 => self
                .bios
                .as_ref()
                .and_then(|bios| bios.get(address as usize))
                .copied()
                .ok_or(MemoryError::Unmapped(address)),
            0x02 => Ok(self.external_ram[(address & 0x3ffff) as usize]),
            0x03 => Ok(self.internal_ram[(address & 0x7fff) as usize]),
            0x04 => self.io.read8(address).ok_or(MemoryError::Unmapped(address)),
            0x05 => Ok(self.palette_ram[(address & 0x3ff) as usize]),
            0x06 => Ok(self.video_ram[vram_index(address)]),
            0x07 => Ok(self.oam[(address & 0x3ff) as usize]),
            0x08..=0x0d => self
                .rom
                .get((address & 0x01ff_ffff) as usize)
                .copied()
                .ok_or(MemoryError::Unmapped(address)),
            _ => Err(MemoryError::Unmapped(address)),
        }
    }

    pub fn write8(&mut self, address: u32, value: u8) -> Result<(), MemoryError> {
        self.write_aligned(address, [value], self.write_access())
    }

    pub fn write16(&mut self, address: u32, value: u16) -> Result<(), MemoryError> {
        self.write_aligned(address, value.to_le_bytes(), self.write_access())
    }

    pub fn write32(&mut self, address: u32, value: u32) -> Result<(), MemoryError> {
        self.write_aligned(address, value.to_le_bytes(), self.write_access())
    }

    fn write_access(&self) -> WriteAccess {
        if self.cpu_access.is_some() {
            WriteAccess::Emulated
        } else {
            WriteAccess::Host
        }
    }

    // Validate the complete access before any RAM or I/O side effects.
    // Timer reload bytes precede control bytes in a combined 32-bit write.
    fn write_aligned<const N: usize>(
        &mut self,
        address: u32,
        bytes: [u8; N],
        access: WriteAccess,
    ) -> Result<(), MemoryError> {
        let index = self.write_index::<N>(address, access)?;
        Self::validate_io_values(&mut self.audio_state(), address, &bytes)?;
        self.record_access(
            address,
            match N {
                1 => AccessWidth::Byte,
                2 => AccessWidth::Halfword,
                4 => AccessWidth::Word,
                _ => unreachable!("unsupported bus width"),
            },
        );
        let mut cartridge = self.cartridge_state();
        cartridge.write(address, &bytes, &self.save_image)?;
        if cartridge.mapped(address) {
            if self.cartridge_step.get().is_some() {
                self.cartridge_step.set(Some(cartridge));
            } else {
                self.cartridge = cartridge;
            }
            return Ok(());
        }
        if address < BIOS_SIZE as u32 {
            // Only validated CPU/DMA writes reach here. Pay the bus cost but
            // never modify the mapped ROM image or drive BIOS read history.
            return Ok(());
        }
        if address >> 24 == 0x03 {
            let (width, value) = match N {
                1 => (AccessWidth::Byte, u32::from(bytes[0])),
                2 => (
                    AccessWidth::Halfword,
                    u32::from(u16::from_le_bytes([bytes[0], bytes[1]])),
                ),
                4 => (
                    AccessWidth::Word,
                    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
                ),
                _ => unreachable!("unsupported bus width"),
            };
            self.iwram_bus.access(address, width, value);
        }
        if address >> 24 == 0x04 {
            if Serial::mapped(address) {
                let mut serial = self.serial_state();
                Self::validate_serial(&serial, address, &bytes)?;
                serial.write(address, &bytes);
                if let Some(mut step) = self.timer_step.get() {
                    step.serial = serial;
                    self.timer_step.set(Some(step));
                } else {
                    self.io.serial = serial;
                }
                return Ok(());
            }
            if Audio::mapped(address) {
                if let Some(mut step) = self.timer_step.get() {
                    step.audio.write(address, &bytes);
                    self.timer_step.set(Some(step));
                } else {
                    self.io.audio.write(address, &bytes);
                }
                return Ok(());
            }
            if address & !3 == KEYINPUT {
                self.io.write_keypad(address, &bytes);
                return Ok(());
            }
            for (offset, byte) in bytes.into_iter().enumerate() {
                let byte_address = address + offset as u32;
                if matches!(byte_address, POSTFLG | HALTCNT) && !self.can_write_power_control() {
                    continue; // BIOS-only CPU writes; ignored elsewhere, including STOP requests.
                }
                if let Some(mut step) = self.timer_step.get() {
                    let handled = step.write8(byte_address, byte);
                    self.timer_step.set(Some(step));
                    if handled {
                        continue;
                    }
                }
                self.io.write8(byte_address, byte);
            }
            if address & !3 == crate::io::WAITCNT {
                // CPU changes are speculative with their timing transaction; host/DMA changes are immediate.
                if let Some(mut trace) = self.cpu_timing.get() {
                    trace.prefetch.configure(self.waitcnt());
                    self.cpu_timing.set(Some(trace));
                } else {
                    self.gamepak_prefetch.configure(self.waitcnt());
                }
            }
            return Ok(());
        }
        if N == 1 && address >> 24 == 0x07 {
            return Ok(()); // OAM ignores byte writes; halfword/word writes remain valid.
        }
        if N == 1 && matches!(address >> 24, 0x05 | 0x06) {
            let bg_limit = if (3..=5).contains(&(self.io.display_control() & 7)) {
                0x14000
            } else {
                0x10000
            };
            // Byte writes to OBJ VRAM are ignored. BG/palette writes duplicate
            // the byte into both halves of the aligned 16-bit location.
            if address >> 24 == 0x06 && index >= bg_limit {
                return Ok(());
            }
            let ram = if address >> 24 == 0x05 {
                &mut self.palette_ram
            } else {
                &mut self.video_ram
            };
            let aligned = index & !1;
            ram[aligned..aligned + 2].fill(bytes[0]);
            return Ok(());
        }
        let ram = match address >> 24 {
            0x02 => &mut self.external_ram,
            0x03 => &mut self.internal_ram,
            0x05 => &mut self.palette_ram,
            0x06 => &mut self.video_ram,
            0x07 => &mut self.oam,
            _ => unreachable!("validated writable region"),
        };
        ram[index..index + N].copy_from_slice(&bytes);
        Ok(())
    }

    fn write_index<const N: usize>(
        &self,
        address: u32,
        access: WriteAccess,
    ) -> Result<usize, MemoryError> {
        if address & (N as u32 - 1) != 0 {
            return Err(MemoryError::Unaligned(address));
        }
        match address >> 24 {
            0x00 if self.bios.is_some() && address < BIOS_SIZE as u32 => {
                if access == WriteAccess::Emulated {
                    Ok(0) // Mapped BIOS ignores emulated writes; host setup stays strict.
                } else {
                    Err(MemoryError::ReadOnly(address))
                }
            }
            0x02 => Ok((address & 0x3ffff) as usize),
            0x03 => Ok((address & 0x7fff) as usize),
            0x05 => Ok((address & 0x3ff) as usize),
            0x06 => Ok(vram_index(address)),
            0x07 => Ok((address & 0x3ff) as usize),
            0x04 => {
                for offset in 0..N as u32 {
                    if !Io::mapped(address + offset) {
                        return Err(MemoryError::Unmapped(address + offset));
                    }
                }
                Ok(0) // I/O does not use a RAM index.
            }
            0x08..=0x0d => {
                for offset in 0..N as u32 {
                    if !self.cartridge_state().mapped(address + offset) {
                        return Err(MemoryError::ReadOnly(address + offset));
                    }
                }
                Ok(0)
            }
            0x0e if self.cartridge_state().save_mapped(address) => {
                if N != 1 {
                    return Err(MemoryError::UnsupportedCartridgeAccess {
                        address,
                        operation: "Flash non-byte write",
                    });
                }
                Ok(0)
            }
            _ => Err(MemoryError::Unmapped(address)),
        }
    }

    fn validate_io_values(
        audio: &mut Audio,
        address: u32,
        bytes: &[u8],
    ) -> Result<(), MemoryError> {
        if address >> 24 == 0x04 {
            for (offset, &value) in bytes.iter().enumerate() {
                let address = address + offset as u32;
                if let Some(operation) = Io::unsupported_write(address, value)
                    .or_else(|| audio.unsupported(address, value))
                {
                    return Err(MemoryError::UnsupportedIo {
                        address,
                        value,
                        operation,
                    });
                }
            }
            if Audio::mapped(address) {
                audio.write(address, bytes);
            }
        }
        Ok(())
    }

    fn validate_serial(serial: &Serial, address: u32, bytes: &[u8]) -> Result<(), MemoryError> {
        if let Some((address, value, operation)) = serial.validate(address, bytes) {
            return Err(MemoryError::UnsupportedIo {
                address,
                value,
                operation,
            });
        }
        Ok(())
    }

    /// Validate a batch before any RAM or I/O writes. Diagnostic errors cannot
    /// leave a partial block store. This is not a model of hardware data aborts.
    /// Mapped I/O reads currently have no side effects; writes cannot fail after validation.
    pub(crate) fn write_words(&mut self, writes: &[(u32, u32)]) -> Result<(), MemoryError> {
        let mut audio = self.audio_state();
        let mut serial = self.serial_state();
        let mut cartridge = self.cartridge_state();
        let timing = self.cpu_timing.get();
        let devices = self.timer_step.get();
        let cartridge_step = self.cartridge_step.get();
        let cartridge_elapsed = self.cartridge_elapsed.get();
        // Preflight ordered data phases on temporary device/timing copies.
        // A serial request can finish before a later control write reaches the bus.
        let validation = (|| {
            for &(address, value) in writes {
                self.write_index::<4>(address, self.write_access())?;
                let bytes = value.to_le_bytes();
                Self::validate_io_values(&mut audio, address, &bytes)?;
                cartridge.write(address, &bytes, &self.save_image)?;
                self.record_access(address, AccessWidth::Word);
                if Serial::mapped(address) {
                    if let Some(mut step) = self.timer_step.get() {
                        Self::validate_serial(&step.serial, address, &bytes)?;
                        step.serial.write(address, &bytes);
                        self.timer_step.set(Some(step));
                    } else {
                        Self::validate_serial(&serial, address, &bytes)?;
                        serial.write(address, &bytes);
                    }
                }
            }
            Ok(())
        })();
        self.cpu_timing.set(timing);
        self.timer_step.set(devices);
        self.cartridge_step.set(cartridge_step);
        self.cartridge_elapsed.set(cartridge_elapsed);
        validation?;
        for &(address, value) in writes {
            // The map cannot change between validation and these writes.
            self.write32(address, value)?;
        }
        Ok(())
    }

    pub fn read16(&self, address: u32) -> Result<u16, MemoryError> {
        if address & 1 != 0 {
            return Err(MemoryError::Unaligned(address));
        }
        self.validate_save_read_width(address)?;
        self.record_access(address, AccessWidth::Halfword);
        let value = u16::from_le_bytes([self.read_byte(address)?, self.read_byte(address + 1)?]);
        self.iwram_bus
            .access(address, AccessWidth::Halfword, u32::from(value));
        Ok(value)
    }

    fn validate_save_read_width(&self, address: u32) -> Result<(), MemoryError> {
        if self.cartridge_state().save_mapped(address) {
            return Err(MemoryError::UnsupportedCartridgeAccess {
                address,
                operation: "Flash non-byte read",
            });
        }
        Ok(())
    }

    /// The bus requires alignment; the CPU applies ARM7TDMI load rotation.
    pub fn read32(&self, address: u32) -> Result<u32, MemoryError> {
        if address & 3 != 0 {
            return Err(MemoryError::Unaligned(address));
        }
        self.validate_save_read_width(address)?;
        self.record_access(address, AccessWidth::Word);
        let value = u32::from_le_bytes([
            self.read_byte(address)?,
            self.read_byte(address + 1)?,
            self.read_byte(address + 2)?,
            self.read_byte(address + 3)?,
        ]);
        self.iwram_bus.access(address, AccessWidth::Word, value);
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        display::CYCLES_PER_LINE,
        io::{DISPSTAT, IF},
    };

    #[test]
    fn affine_state_tracks_without_capture_and_survives_failed_batch_writes() {
        use crate::io::{BG2PA, BG2PB, BG2PD, BG2X, DISPCNT};
        let mut bulk = Memory::new(vec![]).unwrap();
        let mut captured = Memory::new(vec![]).unwrap();
        captured.set_scanline_rendering(true);
        for m in [&mut bulk, &mut captured] {
            m.write16(DISPCNT, 0x403).unwrap();
            m.write16(BG2PA, 256).unwrap();
            m.write16(BG2PB, (-181i16) as u16).unwrap();
            m.write16(BG2PD, 91).unwrap();
            m.write32(BG2X, 12345).unwrap();
        }
        for cycles in [0, 1006, 226, 7319, 280896, 1232 * 317] {
            for m in [&mut bulk, &mut captured] {
                m.advance_cycles(cycles);
            }
            assert_eq!(
                bulk.io.video_settings(true).backgrounds[2].affine,
                captured.io.video_settings(true).backgrounds[2].affine
            );
        }
        let before = bulk.io.video_settings(true).backgrounds[2].affine;
        assert!(bulk.write_words(&[(BG2X, 0), (0x0400_0058, 0)]).is_err());
        assert_eq!(bulk.io.video_settings(true).backgrounds[2].affine, before);
        bulk.set_scanline_rendering(true);
        bulk.set_scanline_rendering(false);
        assert_eq!(bulk.io.video_settings(true).backgrounds[2].affine, before);
    }

    #[test]
    fn window_flags_match_batched_and_captured_clocks_and_survive_failed_writes() {
        use crate::io::{WIN0V, WIN1V};
        let mut bulk = Memory::new(vec![]).unwrap();
        let mut captured = Memory::new(vec![]).unwrap();
        captured.set_scanline_rendering(true);
        for (cycles, first, second) in [
            (1006, 0x0010, 0xc808),
            (226, 0x0010, 0xc808),
            (1232 * 5, 0x0508, 0x0014),
            (280896 * 3, 0xffff, 0xc808),
            (1232 * 500 + 100, 0x00ff, 0xff00),
        ] {
            for m in [&mut bulk, &mut captured] {
                m.write16(WIN0V, first).unwrap();
                m.write16(WIN1V, second).unwrap();
                m.advance_cycles(cycles);
            }
            assert_eq!(
                bulk.io.video_settings(true).vertical_windows,
                captured.io.video_settings(true).vertical_windows
            );
        }
        let before = bulk.io.clone();
        assert!(bulk.write_words(&[(WIN0V, 0), (0x0400_0058, 0)]).is_err());
        assert_eq!(bulk.io, before);
        bulk.set_scanline_rendering(true);
        bulk.set_scanline_rendering(false);
        assert_eq!(bulk.io, before);
    }

    #[test]
    fn mosaic_phase_matches_batched_capture_and_survives_writes_and_capture_toggles() {
        use crate::io::{BG2CNT, BG2PD, DISPCNT, MOSAIC};
        let mut bulk = Memory::new(vec![]).unwrap();
        let mut captured = Memory::new(vec![]).unwrap();
        captured.set_scanline_rendering(true);
        for m in [&mut bulk, &mut captured] {
            m.write16(DISPCNT, 0x403).unwrap();
            m.write16(BG2CNT, 0x40).unwrap();
            m.write16(BG2PD, 91).unwrap();
        }
        for (cycles, sizes) in [
            (6 * CYCLES_PER_LINE, 0x7070),
            (9 * CYCLES_PER_LINE, 0x1010),
            (1006, 0x3010),
            (226, 0x0030),
            (280896 * 3 + 1232 * 5, 0x9030),
        ] {
            for m in [&mut bulk, &mut captured] {
                m.write16(MOSAIC, sizes).unwrap();
                m.advance_cycles(cycles);
            }
            assert_eq!(bulk.io, captured.io);
        }
        let before = bulk.io.clone();
        assert!(bulk.write_words(&[(MOSAIC, 0), (0x0400_0058, 0)]).is_err());
        assert_eq!(bulk.io, before);
        bulk.set_scanline_rendering(true);
        bulk.set_scanline_rendering(false);
        assert_eq!(bulk.io, before);
        let counters = bulk.io.video_settings(true).vertical_mosaic;
        bulk.write16(MOSAIC, 0).unwrap();
        bulk.advance_cycles(0);
        assert_eq!(bulk.io.video_settings(true).vertical_mosaic, counters);
    }

    #[test]
    fn horizontal_history_matches_capture_clock_splits_and_survives_failed_writes() {
        use crate::io::WIN0H;
        let mut bulk = Memory::new(vec![]).unwrap();
        let mut captured = Memory::new(vec![]).unwrap();
        captured.set_scanline_rendering(true);
        for (cycles, bounds) in [
            (81, 0x0a50),
            (3, 0x1450),
            (922, 0x2828),
            (14, 0xff0a),
            (1, 0xff0a),
            (211, 0xff0a),
            (280896 * 3 + 100, 0xf014),
        ] {
            for m in [&mut bulk, &mut captured] {
                m.write32(WIN0H, bounds | ((bounds ^ 0x371b) << 16))
                    .unwrap();
                m.advance_cycles(cycles);
            }
            assert_eq!(bulk.io, captured.io);
        }
        let before = bulk.io.clone();
        assert!(bulk.write_words(&[(WIN0H, 0), (0x0400_0058, 0)]).is_err());
        assert_eq!(bulk.io, before);
        bulk.advance_cycles(0);
        bulk.set_scanline_rendering(true);
        bulk.set_scanline_rendering(false);
        let mut snapshot = Framebuffer::default();
        bulk.render_frame(&mut snapshot).unwrap();
        assert_eq!(bulk.io, before);
    }

    #[test]
    fn failed_batch_does_not_change_comparison_or_latch_match_irq() {
        let mut bus = Memory::new(vec![]).unwrap();
        bus.advance_cycles(7 * CYCLES_PER_LINE);
        let before = bus.display_position();
        // This batch is no longer reachable with one CPU STM: the mapped
        // display register area now exceeds its sixteen-word transfer limit.
        assert_eq!(
            bus.write_words(&[(DISPSTAT, 0x720), (0x0400_0058, 0)]),
            Err(MemoryError::Unmapped(0x0400_0058))
        );
        assert_eq!(bus.display_position(), before);
        assert_eq!(bus.read16(DISPSTAT).unwrap(), 0);
        assert_eq!(bus.read16(IF).unwrap(), 0);
    }
}
