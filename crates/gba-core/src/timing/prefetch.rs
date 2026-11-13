//! Nominal Game Pak opcode prefetch timing. No ROM bytes or CPU instructions live here.
//! Full-buffer pause/restart follows the independently reviewed ares/jgenesis model.
use super::{bus_cycles, AccessKind, AccessWidth};
use crate::snapshot::{Reader, Snapshot, SnapshotError, Writer};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Prefetch {
    waitcnt: u16,
    stream: Option<Stream>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stream {
    head: u32,
    ready: u8,
    remaining: u32,
    running: bool,
}

impl Prefetch {
    pub fn configure(&mut self, waitcnt: u16) {
        // Changes to ROM waits or enable invalidate timing metadata. PHI/SRAM do not.
        // Exact hardware behavior of disabling a partially filled queue remains unverified.
        if (self.waitcnt ^ waitcnt) & 0x47fc != 0 {
            self.clear();
        }
        self.waitcnt = waitcnt;
    }

    pub fn clear(&mut self) {
        self.stream = None;
    }

    fn enabled(&self) -> bool {
        self.waitcnt & 0x4000 != 0
    }

    fn can_produce(address: u32) -> bool {
        (0x08..=0x0d).contains(&(address >> 24)) && address & 0x1fffe != 0
    }

    fn start(&mut self, address: u32) {
        self.stream = Some(Stream {
            head: address,
            ready: 0,
            remaining: bus_cycles(
                self.waitcnt,
                address,
                AccessWidth::Halfword,
                AccessKind::Sequential,
            ),
            running: Self::can_produce(address),
        });
    }

    /// Advance only while the cartridge bus is free. Work is bounded by queue capacity.
    pub fn advance(&mut self, mut cycles: u32) {
        if !self.enabled() {
            return;
        }
        let Some(stream) = &mut self.stream else {
            return;
        };
        while cycles != 0 && stream.running {
            if stream.ready == 8 {
                stream.running = false;
                stream.remaining = 0;
                break;
            }
            let elapsed = cycles.min(stream.remaining);
            cycles -= elapsed;
            stream.remaining -= elapsed;
            if stream.remaining == 0 {
                stream.ready += 1;
                let tail = stream.head.wrapping_add(u32::from(stream.ready) * 2);
                stream.running = Self::can_produce(tail);
                stream.remaining = bus_cycles(
                    self.waitcnt,
                    tail,
                    AccessWidth::Halfword,
                    AccessKind::Sequential,
                );
            }
        }
    }

    /// Cancelling the last cycle of an active halfword adds one nominal stall cycle.
    fn cancel(&mut self) -> u32 {
        let stall = u32::from(
            self.stream
                .is_some_and(|s| s.running && s.ready < 8 && s.remaining == 1),
        );
        self.clear();
        stall
    }

    /// Stall cycles when a DMA transfer's first unit reaches the cartridge bus while a
    /// halfword is in flight. The queue itself is kept: prefetch pauses for the transfer.
    /// As for a CPU data access, interrupting the last cycle of a halfword costs one cycle.
    /// When an internal-memory read precedes a cartridge write, the halfword is one cycle
    /// further along by the time the write starts.
    pub fn dma_stall(&self, source_cartridge: bool, destination_cartridge: bool) -> u32 {
        let last_cycle = match (source_cartridge, destination_cartridge) {
            (true, _) => 1,
            (false, true) => 2,
            (false, false) => return 0,
        };
        u32::from(
            self.enabled()
                && self
                    .stream
                    .is_some_and(|s| s.running && s.ready < 8 && s.remaining == last_cycle),
        )
    }

    pub fn code(&mut self, address: u32, width: AccessWidth, kind: AccessKind) -> u32 {
        let raw = bus_cycles(self.waitcnt, address, width, kind);
        if !(0x08..=0x0d).contains(&(address >> 24)) {
            self.advance(raw);
            return raw;
        }
        if !self.enabled() {
            return raw;
        }
        debug_assert!(matches!(width, AccessWidth::Halfword | AccessWidth::Word));
        let matches = self
            .stream
            .is_some_and(|s| s.head == address && (s.ready != 0 || s.running))
            && Self::can_produce(address);
        if !matches {
            let drained = self
                .stream
                .is_some_and(|s| s.head == address && s.ready == 0 && !s.running);
            let stall = self.cancel();
            let cycles = if drained {
                bus_cycles(self.waitcnt, address, width, AccessKind::NonSequential)
            } else {
                raw
            };
            self.start(address.wrapping_add(width.bytes()));
            return cycles + stall;
        }

        // One CPU-side cycle can deliver either a halfword or a fully queued ARM word.
        let mut cycles = 1;
        self.advance(1);
        for _ in 0..width.bytes() / 2 {
            let mut stream = self.stream.unwrap();
            if stream.ready == 0 {
                if !stream.running {
                    // Full-buffer stop can also be reached between an ARM word's halves.
                    stream.running = true;
                    stream.remaining = bus_cycles(
                        self.waitcnt,
                        stream.head,
                        AccessWidth::Halfword,
                        AccessKind::NonSequential,
                    );
                    self.stream = Some(stream);
                }
                let wait = stream.remaining;
                self.advance(wait);
                cycles += wait;
            }
            let stream = self.stream.as_mut().unwrap();
            debug_assert!(stream.ready > 0);
            stream.ready -= 1;
            stream.head = stream.head.wrapping_add(2);
        }
        cycles
    }

    pub fn data(&mut self, address: u32, width: AccessWidth, kind: AccessKind) -> u32 {
        let raw = bus_cycles(self.waitcnt, address, width, kind);
        if (0x08..=0x0f).contains(&(address >> 24)) {
            raw + self.cancel()
        } else {
            self.advance(raw);
            raw
        }
    }
}

#[cfg(test)]
mod tests;

impl Snapshot for Prefetch {
    fn save(&self, writer: &mut Writer) {
        let Self { waitcnt, stream } = self;
        writer.tag(b"PFCH");
        writer.u16(*waitcnt);
        writer.option(stream.as_ref(), |stream, writer| {
            let Stream {
                head,
                ready,
                remaining,
                running,
            } = stream;
            writer.u32(*head);
            writer.u8(*ready);
            writer.u32(*remaining);
            writer.bool(*running);
        });
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        let Self { waitcnt, stream } = self;
        reader.tag(b"PFCH")?;
        *waitcnt = reader.u16()?;
        *stream = reader.option(|reader| {
            let head = reader.u32()?;
            let ready = reader.u8()?;
            let remaining = reader.u32()?;
            let running = reader.bool()?;
            if ready > 8 {
                return Err(SnapshotError::Corrupt("prefetch queue"));
            }
            Ok(Stream {
                head,
                ready,
                remaining,
                running,
            })
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    fn populated_prefetch() -> Prefetch {
        Prefetch {
            waitcnt: 0x5a34,
            stream: Some(Stream {
                head: 0x0a12_3456,
                ready: 3,
                remaining: 7,
                running: true,
            }),
        }
    }

    fn save(prefetch: &Prefetch) -> Vec<u8> {
        let mut writer = Writer::new();
        prefetch.save(&mut writer);
        writer.finish().unwrap()
    }

    #[test]
    fn prefetch_round_trips_waitcnt_and_stream() {
        let source = populated_prefetch();
        let bytes = save(&source);
        let mut restored = Prefetch::default();
        restored.load(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(restored, source);
    }

    #[test]
    fn every_strict_prefetch_prefix_is_rejected() {
        let bytes = save(&populated_prefetch());
        for end in 0..bytes.len() {
            let mut restored = Prefetch::default();
            assert!(
                restored.load(&mut Reader::new(&bytes[..end])).is_err(),
                "accepted prefix of length {end}"
            );
        }
    }

    #[test]
    fn prefetch_rejects_invalid_stream_values() {
        let mut bytes = save(&populated_prefetch());
        bytes[11] = 9;
        let mut restored = Prefetch::default();
        assert!(matches!(
            restored.load(&mut Reader::new(&bytes)),
            Err(SnapshotError::Corrupt(_))
        ));
    }
}
