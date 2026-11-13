//! Nominal Direct Sound and PSG channels, including single-bank wave playback. No host output.

pub(crate) mod capture;
pub use capture::{AUDIO_QUEUE_CAPACITY, AUDIO_SAMPLE_RATE};
mod modulation;
mod noise;
mod pulse;
mod wave;
use crate::snapshot::{Reader, Snapshot, SnapshotError, Writer};
use noise::Noise;
use pulse::Pulse;
use wave::Wave;

const SEQUENCER_PERIOD: u32 = 32768; // 16,777,216 Hz / 512 Hz.

pub const SOUND_START: u32 = 0x0400_0060;
pub const SOUNDCNT_H: u32 = 0x0400_0082;
pub const SOUNDCNT_X: u32 = 0x0400_0084;
pub const SOUNDBIAS: u32 = 0x0400_0088;
pub const WAVE_RAM: u32 = 0x0400_0090;
pub const FIFO_A: u32 = 0x0400_00a0;
pub const FIFO_B: u32 = 0x0400_00a4;

/// Instantaneous digital mixer levels, not a sampled audio stream.
/// Bias is applied, clipped to ten bits, then centered at 512: -512..=511.
/// Master disable returns zero. PWM quantization and analog filtering are absent.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct StereoLevel {
    pub left: i16,
    pub right: i16,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Fifo {
    words: [u32; 7],
    read: usize,
    len: usize,
    playback: u32,
    remaining: u8,
    sample: i8,
}

impl Fifo {
    fn reset_queue(&mut self) {
        self.words = [0; 7];
        self.read = 0;
        self.len = 0;
    }

    fn write(&mut self, address: u32, bytes: &[u8]) {
        if self.len == 7 {
            self.reset_queue();
            return;
        }
        let slot = (self.read + self.len) % 7;
        for (offset, &byte) in bytes.iter().enumerate() {
            let shift = ((address & 3) as usize + offset) * 8;
            self.words[slot] = (self.words[slot] & !(255 << shift)) | (u32::from(byte) << shift);
        }
        self.len += 1;
    }

    fn clock(&mut self, overflows: u64) -> bool {
        let mut request = false;
        // At most 32 queued/in-flight bytes, then one empty sample. This bounds
        // device-only advances even with a timer overflow on every clock.
        for _ in 0..overflows.min(33) {
            request |= self.len <= 3;
            if self.remaining == 0 && self.len != 0 {
                self.playback = self.words[self.read];
                self.read = (self.read + 1) % 7;
                self.len -= 1;
                self.remaining = 4;
            }
            self.sample = self.playback as i8;
            if self.remaining != 0 {
                self.playback >>= 8;
                self.remaining -= 1;
            }
        }
        request
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Audio {
    enabled: bool,
    psg: [u16; 17],
    pulses: [Pulse; 2],
    noise: Noise,
    wave_channel: Wave,
    sequencer_phase: u16,
    next_step: u8,
    control: u16,
    bias: u16,
    wave: [[u8; 16]; 2],
    fifo: [Fifo; 2],
    requests: [bool; 2],
}

impl Audio {
    pub(crate) fn mapped(address: u32) -> bool {
        matches!(address, SOUND_START..=0x0400_008b | WAVE_RAM..=0x0400_00a7)
    }

    pub(crate) fn unsupported(&self, address: u32, value: u8) -> Option<&'static str> {
        if self.enabled && (0x0400_0070..=0x0400_0075).contains(&address) {
            self.wave_channel.unsupported(address - 0x0400_0070, value)
        } else {
            None
        }
    }

    fn wave_bank(&self) -> usize {
        1 ^ self.wave_channel.bank()
    }

    /// Channel 2 has no sweep register. Its register gap must not alias any
    /// pulse state, particularly the shared engine's sweep configuration.
    fn pulse_register(address: u32) -> Option<(usize, u32)> {
        match address {
            SOUND_START..=0x0400_0065 => Some((0, address - SOUND_START)),
            0x0400_0068..=0x0400_0069 => Some((1, address - 0x0400_0066)),
            0x0400_006c..=0x0400_006d => Some((1, address - 0x0400_0068)),
            _ => None,
        }
    }

    pub(crate) fn read8(&self, address: u32) -> Option<u8> {
        if (0x0400_0070..=0x0400_0075).contains(&address) {
            return Some(self.wave_channel.read(address - 0x0400_0070));
        }
        if (0x0400_0078..=0x0400_007d).contains(&address) {
            return Some(self.noise.read(address - 0x0400_0078));
        }
        if let Some((index, offset)) = Self::pulse_register(address) {
            return Some(self.pulses[index].read(offset));
        }
        if (WAVE_RAM..FIFO_A).contains(&address) {
            return Some(self.wave[self.wave_bank()][(address - WAVE_RAM) as usize]);
        }
        let value = match address & !1 {
            SOUND_START..=0x0400_0080 => {
                let mask = match address & !1 {
                    0x0400_0080 => 0xff77,
                    _ => 0,
                };
                self.psg[((address - SOUND_START) / 2) as usize] & mask
            }
            SOUNDCNT_H => self.control,
            SOUNDCNT_X => {
                (u16::from(self.enabled) << 7)
                    | u16::from(self.pulses[0].modulation.active)
                    | (u16::from(self.pulses[1].modulation.active) << 1)
                    | (u16::from(self.wave_channel.active) << 2)
                    | (u16::from(self.noise.modulation.active) << 3)
            }
            SOUNDBIAS => self.bias,
            0x0400_0086 | 0x0400_008a => 0,
            _ => return None, // FIFO is write-only; open-bus readback is not modeled.
        };
        Some(value.to_le_bytes()[(address & 1) as usize])
    }

    /// One bus access. Partial FIFO writes advance a whole word, not one byte.
    pub(crate) fn write(&mut self, address: u32, bytes: &[u8]) {
        if (FIFO_A..=FIFO_B + 3).contains(&address) {
            self.fifo[((address - FIFO_A) / 4) as usize].write(address, bytes);
            return;
        }
        for (offset, &value) in bytes.iter().enumerate() {
            let address = address + offset as u32;
            if (WAVE_RAM..FIFO_A).contains(&address) {
                self.wave[self.wave_bank()][(address - WAVE_RAM) as usize] = value;
                continue;
            }
            if let Some((index, offset)) = Self::pulse_register(address) {
                if self.enabled {
                    self.pulses[index].write(offset, value, self.next_step);
                }
                continue;
            }
            if (0x0400_0070..=0x0400_0075).contains(&address) {
                if self.enabled {
                    self.wave_channel
                        .write(address - 0x0400_0070, value, self.next_step);
                }
                continue;
            }
            if (0x0400_0078..=0x0400_007d).contains(&address) {
                if self.enabled {
                    self.noise
                        .write(address - 0x0400_0078, value, self.next_step);
                }
                continue;
            }
            let shift = (address & 1) * 8;
            let merge = |old: u16| (old & !(255 << shift)) | (u16::from(value) << shift);
            match address & !1 {
                SOUND_START..=0x0400_0080 if self.enabled => {
                    let register = &mut self.psg[((address - SOUND_START) / 2) as usize];
                    *register = merge(*register);
                }
                SOUNDCNT_H => {
                    self.control = merge(self.control) & 0x770f;
                    if address & 1 != 0 {
                        for (index, mask) in [8, 128].into_iter().enumerate() {
                            if value & mask != 0 {
                                self.fifo[index].reset_queue();
                            }
                        }
                    }
                }
                SOUNDCNT_X if address & 1 == 0 => {
                    if self.enabled && value & 0x80 == 0 {
                        self.psg = [0; 17];
                        self.pulses = [Pulse::default(); 2];
                        self.noise = Noise::default();
                        self.wave_channel = Wave::default();
                        for fifo in &mut self.fifo {
                            fifo.reset_queue();
                        }
                    }
                    if !self.enabled && value & 0x80 != 0 {
                        self.next_step = 0;
                    }
                    self.enabled = value & 0x80 != 0;
                }
                SOUNDBIAS => self.bias = merge(self.bias) & 0xc3fe,
                _ => {}
            }
        }
    }

    /// Free-running 512 Hz divider; master enable resets the sequencer step,
    /// not the divider phase. STOP is gated by the memory bus before this call.
    pub(crate) fn advance(&mut self, mut cycles: u32) {
        if !self.enabled {
            self.sequencer_phase = ((u64::from(self.sequencer_phase) + u64::from(cycles))
                % u64::from(SEQUENCER_PERIOD)) as u16;
            return;
        }
        while cycles != 0 {
            let count = cycles.min(SEQUENCER_PERIOD - u32::from(self.sequencer_phase));
            for pulse in &mut self.pulses {
                pulse.advance(count);
            }
            self.noise.advance(count);
            self.wave_channel.advance(count, &mut self.wave);
            let phase = u32::from(self.sequencer_phase) + count;
            if phase == SEQUENCER_PERIOD {
                self.sequencer_phase = 0;
                for pulse in &mut self.pulses {
                    pulse.clock(self.next_step);
                }
                self.noise.clock(self.next_step);
                self.wave_channel.clock(self.next_step);
                self.next_step = (self.next_step + 1) & 7;
            } else {
                self.sequencer_phase = phase as u16;
            }
            cycles -= count;
        }
    }

    pub(crate) fn timer_overflows(&mut self, timer: usize, count: u64) {
        if !self.enabled || timer > 1 {
            return;
        }
        for index in 0..2 {
            let selected = usize::from((self.control >> (10 + index * 4)) & 1);
            if selected == timer {
                self.requests[index] |= self.fifo[index].clock(count);
            }
        }
    }

    pub(crate) fn take_requests(&mut self) -> [bool; 2] {
        std::mem::take(&mut self.requests)
    }

    pub(crate) fn level(&self) -> StereoLevel {
        if !self.enabled {
            return StereoLevel::default();
        }
        let mut sides = [0i16; 2]; // right, left
        for index in 0..2 {
            let volume = if self.control & (4 << index) != 0 {
                4
            } else {
                2
            };
            for (side, level) in sides.iter_mut().enumerate() {
                if self.control & (1 << (8 + index * 4 + side)) != 0 {
                    *level += i16::from(self.fifo[index].sample) * volume;
                }
            }
        }
        let psg_control = self.psg[16];
        // GBATEK calls ratio 3 prohibited; hardware stores it (mGBA suite I/O read) and
        // mGBA mixes it at 200%. That level is mGBA's choice, not a hardware measurement.
        let ratio = [1, 2, 4, 8][usize::from(self.control & 3)];
        for (side, level) in sides.iter_mut().enumerate() {
            let mut psg_sample = 0;
            for (index, pulse) in self.pulses.iter().enumerate() {
                if psg_control & (1 << (8 + side * 4 + index)) != 0 {
                    psg_sample += pulse.sample() * 4;
                }
            }
            if psg_control & (1 << (11 + side * 4)) != 0 {
                psg_sample += self.noise.sample() * 4;
            }
            if psg_control & (1 << (10 + side * 4)) != 0 {
                psg_sample += self.wave_channel.sample_quarters();
            }
            let volume = ((psg_control >> (side * 4)) & 7) as i16 + 1;
            // Sum PSG channels before scaling, so fractional bits are discarded once.
            *level += (psg_sample * volume * ratio) >> 4;
            *level = (*level + (self.bias & 0x3ff) as i16).clamp(0, 1023) - 512;
        }
        StereoLevel {
            left: sides[1],
            right: sides[0],
        }
    }
}

impl Snapshot for StereoLevel {
    fn save(&self, writer: &mut Writer) {
        let Self { left, right } = self;
        writer.tag(b"STLV");
        writer.i16(*left);
        writer.i16(*right);
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        reader.tag(b"STLV")?;
        let Self { left, right } = self;
        *left = reader.i16()?;
        *right = reader.i16()?;
        Ok(())
    }
}

impl Snapshot for Fifo {
    fn save(&self, writer: &mut Writer) {
        let Self {
            words,
            read,
            len,
            playback,
            remaining,
            sample,
        } = self;
        writer.tag(b"FIF0");
        for word in words {
            writer.u32(*word);
        }
        writer.u32(*read as u32);
        writer.u32(*len as u32);
        writer.u32(*playback);
        writer.u8(*remaining);
        writer.u8(*sample as u8);
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        reader.tag(b"FIF0")?;
        let Self {
            words,
            read,
            len,
            playback,
            remaining,
            sample,
        } = self;
        for word in words {
            *word = reader.u32()?;
        }
        let read_value = reader.u32()?;
        let len_value = reader.u32()?;
        *playback = reader.u32()?;
        *remaining = reader.u8()?;
        *sample = reader.u8()? as i8;
        if read_value >= 7 || len_value > 7 || *remaining > 4 {
            return Err(SnapshotError::Corrupt("FIFO range"));
        }
        *read = read_value as usize;
        *len = len_value as usize;
        Ok(())
    }
}

impl Snapshot for Audio {
    fn save(&self, writer: &mut Writer) {
        let Self {
            enabled,
            psg,
            pulses,
            noise,
            wave_channel,
            sequencer_phase,
            next_step,
            control,
            bias,
            wave,
            fifo,
            requests,
        } = self;
        writer.tag(b"AUD0");
        writer.bool(*enabled);
        for register in psg {
            writer.u16(*register);
        }
        for pulse in pulses {
            pulse.save(writer);
        }
        noise.save(writer);
        wave_channel.save(writer);
        writer.u16(*sequencer_phase);
        writer.u8(*next_step);
        writer.u16(*control);
        writer.u16(*bias);
        writer.raw(&wave[0]);
        writer.raw(&wave[1]);
        for fifo in fifo {
            fifo.save(writer);
        }
        for request in requests {
            writer.bool(*request);
        }
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        reader.tag(b"AUD0")?;
        let Self {
            enabled,
            psg,
            pulses,
            noise,
            wave_channel,
            sequencer_phase,
            next_step,
            control,
            bias,
            wave,
            fifo,
            requests,
        } = self;
        *enabled = reader.bool()?;
        for register in psg {
            *register = reader.u16()?;
        }
        for pulse in pulses {
            pulse.load(reader)?;
        }
        noise.load(reader)?;
        wave_channel.load(reader)?;
        *sequencer_phase = reader.u16()?;
        *next_step = reader.u8()?;
        *control = reader.u16()?;
        *bias = reader.u16()?;
        reader.raw_into(&mut wave[0])?;
        reader.raw_into(&mut wave[1])?;
        for fifo in fifo {
            fifo.load(reader)?;
        }
        for request in requests {
            *request = reader.bool()?;
        }
        if u32::from(*sequencer_phase) >= SEQUENCER_PERIOD
            || *next_step > 7
            || *control & !0x770f != 0
            || *bias & !0xc3fe != 0
        {
            return Err(SnapshotError::Corrupt("audio range"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    fn encode<T: Snapshot>(value: &T) -> Vec<u8> {
        let mut writer = Writer::new();
        value.save(&mut writer);
        writer.finish().unwrap()
    }

    fn check_prefixes<T: Snapshot + Default>(bytes: &[u8], mut target: T) {
        for end in 0..bytes.len() {
            assert!(target.load(&mut Reader::new(&bytes[..end])).is_err());
        }
    }

    #[test]
    fn stereo_level_round_trip_and_prefixes() {
        let source = StereoLevel {
            left: -371,
            right: 248,
        };
        let bytes = encode(&source);
        let mut loaded = StereoLevel::default();
        loaded.load(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(loaded, source);
        check_prefixes(&bytes, StereoLevel::default());
    }

    #[test]
    fn fifo_round_trip_prefixes_and_invalid_index() {
        let source = Fifo {
            words: [0x1234_5678, 0x9abc_def0, 3, 4, 5, 6, 7],
            read: 5,
            len: 6,
            playback: 0xabcd_9876,
            remaining: 3,
            sample: -47,
        };
        let bytes = encode(&source);
        let mut loaded = Fifo::default();
        loaded.load(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(loaded, source);
        check_prefixes(&bytes, Fifo::default());

        let mut corrupt = bytes;
        corrupt[32..36].copy_from_slice(&7u32.to_le_bytes());
        assert_eq!(
            loaded.load(&mut Reader::new(&corrupt)),
            Err(SnapshotError::Corrupt("FIFO range"))
        );
    }

    #[test]
    fn audio_round_trip_prefixes_and_invalid_boolean() {
        let mut source = Audio {
            enabled: true,
            psg: [0x1234; 17],
            ..Audio::default()
        };
        source.pulses[0].write(0, 0x09, 0);
        source.pulses[0].write(2, 0x9d, 0);
        source.pulses[0].write(3, 0xa3, 0);
        source.pulses[0].write(4, 0x7b, 0);
        source.pulses[0].write(5, 0x84, 0);
        source.pulses[0].advance(23);
        source.pulses[0].clock(2);
        source.pulses[1].write(2, 0x5b, 0);
        source.pulses[1].write(3, 0x71, 0);
        source.pulses[1].write(4, 0x42, 0);
        source.pulses[1].write(5, 0x83, 0);
        source.pulses[1].advance(11);
        source.noise.write(0, 5, 0);
        source.noise.write(1, 0xa3, 0);
        source.noise.write(4, 0x39, 0);
        source.noise.write(5, 0x80, 0);
        source.noise.advance(3_601);
        source.wave_channel.write(0, 0xa0, 0);
        source.wave_channel.write(2, 55, 0);
        source.wave_channel.write(3, 0xe0, 0);
        source.wave_channel.write(4, 0x7b, 0);
        source.wave_channel.write(5, 0xc1, 0);
        source.wave_channel.advance(13_352, &mut source.wave);
        source.sequencer_phase = 12_345;
        source.next_step = 5;
        source.control = 0x7105;
        source.bias = 0x83ac;
        source.wave = [[0x12; 16], [0xa5; 16]];
        source.fifo = [
            Fifo {
                words: [0x1122_3344; 7],
                read: 3,
                len: 5,
                playback: 0xaabb_ccdd,
                remaining: 2,
                sample: -91,
            },
            Fifo {
                words: [0x5566_7788; 7],
                read: 6,
                len: 7,
                playback: 0x1234_5678,
                remaining: 4,
                sample: 102,
            },
        ];
        source.requests = [true, true];

        let bytes = encode(&source);
        let mut loaded = Audio::default();
        loaded.load(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(loaded, source);
        check_prefixes(&bytes, Audio::default());

        let mut corrupt = bytes;
        corrupt[4] = 2;
        assert_eq!(
            loaded.load(&mut Reader::new(&corrupt)),
            Err(SnapshotError::Corrupt("boolean"))
        );
    }
}
