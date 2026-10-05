use super::*;
use gba_core::{input::Buttons, io::KEYCNT};

const KEY_IRQ: u16 = 1 << 12;

#[test]
fn arm_thumb_stop_waits_for_keypad_and_restores_user_system_callers() {
    for thumb in [false, true] {
        for status in [0x10, 0x50, 0x90, 0xd0, 0x1f, 0x5f, 0x9f, 0xdf] {
            for ime in [0, 1] {
                let (mut m, pc) = call_status(3, thumb, [0x1234, 0x5678, 0x9abc], status);
                m.memory_mut().write16(KEYCNT, 0x4001).unwrap();
                m.memory_mut().write16(IE, KEY_IRQ).unwrap();
                m.memory_mut().write16(IME, ime).unwrap();
                m.memory_mut()
                    .write32(bios::SVC_STACK - 32, 0x1234_5678)
                    .unwrap();
                let before = m.cpu().clone();
                for _ in 0..200 {
                    if m.stopped() {
                        break;
                    }
                    m.step().unwrap();
                }
                assert!(m.stopped());
                assert!(!m.halted());
                let cycles = m.cycles();
                let asleep = m.cpu().clone();
                for _ in 0..3 {
                    assert_eq!(m.step().unwrap(), StepKind::StopIdle);
                }
                assert_eq!(m.cycles(), cycles);
                assert_eq!(m.cpu(), &asleep);
                m.memory_mut().set_buttons(Buttons::from_bits(1));
                assert!(!m.stopped());
                assert_eq!(m.memory().read16(IF).unwrap(), 0);
                reach(&mut m, pc, 100);
                assert_eq!(&m.cpu().registers()[..15], &before.registers()[..15]);
                assert_eq!(m.cpu().cpsr(), before.cpsr());
                assert_eq!(m.memory().read16(IE).unwrap(), KEY_IRQ);
                assert_eq!(m.memory().read16(IME).unwrap(), ime);
                assert_eq!(m.memory().read16(KEYCNT).unwrap(), 0x4001);
                assert_eq!(
                    m.memory().read32(bios::SVC_STACK - 32).unwrap(),
                    0x1234_5678
                );
            }
        }
    }
}

#[test]
fn stop_service_returns_immediately_for_live_matching_input() {
    for thumb in [false, true] {
        let (mut m, pc) = call(3, thumb, [0, 0, 0]);
        m.memory_mut().write16(KEYCNT, 0xc003).unwrap();
        m.memory_mut().write16(IE, KEY_IRQ).unwrap();
        m.memory_mut().set_buttons(Buttons::from_bits(3));
        m.memory_mut().write16(IF, KEY_IRQ).unwrap();
        finish(&mut m, pc);
        assert!(!m.stopped());
        assert_eq!(m.memory().read16(IF).unwrap(), 0);
    }
}

#[test]
fn stop_service_with_no_wake_source_reports_stopped_instead_of_advancing_frames() {
    let (mut m, _) = call(3, false, [0, 0, 0]);
    assert_eq!(
        m.run_until_vblank(1000),
        Err(gba_core::machine::FrameRunError::Stopped)
    );
    assert!(m.stopped());
    let cycles = m.cycles();
    assert_eq!(
        m.run_until_vblank(1000),
        Err(gba_core::machine::FrameRunError::Stopped)
    );
    assert_eq!(m.cycles(), cycles);
}
