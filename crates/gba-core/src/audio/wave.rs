//! Nominal single-bank wave playback. RAM rotates in high-nibble-first order.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Wave {
    control: u8,
    volume: u8,
    frequency: u16,
    length: u16,
    length_enabled: bool,
    pub(super) active: bool,
    remaining: u32,
    sample: Option<u8>,
}

impl Wave {
    pub(super) fn bank(&self) -> usize {
        usize::from((self.control >> 6) & 1)
    }

    pub(super) fn unsupported(&self, offset: u32, value: u8) -> Option<&'static str> {
        if (offset == 5 && value & 0x80 != 0 && self.control & 0xa0 == 0xa0)
            || (offset == 0 && self.active && value & 0xa0 == 0xa0)
        {
            return Some("64-sample wave playback");
        }
        if offset == 0 && self.active && value & 0x80 != 0 && (value ^ self.control) & 0x40 != 0 {
            return Some("wave bank change during playback");
        }
        None
    }

    pub(super) fn read(&self, offset: u32) -> u8 {
        match offset {
            0 => self.control,
            3 => self.volume,
            5 => u8::from(self.length_enabled) << 6,
            _ => 0,
        }
    }

    fn period(&self) -> u32 {
        8 * (2048 - u32::from(self.frequency))
    }

    pub(super) fn write(&mut self, offset: u32, value: u8, next_step: u8) {
        match offset {
            0 => {
                self.control = value & 0xe0;
                if value & 0x80 == 0 {
                    self.active = false;
                }
            }
            2 => self.length = 256 - u16::from(value),
            3 => self.volume = value & 0xe0,
            4 => self.frequency = (self.frequency & 0x700) | u16::from(value),
            5 => {
                self.frequency = (self.frequency & 255) | (u16::from(value & 7) << 8);
                let extra_clock = next_step & 1 != 0;
                if !self.length_enabled && value & 0x40 != 0 && extra_clock {
                    self.clock_length();
                }
                self.length_enabled = value & 0x40 != 0;
                if value & 0x80 != 0 {
                    self.active = self.control & 0x80 != 0;
                    if self.length == 0 {
                        self.length = 256 - u16::from(self.length_enabled && extra_clock);
                    }
                    self.remaining = self.period();
                    // Nominal startup policy: silence until the first full sample period.
                    // Retrigger cannot restore RAM that playback has already rotated.
                    self.sample = None;
                }
            }
            _ => {}
        }
    }

    fn clock_length(&mut self) {
        if self.length != 0 {
            self.length -= 1;
            if self.length == 0 {
                self.active = false;
            }
        }
    }

    pub(super) fn clock(&mut self, step: u8) {
        if step & 1 == 0 && self.length_enabled {
            self.clock_length();
        }
    }

    pub(super) fn advance(&mut self, cycles: u32, banks: &mut [[u8; 16]; 2]) {
        if !self.active || cycles == 0 {
            return;
        }
        if cycles < self.remaining {
            self.remaining -= cycles;
            return;
        }
        let rest = cycles - self.remaining;
        let edges = 1 + rest / self.period();
        self.remaining = self.period() - rest % self.period();
        let data = &mut banks[self.bank()];
        let old = *data;
        let digit = |index: usize| {
            let byte = old[(index % 32) / 2];
            (byte >> if index & 1 == 0 { 4 } else { 0 }) & 15
        };
        self.sample = Some(digit(((edges - 1) % 32) as usize));
        let rotation = (edges % 32) as usize;
        for (index, byte) in data.iter_mut().enumerate() {
            *byte = (digit(index * 2 + rotation) << 4) | digit(index * 2 + rotation + 1);
        }
    }

    /// Quarter-units of the pulse/noise amplitude. Keep fractional wave volume
    /// until after routing, master volume, and PSG ratio have been combined.
    pub(super) fn sample_quarters(&self) -> i16 {
        if !self.active {
            return 0;
        }
        let Some(sample) = self.sample else { return 0 };
        let gain = if self.volume & 0x80 != 0 {
            3
        } else {
            [0, 4, 2, 1][usize::from(self.volume >> 5)]
        };
        (i16::from(sample) - 8) * 2 * gain
    }
}

#[cfg(test)]
mod tests;
