//! Original regressions for the ARM unused-memory open-bus subset.
use super::*;
use crate::{
    dma::{DmaError, DMA_BASE, DMA_STRIDE},
    io::TIMER_BASE,
    machine::{Machine, MachineError, StepKind},
    memory::{BIOS_SIZE, ROM_START},
    timing::{bus_cycles, AccessKind, AccessWidth},
};

const DATA: u32 = 0xf233_80a5;
const UNUSED: u32 = 0x8000_0000;
const RAM: u32 = 0x0200_0100;
const LDR: u32 = 0xe590_1000; // LDR r1, [r0]

fn memory(instruction: u32) -> Memory {
    let words = [instruction, 0xe1a0_0000, DATA];
    Memory::new(words.into_iter().flat_map(u32::to_le_bytes).collect()).unwrap()
}

#[test]
fn arm_unused_ranges_return_pc_plus_eight_not_address_mirrors() {
    for address in [
        0x0000_4000,
        0x0000_fffc,
        0x00ff_fffc,
        0x0100_0000,
        0x01ff_fffc,
        0x1000_0000,
        0x1800_0000,
        0x8000_0000,
        0xffff_fffc,
    ] {
        let mut bus = memory(LDR);
        let mut cpu = Cpu::new(ROM_START);
        cpu.registers[0] = address;
        let mut expected = cpu.clone();
        expected.registers[1] = DATA;
        expected.registers[15] += 4;
        cpu.step(&mut bus).unwrap();
        assert_cpu_arch_eq!(cpu, expected, "{address:#010x}");
        // Inspection outside CPU execution remains strict, including every lane.
        for low in 0..4 {
            assert_eq!(
                bus.read8(address + low),
                Err(MemoryError::Unmapped(address + low))
            );
        }
        assert_eq!(bus.read16(address), Err(MemoryError::Unmapped(address)));
        assert_eq!(bus.read32(address), Err(MemoryError::Unmapped(address)));
    }
}

#[test]
fn open_bus_loads_select_lanes_rotate_and_sign_extend_normally() {
    // Independent expectations for bytes [a5,80,33,f2].
    for (instruction, values) in [
        (LDR, [0xf233_80a5, 0xa5f2_3380, 0x80a5_f233, 0x3380_a5f2]),
        (0xe5d0_1000, [0xa5, 0x80, 0x33, 0xf2]), // LDRB
        (0xe1d0_10b0, [0x80a5, 0xa500_0080, 0xf233, 0x3300_00f2]), // LDRH
        (0xe1d0_10d0, [0xffff_ffa5, 0xffff_ff80, 0x33, 0xffff_fff2]), // LDRSB
        (
            0xe1d0_10f0,
            [0xffff_80a5, 0xffff_ff80, 0xffff_f233, 0xffff_fff2],
        ), // LDRSH
    ] {
        for (low, value) in values.into_iter().enumerate() {
            let mut bus = memory(instruction);
            let mut cpu = Cpu::new(ROM_START);
            cpu.registers[0] = UNUSED + low as u32;
            cpu.apply_status(0xb000_00df, Mode::System);
            let mut expected = cpu.clone();
            expected.registers[1] = value;
            expected.registers[15] += 4;
            let timing = cpu.step_timed(&mut bus).unwrap();
            assert_cpu_arch_eq!(cpu, expected, "{instruction:#010x}, lane {low}");
            assert_eq!(timing.data_cycles, 1);
            assert_eq!(timing.internal_cycles, 1);
            assert_eq!(timing.code_cycles, 8); // ROM boundary; no extra lookahead cost
            assert_eq!(bus.cycles(), 0);
        }
    }
}

#[test]
fn prefetch_snapshot_uses_the_executing_region_and_rom_window() {
    for pc in [
        0x100,
        RAM,
        0x0300_0100,
        0x0500_0100,
        0x0600_0100,
        0x0700_0100,
        ROM_START,
        0x0a00_0000,
        0x0c00_0000,
    ] {
        let mut bus = if pc < BIOS_SIZE as u32 {
            let mut bios = vec![0; BIOS_SIZE];
            bios[pc as usize..pc as usize + 4].copy_from_slice(&LDR.to_le_bytes());
            bios[pc as usize + 8..pc as usize + 12].copy_from_slice(&DATA.to_le_bytes());
            Memory::with_bios(vec![], bios).unwrap()
        } else if pc >= ROM_START {
            memory(LDR)
        } else {
            let mut bus = Memory::new(vec![]).unwrap();
            bus.write32(pc, LDR).unwrap();
            bus.write32(pc + 8, DATA).unwrap();
            bus
        };
        let mut cpu = Cpu::new(pc);
        cpu.registers[0] = UNUSED;
        let timing = cpu.step_timed(&mut bus).unwrap();
        assert_eq!(cpu.registers[1], DATA, "{pc:#010x}");
        assert_eq!(timing.data_cycles, 1);
        assert_eq!(
            timing.code_cycles,
            bus_cycles(0, pc, AccessWidth::Word, AccessKind::Sequential)
        );
    }
}

#[test]
fn rrx_offset_load_preserves_carry_and_writes_back_the_high_address() {
    let mut bus = memory(0xe7b1_2060); // LDR r2, [r1, r0, RRX]!
    let mut cpu = Cpu::new(ROM_START);
    cpu.flags.carry = true;
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.registers[0], 0);
    assert_eq!(cpu.registers[1], UNUSED);
    assert_eq!(cpu.registers[2], DATA);
    assert_eq!(cpu.cpsr(), 0x2000_001f);
}

#[test]
fn block_loads_share_one_prefetch_word_and_single_load_aliases_still_work() {
    let mut bus = memory(0xe8b0_000e); // LDMIA r0!, {r1-r3}
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[0] = UNUSED;
    let timing = cpu.step_timed(&mut bus).unwrap();
    assert_eq!(&cpu.registers[1..4], &[DATA; 3]);
    assert_eq!(cpu.registers[0], UNUSED + 12);
    assert_eq!(timing.data_cycles, 3);

    let mut bus = memory(0xe5b0_0004); // LDR r0, [r0, #4]!
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[0] = UNUSED;
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.registers[0], DATA);
}

#[test]
fn pc_load_uses_old_pc_snapshot_then_resamples_at_the_branch_target() {
    let mut bus = memory(0xe590_f000); // LDR pc, [r0]
    bus.write32(RAM, 0xe590_f000).unwrap();
    bus.write32(RAM + 8, RAM + 0x40).unwrap();
    bus.write32(RAM + 0x40, LDR).unwrap();
    bus.write32(RAM + 0x48, DATA).unwrap();
    let mut cpu = Cpu::new(RAM);
    cpu.registers[0] = UNUSED;
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.pc(), RAM + 0x40);
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.registers[1], DATA);
}

#[test]
fn missing_prefetch_only_errors_when_an_unused_read_needs_it() {
    for size in 4..12 {
        let mut rom = vec![0; size];
        rom[..4].copy_from_slice(&LDR.to_le_bytes());
        let mut bus = Memory::new(rom).unwrap();
        bus.write32(RAM, DATA).unwrap();
        let mut cpu = Cpu::new(ROM_START);
        cpu.registers[0] = UNUSED;
        let before = cpu.clone();
        assert_eq!(
            cpu.step_timed(&mut bus),
            Err(CpuError::Memory(MemoryError::Unmapped(UNUSED)))
        );
        assert_eq!(cpu, before);
        assert_eq!(bus.read32(UNUSED), Err(MemoryError::Unmapped(UNUSED)));
        cpu.registers[0] = RAM;
        let timing = cpu.step_timed(&mut bus).unwrap();
        assert_eq!(cpu.registers[1], DATA);
        assert_eq!(timing.data_cycles, 6); // No trace leaked from failed step.
    }
    // PC+8 is itself unused. Do not recursively manufacture a prefetch word.
    let mut bios = vec![0; BIOS_SIZE];
    bios[BIOS_SIZE - 8..BIOS_SIZE - 4].copy_from_slice(&LDR.to_le_bytes());
    let mut bus = Memory::with_bios(vec![], bios).unwrap();
    let mut cpu = Cpu::new(BIOS_SIZE as u32 - 8);
    cpu.registers[0] = UNUSED;
    assert_eq!(
        cpu.step(&mut bus),
        Err(CpuError::Memory(MemoryError::Unmapped(UNUSED)))
    );
}

#[test]
fn skipped_loads_need_no_prefetch_and_charge_no_data_cycles() {
    let mut bus = Memory::new((LDR & 0x0fff_ffff).to_le_bytes().to_vec()).unwrap(); // LDREQ
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[0] = UNUSED;
    let timing = cpu.step_timed(&mut bus).unwrap();
    assert_eq!(cpu.pc(), ROM_START + 4);
    assert_eq!(cpu.registers[1], 0);
    assert_eq!(timing.data_cycles, 0);
    assert_eq!(timing.internal_cycles, 0);
}

#[test]
fn missing_bios_rom_save_and_unsupported_io_reads_remain_errors() {
    for address in [
        0,
        0x3ffc,
        0x0400_00e0,
        ROM_START + 12,
        0x0e00_0000,
        0x0fff_fffc,
    ] {
        let mut bus = memory(LDR);
        let mut cpu = Cpu::new(ROM_START);
        cpu.registers[0] = address;
        let before = cpu.clone();
        assert_eq!(
            cpu.step(&mut bus),
            Err(CpuError::Memory(MemoryError::Unmapped(address)))
        );
        assert_eq!(cpu, before);
        assert_eq!(bus.read32(UNUSED), Err(MemoryError::Unmapped(UNUSED)));
    }
}

#[test]
fn cold_iwram_thumb_reads_and_unused_fetches_do_not_reuse_arm_context() {
    let mut bus = memory(LDR);
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[0] = UNUSED;
    cpu.step(&mut bus).unwrap();
    // A successful ARM read must not leave a context for later fetches or Thumb.
    let mut invalid = Cpu::new(UNUSED);
    assert_eq!(
        invalid.step(&mut bus),
        Err(CpuError::Memory(MemoryError::Unmapped(UNUSED)))
    );
    bus.write16(0x0300_0100, 0x6801).unwrap(); // IWRAM Thumb LDR r1,[r0]
    cpu.registers[15] = 0x0300_0100;
    cpu.instruction_set = InstructionSet::Thumb;
    let before = cpu.clone();
    assert_eq!(
        cpu.step(&mut bus),
        Err(CpuError::Memory(MemoryError::Unmapped(UNUSED)))
    );
    assert_eq!(cpu, before);
}

#[test]
fn unused_stores_and_swaps_remain_atomic_diagnostics() {
    for instruction in [0xe580_1000, 0xe5c0_1000, 0xe1c0_10b0, 0xe100_1092] {
        let mut bus = memory(instruction); // STR, STRB, STRH, SWP
        let mut cpu = Cpu::new(ROM_START);
        cpu.registers[0] = UNUSED;
        cpu.registers[1] = 0x1234;
        let before = cpu.clone();
        assert_eq!(
            cpu.step(&mut bus),
            Err(CpuError::Memory(MemoryError::Unmapped(UNUSED)))
        );
        assert_eq!(cpu, before);
        assert_eq!(bus.read32(UNUSED), Err(MemoryError::Unmapped(UNUSED)));
    }
}

#[test]
fn machine_charges_one_data_access_and_dma_does_not_inherit_cpu_context() {
    let mut bus = memory(LDR);
    bus.write32(TIMER_BASE, 0x0080_0000).unwrap();
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[0] = UNUSED;
    let mut machine = Machine::new(cpu, bus);
    assert_eq!(machine.step().unwrap(), StepKind::Instruction);
    assert_eq!(machine.cpu().registers[1], DATA);
    assert_eq!(machine.cycles(), 10); // 8 code + 1 data + 1 internal
    assert_eq!(machine.memory().read16(TIMER_BASE).unwrap(), 10);
    let before = machine.cpu().clone();
    let timing = machine.last_timing();
    // DMA3 exposes all 28 source address bits. Save memory is still unsupported.
    let dma = DMA_BASE + 3 * DMA_STRIDE;
    machine.memory_mut().write32(dma, 0x0e00_0000).unwrap();
    machine.memory_mut().write32(dma + 4, RAM).unwrap();
    machine.memory_mut().write32(dma + 8, 0x8400_0001).unwrap();
    assert_eq!(
        machine.step(),
        Err(MachineError::Dma(DmaError::Memory {
            channel: 3,
            error: MemoryError::Unmapped(0x0e00_0000),
        }))
    );
    assert_eq!(machine.cpu(), &before);
    assert_eq!(machine.cycles(), 10);
    assert_eq!(machine.last_timing(), timing);
    assert_eq!(machine.memory().read32(RAM).unwrap(), 0);
}
