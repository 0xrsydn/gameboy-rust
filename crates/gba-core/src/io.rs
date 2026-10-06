//! GBA I/O subset: DMA, timers, interrupts, WAITCNT, display, keypad input,
//! Direct Sound, and disconnected serial initialization.
//! Timers share a free-running prescaler phase. Hardware startup/write delays
//! and interrupt delivery delays are not modeled.

mod inactive;
use crate::audio::Audio;
pub use crate::audio::{FIFO_A, FIFO_B, SOUNDBIAS, SOUNDCNT_H, SOUNDCNT_X, SOUND_START, WAVE_RAM};
pub use inactive::{JOYCNT, JOY_RECV, JOY_TRANS, RCNT, SIOCNT, SIODATA32, SIODATA8};
mod timer_step;
pub(crate) use timer_step::TimerStep;
use timer_step::TIMER_IRQ_MASK;

use crate::{
    display::{Display, DisplayPosition, CYCLES_PER_LINE, HBLANK_START, VISIBLE_LINES},
    dma::{Dma, DmaError, Transfer, DMA_BASE, DMA_END},
    input::{Buttons, Keypad},
};

pub const DISPCNT: u32 = 0x0400_0000;
pub const GREENSWAP: u32 = 0x0400_0002;
pub const DISPSTAT: u32 = 0x0400_0004;
pub const VCOUNT: u32 = 0x0400_0006;
pub const BG0CNT: u32 = 0x0400_0008;
pub const BG1CNT: u32 = 0x0400_000a;
pub const BG2CNT: u32 = 0x0400_000c;
pub const BG3CNT: u32 = 0x0400_000e;
pub const BG0HOFS: u32 = 0x0400_0010;
pub const BG0VOFS: u32 = 0x0400_0012;
pub const BG1HOFS: u32 = 0x0400_0014;
pub const BG1VOFS: u32 = 0x0400_0016;
pub const BG2HOFS: u32 = 0x0400_0018;
pub const BG2VOFS: u32 = 0x0400_001a;
pub const BG3HOFS: u32 = 0x0400_001c;
pub const BG3VOFS: u32 = 0x0400_001e;
pub const BG2PA: u32 = 0x0400_0020;
pub const BG2PB: u32 = 0x0400_0022;
pub const BG2PC: u32 = 0x0400_0024;
pub const BG2PD: u32 = 0x0400_0026;
pub const BG2X: u32 = 0x0400_0028;
pub const BG2Y: u32 = 0x0400_002c;
pub const BG3PA: u32 = 0x0400_0030;
pub const BG3PB: u32 = 0x0400_0032;
pub const BG3PC: u32 = 0x0400_0034;
pub const BG3PD: u32 = 0x0400_0036;
pub const BG3X: u32 = 0x0400_0038;
pub const BG3Y: u32 = 0x0400_003c;
pub const WIN0H: u32 = 0x0400_0040;
pub const WIN1H: u32 = 0x0400_0042;
pub const WIN0V: u32 = 0x0400_0044;
pub const WIN1V: u32 = 0x0400_0046;
pub const WININ: u32 = 0x0400_0048;
pub const WINOUT: u32 = 0x0400_004a;
pub const MOSAIC: u32 = 0x0400_004c;
pub const BLDCNT: u32 = 0x0400_0050;
pub const BLDALPHA: u32 = 0x0400_0052;
pub const BLDY: u32 = 0x0400_0054;
pub const KEYINPUT: u32 = 0x0400_0130;
pub const KEYCNT: u32 = 0x0400_0132;
pub const TIMER_BASE: u32 = 0x0400_0100;
pub const IE: u32 = 0x0400_0200;
pub const IF: u32 = 0x0400_0202;
pub const WAITCNT: u32 = 0x0400_0204;
pub const IME: u32 = 0x0400_0208;
pub const POSTFLG: u32 = 0x0400_0300;
pub const HALTCNT: u32 = 0x0400_0301;

const IRQ_MASK: u16 = 0x3fff;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Timer {
    reload: u16,
    counter: u16,
    control: u8,
}

impl Timer {
    fn read8(&self, address: u32) -> u8 {
        let value = if address & 2 == 0 {
            self.counter
        } else {
            u16::from(self.control)
        };
        value.to_le_bytes()[(address & 1) as usize]
    }

    fn write8(&mut self, address: u32, value: u8, index: usize) {
        if address & 2 == 0 {
            self.reload = replace_byte(self.reload, address, value);
        } else if address & 1 == 0 {
            self.set_control(value, index);
        }
    }

    fn enabled(&self) -> bool {
        self.control & 0x80 != 0
    }

    fn count_up(&self) -> bool {
        self.control & 4 != 0
    }

    fn set_control(&mut self, value: u8, index: usize) {
        let value = value & if index == 0 { 0xc3 } else { 0xc7 };
        let start = !self.enabled() && value & 0x80 != 0;
        if start {
            self.counter = self.reload;
        }
        self.control = value;
    }

    /// Next independently clocked overflow. Cascaded timers receive their
    /// pulses at a predecessor's event and cannot overflow earlier.
    fn next_overflow(&self, phase: u16) -> Option<u32> {
        if !self.enabled() || self.count_up() {
            return None;
        }
        let divisor = [1, 64, 256, 1024][usize::from(self.control & 3)];
        Some((0x1_0000 - u32::from(self.counter)) * divisor - u32::from(phase) % divisor)
    }

    /// Advance without looping once per cycle or overflow. Return the number of
    /// overflows, not just a boolean, so cascaded timers receive every pulse.
    fn advance(&mut self, cycles: u32, previous_overflows: u64, phase: u16) -> u64 {
        if !self.enabled() {
            return 0;
        }
        let ticks = if self.count_up() {
            previous_overflows
        } else {
            let divisor = [1, 64, 256, 1024][usize::from(self.control & 3)];
            // Count shared divider edges in (start, end], independently of enable time.
            let start = u64::from(phase);
            (start + u64::from(cycles)) / divisor - start / divisor
        };
        let until_overflow = 0x1_0000 - u64::from(self.counter);
        if ticks < until_overflow {
            self.counter += ticks as u16;
            return 0;
        }
        let remaining = ticks - until_overflow;
        let period = 0x1_0000 - u64::from(self.reload);
        self.counter = (u64::from(self.reload) + remaining % period) as u16;
        1 + remaining / period
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum PowerState {
    #[default]
    Running,
    Halt,
    Stop,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Io {
    inactive: inactive::Inactive,
    pub(crate) audio: Audio,
    timers: [Timer; 4],
    // Low ten system-clock bits cover every supported divider. STOP freezes this phase.
    timer_phase: u16,
    enable: u16,
    pending: u16,
    master_enable: bool,
    waitcnt: u16,
    dispcnt: u16,
    greenswap: bool,
    mosaic: u16,
    vertical_mosaic: crate::video::mosaic::VerticalMosaic,
    backgrounds: [crate::video::Background; 4],
    effects: crate::video::effects::Effects,
    window_edges: crate::video::windows::WindowEdges,
    horizontal_windows: crate::video::windows::horizontal::HorizontalWindows,
    keypad: Keypad,
    display: Display,
    dma: Dma,
    postflg: bool,
    power: PowerState,
}

impl Io {
    pub(crate) fn mapped(address: u32) -> bool {
        Audio::mapped(address)
            || inactive::Inactive::mapped(address)
            || matches!(address, 0x0400_0000..=0x0400_0057 | DMA_BASE..=DMA_END | 0x0400_0100..=0x0400_010f | 0x0400_0130..=0x0400_0133 | 0x0400_0200..=0x0400_020b | POSTFLG..=0x0400_0303)
    }

    pub(crate) fn unsupported_write(address: u32, value: u8) -> Option<&'static str> {
        inactive::Inactive::unsupported(address, value)
    }

    pub(crate) fn read8(&self, address: u32) -> Option<u8> {
        if Audio::mapped(address) {
            return self.audio.read8(address);
        }
        if inactive::Inactive::mapped(address) {
            return self.inactive.read8(address);
        }
        if (DMA_BASE..=DMA_END).contains(&address) {
            return Some(self.dma.read8(address));
        }
        if (TIMER_BASE..=TIMER_BASE + 15).contains(&address) {
            return Some(self.timers[((address - TIMER_BASE) / 4) as usize].read8(address));
        }
        let value = match address & !1 {
            DISPCNT => self.dispcnt,
            GREENSWAP => u16::from(self.greenswap),
            DISPSTAT => self.display.status(),
            VCOUNT => self.display.position().scanline,
            BG0CNT..=BG3CNT => self.backgrounds[((address - BG0CNT) / 2) as usize].control,
            BG0HOFS..=0x0400_003e => 0, // Scroll/affine registers are write-only; not open bus.
            WIN0H..=WIN1V | MOSAIC | 0x0400_004e | BLDY | 0x0400_0056 => 0, // Write-only/unused; no open bus yet.
            WININ => self.effects.inside,
            WINOUT => self.effects.outside,
            BLDCNT => self.effects.control,
            BLDALPHA => self.effects.alpha,
            KEYINPUT => self.keypad.keyinput(),
            KEYCNT => self.keypad.control(),
            IE => self.enable,
            IF => self.pending,
            WAITCNT => self.waitcnt,
            0x0400_0206 => 0,
            IME => u16::from(self.master_enable),
            0x0400_020a => 0,
            POSTFLG => u16::from(self.postflg), // HALTCNT reads zero, not an open-bus model.
            0x0400_0302 => 0,
            _ => return None,
        };
        Some(value.to_le_bytes()[(address & 1) as usize])
    }

    /// The memory bus validates every byte before calling this method.
    /// Keypad accesses use write_keypad instead, preserving transfer boundaries.
    pub(crate) fn write8(&mut self, address: u32, value: u8) {
        debug_assert!(Self::mapped(address));
        debug_assert!(!(KEYINPUT..=KEYCNT + 1).contains(&address));
        if inactive::Inactive::mapped(address) {
            self.inactive.write8(address, value);
            return;
        }
        if (DMA_BASE..=DMA_END).contains(&address) {
            self.dma.write8(address, value);
            return;
        }
        match address & !1 {
            DISPCNT => self.dispcnt = replace_byte(self.dispcnt, address, value) & 0xfff7,
            GREENSWAP if address & 1 == 0 => self.greenswap = value & 1 != 0,
            DISPSTAT => self.pending |= self.display.write_status(address & 1 != 0, value),
            VCOUNT => {} // Read-only; writes are ignored.
            BG0CNT..=BG3CNT => {
                let index = ((address - BG0CNT) / 2) as usize;
                let control = &mut self.backgrounds[index].control;
                // Overflow applies to affine BG2/BG3 only; bits 4 and 5 are unused.
                *control = replace_byte(*control, address, value)
                    & if index < 2 { 0xdfcf } else { 0xffcf };
            }
            BG0HOFS..=BG3VOFS => {
                let bg = &mut self.backgrounds[((address - BG0HOFS) / 4) as usize];
                let offset = if address & 2 == 0 {
                    &mut bg.x
                } else {
                    &mut bg.y
                };
                *offset = replace_byte(*offset, address, value) & 0x1ff;
            }
            BG2PA..=0x0400_003e => {
                let index = 2 + ((address - BG2PA) / 16) as usize;
                let position = self.display.position();
                let after_row = u32::from(position.line_cycle) >= HBLANK_START
                    || u32::from(position.scanline) >= VISIBLE_LINES;
                self.backgrounds[index]
                    .affine
                    .write8((address & 15) as usize, value, after_row);
            }
            WIN0H..=WIN1V => {
                let index = ((address - WIN0H) / 2) as usize;
                let bounds = if index < 2 {
                    &mut self.effects.horizontal[index]
                } else {
                    &mut self.effects.vertical[index - 2]
                };
                *bounds = replace_byte(*bounds, address, value);
            }
            MOSAIC => self.mosaic = replace_byte(self.mosaic, address, value),
            WININ | WINOUT | BLDCNT | BLDALPHA | BLDY => {
                let (latch, mask) = match address & !1 {
                    WININ => (&mut self.effects.inside, 0x3f3f),
                    WINOUT => (&mut self.effects.outside, 0x3f3f),
                    BLDCNT => (&mut self.effects.control, 0x3fff),
                    BLDALPHA => (&mut self.effects.alpha, 0x1f1f),
                    _ => (&mut self.effects.brightness, 0x1f),
                };
                *latch = replace_byte(*latch, address, value) & mask;
            }
            0x0400_0100..=0x0400_010e => {
                let index = ((address - TIMER_BASE) / 4) as usize;
                self.timers[index].write8(address, value, index);
            }
            IE => self.enable = replace_byte(self.enable, address, value) & IRQ_MASK,
            WAITCNT => self.waitcnt = replace_byte(self.waitcnt, address, value) & 0x5fff,
            IF => self.pending &= !(u16::from(value) << ((address & 1) * 8)),
            IME if address & 1 == 0 => self.master_enable = value & 1 != 0,
            POSTFLG if address == POSTFLG => self.postflg = value & 1 != 0,
            POSTFLG => {
                // The bus has enforced BIOS-only CPU access. Low seven bits are ignored.
                self.power = if value & 0x80 == 0 {
                    PowerState::Halt
                } else {
                    PowerState::Stop
                };
            }
            _ => {} // Unused I/O bytes ignore writes.
        }
        self.wake_if_requested();
    }

    pub(crate) fn halted(&self) -> bool {
        self.power == PowerState::Halt
    }

    pub(crate) fn stopped(&self) -> bool {
        self.power == PowerState::Stop
    }

    fn wake_if_requested(&mut self) {
        // Both wake paths ignore IME and CPSR.I and leave IF unchanged.
        // STOP uses the live keypad condition, not a stale IF latch.
        let wake = match self.power {
            PowerState::Running => false,
            PowerState::Halt => self.enable & self.pending != 0,
            PowerState::Stop => self.enable & (1 << 12) != 0 && self.keypad.wake_condition(),
        };
        if wake {
            self.power = PowerState::Running;
        }
    }

    /// Bound an idle batch by the next display edge or independent timer overflow.
    /// Display edges keep this finite even without an enabled wake source.
    pub(crate) fn next_event_cycles(&self) -> u32 {
        self.timers
            .iter()
            .filter_map(|timer| timer.next_overflow(self.timer_phase))
            .fold(self.display.next_event_cycles(), u32::min)
    }

    pub(crate) fn display_position(&self) -> DisplayPosition {
        self.display.position()
    }

    pub(crate) fn set_buttons(&mut self, buttons: Buttons) {
        if self.stopped() {
            // System clock is off: update wake inputs without latching IF/history.
            self.keypad.update_stopped_buttons(buttons);
        } else if self.keypad.set_buttons(buttons) {
            self.pending |= 1 << 12;
        }
        self.wake_if_requested();
    }

    /// Commit one validated byte/halfword/word access in the keypad block.
    /// KEYINPUT writes are ignored. Sample once after both KEYCNT bytes merge,
    /// never on a transient low-byte value during a halfword or word write.
    pub(crate) fn write_keypad(&mut self, address: u32, bytes: &[u8]) {
        debug_assert!(address >= KEYINPUT && address + bytes.len() as u32 <= KEYCNT + 2);
        let mut control = self.keypad.control();
        let mut touched = false;
        for (offset, byte) in bytes.iter().enumerate() {
            let address = address + offset as u32;
            if (KEYCNT..=KEYCNT + 1).contains(&address) {
                control = replace_byte(control, address, *byte);
                touched = true;
            }
        }
        if touched {
            if self.stopped() {
                self.keypad.update_control(control);
            } else if self.keypad.write_control(control) {
                self.pending |= 1 << 12;
            }
        }
        self.wake_if_requested();
    }

    pub(crate) fn display_control(&self) -> u16 {
        self.dispcnt
    }

    pub(crate) fn video_settings(&self, captured: bool) -> crate::video::RenderSettings<'_> {
        crate::video::RenderSettings {
            control: self.dispcnt,
            prepared_objects: None,
            internal_affine: captured,
            vertical_windows: captured.then_some(self.window_edges.active),
            horizontal_windows: captured.then_some(&self.horizontal_windows),
            green_swap: self.greenswap,
            mosaic: self.mosaic,
            vertical_mosaic: captured.then_some(self.vertical_mosaic),
            backgrounds: &self.backgrounds,
            effects: &self.effects,
        }
    }

    pub(crate) fn waitcnt(&self) -> u16 {
        self.waitcnt
    }

    pub(crate) fn irq_pending(&self) -> bool {
        self.master_enable && self.enable & self.pending != 0
    }

    pub(crate) fn next_dma(&self) -> Result<Option<Transfer>, DmaError> {
        self.dma.next()
    }

    pub(crate) fn complete_dma_unit(&mut self, channel: usize, data_latch: u32) {
        self.pending |= self.dma.complete_unit(channel, data_latch);
        self.wake_if_requested();
    }

    pub(crate) fn timer_step(&self) -> TimerStep {
        TimerStep::new(self)
    }

    pub(crate) fn commit_timer_step(&mut self, step: TimerStep) {
        self.audio = step.audio;
        self.dma.trigger_sound(self.audio.take_requests());
        self.timers = step.timers;
        self.timer_phase = step.phase;
        self.pending = (self.pending & !TIMER_IRQ_MASK) | step.pending;
        self.wake_if_requested();
    }

    fn advance_timer_bank(
        timers: &mut [Timer; 4],
        phase: &mut u16,
        audio: &mut Audio,
        cycles: u32,
    ) -> u16 {
        audio.advance(cycles);
        let mut pending = 0;
        let mut overflows = 0;
        for (index, timer) in timers.iter_mut().enumerate() {
            overflows = timer.advance(cycles, overflows, *phase);
            audio.timer_overflows(index, overflows);
            if overflows != 0 && timer.control & 0x40 != 0 {
                pending |= 1 << (3 + index);
            }
        }
        // Advance even when every timer is disabled; writes never reset this clock.
        *phase = ((u64::from(*phase) + u64::from(cycles)) & 1023) as u16;
        pending
    }

    pub(crate) fn advance(&mut self, cycles: u32, advance_timers: bool) {
        // Hardware state tracks time even when host-side frame capture is disabled.
        let position = self.display.position();
        let phase = u32::from(position.scanline) * CYCLES_PER_LINE + u32::from(position.line_cycle);
        self.window_edges
            .advance(phase, cycles, self.effects.vertical);
        self.horizontal_windows
            .advance(phase, cycles, self.effects.horizontal);
        let mode = self.dispcnt & 7;
        for index in 2..4 {
            let active = self.dispcnt & (0x100 << index) != 0
                && if index == 2 {
                    (1..=5).contains(&mode)
                } else {
                    mode == 2
                };
            let (height, counter) = if self.backgrounds[index].control & 0x40 != 0 {
                (
                    u32::from((self.mosaic >> 4) & 15) + 1,
                    self.vertical_mosaic.background,
                )
            } else {
                (1, 0)
            };
            self.backgrounds[index]
                .affine
                .advance(phase, cycles, active, height, counter);
        }
        self.vertical_mosaic.advance(phase, cycles, self.mosaic);
        let (vblank, hblank) = self.display.dma_events(cycles);
        self.dma.trigger(vblank, hblank);
        self.pending |= self.display.advance(cycles);
        if advance_timers {
            self.pending |= Self::advance_timer_bank(
                &mut self.timers,
                &mut self.timer_phase,
                &mut self.audio,
                cycles,
            );
            self.dma.trigger_sound(self.audio.take_requests());
        }
        self.wake_if_requested();
    }
}

fn replace_byte(previous: u16, address: u32, value: u8) -> u16 {
    let shift = (address & 1) * 8;
    (previous & !(0xff << shift)) | (u16::from(value) << shift)
}
