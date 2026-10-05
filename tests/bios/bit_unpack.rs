use super::*;
use gba_rust::{
    bios::INVALID_ARGUMENT_TRAP,
    dma::DMA_BASE,
    memory::{OAM_START, PALETTE_START},
};

const INFO: u32 = SOURCE + 0x1000;

fn info(m: &mut Machine, length: u16, source_width: u8, dest_width: u8, offset: u32) {
    m.memory_mut().write16(INFO, length).unwrap();
    m.memory_mut().write8(INFO + 2, source_width).unwrap();
    m.memory_mut().write8(INFO + 3, dest_width).unwrap();
    m.memory_mut().write32(INFO + 4, offset).unwrap();
}

fn prepare(
    thumb: bool,
    destination: u32,
    data: &[u8],
    sw: u8,
    dw: u8,
    offset: u32,
) -> (Machine, u32) {
    let (mut m, pc) = call_status_data(0x10, thumb, [ROM_DATA, destination, INFO], 0x1f, data);
    info(&mut m, data.len().try_into().unwrap(), sw, dw, offset);
    (m, pc)
}

/// Expand individual bits and repack words independently of the emitted ARM loops.
fn reference(data: &[u8], sw: u8, dw: u8, offset: u32) -> Vec<u32> {
    let mut bits = Vec::new();
    for byte in data {
        for position in (0..8).step_by(sw as usize) {
            let mut value = 0u64;
            for bit in 0..sw {
                value += u64::from((byte >> (position + bit)) & 1) << bit;
            }
            if value != 0 || offset & 0x8000_0000 != 0 {
                value += u64::from(offset & 0x7fff_ffff);
            }
            assert!(value < (1u64 << dw));
            for bit in 0..dw {
                bits.push((value >> bit) & 1);
            }
        }
    }
    assert_eq!(bits.len() % 32, 0);
    bits.chunks_exact(32)
        .map(|chunk| {
            chunk
                .iter()
                .enumerate()
                .fold(0, |word, (bit, v)| word | (*v as u32) << bit)
        })
        .collect()
}

fn complete(m: &mut Machine, pc: u32) {
    m.step().unwrap();
    reach(m, pc, 3_000_000);
}

fn verify(thumb: bool, dest: u32, data: &[u8], sw: u8, dw: u8, offset: u32) {
    let expected = reference(data, sw, dw, offset);
    let (mut m, pc) = prepare(thumb, dest, data, sw, dw, offset);
    let before = m.cpu().clone();
    let end = dest + expected.len() as u32 * 4;
    for address in [dest - 4, end, bios::IRQ_STACK] {
        m.memory_mut().write32(address, 0xa55a_1234).unwrap();
    }
    complete(&mut m, pc);
    for (index, word) in expected.iter().enumerate() {
        assert_eq!(
            m.memory().read32(dest + index as u32 * 4).unwrap(),
            *word,
            "word={index}, sw={sw}, dw={dw}"
        );
    }
    for address in [dest - 4, end, bios::IRQ_STACK] {
        assert_eq!(m.memory().read32(address).unwrap(), 0xa55a_1234);
    }
    assert_eq!(&m.cpu().registers()[..15], &before.registers()[..15]);
    assert_eq!(m.cpu().cpsr(), before.cpsr());
}

fn fail(m: &mut Machine) -> MachineError {
    for _ in 0..3000 {
        let before = m.cpu().clone();
        let cycles = m.cycles();
        let timing = m.last_timing();
        if let Err(error) = m.step() {
            assert_eq!(m.cpu(), &before);
            assert_eq!(m.cycles(), cycles);
            assert_eq!(m.last_timing(), timing);
            assert_eq!(m.step(), Err(error.clone()));
            return error;
        }
    }
    panic!("invalid unpack did not produce a bounded diagnostic");
}

fn assert_invalid(error: MachineError) {
    assert!(matches!(
        error,
        MachineError::Cpu(CpuError::UnsupportedInstruction {
            instruction: INVALID_ARGUMENT_TRAP,
            ..
        })
    ));
}

#[test]
fn all_supported_width_pairs_and_input_bytes_match_bit_reference() {
    let data: Vec<_> = (0..=255).collect();
    for sw in [1, 2, 4, 8] {
        for dw in [1, 2, 4, 8, 16, 32].into_iter().filter(|dw| *dw >= sw) {
            for thumb in [false, true] {
                verify(
                    thumb,
                    if thumb { VRAM_START + 4 } else { DEST },
                    &data,
                    sw,
                    dw,
                    0,
                );
            }
        }
    }
}

#[test]
fn source_units_and_destination_units_are_least_significant_first() {
    let (mut m, pc) = prepare(false, DEST, &[0b1011_0001], 1, 4, 0);
    complete(&mut m, pc);
    assert_eq!(m.memory().read32(DEST).unwrap(), 0x1011_0001);
    let (mut m, pc) = prepare(true, DEST, &[0xe4], 2, 8, 0);
    complete(&mut m, pc);
    assert_eq!(m.memory().read32(DEST).unwrap(), 0x0302_0100);
}

#[test]
fn offsets_apply_to_nonzero_units_and_optionally_to_zero() {
    for zero in [0, 0x8000_0000] {
        for offset in [0, 1, 0x1234, 0x7fff_ffff] {
            verify(false, DEST, &[0, 1, 0xfe, 0xff], 8, 32, offset | zero);
        }
        verify(true, VRAM_START + 4, &[0x10, 0x21], 4, 8, 0x80 | zero);
    }
}

#[test]
fn seeded_width_offset_and_data_combinations_match_reference() {
    let mut seed = 0x615c_ae39u32;
    for case in 0..96 {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let sw = 1u8 << (seed & 3);
        let choices: Vec<u8> = [1, 2, 4, 8, 16, 32]
            .into_iter()
            .filter(|dw| *dw >= sw)
            .collect();
        let dw = choices[(seed >> 8) as usize % choices.len()];
        let capacity = ((1u64 << dw) - (1u64 << sw)).min(0x7fff_ffff);
        let bias =
            ((seed as u64) % (capacity + 1)) as u32 | if case % 2 == 0 { 0x8000_0000 } else { 0 };
        let data: Vec<_> = (0..20)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed >> 24) as u8
            })
            .collect();
        verify(case % 2 != 0, DEST, &data, sw, dw, bias);
    }
}

#[test]
fn maximum_source_byte_count_is_unsigned_and_fully_processed() {
    let data: Vec<_> = (0..65535u32).map(|i| i as u8).collect();
    // Keep the descriptor in IWRAM: the output fills almost all EWRAM.
    let (mut m, pc) = call_status_data(0x10, false, [ROM_DATA, SOURCE, 0x0300_0100], 0x1f, &data);
    m.memory_mut().write32(0x0300_0100, 0x2008_ffff).unwrap();
    m.memory_mut().write32(0x0300_0104, 0).unwrap();
    m.memory_mut()
        .write32(SOURCE + 65535 * 4, 0xa55a_1234)
        .unwrap();
    complete(&mut m, pc);
    for (index, byte) in data.iter().enumerate() {
        assert_eq!(
            m.memory().read32(SOURCE + index as u32 * 4).unwrap(),
            u32::from(*byte)
        );
    }
    assert_eq!(m.memory().read32(SOURCE + 65535 * 4).unwrap(), 0xa55a_1234);
}

#[test]
fn byte_aligned_sources_work_in_rom_and_both_ram_regions() {
    for source in [
        ROM_DATA + 1,
        ROM_DATA + 2,
        ROM_DATA + 3,
        SOURCE + 1,
        0x0300_0103,
    ] {
        let mut data = vec![0xcc; 3];
        data.extend([0x12, 0x34, 0x56, 0x78]);
        let (mut m, pc) = call_status_data(0x10, true, [source, DEST, INFO], 0x1f, &data);
        info(&mut m, 4, 8, 8, 0);
        let expected = if source >= ROM_DATA {
            &data[(source - ROM_DATA) as usize..(source - ROM_DATA) as usize + 4]
        } else {
            &[0x12, 0x34, 0x56, 0x78]
        };
        if source < ROM_DATA {
            for (index, byte) in expected.iter().enumerate() {
                m.memory_mut().write8(source + index as u32, *byte).unwrap();
            }
        }
        complete(&mut m, pc);
        assert_eq!(
            m.memory().read32(DEST).unwrap(),
            u32::from_le_bytes(expected.try_into().unwrap())
        );
    }
}

#[test]
fn unpack_descriptor_can_be_in_cartridge_rom() {
    let data = [4, 0, 8, 8, 0, 0, 0, 0, 0x12, 0x34, 0x56, 0x78];
    let (mut m, pc) = call_status_data(0x10, false, [ROM_DATA + 8, DEST, ROM_DATA], 0x1f, &data);
    complete(&mut m, pc);
    assert_eq!(m.memory().read32(DEST).unwrap(), 0x7856_3412);
}

#[test]
fn caller_registers_status_and_stack_boundaries_are_preserved() {
    for thumb in [false, true] {
        for status in [0x10, 0x50, 0x90, 0xd0, 0x1f, 0x5f, 0x9f, 0xdf] {
            let (mut m, pc) =
                call_status_data(0x10, thumb, [ROM_DATA, DEST, INFO], status, &[0x12, 0x34]);
            info(&mut m, 2, 4, 8, 0x10);
            let before = m.cpu().clone();
            m.memory_mut()
                .write32(bios::IRQ_STACK, 0x1234_5678)
                .unwrap();
            complete(&mut m, pc);
            assert_eq!(&m.cpu().registers()[..15], &before.registers()[..15]);
            assert_eq!(m.cpu().cpsr(), before.cpsr());
            assert_eq!(m.memory().read32(bios::IRQ_STACK).unwrap(), 0x1234_5678);
        }
    }
}

#[test]
fn zero_length_does_not_access_source_or_destination_after_validation() {
    let (mut m, pc) = call(0x10, false, [0x0e00_0000, 0x0e00_0000, INFO]);
    info(&mut m, 0, 1, 32, 0);
    complete(&mut m, pc);
}

#[test]
fn invalid_widths_and_narrowing_produce_diagnostics() {
    for width in 0..=255u8 {
        if ![1, 2, 4, 8].contains(&width) {
            let (mut m, _) = prepare(false, DEST, &[0; 4], width, 32, 0);
            assert_invalid(fail(&mut m));
            assert_eq!(m.memory().read32(DEST).unwrap(), 0);
        }
        if ![1, 2, 4, 8, 16, 32].contains(&width) {
            let (mut m, _) = prepare(true, DEST, &[0; 4], 1, width, 0);
            assert_invalid(fail(&mut m));
        }
    }
    for (sw, dw) in [(2, 1), (4, 1), (4, 2), (8, 1), (8, 2), (8, 4)] {
        let (mut m, _) = prepare(false, DEST, &[0; 4], sw, dw, 0);
        assert_invalid(fail(&mut m));
    }
}

#[test]
fn alignment_protected_source_and_wrapping_ranges_are_rejected() {
    for (source, dest, descriptor) in [
        (0, DEST, INFO),
        (0x3fff, DEST, INFO),
        (0x0100_0000, DEST, INFO),
        (ROM_DATA, DEST + 1, INFO),
        (ROM_DATA, DEST + 2, INFO),
        (ROM_DATA, DEST + 3, INFO),
        (ROM_DATA, DEST, INFO + 1),
        (ROM_DATA, DEST, INFO + 2),
        (ROM_DATA, DEST, INFO + 3),
        (0xffff_fffe, DEST, INFO),
        (ROM_DATA, 0xffff_fffc, INFO),
    ] {
        let (mut m, _) = call_status_data(0x10, false, [source, dest, descriptor], 0x1f, &[1; 4]);
        info(&mut m, 4, 8, 32, 0);
        assert_invalid(fail(&mut m));
        assert_eq!(m.memory().read32(DEST).unwrap(), 0);
    }
}

#[test]
fn incomplete_output_words_are_rejected_before_source_reads() {
    for (length, sw, dw) in [(1, 8, 8), (2, 8, 8), (3, 8, 8), (1, 4, 8), (6, 8, 8)] {
        let (mut m, _) = call(0x10, false, [0x0e00_0000, DEST, INFO]);
        info(&mut m, length, sw, dw, 0);
        assert_invalid(fail(&mut m));
        assert_eq!(m.memory().read32(DEST).unwrap(), 0);
    }
}

#[test]
fn offset_overflow_keeps_completed_words_but_discards_pending_word() {
    // First four values fit; a later 255+1 does not fit eight destination bits.
    let (mut m, _) = prepare(false, DEST, &[1, 2, 3, 4, 5, 0xff, 7, 8], 8, 8, 1);
    m.memory_mut().write32(DEST + 4, 0xcccc_cccc).unwrap();
    assert_invalid(fail(&mut m));
    assert_eq!(m.memory().read32(DEST).unwrap(), 0x0504_0302);
    assert_eq!(m.memory().read32(DEST + 4).unwrap(), 0xcccc_cccc);
    // Offsets on transparent zero units are checked only when the flag requests them.
    verify(true, DEST, &[0; 4], 8, 8, 256);
    let (mut m, _) = prepare(true, DEST, &[0; 4], 8, 8, 0x8000_0100);
    assert_invalid(fail(&mut m));
}

#[test]
fn truncated_descriptors_and_sources_return_memory_diagnostics() {
    let descriptor = [4, 0, 8, 8, 0, 0, 0, 0];
    for length in 0..8 {
        let (mut m, _) = call_status_data(
            0x10,
            false,
            [SOURCE, DEST, ROM_DATA],
            0x1f,
            &descriptor[..length],
        );
        assert_eq!(
            fail(&mut m),
            MachineError::Cpu(MemoryError::Unmapped(ROM_DATA + length as u32).into())
        );
    }
    for length in 0..8 {
        let (mut m, _) = prepare(false, DEST, &vec![0x12; length], 8, 8, 0);
        info(&mut m, 8, 8, 8, 0);
        m.memory_mut().write32(DEST, 0xcccc_cccc).unwrap();
        m.memory_mut().write32(DEST + 4, 0xcccc_cccc).unwrap();
        assert_eq!(
            fail(&mut m),
            MachineError::Cpu(MemoryError::Unmapped(ROM_DATA + length as u32).into())
        );
        assert_eq!(
            m.memory().read32(DEST).unwrap(),
            if length >= 4 {
                0x1212_1212
            } else {
                0xcccc_cccc
            }
        );
        assert_eq!(m.memory().read32(DEST + 4).unwrap(), 0xcccc_cccc);
    }
}

#[test]
fn destination_failure_preserves_completed_word() {
    let (mut m, _) = prepare(
        false,
        0x07ff_fffc,
        &[0x12, 0x34, 0x56, 0x78, 1, 2, 3, 4],
        8,
        8,
        0,
    );
    assert_eq!(
        fail(&mut m),
        MachineError::Cpu(MemoryError::ReadOnly(ROM_START).into())
    );
    assert_eq!(m.memory().read32(0x07ff_fffc).unwrap(), 0x7856_3412);
}

#[test]
fn word_stores_support_palette_oam_and_video_memory() {
    for dest in [PALETTE_START + 4, OAM_START + 4, VRAM_START + 4] {
        verify(false, dest, &[0xe4, 0x1b], 2, 4, 0);
    }
}

#[test]
fn timers_and_dma_continue_while_irq_delivery_is_masked() {
    let (mut m, pc) = prepare(false, DEST, &[0x12; 16], 8, 32, 0);
    m.step().unwrap(); // Enter SWI before enabling IRQ sources.
    m.memory_mut().write16(IE, 8).unwrap();
    m.memory_mut().write16(IME, 1).unwrap();
    m.memory_mut().write32(TIMER_BASE, 0x00c0_fff0).unwrap();
    m.memory_mut().write32(SOURCE, 0xa55a_1234).unwrap();
    m.memory_mut().write32(DMA_BASE, SOURCE).unwrap();
    m.memory_mut().write32(DMA_BASE + 4, SOURCE + 4).unwrap();
    m.memory_mut().write32(DMA_BASE + 8, 0x8400_0001).unwrap();
    let mut dma = 0;
    for _ in 0..5000 {
        if m.cpu().pc() == pc {
            break;
        }
        let step = m.step().unwrap();
        assert_ne!(step, StepKind::IrqEntry);
        if matches!(step, StepKind::Dma { channel: 0 }) {
            dma += 1;
        }
    }
    assert_eq!(m.cpu().pc(), pc);
    assert_eq!(dma, 1);
    assert_eq!(m.memory().read32(SOURCE + 4).unwrap(), 0xa55a_1234);
    assert_eq!(m.memory().read16(IF).unwrap() & 8, 8);
    assert_eq!(m.memory().read16(IME).unwrap(), 1);
    assert_eq!(m.memory().read16(IRQ_FLAGS).unwrap(), 0);
    for index in 0..16 {
        assert_eq!(m.memory().read32(DEST + index * 4).unwrap(), 0x12);
    }
}
