use super::*;
use gba_rust::{bios::INVALID_ARGUMENT_TRAP, dma::DMA_BASE, memory::OAM_START};

#[derive(Clone)]
enum Block {
    Literal(Vec<u8>),
    Repeat(u8, usize),
}

/// Encode structured blocks and independently expand their expected bytes.
fn stream(blocks: &[Block]) -> (Vec<u8>, Vec<u8>) {
    let mut encoded = vec![0x30, 0, 0, 0];
    let mut expected = Vec::new();
    for block in blocks {
        match block {
            Block::Literal(bytes) => {
                assert!((1..=128).contains(&bytes.len()));
                encoded.push((bytes.len() - 1) as u8);
                encoded.extend(bytes);
                expected.extend(bytes);
            }
            Block::Repeat(byte, count) => {
                assert!((3..=130).contains(count));
                encoded.extend([0x80 | (*count - 3) as u8, *byte]);
                expected.extend(std::iter::repeat_n(*byte, *count));
            }
        }
    }
    encoded[..4].copy_from_slice(&((expected.len() as u32) << 8 | 0x30).to_le_bytes());
    (encoded, expected)
}

fn prepare(service: u8, thumb: bool, destination: u32, data: &[u8]) -> (Machine, u32) {
    call_status_data(
        service,
        thumb,
        [ROM_DATA, destination, 0xa55a_1234],
        0x1f,
        data,
    )
}

fn complete(m: &mut Machine, return_pc: u32) {
    m.step().unwrap();
    reach(m, return_pc, 2_000_000);
}

fn verify(service: u8, thumb: bool, destination: u32, data: &[u8], expected: &[u8]) {
    let (mut m, return_pc) = prepare(service, thumb, destination, data);
    let before = m.cpu().clone();
    let end = destination + expected.len() as u32;
    // Halfword sentinels support both RAM and video memory.
    for address in [(destination - 1) & !1, end & !1] {
        m.memory_mut().write16(address, 0xa55a).unwrap();
    }
    // The 60-byte SVC frame must not touch the IRQ stack boundary at SVC_SP-64.
    m.memory_mut()
        .write32(bios::IRQ_STACK, 0x5a5a_a5a5)
        .unwrap();
    complete(&mut m, return_pc);
    for (index, byte) in expected.iter().enumerate() {
        assert_eq!(
            m.memory().read8(destination + index as u32).unwrap(),
            *byte,
            "byte={index}"
        );
    }
    for address in [destination - 1, end] {
        assert_eq!(
            m.memory().read8(address).unwrap(),
            if address & 1 == 0 { 0x5a } else { 0xa5 }
        );
    }
    assert_eq!(m.memory().read32(bios::IRQ_STACK).unwrap(), 0x5a5a_a5a5);
    assert_eq!(&m.cpu().registers()[..15], &before.registers()[..15]);
    assert_eq!(m.cpu().cpsr(), before.cpsr());
}

fn fail(m: &mut Machine) -> MachineError {
    for _ in 0..2000 {
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
    panic!("invalid stream did not reach a bounded diagnostic");
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
fn every_block_control_value_matches_reference_in_arm_and_thumb() {
    for flag in 0u8..=255 {
        let count = usize::from(flag & 127) + if flag & 128 == 0 { 1 } else { 3 };
        let block = if flag & 128 == 0 {
            Block::Literal((0..count).map(|i| (i * 37) as u8).collect())
        } else {
            Block::Repeat(flag, count)
        };
        let mut blocks = vec![block];
        // The final literal can complete a halfword started by the prior block.
        if count % 2 != 0 {
            blocks.push(Block::Literal(vec![0x42]));
        }
        let (data, expected) = stream(&blocks);
        assert_eq!(data[4], flag);
        for thumb in [false, true] {
            verify(0x14, thumb, DEST + 1, &data, &expected);
            verify(0x15, thumb, VRAM_START + 4, &data, &expected);
        }
    }
}

#[test]
fn repeated_values_cover_all_bytes_and_cross_halfword_boundaries() {
    let blocks: Vec<_> = (0..=255).map(|byte| Block::Repeat(byte, 3)).collect();
    let (data, expected) = stream(&blocks);
    verify(0x14, false, DEST, &data, &expected);
    verify(0x15, true, DEST, &data, &expected);
    verify(0x15, false, VRAM_START + 4, &data, &expected);
}

#[test]
fn seeded_mixed_streams_match_independent_block_expansion() {
    let mut seed = 0x83ca_206bu32;
    for case in 0..64 {
        let mut blocks = Vec::new();
        let mut length = 0;
        for _ in 0..32 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let repeat = seed & 128 != 0;
            let count = ((seed >> 8) & 127) as usize + if repeat { 3 } else { 1 };
            blocks.push(if repeat {
                Block::Repeat((seed >> 24) as u8, count)
            } else {
                Block::Literal(
                    (0..count)
                        .map(|i| (seed.wrapping_add(i as u32 * 71) >> 3) as u8)
                        .collect(),
                )
            });
            length += count;
        }
        if length % 2 != 0 {
            blocks.push(Block::Literal(vec![0xab]));
        }
        let (data, expected) = stream(&blocks);
        verify(0x14, case % 2 == 0, DEST, &data, &expected);
        verify(0x15, case % 2 != 0, VRAM_START + 4, &data, &expected);
    }
}

#[test]
fn output_length_uses_all_twenty_four_header_bits() {
    let mut blocks = vec![Block::Repeat(0x5a, 130); 504];
    blocks.push(Block::Repeat(0xa5, 16)); // 65,536 bytes, crossing the 16-bit count boundary.
    let (data, expected) = stream(&blocks);
    assert_eq!(expected.len(), 65536);
    assert_eq!(&data[..4], &[0x30, 0, 0, 1]);
    verify(0x14, false, DEST, &data, &expected);
    verify(0x15, true, DEST, &data, &expected);
}

#[test]
fn source_can_be_in_either_work_ram_region() {
    let (data, expected) = stream(&[Block::Literal(vec![1, 2, 3]), Block::Repeat(0xaa, 3)]);
    for source in [SOURCE, 0x0300_0100] {
        for service in [0x14, 0x15] {
            let (mut m, return_pc) = call(service, false, [source, DEST, 0]);
            for (index, byte) in data.iter().enumerate() {
                m.memory_mut().write8(source + index as u32, *byte).unwrap();
            }
            complete(&mut m, return_pc);
            for (index, byte) in expected.iter().enumerate() {
                assert_eq!(m.memory().read8(DEST + index as u32).unwrap(), *byte);
            }
        }
    }
}

#[test]
fn user_system_status_and_all_caller_registers_are_restored() {
    let (data, _) = stream(&[Block::Repeat(0x88, 3), Block::Literal(vec![0x12])]);
    for service in [0x14, 0x15] {
        for thumb in [false, true] {
            for status in [0x10, 0x50, 0x90, 0xd0, 0x1f, 0x5f, 0x9f, 0xdf] {
                let (mut m, return_pc) =
                    call_status_data(service, thumb, [ROM_DATA, DEST, 0x12345678], status, &data);
                let before = m.cpu().clone();
                complete(&mut m, return_pc);
                assert_eq!(&m.cpu().registers()[..15], &before.registers()[..15]);
                assert_eq!(m.cpu().cpsr(), before.cpsr());
            }
        }
    }
}

#[test]
fn zero_length_reads_no_blocks_and_does_not_access_destination() {
    for service in [0x14, 0x15] {
        let (mut m, return_pc) = prepare(service, false, 0x0e00_0000, &[0x30, 0, 0, 0]);
        complete(&mut m, return_pc);
    }
}

#[test]
fn bad_type_source_alignment_protection_and_output_wrap_are_diagnostics() {
    for service in [0x14, 0x15] {
        for header in [0u32, 0x0410, 0x0420, 0x0431, 0x04ff] {
            let (mut m, _) = prepare(service, false, DEST, &header.to_le_bytes());
            assert_invalid(fail(&mut m));
            assert_eq!(m.memory().read32(DEST).unwrap(), 0);
        }
        for source in [
            0,
            0x3ffc,
            0x0100_0000,
            ROM_DATA + 1,
            ROM_DATA + 2,
            ROM_DATA + 3,
        ] {
            let (mut m, _) =
                call_status_data(service, false, [source, DEST, 0], 0x1f, &[0x30, 4, 0, 0]);
            assert_invalid(fail(&mut m));
        }
        let (mut m, _) = prepare(service, false, 0xffff_fffe, &[0x30, 4, 0, 0]);
        assert_invalid(fail(&mut m));
    }
}

#[test]
fn halfword_variant_rejects_odd_size_or_address_before_writing() {
    for (destination, data) in [
        (VRAM_START + 4, vec![0x30, 3, 0, 0, 0x80, 0x77]),
        (VRAM_START + 5, vec![0x30, 4, 0, 0, 0x81, 0x77]),
        (VRAM_START + 5, vec![0x30, 0, 0, 0]),
    ] {
        let (mut m, _) = prepare(0x15, true, destination, &data);
        m.memory_mut().write32(VRAM_START + 4, 0xcccc_cccc).unwrap();
        assert_invalid(fail(&mut m));
        assert_eq!(m.memory().read32(VRAM_START + 4).unwrap(), 0xcccc_cccc);
    }
    let (data, expected) = stream(&[Block::Repeat(0x77, 3)]);
    verify(0x14, true, DEST + 1, &data, &expected);
}

#[test]
fn oversized_blocks_fail_before_payload_reads_or_writes() {
    for service in [0x14, 0x15] {
        for flag in [1, 0x7f, 0x80, 0xff] {
            // One literal succeeds, then the next block exceeds one remaining byte.
            // No payload follows the invalid block: its bounds check must fail first.
            let data = [0x30, 2, 0, 0, 0, 0x12, flag];
            let (mut m, _) = prepare(service, false, DEST, &data);
            m.memory_mut().write16(DEST, 0xcccc).unwrap();
            assert_invalid(fail(&mut m));
            assert_eq!(
                m.memory().read16(DEST).unwrap(),
                if service == 0x14 { 0xcc12 } else { 0xcccc }
            );
        }
    }
}

#[test]
fn every_truncated_input_prefix_returns_a_memory_diagnostic() {
    let (data, _) = stream(&[
        Block::Literal(vec![0x12, 0x34, 0x56]),
        Block::Repeat(0x78, 3),
    ]);
    for service in [0x14, 0x15] {
        for length in 0..data.len() {
            let (mut m, _) = prepare(service, false, DEST, &data[..length]);
            assert_eq!(
                fail(&mut m),
                MachineError::Cpu(MemoryError::Unmapped(ROM_DATA + length as u32).into()),
                "service={service:x}, length={length}"
            );
        }
    }
}

#[test]
fn incomplete_halfwords_are_not_committed_after_source_failure() {
    for service in [0x14, 0x15] {
        // Three of four literal bytes are available. One halfword can be committed.
        let data = [0x30, 4, 0, 0, 3, 0x12, 0x34, 0x56];
        let (mut m, _) = prepare(service, false, DEST, &data);
        m.memory_mut().write32(DEST, 0xcccc_cccc).unwrap();
        assert!(matches!(
            fail(&mut m),
            MachineError::Cpu(CpuError::Memory(_))
        ));
        assert_eq!(
            m.memory().read32(DEST).unwrap(),
            if service == 0x14 {
                0xcc56_3412
            } else {
                0xcccc_3412
            }
        );
    }
}

#[test]
fn destination_failure_preserves_earlier_completed_writes() {
    let (data, _) = stream(&[Block::Literal(vec![0x12, 0x34, 0x56, 0x78])]);
    let (mut m, _) = prepare(0x15, false, 0x07ff_fffe, &data);
    assert_eq!(
        fail(&mut m),
        MachineError::Cpu(MemoryError::ReadOnly(ROM_START).into())
    );
    assert_eq!(m.memory().read16(0x07ff_fffe).unwrap(), 0x3412);
}

#[test]
fn declared_end_ignores_padding_and_trailing_invalid_blocks() {
    let (mut data, expected) = stream(&[Block::Repeat(0xab, 4)]);
    data.extend([0x7f, 0xff, 0x80]); // Invalid if read as another block.
    for service in [0x14, 0x15] {
        verify(service, true, DEST, &data, &expected);
    }
}

#[test]
fn byte_and_halfword_variants_use_normal_video_bus_write_rules() {
    let (data, _) = stream(&[Block::Literal(vec![0x12, 0x34])]);
    for service in [0x14, 0x15] {
        for destination in [VRAM_START, OAM_START] {
            let (mut m, return_pc) = prepare(service, false, destination, &data);
            m.memory_mut().write16(destination, 0xcccc).unwrap();
            complete(&mut m, return_pc);
            let expected = if service == 0x15 {
                0x3412
            } else if destination == OAM_START {
                0xcccc
            } else {
                0x3434
            };
            assert_eq!(m.memory().read16(destination).unwrap(), expected);
        }
    }
}

#[test]
fn device_clocks_and_dma_continue_while_service_masks_irq_delivery() {
    let (data, expected) = stream(&[Block::Repeat(0x55, 130)]);
    for service in [0x14, 0x15] {
        let (mut m, return_pc) = prepare(service, false, DEST, &data);
        m.step().unwrap(); // Enter Supervisor mode before requesting any IRQ.
        let start = m.cycles();
        m.memory_mut().write16(IE, 8).unwrap();
        m.memory_mut().write16(IME, 1).unwrap();
        m.memory_mut().write32(TIMER_BASE, 0x00c0_fff0).unwrap();
        m.memory_mut().write32(SOURCE, 0x8765_4321).unwrap();
        m.memory_mut().write32(DMA_BASE, SOURCE).unwrap();
        m.memory_mut().write32(DMA_BASE + 4, SOURCE + 4).unwrap();
        m.memory_mut().write32(DMA_BASE + 8, 0x8400_0001).unwrap();
        let mut dma = 0;
        for _ in 0..5000 {
            if m.cpu().pc() == return_pc {
                break;
            }
            let step = m.step().unwrap();
            assert_ne!(step, StepKind::IrqEntry);
            if matches!(step, StepKind::Dma { channel: 0 }) {
                dma += 1;
            }
        }
        assert_eq!(m.cpu().pc(), return_pc);
        assert_eq!(dma, 1);
        assert!(m.cycles() - start > 16);
        assert_eq!(m.memory().read32(SOURCE + 4).unwrap(), 0x8765_4321);
        assert_eq!(m.memory().read16(IF).unwrap() & 8, 8);
        assert_eq!(m.memory().read16(IME).unwrap(), 1);
        assert_eq!(m.memory().read16(IRQ_FLAGS).unwrap(), 0);
        for (index, byte) in expected.iter().enumerate() {
            assert_eq!(m.memory().read8(DEST + index as u32).unwrap(), *byte);
        }
    }
}
