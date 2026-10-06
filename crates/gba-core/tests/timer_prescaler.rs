//! Original shared-divider checks. Startup and register-write delays remain outside this model.
use gba_core::{
    cpu::Cpu,
    input::Buttons,
    io::{HALTCNT, IE, IF, KEYCNT, TIMER_BASE},
    machine::{Machine, StepKind},
    memory::{Memory, ROM_START},
};

const DIVISORS: [u64; 4] = [1, 64, 256, 1024];

fn start(memory: &mut Memory, timer: u32, reload: u16, control: u16) {
    memory
        .write32(
            TIMER_BASE + timer * 4,
            u32::from(reload) | u32::from(control) << 16,
        )
        .unwrap();
}

#[test]
fn every_shared_phase_selects_the_next_edge_without_replaying_the_current_edge() {
    for phase in 0..1024_u32 {
        let mut memory = Memory::new(Vec::new()).unwrap();
        memory.advance_cycles(phase); // The divider runs with all timers disabled.
        for selection in 0..4 {
            start(&mut memory, selection, 0, 0x80 | selection as u16);
        }
        memory.advance_cycles(0);
        for selection in 0..4 {
            assert_eq!(memory.read16(TIMER_BASE + selection * 4).unwrap(), 0);
        }
        // Cover two complete divider periods after every possible enable phase.
        let mut elapsed = 0;
        for delta in [1, 62, 1, 191, 1, 767, 1, 1024] {
            memory.advance_cycles(delta);
            elapsed += delta;
            for (selection, divisor) in DIVISORS.into_iter().enumerate() {
                let expected = (u64::from(phase + elapsed) / divisor) - u64::from(phase) / divisor;
                assert_eq!(
                    memory.read16(TIMER_BASE + selection as u32 * 4).unwrap(),
                    expected as u16,
                    "phase={phase}, elapsed={elapsed}, divisor={divisor}"
                );
            }
        }
    }
}

#[test]
fn staggered_timers_share_ticks_and_cascade_ignores_prescaler_selection() {
    let mut memory = Memory::new(Vec::new()).unwrap();
    memory.advance_cycles(10);
    start(&mut memory, 0, 0xfffe, 0xc1);
    memory.advance_cycles(15);
    start(&mut memory, 1, 0xfffe, 0xc1);
    memory.advance_cycles(38);
    start(&mut memory, 2, 0xfffe, 0xc1);
    start(&mut memory, 3, 0xffff, 0xc7); // Cascaded, /1024 field ignored.
    memory.advance_cycles(1); // All independent timers tick at system cycle 64.
    for index in 0..3 {
        assert_eq!(memory.read16(TIMER_BASE + index * 4).unwrap(), 0xffff);
    }
    assert_eq!(memory.read16(IF).unwrap(), 0);
    memory.advance_cycles(64);
    for index in 0..3 {
        assert_eq!(memory.read16(TIMER_BASE + index * 4).unwrap(), 0xfffe);
    }
    assert_eq!(memory.read16(TIMER_BASE + 12).unwrap(), 0xffff);
    assert_eq!(memory.read16(IF).unwrap(), 0x78);
}

#[test]
fn leaving_cascade_joins_the_existing_divider_instead_of_starting_a_new_period() {
    let mut memory = Memory::new(Vec::new()).unwrap();
    start(&mut memory, 1, 0, 0x85); // Timer0 disabled: no cascade pulses.
    memory.advance_cycles(63);
    memory.write16(TIMER_BASE + 6, 0x81).unwrap();
    memory.advance_cycles(1);
    assert_eq!(memory.read16(TIMER_BASE + 4).unwrap(), 1);
    memory.write16(TIMER_BASE + 6, 0x85).unwrap();
    memory.advance_cycles(63);
    memory.write16(TIMER_BASE + 6, 0x81).unwrap();
    memory.advance_cycles(1);
    assert_eq!(memory.read16(TIMER_BASE + 4).unwrap(), 2);
}

#[test]
fn halt_stops_at_the_shared_edge_and_stop_freezes_the_divider_until_keypad_wake() {
    let mut memory = Memory::new(Vec::new()).unwrap();
    memory.advance_cycles(63);
    start(&mut memory, 0, 0xffff, 0xc1);
    memory.write16(IE, 8).unwrap();
    memory.write8(HALTCNT, 0).unwrap();
    let mut machine = Machine::new(Cpu::new(ROM_START), memory);
    assert_eq!(machine.step().unwrap(), StepKind::HaltIdle);
    assert_eq!(machine.last_timing().idle_cycles, 1);
    assert_eq!(machine.cycles(), 64);
    assert_eq!(machine.memory().read16(IF).unwrap(), 8);
    assert!(!machine.halted());

    let mut memory = Memory::new(Vec::new()).unwrap();
    memory.advance_cycles(63);
    start(&mut memory, 0, 0, 0x81);
    memory.write16(IE, 0x1000).unwrap();
    memory.write16(KEYCNT, 0x4001).unwrap();
    memory.write8(HALTCNT, 0x80).unwrap();
    memory.advance_cycles(1000);
    assert_eq!(memory.cycles(), 63);
    memory.set_buttons(Buttons::from_bits(1));
    assert!(!memory.stopped());
    memory.advance_cycles(1);
    assert_eq!(memory.cycles(), 64);
    assert_eq!(memory.read16(TIMER_BASE).unwrap(), 1);
}

#[test]
fn maximum_batches_keep_low_divider_bits_without_per_cycle_iteration() {
    let mut memory = Memory::new(Vec::new()).unwrap();
    memory.advance_cycles(u32::MAX); // Low ten bits are all set, even with no enabled timer.
    start(&mut memory, 0, 0, 0x83);
    memory.advance_cycles(1);
    assert_eq!(memory.read16(TIMER_BASE).unwrap(), 1);
    memory.advance_cycles(u32::MAX); // 4,194,303 ticks, wrapping the 16-bit counter to zero.
    assert_eq!(memory.read16(TIMER_BASE).unwrap(), 0);
    memory.advance_cycles(1);
    assert_eq!(memory.read16(TIMER_BASE).unwrap(), 1);
    assert_eq!(memory.cycles(), 2 * (u64::from(u32::MAX) + 1));
}

#[derive(Clone, Copy, Default)]
struct ReferenceTimer {
    counter: u16,
    reload: u16,
    control: u16,
}

struct Reference {
    timers: [ReferenceTimer; 4],
    clock: u64,
    pending: u16,
}

impl Reference {
    fn control(&mut self, index: usize, value: u16) {
        let timer = &mut self.timers[index];
        let value = value & if index == 0 { 0xc3 } else { 0xc7 };
        if timer.control & 0x80 == 0 && value & 0x80 != 0 {
            timer.counter = timer.reload;
        }
        timer.control = value;
    }

    fn advance(&mut self, cycles: u32) {
        // Independent literal clock-edge simulation. No phase accumulator or bulk division.
        for _ in 0..cycles {
            self.clock += 1;
            let mut overflow = false;
            for (index, timer) in self.timers.iter_mut().enumerate() {
                let tick = timer.control & 0x80 != 0
                    && if timer.control & 4 != 0 {
                        overflow
                    } else {
                        self.clock
                            .is_multiple_of(DIVISORS[usize::from(timer.control & 3)])
                    };
                overflow = tick && timer.counter == 0xffff;
                if overflow {
                    timer.counter = timer.reload;
                    if timer.control & 0x40 != 0 {
                        self.pending |= 1 << (index + 3);
                    }
                } else if tick {
                    timer.counter += 1;
                }
            }
        }
    }
}

#[test]
fn dynamic_control_reload_and_clock_sequences_match_independent_edge_simulation() {
    for mut seed in [0x1020_3040_u32, 0xdead_beef, 0x7654_3210] {
        let mut memory = Memory::new(Vec::new()).unwrap();
        let mut reference = Reference {
            timers: [ReferenceTimer::default(); 4],
            clock: 0,
            pending: 0,
        };
        for _ in 0..2000 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let index = ((seed >> 8) & 3) as usize;
            let address = TIMER_BASE + index as u32 * 4;
            match seed >> 29 {
                0 | 1 => {
                    let value = ((seed >> 16) as u16 & 0xc7) | 0x80;
                    memory.write16(address + 2, value).unwrap();
                    reference.control(index, value);
                }
                2 => {
                    memory.write16(address + 2, 0).unwrap();
                    reference.control(index, 0);
                }
                3 => {
                    let reload = 0xfff0 | ((seed >> 16) as u16 & 15);
                    memory.write16(address, reload).unwrap();
                    reference.timers[index].reload = reload;
                }
                4 => {
                    let mask = (seed >> 16) as u16 & 0x78;
                    memory.write16(IF, mask).unwrap();
                    reference.pending &= !mask;
                }
                _ => {
                    let cycles = (seed >> 16) & 2047;
                    memory.advance_cycles(cycles);
                    reference.advance(cycles);
                }
            }
            for (index, timer) in reference.timers.iter().enumerate() {
                assert_eq!(
                    memory.read32(TIMER_BASE + index as u32 * 4).unwrap(),
                    u32::from(timer.counter) | u32::from(timer.control) << 16
                );
            }
            assert_eq!(memory.read16(IF).unwrap(), reference.pending);
            assert_eq!(memory.cycles(), reference.clock);
        }
    }
}
