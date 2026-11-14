//! Save-state encoding for the I/O register file and its timers.

use super::{Io, PowerState, Timer};
use crate::snapshot::{Reader, Snapshot, SnapshotError, Writer};

/// Timer control bits that the register can hold (see `Timer::set_control`).
const TIMER_CONTROL_MASK: u8 = 0xc7;

impl Snapshot for Timer {
    fn save(&self, writer: &mut Writer) {
        let Self {
            reload,
            counter,
            control,
            delay,
            stale_reload,
            stopping,
        } = self;
        writer.u16(*reload);
        writer.u16(*counter);
        writer.u8(*control);
        writer.u8(*delay);
        writer.option(stale_reload.as_ref(), |value, writer| writer.u16(*value));
        writer.option(stopping.as_ref(), |control, writer| writer.u8(*control));
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        let Self {
            reload,
            counter,
            control,
            delay,
            stale_reload,
            stopping,
        } = self;
        *reload = reader.u16()?;
        *counter = reader.u16()?;
        *control = reader.u8()?;
        *delay = reader.u8()?;
        *stale_reload = reader.option(Reader::u16)?;
        *stopping = reader.option(Reader::u8)?;
        if *control & !TIMER_CONTROL_MASK != 0 {
            return Err(SnapshotError::Corrupt("timer control"));
        }
        if *delay > super::TIMER_START_DELAY {
            return Err(SnapshotError::Corrupt("timer start delay"));
        }
        Ok(())
    }
}

impl PowerState {
    fn to_byte(self) -> u8 {
        match self {
            Self::Running => 0,
            Self::Halt => 1,
            Self::Stop => 2,
        }
    }

    fn from_byte(byte: u8) -> Result<Self, SnapshotError> {
        match byte {
            0 => Ok(Self::Running),
            1 => Ok(Self::Halt),
            2 => Ok(Self::Stop),
            _ => Err(SnapshotError::Corrupt("power state")),
        }
    }
}

impl Snapshot for Io {
    fn save(&self, writer: &mut Writer) {
        if self.deferred != 0 {
            // Save the devices at the current time; deferral is not machine state.
            let mut settled = self.clone();
            settled.settle_devices();
            return settled.save(writer);
        }
        let Self {
            serial,
            audio,
            audio_capture,
            timers,
            timer_phase,
            enable,
            pending,
            irq_wait,
            master_enable,
            waitcnt,
            dispcnt,
            bg_enable_lines,
            greenswap,
            mosaic,
            vertical_mosaic,
            backgrounds,
            effects,
            window_edges,
            horizontal_windows,
            keypad,
            display,
            dma,
            postflg,
            power,
            deferred: _,
            quiet_left: _,
        } = self;
        writer.tag(b"IO00");
        serial.save(writer);
        audio.save(writer);
        audio_capture.save(writer);
        for timer in timers {
            timer.save(writer);
        }
        writer.u16(*timer_phase);
        writer.u16(*enable);
        writer.u16(*pending);
        writer.option(irq_wait.as_ref(), |wait, writer| writer.u32(*wait));
        writer.bool(*master_enable);
        writer.u16(*waitcnt);
        writer.u16(*dispcnt);
        writer.bytes(bg_enable_lines);
        writer.bool(*greenswap);
        writer.u16(*mosaic);
        vertical_mosaic.save(writer);
        for background in backgrounds {
            background.save(writer);
        }
        effects.save(writer);
        window_edges.save(writer);
        horizontal_windows.save(writer);
        keypad.save(writer);
        display.save(writer);
        dma.save(writer);
        writer.bool(*postflg);
        writer.u8(power.to_byte());
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        let Self {
            serial,
            audio,
            audio_capture,
            timers,
            timer_phase,
            enable,
            pending,
            irq_wait,
            master_enable,
            waitcnt,
            dispcnt,
            bg_enable_lines,
            greenswap,
            mosaic,
            vertical_mosaic,
            backgrounds,
            effects,
            window_edges,
            horizontal_windows,
            keypad,
            display,
            dma,
            postflg,
            power,
            deferred,
            quiet_left,
        } = self;
        *deferred = 0;
        *quiet_left = 0;
        reader.tag(b"IO00")?;
        serial.load(reader)?;
        audio.load(reader)?;
        audio_capture.load(reader)?;
        for timer in timers {
            timer.load(reader)?;
        }
        *timer_phase = reader.u16()?;
        *enable = reader.u16()?;
        *pending = reader.u16()?;
        *irq_wait = reader.option(Reader::u32)?;
        *master_enable = reader.bool()?;
        *waitcnt = reader.u16()?;
        *dispcnt = reader.u16()?;
        reader.bytes_into(bg_enable_lines)?;
        *greenswap = reader.bool()?;
        *mosaic = reader.u16()?;
        vertical_mosaic.load(reader)?;
        for background in backgrounds {
            background.load(reader)?;
        }
        effects.load(reader)?;
        window_edges.load(reader)?;
        horizontal_windows.load(reader)?;
        keypad.load(reader)?;
        display.load(reader)?;
        dma.load(reader)?;
        *postflg = reader.bool()?;
        *power = PowerState::from_byte(reader.u8()?)?;
        Ok(())
    }
}
