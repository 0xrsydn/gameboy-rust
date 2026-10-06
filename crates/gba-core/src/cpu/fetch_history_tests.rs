//! Original sample-consumption tests. The split fetch/execute is a test-only mutation seam,
//! not a claim that DMA can interrupt an instruction in the current scheduler.
use super::*;

const UNUSED: u32 = 0x8000_0000;
const DATA: u32 = 0x0300_1000;
const OLD: u32 = 0x1234_5678;
const NEW: u32 = 0x9abc_def0;

#[test]
fn arm_open_bus_uses_the_sample_already_taken_for_the_pipeline() {
    for pc in [0x0200_0100, 0x0300_0100, 0x0700_0100] {
        let mut bus = Memory::new(vec![]).unwrap();
        bus.write32(pc, 0xe590_6000).unwrap(); // LDR r6,[r0].
        bus.write32(pc + 8, OLD).unwrap();
        let mut cpu = Cpu::new(pc);
        cpu.registers[0] = UNUSED;
        let fetched = cpu.fetch(&bus).unwrap();
        bus.write32(pc + 8, NEW).unwrap();
        cpu.execute_fetched(fetched, &mut bus).unwrap();
        assert_eq!(cpu.registers[6], OLD);
        assert_eq!(bus.read32(pc + 8).unwrap(), NEW);
        assert_eq!(bus.cycles(), 0);
    }
}

#[test]
fn thumb_narrow_open_bus_uses_the_sample_already_taken_for_the_pipeline() {
    for pc in [0x0200_0100, 0x0500_0100, 0x0600_0100] {
        for alignment in [0, 2] {
            let pc = pc + alignment;
            let mut bus = Memory::new(vec![]).unwrap();
            bus.write16(pc, 0x6806).unwrap(); // LDR r6,[r0].
            bus.write16(pc + 4, OLD as u16).unwrap();
            let mut cpu = Cpu::new(pc);
            cpu.instruction_set = InstructionSet::Thumb;
            cpu.registers[0] = UNUSED;
            let fetched = cpu.fetch(&bus).unwrap();
            bus.write16(pc + 4, NEW as u16).unwrap();
            cpu.execute_fetched(fetched, &mut bus).unwrap();
            assert_eq!(cpu.registers[6], 0x5678_5678);
        }
    }
}

#[test]
fn thumb_wide_open_bus_retains_both_lanes_of_the_fetch_sample() {
    for pc in [0x0700_0100, 0x0700_0102] {
        let mut bus = Memory::new(vec![]).unwrap();
        bus.write16(pc, 0x6806).unwrap();
        bus.write32((pc + 4) & !3, OLD).unwrap();
        let mut cpu = Cpu::new(pc);
        cpu.instruction_set = InstructionSet::Thumb;
        cpu.registers[0] = UNUSED;
        let fetched = cpu.fetch(&bus).unwrap();
        bus.write32((pc + 4) & !3, NEW).unwrap();
        cpu.execute_fetched(fetched, &mut bus).unwrap();
        assert_eq!(cpu.registers[6], OLD);
    }
}

#[test]
fn iwram_lane_history_uses_new_fetch_data_not_retained_opcodes_or_reread_bytes() {
    for pc in [0x0300_0100, 0x0300_0102] {
        let mut bus = Memory::new(vec![]).unwrap();
        for (index, half) in [0x680a, 0x6806, 0x46c0, 0x5678].into_iter().enumerate() {
            bus.write16(pc + index as u32 * 2, half).unwrap();
        }
        bus.write32(DATA, 0x1122_3344).unwrap();
        let mut cpu = Cpu::new(pc);
        cpu.instruction_set = InstructionSet::Thumb;
        cpu.registers[0] = UNUSED;
        cpu.registers[1] = DATA;
        cpu.step(&mut bus).unwrap(); // Data read establishes all local bus lanes.
        let fetched = cpu.fetch(&bus).unwrap();
        bus.write16(pc + 6, 0xdef0).unwrap();
        cpu.execute_fetched(fetched, &mut bus).unwrap();
        let expected = if pc & 2 == 0 {
            0x5678_3344
        } else {
            0x1122_5678
        };
        assert_eq!(cpu.registers[6], expected);
    }
}

#[test]
fn failed_execution_discards_its_sample_and_retry_takes_a_new_one() {
    let pc = 0x0300_0100;
    let mut bus = Memory::new(vec![]).unwrap();
    for (index, half) in [0x680a, 0x6823, 0x6806, 0x5678, 0x9abc]
        .into_iter()
        .enumerate()
    {
        bus.write16(pc + index as u32 * 2, half).unwrap();
    }
    bus.write32(DATA, 0x1122_3344).unwrap();
    let mut cpu = Cpu::new(pc);
    cpu.instruction_set = InstructionSet::Thumb;
    cpu.registers[0] = UNUSED;
    cpu.registers[1] = DATA;
    cpu.registers[4] = 0x0e00_0000;
    cpu.step(&mut bus).unwrap();
    let before = cpu.clone();
    let fetched = cpu.fetch(&bus).unwrap();
    assert!(cpu.execute_fetched(fetched, &mut bus).is_err());
    assert_eq!(cpu, before);
    bus.write16(pc + 6, 0xdef0).unwrap();
    cpu.registers[4] = UNUSED; // Retry reads open bus instead of unsupported save memory.
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.registers[3], 0xdef0_3344);
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.registers[6], 0xdef0_9abc);
}
