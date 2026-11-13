//! Original multiplayer mode/alias checks at CPU bus phases and during preflight.
use super::timing_event_tests::prepared;
use crate::{
    io::{IF, SIOCNT, SIODATA32, SIOMLT_SEND, SIOMULTI0, SIOMULTI2},
    machine::Machine,
};

#[test]
fn mode_change_uses_normal_completion_phase_before_read_only_busy_masking() {
    for elapsed in [56, 57] {
        let (cpu, mut memory) = prepared(false, &[0xe5c10000], &[(0, 0x20), (1, SIOCNT + 1)]); // STRB r0,[r1]
        memory.write16(SIOCNT, 0x4083).unwrap();
        memory.advance_cycles(elapsed);
        let serial = memory.io.serial;
        let mut machine = Machine::new(cpu, memory);
        let cpu = machine.cpu().clone();
        if elapsed == 57 {
            machine.step().unwrap(); // Normal completion, then multiplayer selection at cycle 64.
            assert_eq!(machine.cycles(), 64);
            assert_eq!(machine.memory().read16(SIOCNT).unwrap(), 0x200f);
            assert_eq!(machine.memory().read16(IF).unwrap(), 0x80);
            assert_eq!(machine.memory().io.serial.next_event_cycles(), None);
        } else {
            let error = machine.step().unwrap_err();
            assert!(error.to_string().contains("serial reconfiguration"));
            assert_eq!(machine.step(), Err(error));
            assert_eq!(machine.cpu(), &cpu);
            assert_eq!(machine.cycles(), 56);
            assert_eq!(machine.memory().io.serial, serial);
            assert_eq!(machine.memory().read16(IF).unwrap(), 0);
        }
    }
}

#[test]
fn arm_block_store_ignores_receive_registers_and_validates_a_later_uart_send_first() {
    for unsupported in [false, true] {
        let (cpu, mut memory) = prepared(
            false,
            &[0xe8a0000e],
            &[
                (0, SIOMULTI0),
                (1, 0x12345678),
                (2, 0xabcdef90),
                // SIOCNT and SIOMLT_SEND. 0x3400 selects UART with sending enabled.
                (3, if unsupported { 0x98763400 } else { 0x98762000 }),
            ],
        );
        memory.write16(SIOCNT, 0x2000).unwrap();
        let serial = memory.io.serial;
        let mut machine = Machine::new(cpu, memory);
        let cpu = machine.cpu().clone();
        if unsupported {
            let error = machine.step().unwrap_err();
            assert!(error.to_string().contains("UART transmission"));
            assert_eq!(machine.step(), Err(error));
            assert_eq!(machine.memory().io.serial, serial);
            assert_eq!(machine.cpu(), &cpu);
            assert_eq!(machine.cycles(), 0);
        } else {
            machine.step().unwrap();
            assert_eq!(machine.cpu().registers()[0], SIOMULTI0 + 12);
            // Only transfers fill SIOMULTI0-3; CPU writes are ignored.
            assert_eq!(machine.memory().read32(SIOMULTI0).unwrap(), 0);
            assert_eq!(machine.memory().read32(SIOMULTI2).unwrap(), 0);
            assert_eq!(machine.memory().read16(SIOMLT_SEND).unwrap(), 0x9876);
            assert_eq!(machine.memory().read16(SIOCNT).unwrap(), 0x200c);
            assert_eq!(machine.memory().read16(IF).unwrap(), 0);
            assert_eq!(machine.last_timing().data_cycles, 3);
        }
    }
}

#[test]
fn arm_block_load_observes_all_multiplayer_lanes_from_staged_registers() {
    let (cpu, mut memory) = prepared(false, &[0xe8b0000e], &[(0, SIOMULTI0)]); // LDMIA r0!,{r1-r3}
    memory.write32(SIODATA32, 0x12345678).unwrap(); // Before multiplayer selection.
    memory.write32(SIOCNT, 0x98766083).unwrap();
    memory.write32(SIOMULTI2, 0xabcdef90).unwrap();
    let mut machine = Machine::new(cpu, memory);
    machine.step().unwrap();
    // SIOMULTI0-1 share the SIODATA32 latch. Start stays set without a parent's clock.
    assert_eq!(
        &machine.cpu().registers()[1..4],
        &[0x12345678, 0, 0x9876608f]
    );
    assert_eq!(machine.cpu().registers()[0], SIOMULTI0 + 12);
    assert_eq!(machine.memory().read16(IF).unwrap(), 0);
}

#[test]
fn data_reads_follow_the_staged_mode_and_a_later_diagnostic_preserves_atomicity() {
    let (_, mut memory) = prepared(false, &[0xe1a00000], &[]);
    // Normal 8-bit mode keeps the SIODATA32 write but reads zero there.
    memory.write32(SIODATA32, 0x12345678).unwrap();
    assert_eq!(memory.read32(SIODATA32).unwrap(), 0);
    memory.write16(SIOCNT, 0x1000).unwrap();
    assert_eq!(memory.read32(SIODATA32).unwrap(), 0x12345678);
    memory.write16(SIOCNT, 0x2000).unwrap();
    memory.write32(SIOMULTI0, 0).unwrap();
    assert_eq!(memory.read32(SIOMULTI0).unwrap(), 0x12345678);
    assert_eq!(memory.read32(SIOMULTI2).unwrap(), 0);
    let serial = memory.io.serial;
    assert!(memory
        .write_words(&[(0x02000000, 0xabcd), (SIOCNT, 0x9876_3400)])
        .is_err());
    assert_eq!(memory.io.serial, serial);
    assert_eq!(memory.read32(0x02000000).unwrap(), 0);
    // A staged read cannot fall back to committed multiplayer data after leaving the mode.
    memory.begin_timer_step();
    memory.write16(SIOCNT, 0).unwrap();
    assert_eq!(memory.read32(SIOMULTI0).unwrap(), 0);
    memory.discard_timer_step();
    assert_eq!(memory.read32(SIOMULTI0).unwrap(), 0x12345678);
}
