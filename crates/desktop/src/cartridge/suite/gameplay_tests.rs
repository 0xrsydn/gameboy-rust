use super::tests::{boot, suite};
use super::*;
use gba_core::io::{DISPCNT, KEYINPUT};

fn value() -> Value {
    json!({"name":"input", "rom":"original.gba", "step_limit":1000000,
        "completion":{"vblanks":4},
        "inputs":[{"vblank":0,"buttons":1}, {"vblank":1,"buttons":17}, {"vblank":3,"buttons":0}],
        "checks":[{"kind":"pixel","x":0,"y":0,"equals":255}]})
}
fn config(value: Value) -> Case {
    suite(json!({"version":2,"cases":[value]}))
        .unwrap()
        .cases
        .remove(0)
}

#[test]
fn strict_gameplay_schema_and_bounds() {
    assert!(suite(json!({"version":1,"cases":[value()]})).is_err());
    assert!(suite(json!({"version":2,"cases":[value()]})).is_ok());
    for (pointer, replacement) in [
        ("/completion", json!({"vblanks":0})),
        ("/completion", json!({"vblanks":100001})),
        (
            "/completion",
            json!({"vblanks":1,"pc":0,"instruction_set":"arm"}),
        ),
        ("/completion", json!({"vblanks":1,"typo":0})),
        ("/completion", json!({"pc":0})),
        ("/completion", json!({"pc":0,"instruction_set":"arm"})),
        ("/inputs", json!([{"vblank":0,"buttons":1024}])),
        ("/inputs", json!([{"vblank":4,"buttons":0}])),
        (
            "/inputs",
            json!([{"vblank":1,"buttons":0},{"vblank":1,"buttons":1}]),
        ),
        (
            "/inputs",
            json!([{"vblank":2,"buttons":0},{"vblank":1,"buttons":1}]),
        ),
        ("/inputs", json!([{"vblank":0,"buttons":0,"typo":0}])),
        ("/inputs", json!(vec![json!({"vblank":0,"buttons":0}); 257])),
        ("/inputs/0/buttons", json!(-1)),
        ("/inputs/0/vblank", json!(0.5)),
        ("/checks/0/x", json!(240)),
        ("/checks/0/y", json!(160)),
        ("/checks/0/equals", json!(0x1000000)),
    ] {
        let mut input = value();
        *input.pointer_mut(pointer).unwrap() = replacement;
        assert!(
            suite(json!({"version":2,"cases":[input]})).is_err(),
            "{pointer}"
        );
    }
    let mut input = value();
    input["completion"]["vblanks"] = json!(100000);
    input["inputs"] = json!([{"vblank":99999,"buttons":"0x3ff"}]);
    input["checks"][0] = json!({"kind":"pixel","x":239,"y":159,"equals":"0xffffff"});
    config(input);
}

#[test]
fn cpu_observes_initial_held_replaced_and_released_snapshots() {
    for (target, expected, keys) in [
        (1, 0xff0000, 0x3fe),
        (2, 0x00ff00, 0x3ee),
        (3, 0x00ff00, 0x3ee),
        (4, 0x0000ff, 0x3ff),
    ] {
        let mut input = value();
        input["completion"]["vblanks"] = json!(target);
        input["inputs"]
            .as_array_mut()
            .unwrap()
            .retain(|event| event["vblank"].as_u64().unwrap() < target);
        input["checks"][0]["equals"] = json!(expected);
        let case = config(input);
        let mut machine = bios::boot(gba_demos::input_rom()).unwrap();
        let (_, outcome) = run_to_checkpoint(&mut machine, &case);
        assert_eq!(outcome, Outcome::Checkpoint);
        assert_eq!(machine.memory().captured_vblank(), Some(target));
        assert_eq!(machine.memory().read16(KEYINPUT).unwrap(), keys);
        assert_eq!(check(&machine, &case.checks[0])["passed"], true);
        // A post-capture palette write must not change the asserted image.
        machine.memory_mut().write16(0x05000000, 0x7fff).unwrap();
        assert_eq!(check(&machine, &case.checks[0])["actual"], expected);
        let mut frame = Framebuffer::default();
        machine.memory().present_frame(&mut frame).unwrap();
        assert!(frame.pixels().iter().all(|pixel| *pixel == expected));
    }
}

#[test]
fn frame_completion_accepts_final_step_but_not_a_short_budget() {
    let mut case = config(value());
    let run =
        |case: &Case| run_to_checkpoint(&mut bios::boot(gba_demos::input_rom()).unwrap(), case);
    let (stats, outcome) = run(&case);
    assert_eq!(outcome, Outcome::Checkpoint);
    case.step_limit = stats.steps;
    assert_eq!(run(&case).1, Outcome::Checkpoint);
    case.step_limit -= 1;
    assert_eq!(run(&case).1, Outcome::StepLimit);
}

#[test]
fn no_frame_is_not_a_black_pixel_pass_and_mismatch_is_reported() {
    let mut machine = bios::boot(gba_demos::input_rom()).unwrap();
    let black = Check::Pixel {
        x: 0,
        y: 0,
        equals: 0,
    };
    let result = check(&machine, &black);
    assert_eq!(result["passed"], false);
    assert!(result["actual"].is_null());
    assert!(result["error"].is_string());
    run_to_checkpoint(&mut machine, &config(value()));
    let result = check(&machine, &black);
    assert_eq!(result["passed"], false);
    assert_eq!(result["actual"], 255);
}

#[test]
fn first_render_diagnostic_stops_before_later_checkpoint() {
    let mut machine = boot(&[0xeaff_fffe]);
    machine.memory_mut().write16(DISPCNT, 7).unwrap();
    let (stats, outcome) = run_to_checkpoint(&mut machine, &config(value()));
    assert!(matches!(outcome, Outcome::RenderError(_)));
    assert_eq!(machine.memory().display_position().vblanks, 1);
    assert!(stats.steps > 0);
}

#[test]
fn future_input_does_not_advance_stopped_hardware() {
    let mut machine = boot(&[0xef03_0000, 0xeaff_fffe]);
    let (_, outcome) = run_to_checkpoint(&mut machine, &config(value()));
    assert_eq!(outcome, Outcome::Stopped);
    assert_eq!(machine.memory().display_position().vblanks, 0);
}
