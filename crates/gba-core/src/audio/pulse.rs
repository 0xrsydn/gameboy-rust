//! Shared nominal pulse oscillator and sweep unit. No host-clock dependency.
//! Channel 2 never receives sweep-register writes, so its sweep remains disabled.
use super::modulation::Modulation;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Pulse {
    pub(super) modulation: Modulation,
    sweep: u8,
    duty: u8,
    frequency: u16,
    phase: u8,
    remaining: u32,
    sweep_shadow: u16,
    sweep_ticks: u8,
    sweep_running: bool,
    subtracted: bool,
}

impl Pulse {
    pub(super) fn read(&self, offset: u32) -> u8 {
        match offset {
            0 => self.sweep,
            2 => self.duty << 6,
            3 => self.modulation.envelope,
            5 => u8::from(self.modulation.length_enabled) << 6,
            _ => 0,
        }
    }

    fn period(&self) -> u32 {
        16 * (2048 - u32::from(self.frequency))
    }

    fn sweep_period(&self) -> u8 {
        let period = (self.sweep >> 4) & 7;
        if period == 0 {
            8
        } else {
            period
        }
    }

    fn calculate_sweep(&mut self) -> u16 {
        let delta = self.sweep_shadow >> (self.sweep & 7);
        if self.sweep & 8 != 0 {
            self.subtracted = true;
            self.sweep_shadow - delta
        } else {
            self.sweep_shadow + delta
        }
    }

    pub(super) fn write(&mut self, offset: u32, value: u8, next_step: u8) {
        match offset {
            0 => {
                if self.subtracted && self.sweep & 8 != 0 && value & 8 == 0 {
                    self.modulation.active = false;
                }
                self.sweep = value & 0x7f;
            }
            2 => {
                self.duty = value >> 6;
                self.modulation.write_length(value);
            }
            3 => self.modulation.write_envelope(value),
            4 => self.frequency = (self.frequency & 0x700) | u16::from(value),
            5 => {
                self.frequency = (self.frequency & 255) | (u16::from(value & 7) << 8);
                if self.modulation.write_control(value, next_step) {
                    // Retrigger reloads the oscillator timer but preserves duty position.
                    // Sub-divider alignment and hardware startup latency are not modeled.
                    self.remaining = self.period();
                    self.sweep_shadow = self.frequency;
                    self.sweep_ticks = self.sweep_period();
                    self.sweep_running = self.sweep & 0x77 != 0;
                    self.subtracted = false;
                    if self.sweep & 7 != 0 && self.calculate_sweep() > 2047 {
                        self.modulation.active = false;
                    }
                }
            }
            _ => {}
        }
    }

    /// Batch oscillator edges until the next modulation event or bus access.
    pub(super) fn advance(&mut self, cycles: u32) {
        if !self.modulation.active || cycles == 0 {
            return;
        }
        if cycles < self.remaining {
            self.remaining -= cycles;
            return;
        }
        let rest = cycles - self.remaining;
        let period = self.period();
        self.phase = ((u32::from(self.phase) + 1 + rest / period) & 7) as u8;
        self.remaining = period - rest % period;
    }

    pub(super) fn clock(&mut self, step: u8) {
        self.modulation.clock(step);
        if step & 3 == 2 && self.sweep_running {
            self.sweep_ticks -= 1;
            if self.sweep_ticks == 0 {
                self.sweep_ticks = self.sweep_period();
                if self.sweep & 0x70 != 0 {
                    let next = self.calculate_sweep();
                    if next > 2047 {
                        self.modulation.active = false;
                    } else if self.sweep & 7 != 0 {
                        self.frequency = next;
                        self.sweep_shadow = next;
                        if self.calculate_sweep() > 2047 {
                            self.modulation.active = false;
                        }
                    }
                }
            }
        }
    }

    /// Signed pulse amplitude before PSG master volume and stereo routing.
    pub(super) fn sample(&self) -> i16 {
        if !self.modulation.active {
            return 0;
        }
        let high = [0x80u8, 0x81, 0xe1, 0x7e][usize::from(self.duty)] & (1 << self.phase) != 0;
        if high {
            i16::from(self.modulation.volume)
        } else {
            -i16::from(self.modulation.volume)
        }
    }
}

#[cfg(test)]
mod tests;
