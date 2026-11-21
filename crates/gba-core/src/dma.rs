//! Four-channel GBA direct memory access (DMA) controller.
//! Transfers are scheduled one data unit at a time. Startup/resumption delays,
//! CPU bus handoff, and Game Pak DRQ are not modeled.
//! Special timing serves the sound FIFOs on DMA1/2 and video capture on DMA3.
//! Each channel retains known source data across blocks, including blocked reads.

use std::{error::Error, fmt};

use crate::snapshot::{Reader, Snapshot, SnapshotError, Writer};
use crate::{display::DmaEvents, memory::MemoryError, timing::AccessWidth};

pub const DMA_BASE: u32 = 0x0400_00b0;
pub const DMA_STRIDE: u32 = 12;
pub(crate) const DMA_END: u32 = DMA_BASE + 4 * DMA_STRIDE - 1;
/// The last scanline whose HBlank requests a DMA3 video-capture block.
const LAST_CAPTURE_LINE: u16 = 161;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DmaError {
    Memory { channel: usize, error: MemoryError },
    RegisterDestination { channel: usize, address: u32 },
    PowerControlDestination { channel: usize, address: u32 },
}

impl fmt::Display for DmaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Memory { channel, error } => write!(f, "DMA{channel}: {error}"),
            Self::PowerControlDestination { channel, address } => {
                write!(
                    f,
                    "DMA{channel}: power-control writes are not supported ({address:#010x})"
                )
            }
            Self::RegisterDestination { channel, address } => {
                write!(
                    f,
                    "DMA{channel}: writes to DMA registers are not supported ({address:#010x})"
                )
            }
        }
    }
}

impl Error for DmaError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Memory { error, .. } => Some(error),
            _ => None,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Channel {
    source: u32,
    destination: u32,
    count: u16,
    control: u16,
    current_source: u32,
    current_destination: u32,
    remaining: u32,
    active: bool,
    // Cycles before an immediate transfer enabled by the CPU may start; see START_DELAY.
    start_delay: u8,
    first: bool,
    // Unknown at synthetic startup. Enable/disable does not reset retained data.
    data_latch: Option<u32>,
}

impl Channel {
    fn enabled(&self) -> bool {
        self.control & 0x8000 != 0
    }

    fn start_timing(&self) -> u16 {
        (self.control >> 12) & 3
    }

    /// DMA1/2 special timing feeds a sound FIFO: four words to a fixed destination.
    /// As in mGBA 0.10.5, Game Pak DRQ is ignored (the transfer keeps its programmed
    /// timing), prohibited source mode 3 increments, and DMA1/2 special timing serves
    /// whichever FIFO its destination names.
    fn sound(&self, index: usize) -> bool {
        self.start_timing() == 3 && (index == 1 || index == 2)
    }

    fn width(&self, index: usize) -> AccessWidth {
        if self.sound(index) || self.control & 0x400 != 0 {
            AccessWidth::Word
        } else {
            AccessWidth::Halfword
        }
    }

    fn initial_count(&self, index: usize) -> u32 {
        if self.sound(index) {
            4
        } else if self.count != 0 {
            u32::from(self.count)
        } else if index == 3 {
            0x1_0000
        } else {
            0x4000
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Dma {
    channels: [Channel; 4],
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Transfer {
    pub channel: usize,
    pub source: u32,
    pub destination: u32,
    pub width: AccessWidth,
    pub first: bool,
    pub data_latch: Option<u32>,
    /// Units left in this block, including this one.
    pub remaining: u32,
}

impl Dma {
    /// Address/count registers are write-only. Zero is a diagnostic placeholder,
    /// not hardware open-bus behavior. Only CNT_H has meaningful readback.
    pub(crate) fn read8(&self, address: u32) -> u8 {
        let offset = address - DMA_BASE;
        let channel = &self.channels[(offset / DMA_STRIDE) as usize];
        match offset % DMA_STRIDE {
            10 => channel.control as u8,
            11 => (channel.control >> 8) as u8,
            _ => 0,
        }
    }

    pub(crate) fn write8(&mut self, address: u32, value: u8) {
        let offset = address - DMA_BASE;
        let index = (offset / DMA_STRIDE) as usize;
        let channel = &mut self.channels[index];
        let register_byte = offset % DMA_STRIDE;
        match register_byte {
            0..=3 => {
                channel.source = replace_byte(channel.source, register_byte, value)
                    & if index == 0 { 0x07ff_fffe } else { 0x0fff_fffe };
            }
            4..=7 => {
                channel.destination = replace_byte(channel.destination, register_byte - 4, value)
                    & if index == 3 { 0x0fff_fffe } else { 0x07ff_fffe };
            }
            8..=9 => {
                channel.count = replace_byte(u32::from(channel.count), register_byte - 8, value)
                    as u16
                    & if index == 3 { 0xffff } else { 0x3fff };
            }
            10..=11 => {
                let was_enabled = channel.enabled();
                channel.control =
                    replace_byte(u32::from(channel.control), register_byte - 10, value) as u16
                        & if index == 3 { 0xffe0 } else { 0xf7e0 };
                if !channel.enabled() {
                    channel.active = false;
                    channel.start_delay = 0; // A cancelled transfer has no start to wait for.
                } else if !was_enabled {
                    let mask = !(channel.width(index).bytes() - 1);
                    channel.current_source = channel.source & mask;
                    channel.current_destination = channel.destination & mask;
                    channel.remaining = channel.initial_count(index);
                    channel.first = true;
                    channel.active = channel.start_timing() == 0;
                    channel.start_delay = if channel.active { START_DELAY } else { 0 };
                }
            }
            _ => unreachable!("DMA register byte"),
        }
    }

    /// Blanking requests are edges, not queued jobs. An active channel ignores
    /// further edges until its current block completes. Each delay is what remains of
    /// START_DELAY at the end of the clock batch that crossed that edge.
    pub(crate) fn trigger(&mut self, events: DmaEvents, vblank_delay: u8, hblank_delay: u8) {
        for (index, channel) in self.channels.iter_mut().enumerate() {
            if !channel.enabled() || channel.active {
                continue;
            }
            let delay = match channel.start_timing() {
                1 if events.vblank => vblank_delay,
                2 if events.hblank => hblank_delay,
                // DMA3 video capture starts at the HBlank of lines 2-161, as mGBA 0.10.5
                // does (`GBADMARunDisplayStart` from `_startHblank`); not hardware-verified.
                3 if events.capture && index == 3 => hblank_delay,
                _ => continue,
            };
            channel.active = true;
            channel.start_delay = delay;
        }
    }

    /// A FIFO request starts DMA1 or DMA2 when its destination is that FIFO, as in mGBA
    /// 0.10.5. A special-timing channel aimed elsewhere is never requested.
    pub(crate) fn trigger_sound(&mut self, requests: [bool; 2]) {
        for channel in &mut self.channels[1..=2] {
            let requested = (0..2).any(|fifo| {
                requests[fifo] && channel.destination == crate::audio::FIFO_A + fifo as u32 * 4
            });
            if requested && channel.enabled() && channel.start_timing() == 3 && !channel.active {
                channel.active = true;
            }
        }
    }

    /// Select the highest-priority ready channel without changing its state.
    /// DMA0 special timing never triggers, as in mGBA.
    pub(crate) fn next(&self) -> Option<Transfer> {
        self.channels
            .iter()
            .enumerate()
            .find(|(_, channel)| channel.enabled() && channel.active && channel.start_delay == 0)
            .map(|(index, channel)| Transfer {
                channel: index,
                source: channel.current_source,
                destination: channel.current_destination,
                width: channel.width(index),
                first: channel.first,
                data_latch: channel.data_latch,
                remaining: channel.remaining,
            })
    }

    /// Commit one successful data unit after its bus cycles. Return completion IRQ bits.
    /// `line` is the current scanline: a video-capture block that ends on line 161
    /// disables DMA3, as mGBA 0.10.5's `_dmaEvent` does.
    pub(crate) fn complete_unit(&mut self, index: usize, data_latch: u32, line: u16) -> u16 {
        let channel = &mut self.channels[index];
        channel.data_latch = Some(data_latch);
        let width = channel.width(index).bytes();
        // The Game Pak source counter increments regardless of source control.
        let source_mode = if (0x08..=0x0d).contains(&(channel.current_source >> 24)) {
            0
        } else {
            (channel.control >> 7) & 3
        };
        channel.current_source = advance_address(channel.current_source, width, source_mode);
        channel.current_destination = advance_address(
            channel.current_destination,
            width,
            if channel.sound(index) {
                2
            } else {
                (channel.control >> 5) & 3
            },
        );
        channel.remaining -= 1;
        channel.first = false;
        if channel.remaining != 0 {
            return 0;
        }
        channel.active = false;
        let capture_done = index == 3 && channel.start_timing() == 3 && line == LAST_CAPTURE_LINE;
        if channel.control & 0x200 != 0 && channel.start_timing() != 0 && !capture_done {
            channel.remaining = channel.initial_count(index);
            channel.first = true;
            if !channel.sound(index) && channel.control & 0x60 == 0x60 {
                channel.current_destination = channel.destination & !(width - 1);
            }
        } else {
            channel.control &= !0x8000;
        }
        if channel.control & 0x4000 != 0 {
            1 << (8 + index)
        } else {
            0
        }
    }
}

fn advance_address(address: u32, width: u32, mode: u16) -> u32 {
    match mode {
        0 | 3 => address.wrapping_add(width),
        1 => address.wrapping_sub(width),
        2 => address,
        _ => unreachable!("two-bit address control"),
    }
}

fn replace_byte(previous: u32, byte: u32, value: u8) -> u32 {
    let shift = byte * 8;
    (previous & !(0xff << shift)) | (u32::from(value) << shift)
}

/// VBlank and HBlank transfers become ready this many cycles after their display edge, as
/// mGBA schedules them. It also bounds every pending delay in a save state.
pub const START_DELAY: u8 = 3;

/// An immediate transfer enabled by a CPU store becomes ready this many cycles after the
/// write ends, and starts before the first CPU access or internal cycle that begins from
/// then on, as Mesen 2 schedules it. `Machine::step` applies this at instruction
/// boundaries; see [`Dma::start_within`].
pub const CPU_START_DELAY: u8 = 2;

impl Dma {
    /// Count down pending start delays as cycles elapse.
    pub(crate) fn elapse(&mut self, cycles: u32) {
        for channel in &mut self.channels {
            channel.start_delay = channel
                .start_delay
                .saturating_sub(cycles.min(u32::from(u8::MAX)) as u8);
        }
    }

    /// Channels waiting for their trigger or start delay, as a bit mask, so that a CPU store
    /// can find the transfers it enables.
    pub(crate) fn active_mask(&self) -> u8 {
        self.channels
            .iter()
            .enumerate()
            .filter(|(_, channel)| channel.active)
            .fold(0, |mask, (index, _)| mask | 1 << index)
    }

    /// Date the immediate transfers that a CPU store just enabled (active now, not in
    /// `before`): ready `CPU_START_DELAY` cycles after the write ends, `write_end` cycles
    /// into the step. Step completion then counts the whole step down.
    pub(crate) fn date_cpu_enables(&mut self, before: u8, write_end: u32) {
        let delay = (write_end + u32::from(CPU_START_DELAY)).min(u32::from(u8::MAX)) as u8;
        for (index, channel) in self.channels.iter_mut().enumerate() {
            if channel.active && before & 1 << index == 0 && channel.start_delay != 0 {
                channel.start_delay = delay;
            }
        }
    }

    /// Cycles until the nearest waiting immediate transfer becomes ready.
    pub(crate) fn immediate_start_wait(&self) -> Option<u32> {
        self.channels
            .iter()
            .filter(|channel| {
                channel.active && channel.start_timing() == 0 && channel.start_delay != 0
            })
            .map(|channel| u32::from(channel.start_delay))
            .min()
    }

    /// Start the immediate transfers that become ready within `cycles`: the next
    /// instruction's code fetch takes that long and is followed by a bus access or internal
    /// cycle, which the transfer precedes. Transfers run between instructions here, so it
    /// starts before that instruction instead of inside it; the data the instruction then
    /// reads is the same. Blanking transfers keep mGBA's boundary rule (`START_DELAY`).
    pub(crate) fn start_within(&mut self, cycles: u32) {
        for channel in &mut self.channels {
            if channel.active
                && channel.start_timing() == 0
                && u32::from(channel.start_delay) <= cycles
            {
                channel.start_delay = 0;
            }
        }
    }

    /// Host (and DMA) writes start transfers at once; only CPU stores pay the delay.
    pub(crate) fn expedite(&mut self) {
        for channel in &mut self.channels {
            channel.start_delay = 0;
        }
    }

    /// Cycles until a delayed transfer may start, to bound HALT idling.
    /// Whether a transfer would run at the next step boundary.
    pub(crate) fn ready(&self) -> bool {
        self.channels
            .iter()
            .any(|channel| channel.enabled() && channel.active && channel.start_delay == 0)
    }

    pub(crate) fn next_start_cycles(&self) -> Option<u32> {
        self.channels
            .iter()
            .filter(|channel| channel.active && channel.start_delay != 0)
            .map(|channel| u32::from(channel.start_delay))
            .min()
    }
}

fn save_channel(channel: &Channel, writer: &mut Writer) {
    let Channel {
        source,
        destination,
        count,
        control,
        current_source,
        current_destination,
        remaining,
        active,
        start_delay,
        first,
        data_latch,
    } = channel;
    writer.u32(*source);
    writer.u32(*destination);
    writer.u16(*count);
    writer.u16(*control);
    writer.u32(*current_source);
    writer.u32(*current_destination);
    writer.u32(*remaining);
    writer.bool(*active);
    writer.u8(*start_delay);
    writer.bool(*first);
    writer.option(data_latch.as_ref(), |value, writer| writer.u32(*value));
}

fn load_channel(
    channel: &mut Channel,
    index: usize,
    reader: &mut Reader<'_>,
) -> Result<(), SnapshotError> {
    let Channel {
        source,
        destination,
        count,
        control,
        current_source,
        current_destination,
        remaining,
        active,
        start_delay,
        first,
        data_latch,
    } = channel;
    *source = reader.u32()?;
    *destination = reader.u32()?;
    *count = reader.u16()?;
    *control = reader.u16()?;
    *current_source = reader.u32()?;
    *current_destination = reader.u32()?;
    *remaining = reader.u32()?;
    *active = reader.bool()?;
    *start_delay = reader.u8()?;
    *first = reader.bool()?;
    *data_latch = reader.option(Reader::u32)?;

    let source_mask = if index == 0 { 0x07ff_fffe } else { 0x0fff_fffe };
    let destination_mask = if index == 3 { 0x0fff_fffe } else { 0x07ff_fffe };
    let count_mask = if index == 3 { 0xffff } else { 0x3fff };
    let control_mask = if index == 3 { 0xffe0 } else { 0xf7e0 };
    let max_remaining = if index == 3 { 0x1_0000 } else { 0x4000 };
    if *source & !source_mask != 0
        || *destination & !destination_mask != 0
        || *count & !count_mask != 0
        || *control & !control_mask != 0
        || *remaining > max_remaining
        || (*active && (*remaining == 0 || *control & 0x8000 == 0))
        || *start_delay > START_DELAY
        || (*start_delay != 0 && !*active)
    {
        return Err(SnapshotError::Corrupt("DMA channel"));
    }
    Ok(())
}

impl Snapshot for Dma {
    fn save(&self, writer: &mut Writer) {
        let Self { channels } = self;
        writer.tag(b"DMA0");
        for channel in channels {
            save_channel(channel, writer);
        }
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        let Self { channels } = self;
        reader.tag(b"DMA0")?;
        for (index, channel) in channels.iter_mut().enumerate() {
            load_channel(channel, index, reader)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    fn state() -> Dma {
        Dma {
            channels: [
                Channel {
                    source: 0x0200_1000,
                    destination: 0x0300_1000,
                    count: 0x1234,
                    control: 0x8000,
                    current_source: 0x0200_2000,
                    current_destination: 0x0300_3000,
                    remaining: 0x1234,
                    active: true,
                    start_delay: 0,
                    first: true,
                    data_latch: Some(0x1122_3344),
                },
                Channel {
                    source: 0x0800_0000,
                    destination: 0x0600_1000,
                    count: 0x2345,
                    control: 0xc660,
                    current_source: 0x0800_1234,
                    current_destination: 0x0600_2344,
                    remaining: 0x2345,
                    active: true,
                    start_delay: 0,
                    first: false,
                    data_latch: None,
                },
                Channel {
                    source: 0x0800_2000,
                    destination: 0x0600_2000,
                    count: 0x0234,
                    control: 0xa000,
                    current_source: 0x0800_3000,
                    current_destination: 0x0600_4000,
                    remaining: 5,
                    active: true,
                    start_delay: 0,
                    first: false,
                    data_latch: Some(0x5566_7788),
                },
                Channel {
                    source: 0x0a00_4000,
                    destination: 0x0800_4000,
                    count: 0xabcd,
                    control: 0x4000,
                    current_source: 0x0a00_5000,
                    current_destination: 0x0800_6000,
                    remaining: 0x4567,
                    active: false,
                    start_delay: 0,
                    first: true,
                    data_latch: None,
                },
            ],
        }
    }

    fn encode(dma: &Dma) -> Vec<u8> {
        let mut writer = Writer::new();
        dma.save(&mut writer);
        writer.finish().unwrap()
    }

    #[test]
    fn round_trip_and_reject_every_strict_prefix() {
        let source = state();
        let bytes = encode(&source);
        let mut restored = Dma::default();
        let mut reader = Reader::new(&bytes);
        restored.load(&mut reader).unwrap();
        reader.finish().unwrap();
        assert_eq!(restored, source);

        for end in 0..bytes.len() {
            let mut truncated = Reader::new(&bytes[..end]);
            assert!(
                Dma::default().load(&mut truncated).is_err(),
                "accepted prefix of length {end}"
            );
        }
    }

    #[test]
    fn invalid_boolean_discriminant_is_corrupt() {
        let mut bytes = encode(&state());
        bytes[28] = 2;
        let result = Dma::default().load(&mut Reader::new(&bytes));
        assert!(matches!(result, Err(SnapshotError::Corrupt(_))));
    }
}
