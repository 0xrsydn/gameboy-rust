//! Original GPIO bus-phase, fetch, and diagnostic-rollback checks.
use super::{timing_event_tests::prepared, *};
use crate::{
    cartridge::{CartridgeHardware, GPIO_CONTROL, GPIO_DATA, GPIO_DIRECTION},
    machine::Machine,
};

#[test]
fn arm_and_thumb_stores_pay_game_pak_costs_and_keep_rom_immutable() {
    for thumb in [false, true] {
        let (cpu, mut memory) = prepared(
            thumb,
            &[if thumb { 0x8008 } else { 0xe1c100b0 }], // STRH r0,[r1]
            &[(0, 1), (1, GPIO_CONTROL)],
        );
        memory.set_cartridge_hardware(CartridgeHardware::Rtc);
        let rom = memory.rom.clone();
        let mut machine = Machine::new(cpu, memory);
        machine.step().unwrap();
        assert_eq!(machine.memory().read16(GPIO_CONTROL).unwrap(), 1);
        assert_eq!(machine.memory().rom, rom);
        assert_eq!(machine.last_timing().data_cycles, 5); // Default non-sequential Game Pak halfword.
    }
}

#[test]
fn rejected_rtc_command_edge_rolls_back_arm_and_thumb_steps() {
    for thumb in [false, true] {
        let (cpu, mut memory) = prepared(
            thumb,
            &[if thumb { 0x8008 } else { 0xe1c100b0 }],
            &[(0, 7), (1, GPIO_DATA)],
        );
        memory.set_cartridge_hardware(CartridgeHardware::Rtc);
        memory.write16(GPIO_CONTROL, 1).unwrap();
        memory.write16(GPIO_DATA, 1).unwrap();
        memory.write16(GPIO_DIRECTION, 7).unwrap();
        memory.write16(GPIO_DATA, 5).unwrap();
        // All but the final rising edge of an original date/time read command (0x65).
        for bit in (1..8).rev() {
            let pins = 4 | (((0x65 >> bit) & 1) << 1);
            memory.write16(GPIO_DATA, pins).unwrap();
            memory.write16(GPIO_DATA, pins | 1).unwrap();
        }
        memory.write16(GPIO_DATA, 6).unwrap();
        let cartridge = memory.cartridge;
        let mut machine = Machine::new(cpu, memory);
        let cpu = machine.cpu().clone();
        let error = machine.step().unwrap_err();
        assert!(error.to_string().contains("RTC calendar access"));
        assert_eq!(machine.step(), Err(error));
        assert_eq!(machine.cpu(), &cpu);
        assert_eq!(machine.memory().cartridge, cartridge);
        assert_eq!(machine.memory().read16(GPIO_DATA).unwrap(), 6);
        assert_eq!(machine.cycles(), 0);
    }
}

#[test]
fn block_store_failure_retains_gpio_and_cpu_state() {
    // First word targets DATA/DIRECTION. Second reaches unmapped padding after CONTROL.
    let (cpu, mut memory) = prepared(
        false,
        &[0xe8a00006],
        &[(0, GPIO_DATA), (1, 0x00030002), (2, 1)],
    );
    memory.set_cartridge_hardware(CartridgeHardware::Rtc);
    memory.write16(GPIO_CONTROL, 1).unwrap();
    let cartridge = memory.cartridge;
    let mut machine = Machine::new(cpu, memory);
    let cpu = machine.cpu().clone();
    let error = machine.step().unwrap_err();
    assert_eq!(machine.step(), Err(error));
    assert_eq!(machine.cpu(), &cpu);
    assert_eq!(machine.memory().cartridge, cartridge);
    assert_eq!(machine.cycles(), 0);
}

#[test]
fn preflight_applies_earlier_gpio_direction_changes_to_later_pin_validation() {
    let (_, mut memory) = prepared(false, &[0xe1a00000], &[]);
    memory.set_cartridge_hardware(CartridgeHardware::Rtc);
    memory.write16(GPIO_CONTROL, 1).unwrap();
    let cartridge = memory.cartridge;
    assert!(memory
        .write_words(&[
            (0x02000000, 0x12345678),
            (GPIO_DATA, 0x00070001), // Drive clock high, select low.
            (GPIO_DATA, 0x00070004), // CS rises with clock low: diagnostic.
        ])
        .is_err());
    assert_eq!(memory.cartridge, cartridge);
    assert_eq!(memory.read32(0x02000000).unwrap(), 0);
    memory
        .write_words(&[(GPIO_DATA, 0x00070001), (GPIO_DATA, 0x00070005)])
        .unwrap();
    assert_eq!(memory.read32(GPIO_DATA).unwrap(), 0x00070005);
}

#[test]
fn staged_overlay_is_shared_by_data_and_instruction_reads_then_discarded() {
    let mut memory = Memory::new(vec![0xa5; 0x100]).unwrap();
    memory.set_cartridge_hardware(CartridgeHardware::Rtc);
    memory.begin_timer_step();
    memory.write16(GPIO_CONTROL, 1).unwrap();
    memory.write16(GPIO_DIRECTION, 3).unwrap();
    memory.write16(GPIO_DATA, 1).unwrap();
    assert_eq!(memory.read32(GPIO_DATA).unwrap(), 0x00030001);
    assert_eq!(
        memory
            .fetch_instruction(GPIO_DATA, InstructionSet::Arm)
            .instruction,
        Ok(0x00030001)
    );
    assert_eq!(memory.cartridge.read8(GPIO_DATA), None);
    memory.discard_timer_step();
    assert_eq!(memory.read32(GPIO_DATA).unwrap(), 0xa5a5a5a5);
    assert_eq!(
        memory
            .fetch_instruction(GPIO_DATA, InstructionSet::Arm)
            .instruction,
        Ok(0xa5a5a5a5)
    );
}
