//! Original disconnected external-clock probes. No partner, clock edges, or completion is invented.
use gba_core::{
    cpu::Cpu,
    dma::DMA_BASE,
    io::{HALTCNT, IE, IF, IME, RCNT, SIOCNT, SIODATA32, SIODATA8},
    machine::{Machine, StepKind},
    memory::{Memory, MemoryError, ROM_START},
};

fn memory() -> Memory {
    Memory::new(0xeaff_fffe_u32.to_le_bytes().to_vec()).unwrap()
}

#[test]
fn every_low_control_byte_retains_external_busy_but_rejects_internal_start_atomically() {
    let mut bus = memory();
    for high in [0, 0x10, 0x40, 0x50] {
        for previous in [0x0003, 0x1000, 0x4080, 0x5082] {
            for low in 0..=u8::MAX {
                bus.write16(SIOCNT, previous).unwrap();
                let before = bus.read16(SIOCNT).unwrap();
                let result = bus.write8(SIOCNT, low);
                if low & 0x80 != 0 && low & 1 != 0 {
                    assert_eq!(
                        result,
                        Err(MemoryError::UnsupportedIo {
                            address: SIOCNT,
                            value: low,
                            operation: "internally clocked serial transfer",
                        })
                    );
                    assert_eq!(bus.read16(SIOCNT).unwrap(), before);
                } else {
                    result.unwrap();
                    assert_eq!(
                        bus.read16(SIOCNT).unwrap(),
                        (previous & 0x5000) | u16::from(low & 0x8b) | 4
                    );
                    bus.write8(SIOCNT + 1, high).unwrap();
                    assert_eq!(
                        bus.read16(SIOCNT).unwrap(),
                        (u16::from(high) << 8) | u16::from(low & 0x8b) | 4
                    );
                }
                assert_eq!(bus.read16(IF).unwrap() & 0x80, 0);
            }
        }
    }
}

#[test]
fn disconnected_external_requests_wait_at_both_widths_and_rates_until_software_cancels() {
    let mut bus = memory();
    bus.write16(IE, 0x80).unwrap();
    bus.write16(IME, 1).unwrap();
    for control in [0x4080, 0x4082, 0x5080, 0x5082] {
        bus.write32(SIODATA32, 0x12345678).unwrap();
        bus.write16(SIODATA8, 0xab).unwrap();
        bus.write16(SIOCNT, control).unwrap();
        for cycles in [1, 63, 64, 512, 2048, 1000000, u32::MAX] {
            bus.advance_cycles(cycles);
            assert_eq!(bus.read16(SIOCNT).unwrap(), control | 4);
            assert_eq!(bus.read32(SIODATA32).unwrap(), 0x12345678);
            assert_eq!(bus.read16(SIODATA8).unwrap(), 0xab);
            assert_eq!(bus.read16(IF).unwrap() & 0x80, 0);
            assert!(!bus.irq_pending());
        }
        // Rewriting start does not manufacture a clock edge or transfer completion.
        bus.write16(SIOCNT, control).unwrap();
        bus.write8(SIOCNT, 8).unwrap(); // Explicit software cancellation; SO high.
        assert_eq!(bus.read16(SIOCNT).unwrap(), (control & 0x5000) | 12);
        assert_eq!(bus.read32(SIODATA32).unwrap(), 0x12345678);
        assert_eq!(bus.read16(SIODATA8).unwrap(), 0xab);
    }
}

#[test]
fn gpio_selection_does_not_turn_a_retained_start_bit_into_a_transfer_or_irq() {
    let mut bus = memory();
    bus.write16(RCNT, 0x8040).unwrap(); // SI output low, GPIO IRQ disabled.
    bus.write16(SIOCNT, 0x5080).unwrap();
    bus.advance_cycles(1000000);
    assert_eq!(bus.read16(SIOCNT).unwrap(), 0x5080);
    bus.write16(RCNT, 0).unwrap(); // Disconnected normal SI is pulled high.
    bus.advance_cycles(1000000);
    assert_eq!(bus.read16(SIOCNT).unwrap(), 0x5084);
    assert_eq!(bus.read16(IF).unwrap() & 0x80, 0);
    // Clock-source changes with start still set remain explicit diagnostics.
    assert!(bus.write8(SIOCNT, 0x81).is_err());
    assert_eq!(bus.read16(SIOCNT).unwrap(), 0x5084);
    bus.write16(SIOCNT, 0x5001).unwrap(); // Cancel first, then select internal clock while idle.
    assert_eq!(bus.read16(SIOCNT).unwrap(), 0x5005);
}

#[test]
fn external_request_does_not_wake_halt_or_stop() {
    for stop in [false, true] {
        let mut bus = memory();
        bus.write16(IE, 0x80).unwrap();
        bus.write16(IME, 1).unwrap();
        bus.write16(SIOCNT, 0x5080).unwrap();
        bus.write8(HALTCNT, if stop { 0x80 } else { 0 }).unwrap();
        let mut machine = Machine::new(Cpu::new(ROM_START), bus);
        for _ in 0..100 {
            assert_eq!(
                machine.step().unwrap(),
                if stop {
                    StepKind::StopIdle
                } else {
                    StepKind::HaltIdle
                }
            );
        }
        assert_eq!(machine.stopped(), stop);
        assert_eq!(machine.halted(), !stop);
        assert_eq!(machine.memory().read16(IF).unwrap() & 0x80, 0);
        assert_eq!(machine.memory().read16(SIOCNT).unwrap(), 0x5084);
        assert_eq!(machine.cycles() == 0, stop);
    }
}

fn prepared(thumb: bool, instruction: u32, value: u32) -> Machine {
    let code = if thumb {
        vec![
            0xe59f0010, 0xe59f1010, 0xe28f2001, 0xe12fff12, 0xe7fe8001, 0xeafffffe, SIOCNT, value,
        ]
    } else {
        vec![
            0xe59f0008,
            0xe59f1008,
            instruction,
            0xeafffffe,
            SIOCNT,
            value,
        ]
    };
    let mut bus = Memory::new(code.into_iter().flat_map(u32::to_le_bytes).collect()).unwrap();
    let mut cpu = Cpu::new(ROM_START);
    for _ in 0..if thumb { 4 } else { 2 } {
        cpu.step(&mut bus).unwrap();
    }
    bus.write16(SIOCNT, 0x1003).unwrap();
    Machine::new(cpu, bus)
}

#[test]
fn arm_and_thumb_external_starts_complete_as_instructions_not_as_serial_transfers() {
    for thumb in [false, true] {
        for value in [0x5084, 0x5081] {
            let mut machine = prepared(thumb, 0xe1c010b0, value); // STRH r1,[r0]
            let cpu = machine.cpu().clone();
            if value == 0x5084 {
                assert_eq!(machine.step().unwrap(), StepKind::Instruction);
                assert_eq!(machine.memory().read16(SIOCNT).unwrap(), 0x5084);
                assert!(machine.cycles() > 0);
                for _ in 0..100 {
                    machine.step().unwrap();
                }
                assert_eq!(machine.memory().read16(SIOCNT).unwrap(), 0x5084);
                assert_eq!(machine.memory().read16(IF).unwrap() & 0x80, 0);
            } else {
                let error = machine.step().unwrap_err();
                assert!(error
                    .to_string()
                    .contains("internally clocked serial transfer"));
                assert_eq!(machine.cpu(), &cpu);
                assert_eq!(machine.cycles(), 0);
                assert_eq!(machine.memory().read16(SIOCNT).unwrap(), 0x1007);
                assert_eq!(machine.step(), Err(error));
            }
        }
    }
}

#[test]
fn invalid_word_and_block_writes_do_not_leave_a_partial_external_start() {
    let mut bus = memory();
    bus.write16(SIOCNT, 0x5000).unwrap();
    bus.write16(SIODATA8, 0x12).unwrap();
    assert!(bus.write32(SIOCNT, 0x00ab2080).is_err());
    assert_eq!(bus.read16(SIOCNT).unwrap(), 0x5004);
    assert_eq!(bus.read16(SIODATA8).unwrap(), 0x12);
    bus.write32(SIOCNT, 0x00ab5080).unwrap();
    assert_eq!(bus.read16(SIOCNT).unwrap(), 0x5084);
    assert_eq!(bus.read16(SIODATA8).unwrap(), 0xab);

    let mut machine = prepared(false, 0xe8a00006, 0x00ab5080); // STMIA r0!,{r1,r2}; second word unmapped.
    let before = machine.cpu().clone();
    let error = machine.step().unwrap_err();
    assert!(error.to_string().contains("unmapped memory at 0x0400012c"));
    assert_eq!(machine.cpu(), &before);
    assert_eq!(machine.cycles(), 0);
    assert_eq!(machine.memory().read16(SIOCNT).unwrap(), 0x1007);
    assert_eq!(machine.memory().read16(SIODATA8).unwrap(), 0);
    assert_eq!(machine.step(), Err(error));
}

#[test]
fn dma_start_preserves_serial_wait_and_reports_only_dma_completion() {
    let mut bus = memory();
    bus.write32(0x02000000, 0x00ab5081).unwrap();
    bus.write32(DMA_BASE, 0x02000000).unwrap();
    bus.write32(DMA_BASE + 4, SIOCNT).unwrap();
    bus.write32(DMA_BASE + 8, 0xc4000001).unwrap();
    let mut machine = Machine::new(Cpu::new(ROM_START), bus);
    let error = machine.step().unwrap_err();
    assert!(error
        .to_string()
        .contains("internally clocked serial transfer"));
    assert_eq!(machine.step(), Err(error));
    assert_eq!(machine.cycles(), 0);
    assert_eq!(machine.memory().read16(SIOCNT).unwrap(), 4);
    assert_eq!(machine.memory().read16(SIODATA8).unwrap(), 0);
    machine
        .memory_mut()
        .write32(0x02000000, 0x00ab5080)
        .unwrap();
    assert_eq!(machine.step().unwrap(), StepKind::Dma { channel: 0 });
    assert_eq!(machine.memory().read16(SIOCNT).unwrap(), 0x5084);
    assert_eq!(machine.memory().read16(SIODATA8).unwrap(), 0xab);
    assert_eq!(machine.memory().read16(IF).unwrap(), 0x100); // DMA IRQ, not serial IRQ.
    machine.memory_mut().advance_cycles(1000000);
    assert_eq!(machine.memory().read16(SIOCNT).unwrap(), 0x5084);
    assert_eq!(machine.memory().read16(IF).unwrap(), 0x100);
}
