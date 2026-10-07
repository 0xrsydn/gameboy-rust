//! Original Flash CPU bus phases, failed writes, fetch restrictions, and preflight checks.
use super::{timing_event_tests::prepared, *};
use crate::{
    cartridge::{SaveDevice, SAVE_START},
    cpu::Cpu,
    io::{IF, WAITCNT},
    machine::Machine,
};

const HI: u32 = SAVE_START + 0x5555;
const LO: u32 = SAVE_START + 0x2aaa;
fn unlocked(memory: &mut Memory) {
    memory.set_save_device(SaveDevice::Flash128);
    memory.write8(HI, 0xaa).unwrap();
    memory.write8(LO, 0x55).unwrap();
}
fn ram_reader(thumb: bool, base: u32) -> (Cpu, Memory) {
    let (mut cpu, mut memory) = prepared(
        false,
        &[0xe12fff12],
        &[(1, SAVE_START), (2, base | u32::from(thumb))],
    );
    for at in (0..32).step_by(4) {
        memory
            .write32(base + at, if thumb { 0x46c046c0 } else { 0xe1a00000 })
            .unwrap();
    }
    if thumb {
        memory.write16(base, 0x7808).unwrap();
    } else {
        memory.write32(base, 0xe5d10000).unwrap();
    }
    cpu.step(&mut memory).unwrap(); // BX from setup ROM into original RAM reader.
    (cpu, memory)
}

#[test]
fn arm_and_thumb_flash_commands_commit_once_at_all_save_wait_settings() {
    for thumb in [false, true] {
        for (wait, cycles) in [5, 4, 3, 9].into_iter().enumerate() {
            let (cpu, mut memory) = prepared(
                thumb,
                &[if thumb { 0x7008 } else { 0xe5c10000 }],
                &[(0, 0x90), (1, HI)],
            );
            unlocked(&mut memory);
            memory.write16(WAITCNT, wait as u16).unwrap();
            let mut machine = Machine::new(cpu, memory);
            machine.step().unwrap();
            assert_eq!(machine.memory().read8(SAVE_START).unwrap(), 0xc2);
            assert_eq!(machine.last_timing().data_cycles, cycles);
            assert_eq!(machine.memory().read16(IF).unwrap(), 0);
        }
    }
}

#[test]
fn work_ram_byte_loads_observe_selected_id_with_save_bus_costs() {
    for thumb in [false, true] {
        for base in [0x02001000, 0x03001000] {
            for (wait, cycles) in [5, 4, 3, 9].into_iter().enumerate() {
                let (cpu, mut memory) = ram_reader(thumb, base);
                unlocked(&mut memory);
                memory.write8(HI, 0x90).unwrap();
                memory.write16(WAITCNT, wait as u16).unwrap();
                let mut machine = Machine::new(cpu, memory);
                machine.step().unwrap();
                assert_eq!(machine.cpu().registers()[0], 0xc2);
                assert_eq!(machine.last_timing().data_cycles, cycles);
            }
        }
    }
}

#[test]
fn rejected_commands_and_rom_execution_reads_roll_back_cpu_and_command_state() {
    for thumb in [false, true] {
        for read in [false, true] {
            let (cpu, mut memory) = prepared(
                thumb,
                &[match (thumb, read) {
                    (false, false) => 0xe5c10000,
                    (true, false) => 0x7008,
                    (false, true) => 0xe5d10000,
                    (true, true) => 0x7808,
                }],
                &[(0, 0x42), (1, if read { SAVE_START } else { HI })],
            );
            unlocked(&mut memory);
            let before = memory.cartridge;
            let mut machine = Machine::new(cpu, memory);
            let cpu = machine.cpu().clone();
            let error = machine.step().unwrap_err();
            assert!(error.to_string().contains(if read {
                "outside work RAM"
            } else {
                "Flash command"
            }));
            assert_eq!(machine.step(), Err(error));
            assert_eq!(machine.cpu(), &cpu);
            assert_eq!(machine.memory().cartridge, before);
            assert_eq!(machine.cycles(), 0);
            machine.memory_mut().write8(HI, 0x90).unwrap(); // Failure retained the accepted unlock prefix.
            assert_eq!(machine.memory().read8(SAVE_START).unwrap(), 0xc2);
        }
    }
}

#[test]
fn staged_id_reads_and_discard_do_not_mutate_array_or_committed_controller() {
    let (_, mut memory) = prepared(false, &[0xe1a00000], &[]);
    unlocked(&mut memory);
    let before = memory.cartridge;
    memory.begin_timer_step();
    memory.write8(HI, 0x90).unwrap();
    assert_eq!(memory.read8(SAVE_START).unwrap(), 0xc2);
    assert_eq!(memory.cartridge, before);
    memory.discard_timer_step();
    assert_eq!(memory.read8(SAVE_START).unwrap(), 255);
    memory.write8(HI, 0x90).unwrap();
    memory.begin_timer_step();
    memory.write8(HI, 0xaa).unwrap();
    memory.write8(LO, 0x55).unwrap();
    memory.write8(HI, 0xf0).unwrap();
    assert_eq!(memory.read8(SAVE_START).unwrap(), 255);
    memory.discard_timer_step();
    assert_eq!(memory.read8(SAVE_START).unwrap(), 0xc2);
    assert!(memory.save_image().unwrap().iter().all(|&b| b == 255));
}

#[test]
fn instruction_fetch_and_block_word_access_cannot_bypass_byte_only_device() {
    let (_, mut memory) = prepared(false, &[0xe1a00000], &[]);
    unlocked(&mut memory);
    for state in [InstructionSet::Arm, InstructionSet::Thumb] {
        let error = memory
            .fetch_instruction(SAVE_START, state)
            .instruction
            .unwrap_err();
        assert!(error.to_string().contains("Flash instruction fetch"));
    }
    let before = memory.cartridge;
    assert!(memory
        .write_words(&[(0x02000000, 0x12345678), (SAVE_START, 0)])
        .is_err());
    assert_eq!(memory.read32(0x02000000).unwrap(), 0);
    assert_eq!(memory.cartridge, before);
}
