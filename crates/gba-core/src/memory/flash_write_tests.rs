//! Original completion-at-bus-phase and mutation rollback checks.
use super::{timing_event_tests::prepared, *};
use crate::{
    cartridge::{SaveDevice, FLASH_PROGRAM_CYCLES as PROGRAM, SAVE_START as BASE},
    io::SOUNDBIAS,
    machine::Machine,
};
fn setup(m: &mut Memory) {
    m.set_save_device(SaveDevice::Flash128);
    m.write8(BASE + 0x5555, 0xaa).unwrap();
    m.write8(BASE + 0x2aaa, 0x55).unwrap();
    m.write8(BASE + 0x5555, 0xa0).unwrap();
}
#[test]
fn arm_and_thumb_program_start_excludes_preceding_bus_cycles() {
    for thumb in [false, true] {
        let (cpu, mut m) = prepared(
            thumb,
            &[if thumb { 0x7008 } else { 0xe5c10000 }],
            &[(0, 0x12), (1, BASE)],
        );
        setup(&mut m);
        let mut machine = Machine::new(cpu, m);
        machine.step().unwrap();
        assert_eq!(machine.memory().save_image().unwrap()[0], 255);
        machine.memory_mut().advance_cycles(PROGRAM - 1);
        assert_eq!(machine.memory().save_image().unwrap()[0], 255);
        machine.memory_mut().advance_cycles(1);
        assert_eq!(machine.memory().save_image().unwrap()[0], 0x12);
        assert!(machine.memory().save_modified());
    }
}
#[test]
fn failed_block_load_restores_completion_and_dirty_state() {
    let (cpu, mut m) = prepared(false, &[0xe8910005], &[(1, SOUNDBIAS)]);
    setup(&mut m);
    m.write8(BASE, 0x12).unwrap();
    m.advance_cycles(PROGRAM - 7);
    let controller = m.cartridge;
    let mut machine = Machine::new(cpu, m);
    for _ in 0..2 {
        assert!(machine.step().is_err());
        assert_eq!(machine.memory().cartridge, controller);
        assert_eq!(machine.memory().save_image().unwrap()[0], 255);
        assert!(!machine.memory().save_modified());
        assert_eq!(machine.cycles(), u64::from(PROGRAM - 7));
    }
    machine.memory_mut().advance_cycles(7);
    assert_eq!(machine.memory().save_image().unwrap()[0], 0x12);
}
#[test]
fn read_at_completion_uses_staged_overlay_and_read_before_completion_sees_busy() {
    for elapsed in [PROGRAM - 7, PROGRAM - 6] {
        let (mut cpu, mut m) = prepared(false, &[0xe12fff12], &[(1, BASE), (2, 0x03001000)]);
        for i in 0..8 {
            m.write32(
                0x03001000 + i * 4,
                if i == 0 { 0xe5d10000 } else { 0xe1a00000 },
            )
            .unwrap();
        }
        cpu.step(&mut m).unwrap();
        setup(&mut m);
        m.write8(BASE, 0).unwrap();
        m.advance_cycles(elapsed);
        let mut machine = Machine::new(cpu, m);
        machine.step().unwrap();
        assert_eq!(
            machine.cpu().registers()[0],
            if elapsed == PROGRAM - 6 { 0 } else { 0x80 }
        );
        assert_eq!(machine.memory().save_image().unwrap()[0], 0);
    }
}
#[test]
fn preflight_and_discard_cannot_apply_save_mutations() {
    let (_, mut m) = prepared(false, &[0xe1a00000], &[]);
    setup(&mut m);
    m.write8(BASE, 0x12).unwrap();
    m.advance_cycles(PROGRAM - 1);
    let before = m.cartridge;
    m.begin_timer_step();
    m.advance_timer_step(1);
    assert_eq!(m.read8(BASE).unwrap(), 0x12);
    assert_eq!(m.save_image().unwrap()[0], 255);
    let shadow = m.cartridge_step.get();
    let elapsed = m.cartridge_elapsed.get();
    assert!(m.write_words(&[(0x02000000, 42), (BASE, 0)]).is_err());
    assert_eq!(m.cartridge_step.get(), shadow);
    assert_eq!(m.cartridge_elapsed.get(), elapsed);
    assert_eq!(m.read32(0x02000000).unwrap(), 0);
    m.discard_timer_step();
    assert_eq!(m.cartridge, before);
    assert!(!m.save_modified());
}
