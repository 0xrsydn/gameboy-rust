//! The quiet-step path must leave the machine exactly as the general step path does.
use super::*;
use crate::{cpu::Cpu, dma::DMA_BASE, io::TIMER_BASE, machine::Machine};

const IE: u32 = 0x0400_0200;
const DISPSTAT: u32 = 0x0400_0004;

fn words(code: &[u32]) -> Vec<u8> {
    code.iter().flat_map(|word| word.to_le_bytes()).collect()
}

/// A RAM loop that reads IF every pass and acknowledges both requests every 8 passes, with
/// a timer and HBlank both requesting interrupts while IME stays off, and a repeating
/// HBlank DMA: requests, waits, and transfers keep switching between the two paths.
fn machine(general_steps_only: bool) -> Machine {
    let program = words(&[
        0xe59f_1028, // ldr r1, =0x03000000
        0xe59f_2028, // ldr r2, =0x02000000
        0xe59f_3028, // ldr r3, =IE
        0xe3a0_400a, // mov r4, #10
        0xe591_0000, // loop: ldr r0, [r1]
        0xe280_0001, // add r0, r0, #1
        0xe581_0000, // str r0, [r1]
        0xe582_0000, // str r0, [r2]
        0xe1d3_50b2, // ldrh r5, [r3, #2] (IF)
        0xe310_0007, // tst r0, #7
        0x01c3_40b2, // strheq r4, [r3, #2] (acknowledge HBlank and timer 0)
        0xeaff_fff7, // b loop
        0x0300_0000,
        0x0200_0000,
        IE,
    ]);
    let mut memory = Memory::new(program).unwrap();
    memory.general_steps_only = general_steps_only;
    memory.write16(IE, 0x000a).unwrap(); // HBlank and timer 0; IME stays clear.
    memory.write16(DISPSTAT, 0x0010).unwrap();
    memory.write16(TIMER_BASE, 0xf000).unwrap();
    memory.write16(TIMER_BASE + 2, 0x00c0).unwrap();
    memory.write32(DMA_BASE, 0x0200_0000).unwrap();
    memory.write32(DMA_BASE + 4, 0x0200_0100).unwrap();
    memory.write32(DMA_BASE + 8, 0xa660_0001).unwrap();
    Machine::new(Cpu::new(ROM_START), memory)
}

#[test]
fn quiet_steps_match_general_steps_at_every_boundary() {
    let mut quiet = machine(false);
    let mut general = machine(true);
    for step in 0..400_000 {
        let kinds = (quiet.step().unwrap(), general.step().unwrap());
        assert_eq!(kinds.0, kinds.1, "step {step}");
        assert_eq!(quiet.cycles(), general.cycles(), "step {step}");
        assert_eq!(quiet.cpu(), general.cpu(), "step {step}");
        if step % 4096 == 0 {
            assert_eq!(quiet.save_state(), general.save_state(), "step {step}");
        }
    }
    assert_eq!(quiet.save_state(), general.save_state());
    // The loop really did take both paths and cross frames.
    assert!(quiet.memory().display_position().vblanks >= 3);
    let quiet_steps = quiet.memory().quiet_steps;
    assert!(
        (150_000..350_000).contains(&quiet_steps),
        "{quiet_steps} quiet steps"
    );
    assert_eq!(general.memory().quiet_steps, 0);
}
