//! Four-channel GBA direct memory access (DMA) controller.
//! Transfers are scheduled one data unit at a time. Startup/resumption delays,
//! open-bus latches, sound FIFO, video capture, and Game Pak DRQ are not modeled.

use std::{error::Error, fmt};

use crate::{memory::MemoryError, timing::AccessWidth};

pub const DMA_BASE: u32 = 0x0400_00b0;
pub const DMA_STRIDE: u32 = 12;
pub(crate) const DMA_END: u32 = DMA_BASE + 4 * DMA_STRIDE - 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DmaError {
    Memory { channel: usize, error: MemoryError },
    UnsupportedControl { channel: usize, control: u16 },
    UnsupportedSource { channel: usize, address: u32 },
    RegisterDestination { channel: usize, address: u32 },
    PowerControlDestination { channel: usize, address: u32 },
}

impl fmt::Display for DmaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Memory { channel, error } => write!(f, "DMA{channel}: {error}"),
            Self::UnsupportedControl { channel, control } => {
                write!(f, "DMA{channel}: unsupported control {control:#06x}")
            }
            Self::UnsupportedSource { channel, address } => {
                write!(f, "DMA{channel}: unsupported source {address:#010x}")
            }
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
    first: bool,
}

impl Channel {
    fn enabled(&self) -> bool {
        self.control & 0x8000 != 0
    }

    fn start_timing(&self) -> u16 {
        (self.control >> 12) & 3
    }

    fn unsupported(&self) -> bool {
        self.start_timing() == 3 || self.control & 0x800 != 0 || self.control & 0x180 == 0x180
    }

    fn width(&self) -> AccessWidth {
        if self.control & 0x400 != 0 {
            AccessWidth::Word
        } else {
            AccessWidth::Halfword
        }
    }

    fn initial_count(&self, index: usize) -> u32 {
        if self.count != 0 {
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
                } else if !was_enabled {
                    let mask = !(channel.width().bytes() - 1);
                    channel.current_source = channel.source & mask;
                    channel.current_destination = channel.destination & mask;
                    channel.remaining = channel.initial_count(index);
                    channel.first = true;
                    channel.active = channel.start_timing() == 0;
                }
            }
            _ => unreachable!("DMA register byte"),
        }
    }

    /// Blanking requests are edges, not queued jobs. An active channel ignores
    /// further edges until its current block completes.
    pub(crate) fn trigger(&mut self, vblank: bool, hblank: bool) {
        for channel in &mut self.channels {
            if channel.enabled()
                && !channel.active
                && ((vblank && channel.start_timing() == 1)
                    || (hblank && channel.start_timing() == 2))
            {
                channel.active = true;
            }
        }
    }

    /// Select the highest-priority ready channel without changing its state.
    /// Unsupported enabled modes report a diagnostic rather than silently waiting.
    pub(crate) fn next(&self) -> Result<Option<Transfer>, DmaError> {
        for (index, channel) in self.channels.iter().enumerate() {
            if !channel.enabled() {
                continue;
            }
            if channel.unsupported() {
                return Err(DmaError::UnsupportedControl {
                    channel: index,
                    control: channel.control,
                });
            }
            if channel.active {
                return Ok(Some(Transfer {
                    channel: index,
                    source: channel.current_source,
                    destination: channel.current_destination,
                    width: channel.width(),
                    first: channel.first,
                }));
            }
        }
        Ok(None)
    }

    /// Commit one successful data unit after its bus cycles. Return completion IRQ bits.
    pub(crate) fn complete_unit(&mut self, index: usize) -> u16 {
        let channel = &mut self.channels[index];
        let width = channel.width().bytes();
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
            (channel.control >> 5) & 3,
        );
        channel.remaining -= 1;
        channel.first = false;
        if channel.remaining != 0 {
            return 0;
        }
        channel.active = false;
        if channel.control & 0x200 != 0 && channel.start_timing() != 0 {
            channel.remaining = channel.initial_count(index);
            channel.first = true;
            if channel.control & 0x60 == 0x60 {
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
