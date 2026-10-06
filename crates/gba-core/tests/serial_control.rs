//! Original mode-gating regressions. No link partner or transfer completion is modeled.
use gba_core::{
    cpu::Cpu,
    dma::DMA_BASE,
    io::{HALTCNT, IE, IF, IME, RCNT, SIOCNT},
    machine::{Machine, StepKind},
    memory::{Memory, MemoryError, ROM_START},
};

fn memory() -> Memory {
    Memory::new(0xeaff_fffe_u32.to_le_bytes().to_vec()).unwrap()
}

#[test]
fn rcnt_high_byte_selection_is_independent_of_previous_mode_and_low_byte() {
    let mut bus = memory();
    for previous in [0x8000, 0x80f5, 0x0100, 0x4100] {
        for low in [0, 0x55, 0xff] {
            for high in 0..=u8::MAX {
                for halfword in [false, true] {
                    bus.write16(RCNT, previous).unwrap();
                    let before = (bus.read8(RCNT), bus.read8(RCNT + 1));
                    let result = if halfword {
                        bus.write16(RCNT, u16::from_le_bytes([low, high]))
                    } else {
                        bus.write8(RCNT + 1, high)
                    };
                    // Decode individual mode bits, separately from the implementation masks.
                    let gpio_or_joybus = high & 0x80 != 0;
                    let joybus = gpio_or_joybus && high & 0x40 != 0;
                    let gpio_irq = gpio_or_joybus && high & 1 != 0;
                    if joybus || gpio_irq {
                        assert_eq!(
                            result,
                            Err(MemoryError::UnsupportedIo {
                                address: RCNT + 1,
                                value: high,
                                operation: if joybus {
                                    "Joybus serial mode"
                                } else {
                                    "GPIO serial interrupt enable"
                                },
                            })
                        );
                        assert_eq!((bus.read8(RCNT), bus.read8(RCNT + 1)), before);
                    } else {
                        result.unwrap();
                        assert_eq!(bus.read8(RCNT + 1).unwrap(), high & 0xc1);
                        if gpio_or_joybus {
                            let latch = if halfword { low } else { previous as u8 };
                            let outputs = latch >> 4;
                            let pins = (latch & outputs | !outputs) & 15;
                            assert_eq!(bus.read8(RCNT).unwrap(), (latch & 0xf0) | pins);
                        } else {
                            assert!(bus.read8(RCNT).is_err());
                        }
                    }
                    assert_eq!(bus.read16(IF).unwrap() & 0x80, 0);
                }
            }
        }
    }
}

#[test]
fn inactive_irq_and_joybus_bits_retain_latches_without_interrupts_or_halt_wake() {
    let mut bus = memory();
    bus.write16(IE, 0x80).unwrap();
    bus.write16(IME, 1).unwrap();
    bus.write16(SIOCNT, 0x5008).unwrap();
    bus.write16(RCNT, 0x80f5).unwrap(); // SI output high.
    bus.write8(HALTCNT, 0).unwrap();
    for high in [1, 0x41, 0x40, 0] {
        bus.write8(RCNT + 1, high).unwrap();
        bus.write8(RCNT, 0xf1).unwrap(); // SI latch low, but not in GPIO mode.
        bus.advance_cycles(1000000);
        assert_eq!(bus.read8(RCNT + 1).unwrap(), high);
        assert_eq!(bus.read16(SIOCNT).unwrap(), 0x500c); // Serial input remains pulled high.
        assert_eq!(bus.read16(IF).unwrap() & 0x80, 0);
        assert!(bus.halted());
    }
    bus.write8(RCNT + 1, 0x80).unwrap(); // GPIO, IRQ disabled; lower latches survived.
    assert_eq!(bus.read16(RCNT).unwrap(), 0x80f1);
    assert_eq!(bus.read16(SIOCNT).unwrap(), 0x5008); // GPIO SI output now drives low.
    assert_eq!(bus.read16(IF).unwrap() & 0x80, 0);
    assert!(bus.halted());
}

fn prepared_store(instruction: u32, address: u32, value: u32) -> Machine {
    let code = [
        0xe59f_0008, // LDR r0, destination
        0xe59f_1008, // LDR r1, value
        instruction,
        0xeaff_fffe,
        address,
        value,
    ];
    let mut bus = Memory::new(code.into_iter().flat_map(u32::to_le_bytes).collect()).unwrap();
    bus.write16(RCNT, 0x80f5).unwrap();
    let mut cpu = Cpu::new(ROM_START);
    cpu.step(&mut bus).unwrap();
    cpu.step(&mut bus).unwrap();
    Machine::new(cpu, bus)
}

#[test]
fn cpu_byte_and_halfword_writes_accept_inactive_bits_but_reject_active_modes_atomically() {
    for (instruction, address, shift) in [(0xe5c0_1000, RCNT + 1, 0), (0xe1c0_10b0, RCNT, 8)] {
        for high in [1, 0x41, 0x81, 0xc0, 0xc1] {
            let mut machine = prepared_store(instruction, address, high << shift);
            let cpu = machine.cpu().clone();
            if high & 0x80 == 0 {
                assert_eq!(machine.step().unwrap(), StepKind::Instruction);
                assert_eq!(machine.memory().read8(RCNT + 1).unwrap(), high as u8);
                assert!(machine.cycles() > 0);
            } else {
                let error = machine.step().unwrap_err();
                assert!(error.to_string().contains(if high & 0x40 != 0 {
                    "Joybus serial mode"
                } else {
                    "GPIO serial interrupt enable"
                }));
                assert_eq!(machine.cpu(), &cpu);
                assert_eq!(machine.cycles(), 0);
                assert_eq!(machine.memory().read16(RCNT).unwrap(), 0x80f5);
                assert_eq!(machine.step(), Err(error));
            }
            assert_eq!(machine.memory().read16(IF).unwrap() & 0x80, 0);
        }
    }
}

#[test]
fn dma_halfword_rejection_is_retryable_then_inactive_configuration_completes() {
    for high in [0x81, 0xc0, 0xc1] {
        let mut bus = memory();
        bus.write16(RCNT, 0x80f5).unwrap();
        bus.write16(0x02000000, high << 8).unwrap();
        bus.write32(DMA_BASE, 0x02000000).unwrap();
        bus.write32(DMA_BASE + 4, RCNT).unwrap();
        bus.write32(DMA_BASE + 8, 0x80000001).unwrap(); // One halfword, immediate.
        let mut machine = Machine::new(Cpu::new(ROM_START), bus);
        let cpu = machine.cpu().clone();
        let error = machine.step().unwrap_err();
        assert_eq!(machine.step(), Err(error));
        assert_eq!(machine.cpu(), &cpu);
        assert_eq!(machine.cycles(), 0);
        assert_eq!(machine.memory().read16(RCNT).unwrap(), 0x80f5);
        assert_eq!(machine.memory().read16(DMA_BASE + 10).unwrap(), 0x8000);
        machine.memory_mut().write16(0x02000000, 0x4100).unwrap();
        machine.step().unwrap();
        assert_eq!(machine.memory().read8(RCNT + 1).unwrap(), 0x41);
        assert_eq!(machine.memory().read16(DMA_BASE + 10).unwrap() & 0x8000, 0);
        assert_eq!(machine.memory().read16(IF).unwrap() & 0x80, 0);
    }
}

#[test]
fn rcnt_word_padding_remains_unmapped_without_partial_writes() {
    let mut bus = memory();
    bus.write16(RCNT, 0x80f5).unwrap();
    assert_eq!(
        bus.write32(RCNT, 0x4100),
        Err(MemoryError::Unmapped(RCNT + 2))
    );
    assert_eq!(bus.read16(RCNT).unwrap(), 0x80f5);
}
