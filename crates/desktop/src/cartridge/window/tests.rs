use super::*;
use gba_core::{
    dma::DMA_BASE,
    input::Button,
    io::{DISPCNT, HALTCNT, IE, IME, KEYCNT, KEYINPUT, TIMER_BASE},
    memory::ROM_START,
    video::rgb555_to_rgb888,
};

fn original() -> Session {
    Session::new(gba_demos::input_rom()).unwrap()
}
fn code(words: &[u32]) -> Session {
    Session::new(words.iter().flat_map(|word| word.to_le_bytes()).collect()).unwrap()
}
fn next_frame(session: &mut Session, buttons: Buttons) {
    for _ in 0..100 {
        match session.update(buttons).unwrap() {
            Update::Frame => return,
            Update::Stopped => panic!("unexpected STOP"),
            Update::Running => {}
        }
    }
    panic!("no frame within bounded test updates");
}
fn assert_color(session: &Session, color: u16) {
    assert!(session
        .frame()
        .pixels()
        .iter()
        .all(|&pixel| pixel == rgb555_to_rgb888(color)));
}

#[test]
fn captures_first_frame_from_reset_without_demo_startup_registers() {
    let mut session = original();
    assert_eq!(session.machine.cycles(), 0);
    assert_color(&session, 0);
    assert_eq!(session.update(Buttons::default()).unwrap(), Update::Running);
    assert_eq!(session.stats.steps, SLICE_STEPS as u64);
    assert_eq!(session.frames(), 0);
    assert_color(&session, 0);
    next_frame(&mut session, Buttons::default());
    assert_eq!(session.frames(), 1);
    assert_eq!(session.machine.memory().display_position().vblanks, 1);
    assert_eq!(session.machine.memory().captured_vblank(), Some(1));
    assert_eq!(session.machine.memory().read16(DISPCNT).unwrap(), 0);
    assert_color(&session, 0x7c00);
}

#[test]
fn vblank_on_the_final_budgeted_step_succeeds_and_resets_the_budget() {
    let mut reference = original();
    next_frame(&mut reference, Buttons::default());
    let limit = reference.stats.steps as usize;
    let mut session = original();
    assert_eq!(
        session
            .update_bounded(Buttons::default(), limit, limit)
            .unwrap(),
        Update::Frame
    );
    assert_eq!(session.stats.steps, limit as u64);
    assert_eq!(session.frame_steps, 0);
    next_frame(&mut session, Buttons::default());
    assert_eq!(session.frames(), 2);
    assert_eq!(session.frame_steps, 0);
    assert_color(&session, 0x7c00);
}

#[test]
fn input_changes_cpu_palette_writes_and_complete_frame_pixels() {
    let mut session = original();
    for (buttons, color) in [
        (Buttons::default(), 0x7c00),
        (Buttons::default().with(Button::A, true), 0x001f),
        (
            Buttons::default()
                .with(Button::Right, true)
                .with(Button::A, true),
            0x03e0,
        ),
        (Buttons::default(), 0x7c00),
    ] {
        next_frame(&mut session, buttons);
        assert_color(&session, color);
    }
    assert_eq!(session.frames(), 4);
}

#[test]
fn focused_keyboard_reaches_rom_and_focus_loss_releases_input() {
    use crate::desktop::read_buttons;
    use minifb::Key;
    let focused = !cfg!(target_os = "macos");
    let mut session = original();
    next_frame(&mut session, read_buttons(focused, |key| key == Key::Right));
    assert_color(&session, 0x03e0);
    next_frame(
        &mut session,
        read_buttons(!focused, |_| panic!("must not poll keys")),
    );
    assert_color(&session, 0x7c00);
    assert_eq!(session.machine.memory().read16(KEYINPUT).unwrap(), 0x03ff);
}

#[test]
fn stop_retains_image_and_clocks_then_keypad_input_resumes_frames() {
    let mut session = original();
    next_frame(&mut session, Buttons::default());
    let bus = session.machine.memory_mut();
    bus.write16(KEYCNT, 0x4001).unwrap(); // A OR, keypad enabled
    bus.write16(IE, 1 << 12).unwrap();
    bus.write8(HALTCNT, 0x80).unwrap();
    let cycles = session.machine.cycles();
    let steps = session.stats.steps;
    for _ in 0..10 {
        assert_eq!(session.update(Buttons::default()).unwrap(), Update::Stopped);
        assert_eq!(session.machine.cycles(), cycles);
        assert_eq!(session.stats.steps, steps);
        assert_eq!(session.frames(), 1);
        assert_color(&session, 0x7c00);
    }
    // A non-selected key must not wake STOP.
    assert_eq!(
        session
            .update(Buttons::default().with(Button::B, true))
            .unwrap(),
        Update::Stopped
    );
    next_frame(&mut session, Buttons::default().with(Button::A, true));
    assert!(!session.machine.stopped());
    assert!(session.machine.cycles() > cycles);
    assert_color(&session, 0x001f);
}

#[test]
fn bios_stop_during_boot_keeps_the_initial_black_image() {
    let mut session = code(&[0xef03_0000, 0xeaff_fffe]);
    assert_eq!(session.update(Buttons::default()).unwrap(), Update::Stopped);
    assert_eq!(session.frames(), 0);
    assert_color(&session, 0);
    assert!(session.stats.steps < SLICE_STEPS as u64);
}

#[test]
fn stop_on_the_last_slice_step_is_not_delayed_until_next_poll() {
    let mut session = code(&[0xef03_0000, 0xeaff_fffe]);
    for _ in 0..100 {
        let update = session
            .update_bounded(Buttons::default(), 1, FRAME_STEPS)
            .unwrap();
        if session.machine.stopped() {
            assert_eq!(update, Update::Stopped);
            return;
        }
        assert_eq!(update, Update::Running);
    }
    panic!("BIOS Stop did not complete");
}

#[test]
fn halt_advances_display_and_presents_frames_without_host_wake() {
    let mut session = code(&[0xef02_0000, 0xeaff_fffe]);
    next_frame(&mut session, Buttons::default());
    assert!(session.machine.halted());
    assert!(session.stats.halt_idle > 0);
    assert_eq!(
        session.stats.steps,
        session.stats.instructions + session.stats.halt_idle
    );
    next_frame(&mut session, Buttons::default());
    assert_eq!(session.frames(), 2);
}

#[test]
fn frame_budget_is_retained_across_slices_and_stop_waits() {
    let mut session = original();
    assert_eq!(
        session.update_bounded(Buttons::default(), 1, 2).unwrap(),
        Update::Running
    );
    session
        .machine
        .memory_mut()
        .write16(KEYCNT, 0x4001)
        .unwrap();
    session.machine.memory_mut().write16(IE, 1 << 12).unwrap();
    session.machine.memory_mut().write8(HALTCNT, 0x80).unwrap();
    assert_eq!(
        session.update_bounded(Buttons::default(), 1, 2).unwrap(),
        Update::Stopped
    );
    assert_eq!(session.frame_steps, 1);
    let pressed = Buttons::default().with(Button::A, true);
    assert_eq!(
        session.update_bounded(pressed, 1, 2).unwrap(),
        Update::Running
    );
    let cycles = session.machine.cycles();
    assert!(session
        .update_bounded(pressed, 1, 2)
        .unwrap_err()
        .to_string()
        .contains("2 machine steps without VBlank"));
    assert_eq!(session.stats.steps, 2);
    assert_eq!(session.machine.cycles(), cycles);
    assert_color(&session, 0);
}

#[test]
fn cpu_diagnostics_preserve_the_failed_state_and_last_image() {
    let mut session = code(&[0xee00_0000]);
    assert!(session.update(Buttons::default()).is_err());
    assert_eq!(session.machine.cpu().pc(), ROM_START);
    let cpu = session.machine.cpu().clone();
    let cycles = session.machine.cycles();
    let steps = session.stats.steps;
    assert!(session.update(Buttons::default()).is_err());
    assert_eq!(session.machine.cpu(), &cpu);
    assert_eq!(session.machine.cycles(), cycles);
    assert_eq!(session.stats.steps, steps);
    assert_color(&session, 0);
}

#[test]
fn video_diagnostics_retain_the_last_complete_image() {
    let mut session = original();
    next_frame(&mut session, Buttons::default());
    session.machine.memory_mut().write16(DISPCNT, 6).unwrap();
    let error = (0..100)
        .find_map(|_| match session.update(Buttons::default()) {
            Ok(Update::Running) => None,
            Ok(other) => panic!("unexpected {other:?}"),
            Err(error) => Some(error),
        })
        .expect("video error within bounded updates");
    assert!(error.to_string().contains("6"));
    assert_eq!(session.frames(), 1);
    assert_color(&session, 0x7c00);
    assert_eq!(session.machine.memory().display_position().vblanks, 2);
}

#[test]
fn dma_and_irq_consume_slice_budget_and_dma_failure_preserves_progress() {
    let mut session = original();
    next_frame(&mut session, Buttons::default());
    let bus = session.machine.memory_mut();
    bus.write32(DMA_BASE, 0x0200_0000).unwrap();
    bus.write32(DMA_BASE + 4, 0x0300_0000).unwrap();
    bus.write32(DMA_BASE + 8, 0x8000_0002).unwrap();
    bus.write16(IE, 8).unwrap();
    bus.write16(IME, 1).unwrap();
    bus.write32(TIMER_BASE, 0x00c0_ffff).unwrap();
    bus.advance_cycles(1);
    let steps = session.stats.steps;
    let pc = session.machine.cpu().pc();
    session
        .update_bounded(Buttons::default(), 2, FRAME_STEPS)
        .unwrap();
    assert_eq!(session.stats.dma_units, 2);
    assert_eq!(session.stats.steps, steps + 2);
    assert_eq!(session.machine.cpu().pc(), pc);
    session
        .update_bounded(Buttons::default(), 1, FRAME_STEPS)
        .unwrap();
    assert_eq!(session.stats.irq_entries, 1);
    let bus = session.machine.memory_mut();
    bus.write32(DMA_BASE + 4, ROM_START).unwrap();
    bus.write32(DMA_BASE + 8, 0x8000_0001).unwrap();
    let cycles = session.machine.cycles();
    assert!(session.update(Buttons::default()).is_err());
    assert_eq!(session.stats.steps, steps + 3);
    assert_eq!(session.machine.cycles(), cycles);
    assert_color(&session, 0x7c00);
}

#[test]
fn report_contains_capture_count_and_propagates_output_errors() {
    let mut session = original();
    next_frame(&mut session, Buttons::default());
    let mut output = Vec::new();
    session.report(&mut output, "test result").unwrap();
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("Captured ROM frames: 1"));
    assert!(text.contains("Result: test result"));
    assert!(text.contains("CPSR="));
    assert!(text.contains("r15="));
    struct Closed;
    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    assert_eq!(
        session.report(&mut Closed, "test").unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
}
