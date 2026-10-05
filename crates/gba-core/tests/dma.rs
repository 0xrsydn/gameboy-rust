use gba_core::{
    cpu::{Cpu, Mode},
    display::{CYCLES_PER_FRAME, CYCLES_PER_LINE, HBLANK_START, VBLANK_START},
    dma::{DmaError, DMA_BASE, DMA_STRIDE},
    io::{DISPCNT, DISPSTAT, IE, IF, IME, TIMER_BASE, WAITCNT},
    machine::{FrameRunError, Machine, MachineError, StepKind},
    memory::{Memory, MemoryError, BIOS_SIZE, PALETTE_START, ROM_START, VRAM_START},
    timing::{bus_cycles, AccessKind, AccessWidth},
};

const SOURCE: u32 = 0x0200_0000;
const DEST: u32 = 0x0300_0000;

fn words(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn machine() -> Machine {
    Machine::new(
        Cpu::new(ROM_START),
        Memory::new(words(&[0xeaff_fffe])).unwrap(),
    )
}

fn base(channel: usize) -> u32 {
    DMA_BASE + channel as u32 * DMA_STRIDE
}

fn configure(
    bus: &mut Memory,
    channel: usize,
    source: u32,
    destination: u32,
    count: u16,
    control: u16,
) {
    bus.write32(base(channel), source).unwrap();
    bus.write32(base(channel) + 4, destination).unwrap();
    bus.write32(
        base(channel) + 8,
        u32::from(count) | (u32::from(control | 0x8000) << 16),
    )
    .unwrap();
}

fn advance_to(machine: &mut Machine, cycle: u64) {
    let elapsed = (cycle - machine.cycles()) as u32;
    machine.memory_mut().advance_cycles(elapsed);
}

fn enabled(machine: &Machine, channel: usize) -> bool {
    machine.memory().read16(base(channel) + 10).unwrap() & 0x8000 != 0
}

fn units(machine: &mut Machine, channel: usize, count: usize) {
    let cpu = machine.cpu().clone();
    for _ in 0..count {
        assert_eq!(machine.step().unwrap(), StepKind::Dma { channel });
        assert_eq!(machine.last_timing().code_cycles, 0);
        assert_eq!(machine.cpu(), &cpu);
    }
}

#[test]
fn reset_register_layout_masks_and_write_only_readback() {
    let mut bus = Memory::new(vec![]).unwrap();
    for channel in 0..4 {
        for offset in 0..12 {
            assert_eq!(bus.read8(base(channel) + offset).unwrap(), 0);
            bus.write8(base(channel) + offset, 0xff).unwrap();
        }
        assert_eq!(bus.read32(base(channel)).unwrap(), 0);
        assert_eq!(bus.read32(base(channel) + 4).unwrap(), 0);
        assert_eq!(bus.read16(base(channel) + 8).unwrap(), 0);
        assert_eq!(
            bus.read16(base(channel) + 10).unwrap(),
            if channel == 3 { 0xffe0 } else { 0xf7e0 }
        );
        assert_eq!(
            bus.read32(base(channel) + 8).unwrap(),
            if channel == 3 {
                0xffe0_0000
            } else {
                0xf7e0_0000
            }
        );
        bus.write8(base(channel) + 11, 0).unwrap();
        assert_eq!(bus.read16(base(channel) + 10).unwrap(), 0xe0);
    }
    assert_eq!(
        bus.read8(DMA_BASE - 1),
        Err(MemoryError::Unmapped(DMA_BASE - 1))
    );
    assert_eq!(
        bus.read8(DMA_BASE + 48),
        Err(MemoryError::Unmapped(DMA_BASE + 48))
    );
    assert_eq!(
        bus.read16(DMA_BASE + 1),
        Err(MemoryError::Unaligned(DMA_BASE + 1))
    );
    assert_eq!(
        bus.write32(DMA_BASE + 10, 1),
        Err(MemoryError::Unaligned(DMA_BASE + 10))
    );
    assert_eq!(
        bus.read16(DMA_BASE + 0x400),
        Err(MemoryError::Unmapped(DMA_BASE + 0x400))
    );
}

#[test]
fn immediate_halfword_and_word_copies_pause_cpu_until_completion() {
    for channel in 0..4 {
        for word in [false, true] {
            let mut machine = machine();
            machine.memory_mut().write32(SOURCE, 0x4433_2211).unwrap();
            machine
                .memory_mut()
                .write32(SOURCE + 4, 0x8877_6655)
                .unwrap();
            configure(
                machine.memory_mut(),
                channel,
                SOURCE,
                DEST,
                if word { 2 } else { 4 },
                if word { 0x400 } else { 0 },
            );
            assert_eq!(machine.memory().read32(DEST).unwrap(), 0); // Register writes do not execute DMA.
            assert_eq!(machine.cycles(), 0);
            units(&mut machine, channel, if word { 2 } else { 4 });
            assert!(!enabled(&machine, channel));
            assert_eq!(machine.memory().read32(DEST).unwrap(), 0x4433_2211);
            assert_eq!(machine.memory().read32(DEST + 4).unwrap(), 0x8877_6655);
            assert_eq!(machine.memory().read16(IF).unwrap(), 0);
            assert_eq!(machine.cycles(), if word { 16 } else { 18 });
            assert_eq!(machine.step().unwrap(), StepKind::Instruction);
        }
    }
}

#[test]
fn source_and_destination_address_modes_match_reference_copies() {
    for word in [false, true] {
        let width = if word { 4 } else { 2 };
        for source_mode in 0..3 {
            for dest_mode in 0..4 {
                let mut machine = machine();
                let mut expected = [0_u32; 9];
                for index in 0..9 {
                    machine
                        .memory_mut()
                        .write32(SOURCE + index * 4, 0x0123_0123 * (index + 1))
                        .unwrap();
                }
                let mut source = SOURCE + 16;
                let mut destination = DEST + 16;
                // Read the source and simulate the destination independently of DMA state.
                let mut expected_bytes = [0_u8; 36];
                for _ in 0..4 {
                    for byte in 0..width {
                        expected_bytes[(destination - DEST + byte) as usize] =
                            machine.memory().read8(source + byte).unwrap();
                    }
                    source = match source_mode {
                        0 => source + width,
                        1 => source - width,
                        _ => source,
                    };
                    destination = match dest_mode {
                        0 | 3 => destination + width,
                        1 => destination - width,
                        _ => destination,
                    };
                }
                for (index, chunk) in expected_bytes.chunks_exact(4).enumerate() {
                    expected[index] = u32::from_le_bytes(chunk.try_into().unwrap());
                }
                let control = source_mode << 7 | dest_mode << 5 | if word { 0x400 } else { 0 };
                configure(machine.memory_mut(), 3, SOURCE + 16, DEST + 16, 4, control);
                units(&mut machine, 3, 4);
                for (index, value) in expected.into_iter().enumerate() {
                    assert_eq!(
                        machine.memory().read32(DEST + index as u32 * 4).unwrap(),
                        value
                    );
                }
            }
        }
    }
}

#[test]
fn addresses_mask_channel_bits_and_align_to_transfer_width() {
    for channel in 0..4 {
        for word in [false, true] {
            let mut machine = machine();
            machine.memory_mut().write32(SOURCE, 0xaabb_ccdd).unwrap();
            let source_high = if channel == 0 {
                0xf800_0000
            } else {
                0xf000_0000
            };
            let dest_high = if channel == 3 {
                0xf000_0000
            } else {
                0xf800_0000
            };
            configure(
                machine.memory_mut(),
                channel,
                SOURCE | source_high | 3,
                DEST | dest_high | 3,
                1,
                if word { 0x400 } else { 0 },
            );
            units(&mut machine, channel, 1);
            assert_eq!(
                machine.memory().read32(DEST).unwrap(),
                if word { 0xaabb_ccdd } else { 0xaabb_0000 }
            );
        }
    }
}

#[test]
fn zero_count_means_channel_maximum_and_count_high_bits_are_masked() {
    for (channel, count, expected) in [
        (0, 0, 0x4000),
        (1, 0xc000, 0x4000),
        (2, 0xc003, 3),
        (3, 0, 0x10000),
    ] {
        let mut machine = machine();
        machine.memory_mut().write16(SOURCE, 0x5a5a).unwrap();
        configure(machine.memory_mut(), channel, SOURCE, DEST, count, 0x140); // Both addresses fixed.
        units(&mut machine, channel, expected - 1);
        assert!(enabled(&machine, channel));
        units(&mut machine, channel, 1);
        assert!(!enabled(&machine, channel));
        assert_eq!(machine.memory().read16(DEST).unwrap(), 0x5a5a);
        assert_eq!(machine.cycles(), expected as u64 * 4 + 2);
    }
}

#[test]
fn immediate_repeat_completes_once_and_does_not_stall_forever() {
    let mut machine = machine();
    configure(machine.memory_mut(), 0, SOURCE, DEST, 1, 0x200);
    units(&mut machine, 0, 1);
    assert!(!enabled(&machine, 0));
    assert_eq!(machine.step().unwrap(), StepKind::Instruction);
}

#[test]
fn address_and_count_latches_change_only_on_enable_edge() {
    let mut machine = machine();
    machine.memory_mut().write32(SOURCE, 0x2222_1111).unwrap();
    machine.memory_mut().write16(SOURCE + 16, 0x3333).unwrap();
    configure(machine.memory_mut(), 2, SOURCE, DEST, 2, 0);
    machine.memory_mut().write32(base(2), SOURCE + 16).unwrap();
    machine
        .memory_mut()
        .write32(base(2) + 4, DEST + 16)
        .unwrap();
    machine.memory_mut().write16(base(2) + 8, 1).unwrap();
    machine.memory_mut().write16(base(2) + 10, 0x8000).unwrap(); // Still enabled, not a new edge.
    units(&mut machine, 2, 2);
    assert_eq!(machine.memory().read32(DEST).unwrap(), 0x2222_1111);
    assert_eq!(machine.memory().read16(DEST + 16).unwrap(), 0);
    machine.memory_mut().write16(base(2) + 10, 0x8000).unwrap();
    units(&mut machine, 2, 1);
    assert_eq!(machine.memory().read16(DEST + 16).unwrap(), 0x3333);
    assert!(!enabled(&machine, 2));
}

#[test]
fn disabling_cancels_pending_dma_and_reenable_reloads_latches() {
    let mut machine = machine();
    machine.memory_mut().write32(SOURCE, 0xabcd_1234).unwrap();
    configure(machine.memory_mut(), 1, SOURCE, DEST, 2, 0);
    units(&mut machine, 1, 1);
    machine.memory_mut().write8(base(1) + 11, 0).unwrap();
    assert_eq!(machine.step().unwrap(), StepKind::Instruction);
    assert_eq!(machine.memory().read16(DEST + 2).unwrap(), 0);
    machine.memory_mut().write8(base(1) + 11, 0x80).unwrap();
    units(&mut machine, 1, 2);
    assert_eq!(machine.memory().read32(DEST).unwrap(), 0xabcd_1234);
}

#[test]
fn simultaneous_channels_run_in_priority_order_before_cpu_or_irq() {
    let mut machine = machine();
    machine.memory_mut().write16(IE, 0x0f00).unwrap();
    machine.memory_mut().write16(IME, 1).unwrap();
    for channel in (0..4).rev() {
        machine
            .memory_mut()
            .write16(SOURCE + channel as u32 * 2, channel as u16 + 1)
            .unwrap();
        configure(
            machine.memory_mut(),
            channel,
            SOURCE + channel as u32 * 2,
            DEST,
            1,
            0x4000,
        );
    }
    for channel in 0..4 {
        units(&mut machine, channel, 1);
        assert_eq!(machine.memory().read16(DEST).unwrap(), channel as u16 + 1);
        assert_eq!(
            machine.memory().read16(IF).unwrap(),
            ((1 << (channel + 1)) - 1) << 8
        );
    }
    assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
}

#[test]
fn display_request_preempts_lower_priority_dma_at_unit_boundary() {
    let mut machine = machine();
    machine.memory_mut().write16(SOURCE, 0x1111).unwrap();
    machine.memory_mut().write16(SOURCE + 2, 0x3333).unwrap();
    configure(machine.memory_mut(), 0, SOURCE, DEST, 1, 0x2000); // HBlank.
    configure(machine.memory_mut(), 3, SOURCE + 2, DEST, 3, 0x140); // Immediate fixed fill.
    machine.memory_mut().advance_cycles(HBLANK_START - 1);
    units(&mut machine, 3, 1); // Crosses HBlank with DMA0 pending.
    units(&mut machine, 0, 1);
    assert_eq!(machine.memory().read16(DEST).unwrap(), 0x1111);
    units(&mut machine, 3, 2); // Resumes the lower-priority block.
    assert_eq!(machine.memory().read16(DEST).unwrap(), 0x3333);
    assert!(!enabled(&machine, 3));
}

#[test]
fn blank_requests_do_not_depend_on_dispstat_or_forced_blank() {
    for (timing, edge) in [(0x1000, VBLANK_START), (0x2000, HBLANK_START)] {
        let mut machine = machine();
        configure(machine.memory_mut(), 0, SOURCE, DEST, 1, timing);
        machine.memory_mut().write16(DISPCNT, 0x80).unwrap();
        machine.memory_mut().advance_cycles(edge - 1);
        assert_eq!(machine.memory().read16(DISPSTAT).unwrap() & 0x38, 0);
        assert_eq!(machine.step().unwrap(), StepKind::Instruction); // Clock crosses edge.
        units(&mut machine, 0, 1);
        assert_eq!(machine.memory().read16(IF).unwrap(), 0);
    }
}

#[test]
fn enabling_during_blank_waits_for_next_edge_and_zero_advance_does_not_trigger() {
    for (timing, edge, period) in [
        (0x1000, VBLANK_START, CYCLES_PER_FRAME),
        (0x2000, HBLANK_START, CYCLES_PER_LINE),
    ] {
        let mut machine = machine();
        machine.memory_mut().advance_cycles(edge);
        configure(machine.memory_mut(), 0, SOURCE, DEST, 1, timing);
        machine.memory_mut().advance_cycles(0);
        assert_eq!(machine.step().unwrap(), StepKind::Instruction);
        let elapsed = (machine.cycles() - u64::from(edge)) as u32;
        machine.memory_mut().advance_cycles(period - elapsed);
        units(&mut machine, 0, 1);
    }
}

#[test]
fn hblank_dma_runs_on_all_visible_lines_but_not_hidden_lines() {
    let mut machine = machine();
    configure(machine.memory_mut(), 1, SOURCE, DEST, 1, 0x2340); // Repeat HBlank, fixed addresses.
    machine.memory_mut().write16(DISPSTAT, 0x10).unwrap();
    for line in 0..228_u64 {
        let edge = line * u64::from(CYCLES_PER_LINE) + u64::from(HBLANK_START);
        advance_to(&mut machine, edge);
        assert_eq!(machine.memory().read16(IF).unwrap(), 2); // HBlank IRQ still occurs on hidden lines.
        machine.memory_mut().write16(IF, 2).unwrap();
        if line < 160 {
            units(&mut machine, 1, 1);
        } else {
            assert_eq!(machine.step().unwrap(), StepKind::Instruction);
        }
    }
    let edge = u64::from(CYCLES_PER_FRAME + HBLANK_START);
    advance_to(&mut machine, edge);
    units(&mut machine, 1, 1);
}

#[test]
fn vblank_repeat_preserves_source_reloads_destination_and_count() {
    let mut machine = machine();
    machine.memory_mut().write32(SOURCE, 0x2222_1111).unwrap();
    machine
        .memory_mut()
        .write32(SOURCE + 4, 0x4444_3333)
        .unwrap();
    configure(machine.memory_mut(), 3, SOURCE, DEST, 2, 0x5260); // VBlank, repeat, dest reload, IRQ.
    machine.memory_mut().advance_cycles(VBLANK_START);
    units(&mut machine, 3, 2);
    assert_eq!(machine.memory().read32(DEST).unwrap(), 0x2222_1111);
    assert!(enabled(&machine, 3));
    assert_eq!(machine.memory().read16(IF).unwrap(), 0x800);
    machine.memory_mut().write16(IF, 0x800).unwrap();
    assert_eq!(machine.step().unwrap(), StepKind::Instruction); // Same VBlank does not retrigger.
    let next = u64::from(VBLANK_START + CYCLES_PER_FRAME);
    advance_to(&mut machine, next);
    units(&mut machine, 3, 2);
    assert_eq!(machine.memory().read32(DEST).unwrap(), 0x4444_3333);
    assert_eq!(machine.memory().read16(IF).unwrap(), 0x800);
    assert!(enabled(&machine, 3));
}

#[test]
fn repeated_increment_destination_continues_and_reload_uses_programmed_values() {
    for reload in [false, true] {
        let mut machine = machine();
        machine.memory_mut().write32(SOURCE, 0x2222_1111).unwrap();
        machine
            .memory_mut()
            .write32(SOURCE + 4, 0x4444_3333)
            .unwrap();
        configure(
            machine.memory_mut(),
            0,
            SOURCE,
            DEST,
            1,
            0x1200 | if reload { 0x60 } else { 0 },
        );
        // Initial count/address are latched. Repeat reloads use newly programmed values.
        machine.memory_mut().write16(base(0) + 8, 2).unwrap();
        machine
            .memory_mut()
            .write32(base(0) + 4, DEST + 16)
            .unwrap();
        machine.memory_mut().advance_cycles(VBLANK_START);
        units(&mut machine, 0, 1);
        assert_eq!(machine.memory().read16(DEST).unwrap(), 0x1111);
        let next = u64::from(VBLANK_START + CYCLES_PER_FRAME);
        advance_to(&mut machine, next);
        units(&mut machine, 0, 2);
        let address = if reload { DEST + 16 } else { DEST + 2 };
        assert_eq!(machine.memory().read16(address).unwrap(), 0x2222);
        assert_eq!(machine.memory().read16(address + 2).unwrap(), 0x3333);
    }
}

#[test]
fn pending_requests_coalesce_and_busy_channel_does_not_queue_repeats() {
    let mut machine = machine();
    configure(machine.memory_mut(), 0, SOURCE, DEST, 1, 0x1200);
    machine.memory_mut().advance_cycles(CYCLES_PER_FRAME * 3);
    units(&mut machine, 0, 1);
    assert_eq!(machine.step().unwrap(), StepKind::Instruction);

    let mut machine = self::machine();
    configure(machine.memory_mut(), 0, SOURCE, DEST, 100, 0x2200); // Takes 402 cycles, beyond HBlank.
    machine.memory_mut().advance_cycles(HBLANK_START);
    units(&mut machine, 0, 99);
    // Inject more display edges while the block is still active; no queued repeat.
    machine.memory_mut().advance_cycles(CYCLES_PER_FRAME * 2);
    units(&mut machine, 0, 1);
    assert_eq!(machine.step().unwrap(), StepKind::Instruction);
}

#[test]
fn timer_and_display_clocks_continue_during_dma_and_latch_all_irqs() {
    let mut machine = machine();
    machine
        .memory_mut()
        .write32(TIMER_BASE, 0x00c0_ffff)
        .unwrap();
    machine.memory_mut().write16(DISPSTAT, 0x10).unwrap();
    machine.memory_mut().advance_cycles(HBLANK_START - 2);
    machine.memory_mut().write16(IF, 0x3fff).unwrap();
    configure(machine.memory_mut(), 2, SOURCE, DEST, 1, 0x4000);
    let before = machine.cycles();
    units(&mut machine, 2, 1);
    assert_eq!(machine.cycles(), before + 6);
    assert_eq!(
        machine.memory().display_position().line_cycle,
        HBLANK_START as u16 + 4
    );
    assert_eq!(machine.memory().read16(IF).unwrap(), 0x40a);
    assert!(!machine.memory().irq_pending()); // IE/IME are off, requests still latch.
    machine.memory_mut().write16(IE, 0x400).unwrap();
    machine.memory_mut().write16(IME, 1).unwrap();
    assert!(machine.memory().irq_pending());
}

#[test]
fn dma_completion_irq_enters_handler_acknowledges_and_returns_without_skipping_cpu() {
    for channel in 0..4 {
        let mask = 1_u16 << (8 + channel);
        let mut bios = vec![0; BIOS_SIZE];
        bios[0x18..0x24].copy_from_slice(&words(&[
            0xe1c1_20b2, // STRH r2,[r1,#2] (IF)
            0xe280_0001, // ADD r0,r0,#1
            0xe25e_f004, // SUBS pc,lr,#4
        ]));
        let rom = words(&[
            0xe59f_1008, // LDR r1,[pc,#8] (IE)
            0xe59f_2008, // LDR r2,[pc,#8] (mask)
            0xe283_3001, // ADD r3,r3,#1 (interrupted instruction)
            0xeaff_fffe,
            IE,
            u32::from(mask),
        ]);
        let mut bus = Memory::with_bios(rom, bios).unwrap();
        let mut cpu = Cpu::new(ROM_START);
        cpu.step(&mut bus).unwrap();
        cpu.step(&mut bus).unwrap();
        configure(&mut bus, channel, SOURCE, DEST, 2, 0x4000);
        bus.write16(IE, mask).unwrap();
        bus.write16(IME, 1).unwrap();
        let mut machine = Machine::new(cpu, bus);
        units(&mut machine, channel, 1);
        assert_eq!(machine.memory().read16(IF).unwrap(), 0); // Not yet complete.
        units(&mut machine, channel, 1);
        assert_eq!(machine.memory().read16(IF).unwrap(), mask);
        assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
        assert_eq!(machine.cpu().mode(), Mode::Irq);
        assert_eq!(machine.cpu().registers()[14], ROM_START + 12);
        for _ in 0..3 {
            assert_eq!(machine.step().unwrap(), StepKind::Instruction);
        }
        assert_eq!(machine.memory().read16(IF).unwrap(), 0);
        assert_eq!(machine.cpu().mode(), Mode::System);
        assert_eq!(machine.cpu().pc(), ROM_START + 8);
        assert_eq!(machine.cpu().registers()[0], 1);
        assert_eq!(machine.cpu().registers()[3], 0);
        machine.step().unwrap();
        assert_eq!(machine.cpu().registers()[3], 1);
    }
}

#[test]
fn rom_dma_uses_waitcnt_sequential_costs_and_forces_source_increment() {
    for channel in 1..4 {
        for source_mode in 0..3 {
            for (window, waitcnt) in [(0x0800_0000, 0), (0x0a00_0000, 0x7fff), (0x0c00_0000, 0)] {
                for word in [false, true] {
                    let mut bus = Memory::new(words(&[0x2222_1111, 0x4444_3333])).unwrap();
                    bus.write16(WAITCNT, waitcnt).unwrap();
                    let width = if word {
                        AccessWidth::Word
                    } else {
                        AccessWidth::Halfword
                    };
                    let stride = if word { 4 } else { 2 };
                    configure(
                        &mut bus,
                        channel,
                        window,
                        DEST,
                        2,
                        source_mode << 7 | if word { 0x400 } else { 0 },
                    );
                    let mut machine = Machine::new(Cpu::new(ROM_START), bus);
                    for index in 0..2 {
                        units(&mut machine, channel, 1);
                        let expected = bus_cycles(
                            machine.memory().waitcnt(),
                            window + index * stride,
                            width,
                            if index == 0 {
                                AccessKind::NonSequential
                            } else {
                                AccessKind::Sequential
                            },
                        ) + 1;
                        assert_eq!(machine.last_timing().data_cycles, expected);
                        assert_eq!(
                            machine.last_timing().internal_cycles,
                            if index == 0 { 2 } else { 0 }
                        );
                    }
                    assert_eq!(machine.memory().read32(DEST).unwrap(), 0x2222_1111);
                    if word {
                        assert_eq!(machine.memory().read32(DEST + 4).unwrap(), 0x4444_3333);
                    }
                }
            }
        }
    }
}

#[test]
fn rom_boundary_forces_nonsequential_timing_in_middle_of_dma() {
    let mut machine = Machine::new(
        Cpu::new(ROM_START),
        Memory::new(vec![0x5a; 0x20004]).unwrap(),
    );
    configure(machine.memory_mut(), 3, ROM_START + 0x1fffe, DEST, 3, 0);
    for expected in [8, 6, 4] {
        // First: 5+1+2; boundary: 5+1; then 3+1.
        units(&mut machine, 3, 1);
        assert_eq!(machine.last_timing().total(), expected);
    }
    assert_eq!(machine.memory().read32(DEST).unwrap(), 0x5a5a_5a5a);
}

#[test]
fn overlapping_copies_read_each_unit_after_previous_write() {
    let mut machine = machine();
    machine.memory_mut().write32(SOURCE, 0x2222_1111).unwrap();
    configure(machine.memory_mut(), 0, SOURCE, SOURCE + 2, 3, 0);
    units(&mut machine, 0, 3);
    for offset in [0, 2, 4, 6] {
        assert_eq!(machine.memory().read16(SOURCE + offset).unwrap(), 0x1111);
    }
}

#[test]
fn video_palette_mirrors_and_mapped_io_use_the_normal_bus() {
    for (destination, alias) in [
        (VRAM_START + 0x18000, VRAM_START + 0x10000),
        (PALETTE_START + 0x400, PALETTE_START),
        (DISPCNT, DISPCNT),
    ] {
        let mut machine = machine();
        machine.memory_mut().write32(SOURCE, 0x0001_0403).unwrap();
        configure(machine.memory_mut(), 3, SOURCE, destination, 1, 0x400);
        units(&mut machine, 3, 1);
        assert_eq!(machine.memory().read32(alias).unwrap(), 0x0001_0403);
    }
}

#[test]
fn unsupported_controls_are_explicit_and_leave_state_and_time_unchanged() {
    for channel in 0..4 {
        for control in [0x180, 0x3000, 0x3800] {
            let mut machine = machine();
            configure(machine.memory_mut(), channel, SOURCE, DEST, 1, control);
            let before = machine.cpu().clone();
            let stored = machine.memory().read16(base(channel) + 10).unwrap();
            for _ in 0..2 {
                assert_eq!(
                    machine.step(),
                    Err(MachineError::Dma(DmaError::UnsupportedControl {
                        channel,
                        control: stored
                    }))
                );
                assert_eq!(machine.cpu(), &before);
                assert_eq!(machine.cycles(), 0);
                assert_eq!(machine.last_timing().total(), 0);
                assert_eq!(machine.memory().read32(DEST).unwrap(), 0);
                assert!(enabled(&machine, channel));
            }
        }
    }
    let mut machine = machine();
    configure(machine.memory_mut(), 3, SOURCE, DEST, 1, 0x800); // DRQ even without special timing.
    assert!(matches!(
        machine.step(),
        Err(MachineError::Dma(DmaError::UnsupportedControl { .. }))
    ));
}

#[test]
fn invalid_memory_and_dma_register_destinations_are_diagnostics() {
    for (source, destination, error) in [
        (
            0,
            DEST,
            DmaError::UnsupportedSource {
                channel: 3,
                address: 0,
            },
        ),
        (
            0x0e00_0000,
            DEST,
            DmaError::Memory {
                channel: 3,
                error: MemoryError::Unmapped(0x0e00_0000),
            },
        ),
        (
            SOURCE,
            ROM_START,
            DmaError::Memory {
                channel: 3,
                error: MemoryError::ReadOnly(ROM_START),
            },
        ),
        (
            SOURCE,
            0x0e00_0000,
            DmaError::Memory {
                channel: 3,
                error: MemoryError::Unmapped(0x0e00_0000),
            },
        ),
        (
            SOURCE,
            DMA_BASE,
            DmaError::RegisterDestination {
                channel: 3,
                address: DMA_BASE,
            },
        ),
    ] {
        let mut machine = machine();
        configure(machine.memory_mut(), 3, source, destination, 1, 0x4000);
        let before = machine.cpu().clone();
        assert_eq!(machine.step(), Err(MachineError::Dma(error)));
        assert_eq!(machine.cpu(), &before);
        assert_eq!(machine.cycles(), 0);
        assert_eq!(machine.memory().read16(IF).unwrap(), 0);
        assert!(enabled(&machine, 3));
    }
}

#[test]
fn failing_later_unit_preserves_earlier_data_but_not_partial_current_unit() {
    let mut machine = Machine::new(
        Cpu::new(ROM_START),
        Memory::new(vec![0x11, 0x22, 0x33]).unwrap(),
    );
    configure(machine.memory_mut(), 3, ROM_START, DEST, 2, 0x4000);
    units(&mut machine, 3, 1);
    let timing = machine.last_timing();
    let cycles = machine.cycles();
    let position = machine.memory().display_position();
    for _ in 0..2 {
        assert_eq!(
            machine.step(),
            Err(MachineError::Dma(DmaError::Memory {
                channel: 3,
                error: MemoryError::Unmapped(ROM_START + 3)
            }))
        );
        assert_eq!(machine.memory().read32(DEST).unwrap(), 0x2211);
        assert_eq!(machine.memory().read16(IF).unwrap(), 0);
        assert_eq!(machine.cycles(), cycles);
        assert_eq!(machine.memory().display_position(), position);
        assert_eq!(machine.last_timing(), timing);
        assert!(enabled(&machine, 3));
    }
}

#[test]
fn failed_io_write_does_not_partially_change_display_control() {
    let mut machine = machine();
    machine.memory_mut().write32(SOURCE, 0xffff_ffff).unwrap();
    configure(machine.memory_mut(), 3, SOURCE, 0x0400_0058, 1, 0x400);
    assert!(machine.step().is_err());
    assert_eq!(machine.memory().read16(DISPCNT).unwrap(), 0);
    assert_eq!(machine.cycles(), 0);
}

#[test]
fn cpu_can_enable_dma_with_a_combined_count_control_store() {
    let rom = words(&[
        0xe59f_0010, // LDR r0,[pc,#16] (DMA3CNT)
        0xe59f_1010, // LDR r1,[pc,#16] (count/control)
        0xe580_1000, // STR r1,[r0]
        0xe282_2001, // ADD r2,r2,#1 (must wait for transfer)
        0xeaff_fffe,
        0xe1a0_0000,
        base(3) + 8,
        0x8400_0002,
    ]);
    let mut bus = Memory::new(rom).unwrap();
    bus.write32(SOURCE, 0x1234_5678).unwrap();
    bus.write32(base(3), SOURCE).unwrap();
    bus.write32(base(3) + 4, DEST).unwrap();
    let mut machine = Machine::new(Cpu::new(ROM_START), bus);
    for _ in 0..3 {
        assert_eq!(machine.step().unwrap(), StepKind::Instruction);
    }
    assert_eq!(machine.cpu().pc(), ROM_START + 12);
    units(&mut machine, 3, 2);
    assert_eq!(machine.memory().read32(DEST).unwrap(), 0x1234_5678);
    assert_eq!(machine.cpu().registers()[2], 0);
    machine.step().unwrap();
    assert_eq!(machine.cpu().registers()[2], 1);
}

#[test]
fn failed_cpu_block_store_cannot_enable_dma_or_change_its_hidden_latches() {
    let rom = words(&[
        0xe59f_000c, // LDR r0,[pc,#12] (DMA3CNT)
        0xe59f_100c, // LDR r1,[pc,#12] (count/control)
        0xe8a0_0006, // STMIA r0!,{r1,r2}; second word unmapped.
        0xeaff_fffe,
        0xe1a0_0000,
        base(3) + 8,
        0x8400_0001,
    ]);
    let mut bus = Memory::new(rom).unwrap();
    let mut cpu = Cpu::new(ROM_START);
    cpu.step(&mut bus).unwrap();
    cpu.step(&mut bus).unwrap();
    configure(&mut bus, 3, SOURCE, DEST, 2, 0);
    bus.write16(base(3) + 10, 0).unwrap();
    let before = cpu.clone();
    let mut machine = Machine::new(cpu, bus);
    assert!(matches!(machine.step(), Err(MachineError::Cpu(_))));
    assert_eq!(machine.cpu(), &before);
    assert!(!enabled(&machine, 3));
    assert_eq!(machine.cycles(), 0);
    machine.memory_mut().write16(base(3) + 10, 0x8000).unwrap();
    units(&mut machine, 3, 2); // The count latch is still 2, not 1.
    assert!(!enabled(&machine, 3));
}

#[test]
fn cpu_only_and_clock_only_calls_never_execute_dma() {
    let mut bus = Memory::new(words(&[0xe280_0001])).unwrap();
    bus.write16(SOURCE, 0x1234).unwrap();
    configure(&mut bus, 0, SOURCE, DEST, 1, 0);
    let mut cpu = Cpu::new(ROM_START);
    cpu.step(&mut bus).unwrap();
    bus.advance_cycles(CYCLES_PER_FRAME);
    assert_eq!(cpu.registers()[0], 1);
    assert_eq!(bus.read16(DEST).unwrap(), 0);
    let mut machine = Machine::new(cpu, bus);
    units(&mut machine, 0, 1);
    assert_eq!(machine.memory().read16(DEST).unwrap(), 0x1234);
}

#[test]
fn frame_runner_counts_dma_units_obeys_limits_and_reports_dma_errors() {
    let mut machine = machine();
    configure(machine.memory_mut(), 0, SOURCE, DEST, 3, 0);
    assert_eq!(
        machine.run_until_vblank(2),
        Err(FrameRunError::StepLimit(2))
    );
    assert_eq!(machine.cpu().pc(), ROM_START);
    assert_eq!(machine.cycles(), 10);
    machine.memory_mut().advance_cycles(VBLANK_START - 11);
    assert_eq!(machine.run_until_vblank(1).unwrap(), 1);
    assert_eq!(machine.memory().display_position().scanline, 160);
    assert_eq!(machine.cpu().pc(), ROM_START);
    configure(machine.memory_mut(), 0, SOURCE, DEST, 1, 0x3000);
    assert!(matches!(
        machine.run_until_vblank(1),
        Err(FrameRunError::Dma(_))
    ));
}

#[test]
fn vblank_runner_stops_at_event_before_newly_requested_dma_executes() {
    let mut machine = machine();
    machine.memory_mut().write16(SOURCE, 0x1234).unwrap();
    configure(machine.memory_mut(), 3, SOURCE, DEST, 1, 0x1000);
    machine.memory_mut().advance_cycles(VBLANK_START - 1);
    assert_eq!(machine.run_until_vblank(1).unwrap(), 1);
    assert_eq!(machine.memory().read16(DEST).unwrap(), 0);
    units(&mut machine, 3, 1);
    assert_eq!(machine.memory().read16(DEST).unwrap(), 0x1234);
}
