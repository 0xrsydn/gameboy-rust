//! Idle-loop fast-forward: verified passes are skipped without changing machine state.
use gba_core::{
    cpu::Cpu,
    dma::DMA_BASE,
    io::TIMER_BASE,
    machine::Machine,
    memory::{Memory, ROM_START},
};

const FLAG: u32 = 0x0300_0000;
const VCOUNT: u32 = 0x0400_0006;
/// Address of the `b .` the program reaches once the flag is set.
const EXIT: u32 = ROM_START + 0x18;

fn words(code: &[u32]) -> Vec<u8> {
    code.iter().flat_map(|word| word.to_le_bytes()).collect()
}

/// Load r1 = FLAG and r3 = VCOUNT, poll `[r1]` until it is non-zero, then spin on
/// `b .`. `body` sits inside the loop: a no-op, a device read, or a store.
fn polling_program(body: u32) -> Vec<u8> {
    words(&[
        0xe59f_1014, // ldr r1, [pc, #20]  -> FLAG
        0xe59f_3014, // ldr r3, [pc, #20]  -> VCOUNT
        0xe591_0000, // loop: ldr r0, [r1]
        body,
        0xe350_0000, // cmp r0, #0
        0x0aff_fffb, // beq loop
        0xeaff_fffe, // b .
        FLAG,
        VCOUNT,
    ])
}

fn machine(program: Vec<u8>, idle_skip: bool) -> Machine {
    let mut machine = Machine::new(Cpu::new(ROM_START), Memory::new(program).unwrap());
    machine.set_idle_skip(idle_skip);
    let bus = machine.memory_mut();
    // A timer overflowing every 256 cycles, without an interrupt, bounds every skip.
    bus.write16(TIMER_BASE, 0xff00).unwrap();
    bus.write16(TIMER_BASE + 2, 0x0080).unwrap();
    // A repeating HBlank DMA leaves a ready transfer after each HBlank edge.
    bus.write32(DMA_BASE, 0x0200_0000).unwrap();
    bus.write32(DMA_BASE + 4, 0x0200_0100).unwrap();
    bus.write32(DMA_BASE + 8, 0xa660_0001).unwrap();
    machine
}

/// Run `count` frames, returning the machine steps each took.
fn frames(machine: &mut Machine, count: usize) -> Vec<usize> {
    (0..count)
        .map(|_| machine.run_until_vblank(2_000_000).unwrap())
        .collect()
}

#[test]
fn skipped_passes_leave_every_frame_identical() {
    let program = polling_program(0xe1a0_0000); // mov r0, r0
    let mut stepped = machine(program.clone(), false);
    let mut skipping = machine(program, true);
    for frame in 0..8 {
        let plain = frames(&mut stepped, 1)[0];
        let fast = frames(&mut skipping, 1)[0];
        assert_eq!(skipping.cycles(), stepped.cycles(), "frame {frame}");
        assert_eq!(skipping.cpu(), stepped.cpu(), "frame {frame}");
        assert_eq!(
            skipping.save_state().unwrap(),
            stepped.save_state().unwrap(),
            "frame {frame}"
        );
        assert!(
            fast * 2 < plain,
            "frame {frame}: {fast} steps against {plain}"
        );
    }
    // A host write ends the loop; both leave it the same way.
    for machine in [&mut stepped, &mut skipping] {
        machine.memory_mut().write32(FLAG, 1).unwrap();
    }
    frames(&mut stepped, 2);
    frames(&mut skipping, 2);
    assert_eq!(skipping.cpu().pc(), EXIT);
    assert_eq!(
        skipping.save_state().unwrap(),
        stepped.save_state().unwrap()
    );
}

#[test]
fn loops_that_read_devices_or_store_are_not_skipped() {
    for (name, body) in [
        ("I/O read", 0xe1d3_20b0), // ldrh r2, [r3]
        ("store", 0xe581_0004),    // str r0, [r1, #4]
    ] {
        let program = polling_program(body);
        let mut stepped = machine(program.clone(), false);
        let mut skipping = machine(program, true);
        let plain = frames(&mut stepped, 3);
        let fast = frames(&mut skipping, 3);
        assert_eq!(fast, plain, "{name}");
        assert_eq!(
            skipping.save_state().unwrap(),
            stepped.save_state().unwrap(),
            "{name}"
        );
    }
}
