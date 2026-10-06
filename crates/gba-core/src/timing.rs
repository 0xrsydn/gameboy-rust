//! Nominal GBA bus costs. These calculations do not perform memory accesses.
//! Game Pak prefetch, display-bus contention, and configurable EWRAM timing are not modeled.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessWidth {
    Byte,
    Halfword,
    Word,
}

impl AccessWidth {
    pub(crate) fn bytes(self) -> u32 {
        match self {
            Self::Byte => 1,
            Self::Halfword => 2,
            Self::Word => 4,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum AccessKind {
    NonSequential,
    #[default]
    Sequential,
}

/// Timing breakdown for one instruction, exception entry, DMA unit, or HALT batch.
/// DMA has zero code cycles and charges two internal cycles on its first unit.
/// This is a nominal estimate, not a cycle-accurate bus trace.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct StepTiming {
    pub code_cycles: u32,
    pub data_cycles: u32,
    pub internal_cycles: u32,
    /// Device-clock cycles while the CPU is halted, without CPU or DMA bus work.
    pub idle_cycles: u32,
}

impl StepTiming {
    pub fn total(self) -> u32 {
        self.code_cycles + self.data_cycles + self.internal_cycles + self.idle_cycles
    }
}

/// Compute an access cost from a WAITCNT snapshot. This does not validate the
/// address against the memory map. Unmapped regions retain a one-cycle fallback
/// so a branch can finish before the next instruction fetch reports a diagnostic.
/// WAITCNT's prefetch and PHI bits are stored but do not affect this calculation.
pub fn bus_cycles(waitcnt: u16, address: u32, width: AccessWidth, kind: AccessKind) -> u32 {
    match address >> 24 {
        0x02 => {
            if width == AccessWidth::Word {
                6
            } else {
                3
            }
        }
        0x05 | 0x06 => {
            if width == AccessWidth::Word {
                2
            } else {
                1
            }
        }
        0x08..=0x0d => {
            let window = ((address >> 25) - 4) as usize;
            let first_shift = [2, 5, 8][window];
            let second_shift = [4, 7, 10][window];
            let first = 1 + [4, 3, 2, 8][usize::from((waitcnt >> first_shift) & 3)];
            let second = 1 + if waitcnt & (1 << second_shift) != 0 {
                1
            } else {
                [2, 4, 8][window]
            };
            // The Game Pak bus forces N timing at each 128 KiB boundary.
            let sequential = kind == AccessKind::Sequential && address & 0x1fffe != 0;
            let cycles = if sequential { second } else { first };
            // Each word is two halfwords. The second halfword always uses S timing.
            cycles
                + if width == AccessWidth::Word {
                    second
                } else {
                    0
                }
        }
        // BIOS, IWRAM, I/O, and OAM use a 32-bit bus. Other areas remain unmapped.
        _ => 1,
    }
}

/// One speculative instruction/IRQ timing transaction. Events arrive in source,
/// data, internal, then target-pair order. Device clocks still advance in bulk.
/// Code uses the entry WAITCNT snapshot; data uses settings at each bus access.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct CpuTiming {
    pub total: StepTiming,
    code_waitcnt: u16,
    next_data: Option<u32>,
    // The largest supported step is LDM with PC: source + 16 data + I + 2 targets.
    // Production builds keep only costs and sequencing state, not an event log.
    #[cfg(test)]
    events: [Option<TimingEvent>; 20],
    #[cfg(test)]
    event_count: usize,
}

impl CpuTiming {
    pub fn new(code_waitcnt: u16) -> Self {
        Self {
            code_waitcnt,
            ..Self::default()
        }
    }

    pub fn code(&mut self, address: u32, width: AccessWidth, kind: AccessKind) {
        let cycles = bus_cycles(self.code_waitcnt, address, width, kind);
        self.total.code_cycles += cycles;
        #[cfg(test)]
        self.record(TimingEvent::Code {
            address,
            width,
            kind,
            waitcnt: self.code_waitcnt,
            cycles,
        });
    }

    pub fn data(&mut self, waitcnt: u16, address: u32, width: AccessWidth) {
        let kind = if self.next_data == Some(address) {
            AccessKind::Sequential
        } else {
            AccessKind::NonSequential
        };
        let cycles = bus_cycles(waitcnt, address, width, kind);
        self.total.data_cycles += cycles;
        self.next_data = Some(address.wrapping_add(width.bytes()));
        #[cfg(test)]
        self.record(TimingEvent::Data {
            address,
            width,
            kind,
            waitcnt,
            cycles,
        });
    }

    pub fn internal(&mut self, cycles: u32) {
        self.total.internal_cycles += cycles;
        #[cfg(test)]
        if cycles != 0 {
            self.record(TimingEvent::Internal { cycles });
        }
    }

    #[cfg(test)]
    fn record(&mut self, event: TimingEvent) {
        self.events[self.event_count] = Some(event);
        self.event_count += 1;
    }

    #[cfg(test)]
    pub fn events(&self) -> Vec<TimingEvent> {
        self.events[..self.event_count]
            .iter()
            .map(|event| event.unwrap())
            .collect()
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TimingEvent {
    Code {
        address: u32,
        width: AccessWidth,
        kind: AccessKind,
        waitcnt: u16,
        cycles: u32,
    },
    Data {
        address: u32,
        width: AccessWidth,
        kind: AccessKind,
        waitcnt: u16,
        cycles: u32,
    },
    Internal {
        cycles: u32,
    },
}
