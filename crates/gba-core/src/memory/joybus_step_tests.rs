//! Original Joybus transaction, CPU access, and preflight regressions.
use super::{timing_event_tests::prepared, *};
use crate::{
    io::{IF, JOYCNT, JOYSTAT, JOY_RECV, JOY_TRANS, RCNT, SIOCNT},
    machine::Machine,
};

#[test]
fn arm_and_thumb_transmit_stores_commit_pending_status_once() {
    for thumb in [false, true] {
        let (cpu, mut memory) = prepared(
            thumb,
            &[if thumb { 0x6008 } else { 0xe5810000 }], // STR r0,[r1]
            &[(0, 0x12345678), (1, JOY_TRANS)],
        );
        memory.write16(RCNT, 0xc000).unwrap();
        memory.write16(JOYCNT, 0x40).unwrap();
        let mut machine = Machine::new(cpu, memory);
        machine.step().unwrap();
        assert_eq!(machine.memory().read32(JOY_TRANS).unwrap(), 0x12345678);
        assert_eq!(machine.memory().read16(JOYSTAT).unwrap(), 2);
        assert_eq!(machine.memory().read16(IF).unwrap(), 0);
        assert_eq!(machine.memory().io.serial.next_event_cycles(), None);
    }
}

#[test]
fn joybus_mode_selection_observes_the_normal_completion_bus_phase() {
    for elapsed in [56, 57] {
        let (cpu, mut memory) = prepared(false, &[0xe5c10000], &[(0, 0xc1), (1, RCNT + 1)]);
        memory.write16(SIOCNT, 0x4083).unwrap();
        memory.advance_cycles(elapsed);
        let serial = memory.io.serial;
        let mut machine = Machine::new(cpu, memory);
        let cpu = machine.cpu().clone();
        if elapsed == 57 {
            machine.step().unwrap();
            assert_eq!(machine.cycles(), 64);
            assert_eq!(machine.memory().read8(RCNT + 1).unwrap(), 0xc1);
            assert_eq!(machine.memory().read16(IF).unwrap(), 0x80);
            assert_eq!(machine.memory().io.serial.next_event_cycles(), None);
        } else {
            let error = machine.step().unwrap_err();
            assert!(error.to_string().contains("serial reconfiguration"));
            assert_eq!(machine.step(), Err(error));
            assert_eq!(machine.cpu(), &cpu);
            assert_eq!(machine.cycles(), 56);
            assert_eq!(machine.memory().io.serial, serial);
        }
    }
}

#[test]
fn later_block_padding_failure_discards_data_pending_status_and_writeback() {
    let (cpu, mut memory) = prepared(
        false,
        &[0xe8a0000e], // STMIA r0!,{r1-r3}; final word hits JOYSTAT padding.
        &[(0, JOY_RECV), (1, 0x12345678), (2, 0xabcdef90), (3, 0x30)],
    );
    memory.write16(RCNT, 0xc000).unwrap();
    let serial = memory.io.serial;
    let mut machine = Machine::new(cpu, memory);
    let cpu = machine.cpu().clone();
    let error = machine.step().unwrap_err();
    assert!(error.to_string().contains("0x0400015a"));
    assert_eq!(machine.step(), Err(error));
    assert_eq!(machine.cpu(), &cpu);
    assert_eq!(machine.cycles(), 0);
    assert_eq!(machine.memory().io.serial, serial);
}

#[test]
fn staged_reads_see_transmit_pending_before_commit_and_discard_restores_every_lane() {
    let (_, mut memory) = prepared(false, &[0xe1a00000], &[]);
    let serial = memory.io.serial;
    memory.begin_timer_step();
    memory.write16(RCNT, 0xc100).unwrap();
    memory.write16(JOYCNT, 0x40).unwrap();
    memory.write32(JOY_TRANS, 0x12345678).unwrap();
    memory.write32(JOY_RECV, 0xabcdef90).unwrap();
    memory.write16(JOYSTAT, 0x30).unwrap();
    assert_eq!(memory.read32(JOY_TRANS).unwrap(), 0x12345678);
    assert_eq!(memory.read32(JOY_RECV).unwrap(), 0xabcdef90);
    assert_eq!(memory.read16(JOYSTAT).unwrap(), 0x32);
    assert_eq!(memory.read16(JOYCNT).unwrap(), 0x40);
    assert_eq!(memory.read8(RCNT), Err(MemoryError::Unmapped(RCNT)));
    assert_eq!(memory.io.serial, serial);
    memory.discard_timer_step();
    assert_eq!(memory.io.serial, serial);
    assert_eq!(memory.read16(JOYSTAT).unwrap(), 0);
}
