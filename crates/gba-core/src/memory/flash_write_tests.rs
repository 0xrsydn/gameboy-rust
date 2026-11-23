//! Original completion-at-bus-phase and mutation rollback checks. Programs commit with
//! their storing step (as mGBA applies them); erases keep a nominal delay, so they carry
//! the in-flight checks.
use super::{timing_event_tests::prepared, *};
use crate::{
    cartridge::{SaveDevice, FLASH_ERASE_CYCLES as ERASE, SAVE_START as BASE},
    machine::Machine,
};
fn unlock(m: &mut Memory, command: u8) {
    m.write8(BASE + 0x5555, 0xaa).unwrap();
    m.write8(BASE + 0x2aaa, 0x55).unwrap();
    m.write8(BASE + 0x5555, command).unwrap();
}
/// Flash128 with a zeroed array, so an erase visibly sets bytes to 0xff.
fn zeroed(m: &mut Memory) {
    m.set_save_device(SaveDevice::Flash128);
    m.load_save_image(&vec![0; 131072]).unwrap();
}
/// Arm a sector erase; the next byte store of 0x30 to a sector starts it.
fn arm_erase(m: &mut Memory) {
    zeroed(m);
    unlock(m, 0x80);
    m.write8(BASE + 0x5555, 0xaa).unwrap();
    m.write8(BASE + 0x2aaa, 0x55).unwrap();
}
#[test]
fn a_program_commits_with_its_storing_step() {
    for thumb in [false, true] {
        let (cpu, mut m) = prepared(
            thumb,
            &[if thumb { 0x7008 } else { 0xe5c10000 }],
            &[(0, 0x12), (1, BASE)],
        );
        m.set_save_device(SaveDevice::Flash128);
        unlock(&mut m, 0xa0);
        let mut machine = Machine::new(cpu, m);
        machine.step().unwrap();
        assert_eq!(machine.memory().save_image().unwrap()[0], 0x12);
        assert!(machine.memory().save_modified());
        assert!(!machine.memory().save_write_pending());
    }
}
#[test]
fn arm_and_thumb_erase_start_excludes_preceding_bus_cycles() {
    for thumb in [false, true] {
        let (cpu, mut m) = prepared(
            thumb,
            &[if thumb { 0x7008 } else { 0xe5c10000 }],
            &[(0, 0x30), (1, BASE)],
        );
        arm_erase(&mut m);
        let mut machine = Machine::new(cpu, m);
        machine.step().unwrap();
        assert_eq!(machine.memory().save_image().unwrap()[0], 0);
        machine.memory_mut().advance_cycles(ERASE - 1);
        assert_eq!(machine.memory().save_image().unwrap()[0], 0);
        machine.memory_mut().advance_cycles(1);
        assert_eq!(machine.memory().save_image().unwrap()[0], 0xff);
        assert!(machine.memory().save_modified());
    }
}
#[test]
fn failed_block_load_restores_completion_and_dirty_state() {
    let (cpu, mut m) = prepared(false, &[0xe8910005], &[(1, 0xffff_fffc)]); // Open bus, then a protected BIOS read fails.
    arm_erase(&mut m);
    m.write8(BASE, 0x30).unwrap();
    m.advance_cycles(ERASE - 7);
    let controller = m.cartridge;
    let mut machine = Machine::new(cpu, m);
    for _ in 0..2 {
        assert!(machine.step().is_err());
        assert_eq!(machine.memory().cartridge, controller);
        assert_eq!(machine.memory().save_image().unwrap()[0], 0);
        assert!(!machine.memory().save_modified());
        assert_eq!(machine.cycles(), u64::from(ERASE - 7));
    }
    machine.memory_mut().advance_cycles(7);
    assert_eq!(machine.memory().save_image().unwrap()[0], 0xff);
}
#[test]
fn read_at_erase_completion_uses_staged_overlay_and_read_before_completion_sees_busy() {
    for elapsed in [ERASE - 7, ERASE - 6] {
        let (mut cpu, mut m) = prepared(false, &[0xe12fff12], &[(1, BASE), (2, 0x03001000)]);
        for i in 0..8 {
            m.write32(
                0x03001000 + i * 4,
                if i == 0 { 0xe5d10000 } else { 0xe1a00000 },
            )
            .unwrap();
        }
        cpu.step(&mut m).unwrap();
        arm_erase(&mut m);
        m.write8(BASE, 0x30).unwrap();
        m.advance_cycles(elapsed);
        let mut machine = Machine::new(cpu, m);
        machine.step().unwrap();
        // The sector base polls DQ7 (bit 7 of 0xff, complemented) until the erase ends.
        assert_eq!(
            machine.cpu().registers()[0],
            if elapsed == ERASE - 6 { 0xff } else { 0 }
        );
        assert_eq!(machine.memory().save_image().unwrap()[0], 0xff);
    }
}
#[test]
fn preflight_and_discard_cannot_apply_save_mutations() {
    let (_, mut m) = prepared(false, &[0xe1a00000], &[]);
    arm_erase(&mut m);
    m.write8(BASE, 0x30).unwrap();
    m.advance_cycles(ERASE - 1);
    let before = m.cartridge;
    m.begin_timer_step();
    m.advance_timer_step(1);
    assert_eq!(m.read8(BASE + 1).unwrap(), 0xff);
    assert_eq!(m.save_image().unwrap()[1], 0);
    let shadow = m.cartridge_step.get();
    let elapsed = m.cartridge_elapsed.get();
    assert!(m
        .write_words(&[(0x02000000, 42), (0x0d00_0000, 0)])
        .is_err()); // Read-only window.
    assert_eq!(m.cartridge_step.get(), shadow);
    assert_eq!(m.cartridge_elapsed.get(), elapsed);
    assert_eq!(m.read32(0x02000000).unwrap(), 0);
    m.discard_timer_step();
    assert_eq!(m.cartridge, before);
    assert!(!m.save_modified());
}
