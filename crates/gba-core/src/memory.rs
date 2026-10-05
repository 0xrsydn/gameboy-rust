use std::{cell::Cell, error::Error, fmt};

mod iwram_bus;
use iwram_bus::IwramBus;

use crate::{
    cpu::InstructionSet,
    display::{DisplayPosition, CYCLES_PER_LINE, HBLANK_START, VISIBLE_LINES},
    dma::{DmaError, DMA_BASE, DMA_END},
    input::Buttons,
    io::{Io, HALTCNT, KEYINPUT, POSTFLG},
    timing::{bus_cycles, AccessKind, AccessWidth, DataTiming, StepTiming},
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
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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

#[derive(Clone, Copy)]
struct CpuAccess {
    pc: u32,
    prefetch: Option<u32>,
}

/// Work/video/palette RAM, OAM, cartridge ROM, optional BIOS, and supported I/O.
/// Unused-memory reads use ARM PC+8 or supported region-dependent Thumb snapshots.
/// Protected BIOS reads separately retain the snapshot from BIOS execution.
/// Sequential Thumb IWRAM accesses retain addressed bus lanes transactionally.
/// Successful refills into Thumb IWRAM sample the target pair to establish history.
/// DMA accesses drive existing Thumb IWRAM continuation lanes before the next fetch.
/// Channel data remains separate; general and sub-instruction CPU bus handoff is unmodeled.
/// A full fetch pipeline and ARM-target refill history are not modeled.
pub struct Memory {
    external_ram: Vec<u8>,
    internal_ram: Vec<u8>,
    palette_ram: Vec<u8>,
    video_ram: Vec<u8>,
    oam: Vec<u8>,
    rom: Vec<u8>,
    bios: Option<Vec<u8>>,
    io: Io,
    cycles: u64,
    data_timing: Cell<Option<DataTiming>>,
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
            bios: None,
            io: Io::default(),
            cycles: 0,
            data_timing: Cell::new(None),
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

    /// Total elapsed emulated clock cycles, wrapping at u64::MAX. Reads/writes
    /// alone consume no cycles. STOP discards externally supplied clock advances.
    pub fn cycles(&self) -> u64 {
        self.cycles
    }

    /// Advance devices and optional row capture. Multiple unserviced DMA requests
    /// coalesce. No CPU or DMA work is executed by this method. STOP freezes all
    /// progress: supplied cycles are ignored until an external wake condition.
    pub fn advance_cycles(&mut self, cycles: u32) {
        if !self.stopped() {
            self.advance_running_cycles(cycles);
        }
    }

    /// Finish the instruction/IRQ entry that started while running. A STOP store
    /// still pays its full nominal cost before later steps freeze the system.
    pub(crate) fn complete_cpu_step(&mut self, cycles: u32) {
        self.advance_running_cycles(cycles);
    }

    fn advance_running_cycles(&mut self, cycles: u32) {
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
                self.io.advance(offset - advanced);
                self.prepare_sprite_row(row);
                advanced = offset;
            }
            self.io.advance(cycles - advanced);
        } else {
            // Split at display and sprite-preparation edges. CPU/DMA writes have
            // already committed before these nominal clocks; no per-access timing.
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
                self.io.advance(step);
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
            AccessWidth::Halfword => self.write_index::<2>(transfer.destination),
            AccessWidth::Word => self.write_index::<4>(transfer.destination),
            AccessWidth::Byte => unreachable!("DMA is never byte-wide"),
        }
        .map_err(map_error)?;
        let data_latch = if transfer.source < 0x0200_0000 {
            // DMA cannot read BIOS or the lower unused region. Retain this channel's
            // full word; never read firmware bytes or borrow CPU/another channel's data.
            transfer
                .data_latch
                .expect("unknown blocked source rejected above")
        } else {
            match transfer.width {
                AccessWidth::Halfword => {
                    u32::from(self.read16(transfer.source).map_err(map_error)?) * 0x0001_0001
                }
                AccessWidth::Word => self.read32(transfer.source).map_err(map_error)?,
                AccessWidth::Byte => unreachable!("DMA is never byte-wide"),
            }
        };
        // A blocked halfword read keeps both old lanes. The destination selects
        // one lane; mapped halfword reads have already duplicated their source data.
        let value = match transfer.width {
            AccessWidth::Halfword => data_latch >> ((transfer.destination & 2) * 8),
            AccessWidth::Word => data_latch,
            AccessWidth::Byte => unreachable!("DMA is never byte-wide"),
        };
        let kind = if transfer.first {
            AccessKind::NonSequential
        } else {
            AccessKind::Sequential
        };
        let timing = StepTiming {
            code_cycles: 0,
            data_cycles: bus_cycles(self.waitcnt(), transfer.source, transfer.width, kind)
                + bus_cycles(self.waitcnt(), transfer.destination, transfer.width, kind),
            internal_cycles: if transfer.first { 2 } else { 0 },
            idle_cycles: 0,
        };
        match transfer.width {
            AccessWidth::Halfword => self.write16(transfer.destination, value as u16),
            AccessWidth::Word => self.write32(transfer.destination, value),
            AccessWidth::Byte => unreachable!("DMA is never byte-wide"),
        }
        .map_err(map_error)?;
        // DMA runs before the resumed CPU fetch. Only actual IWRAM accesses drive
        // its local lanes, in read/write order. Channel halfword duplication is not
        // a full IWRAM bus write. Failed units never reach these history commits.
        self.iwram_bus
            .dma_access(transfer.source, transfer.width, data_latch);
        self.iwram_bus
            .dma_access(transfer.destination, transfer.width, value);
        // Keep this channel active during its cycles: another edge must not
        // queue a second block while the current block is still transferring.
        self.advance_cycles(timing.total());
        self.io.complete_dma_unit(channel, data_latch);
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

    pub(crate) fn begin_cpu_access(&mut self, pc: u32, instruction_set: InstructionSet) {
        debug_assert!(self.cpu_access.is_none());
        // Sample before execution, without data timing or open-bus recursion.
        // Missing lookahead bytes must not fail an instruction that never uses them.
        let iwram_pc = (instruction_set == InstructionSet::Thumb && pc >> 24 == 3).then_some(pc);
        let fetched = iwram_pc
            .filter(|pc| pc.wrapping_add(4) >> 24 == 3)
            .and_then(|pc| self.snapshot_mapped_halfword(pc + 4));
        self.iwram_bus.begin(iwram_pc, fetched);
        let prefetch = match instruction_set {
            InstructionSet::Arm => self.snapshot_mapped_word(pc.wrapping_add(8)),
            InstructionSet::Thumb => self.snapshot_thumb_prefetch(pc),
        };
        self.cpu_access = Some(CpuAccess { pc, prefetch });
    }

    fn snapshot_thumb_prefetch(&self, pc: u32) -> Option<u32> {
        let address = pc.wrapping_add(4);
        // Region-crossing fetches need pipeline history, not a rule chosen from the old PC.
        if address >> 24 != pc >> 24 {
            return None;
        }
        match pc >> 24 {
            // BIOS and OAM drive a full word even for a halfword instruction fetch.
            0x00 | 0x07 => self.snapshot_mapped_word(address & !3),
            // The IWRAM halfword fetch replaces one lane of its retained bus word.
            0x03 => self.iwram_bus.snapshot(),
            // These 16-bit regions repeat the fetched halfword in both word lanes.
            0x02 | 0x05 | 0x06 | 0x08..=0x0d => {
                Some(u32::from(self.snapshot_mapped_halfword(address)?) * 0x0001_0001)
            }
            // Unsupported code regions must not manufacture an open-bus snapshot.
            _ => None,
        }
    }

    fn snapshot_mapped_halfword(&self, address: u32) -> Option<u16> {
        Some(u16::from_le_bytes([
            self.read_mapped_byte(address).ok()?,
            self.read_mapped_byte(address + 1).ok()?,
        ]))
    }

    fn snapshot_mapped_word(&self, address: u32) -> Option<u32> {
        let mut bytes = [0; 4];
        for (offset, byte) in bytes.iter_mut().enumerate() {
            *byte = self
                .read_mapped_byte(address.wrapping_add(offset as u32))
                .ok()?;
        }
        Some(u32::from_le_bytes(bytes))
    }

    pub(crate) fn end_cpu_access(&mut self, succeeded: bool, sequential: bool) {
        self.iwram_bus.finish(succeeded, sequential);
        if let Some(access) = self.cpu_access.take() {
            if succeeded && access.pc < BIOS_SIZE as u32 {
                // Commit only successful instruction snapshots under our diagnostic policy.
                // Missing lookahead invalidates history rather than exposing stale bytes.
                self.bios_prefetch = access.prefetch;
            }
        }
    }

    /// Sample only the two target fetches of a completed refill into Thumb IWRAM.
    /// This updates bus history, not the CPU's instruction buffer or nominal costs.
    /// Incomplete or unsupported refills stay unknown without failing the branch early.
    pub(crate) fn refill_cpu_bus_history(&mut self, pc: u32, instruction_set: InstructionSet) {
        self.iwram_bus.invalidate();
        if instruction_set == InstructionSet::Thumb
            && pc >> 24 == 3
            && pc.wrapping_add(2) >> 24 == 3
        {
            if let (Some(first), Some(second)) = (
                self.snapshot_mapped_halfword(pc),
                self.snapshot_mapped_halfword(pc + 2),
            ) {
                self.iwram_bus.refill(pc, [first, second]);
            }
        }
    }

    pub(crate) fn invalidate_cpu_bus_history(&mut self) {
        self.iwram_bus.invalidate();
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

    /// Current Game Pak wait-state control. Prefetch is not implemented.
    pub fn waitcnt(&self) -> u16 {
        self.io.waitcnt()
    }

    pub(crate) fn begin_data_timing(&self) {
        debug_assert!(self.data_timing.get().is_none());
        self.data_timing.set(Some(DataTiming::default()));
    }

    pub(crate) fn end_data_timing(&self) -> u32 {
        self.data_timing.take().map_or(0, |trace| trace.cycles)
    }

    fn record_access(&self, address: u32, width: AccessWidth) {
        if let Some(mut trace) = self.data_timing.get() {
            trace.access(self.waitcnt(), address, width);
            self.data_timing.set(Some(trace));
        }
    }

    pub fn read8(&self, address: u32) -> Result<u8, MemoryError> {
        let value = self.read_byte(address)?;
        self.record_access(address, AccessWidth::Byte);
        self.iwram_bus
            .access(address, AccessWidth::Byte, u32::from(value));
        Ok(value)
    }

    // Multi-byte reads use this helper to avoid charging each byte as a bus access.
    fn read_byte(&self, address: u32) -> Result<u8, MemoryError> {
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
        self.write_aligned(address, [value])
    }

    pub fn write16(&mut self, address: u32, value: u16) -> Result<(), MemoryError> {
        self.write_aligned(address, value.to_le_bytes())
    }

    pub fn write32(&mut self, address: u32, value: u32) -> Result<(), MemoryError> {
        self.write_aligned(address, value.to_le_bytes())
    }

    // Validate the complete access before any RAM or I/O side effects.
    // Timer reload bytes precede control bytes in a combined 32-bit write.
    fn write_aligned<const N: usize>(
        &mut self,
        address: u32,
        bytes: [u8; N],
    ) -> Result<(), MemoryError> {
        let index = self.write_index::<N>(address)?;
        self.record_access(
            address,
            match N {
                1 => AccessWidth::Byte,
                2 => AccessWidth::Halfword,
                4 => AccessWidth::Word,
                _ => unreachable!("unsupported bus width"),
            },
        );
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
            if address & !3 == KEYINPUT {
                self.io.write_keypad(address, &bytes);
                return Ok(());
            }
            for (offset, byte) in bytes.into_iter().enumerate() {
                let byte_address = address + offset as u32;
                if matches!(byte_address, POSTFLG | HALTCNT) && !self.can_write_power_control() {
                    continue; // BIOS-only CPU writes; ignored elsewhere, including STOP requests.
                }
                self.io.write8(byte_address, byte);
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

    fn write_index<const N: usize>(&self, address: u32) -> Result<usize, MemoryError> {
        if address & (N as u32 - 1) != 0 {
            return Err(MemoryError::Unaligned(address));
        }
        match address >> 24 {
            0x00 if self.bios.is_some() && address < BIOS_SIZE as u32 => {
                Err(MemoryError::ReadOnly(address))
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
            0x08..=0x0d => Err(MemoryError::ReadOnly(address)),
            _ => Err(MemoryError::Unmapped(address)),
        }
    }

    /// Validate a batch before any RAM or I/O writes. Diagnostic errors cannot
    /// leave a partial block store. This is not a model of hardware data aborts.
    /// Mapped I/O reads currently have no side effects; writes cannot fail after validation.
    pub(crate) fn write_words(&mut self, writes: &[(u32, u32)]) -> Result<(), MemoryError> {
        for &(address, _) in writes {
            self.write_index::<4>(address)?;
        }
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
        let value = u16::from_le_bytes([self.read_byte(address)?, self.read_byte(address + 1)?]);
        self.record_access(address, AccessWidth::Halfword);
        self.iwram_bus
            .access(address, AccessWidth::Halfword, u32::from(value));
        Ok(value)
    }

    /// The bus requires alignment; the CPU applies ARM7TDMI load rotation.
    pub fn read32(&self, address: u32) -> Result<u32, MemoryError> {
        if address & 3 != 0 {
            return Err(MemoryError::Unaligned(address));
        }
        let value = u32::from_le_bytes([
            self.read_byte(address)?,
            self.read_byte(address + 1)?,
            self.read_byte(address + 2)?,
            self.read_byte(address + 3)?,
        ]);
        self.record_access(address, AccessWidth::Word);
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
