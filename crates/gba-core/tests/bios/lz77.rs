use super::*;
use gba_core::bios::INVALID_ARGUMENT_TRAP;

#[derive(Clone, Copy)]
enum Token {
    Literal(u8),
    Reference { distance: usize, length: usize },
}

/// Test-only token encoder and byte-vector reference. Neither decodes the emitted ARM code.
fn stream(tokens: &[Token]) -> (Vec<u8>, Vec<u8>) {
    let mut compressed = vec![0x10, 0, 0, 0];
    let mut expected = Vec::new();
    for group in tokens.chunks(8) {
        let flags_index = compressed.len();
        compressed.push(0);
        for (index, token) in group.iter().enumerate() {
            match *token {
                Token::Literal(value) => {
                    compressed.push(value);
                    expected.push(value);
                }
                Token::Reference { distance, length } => {
                    assert!((1..=4096).contains(&distance));
                    assert!((3..=18).contains(&length));
                    assert!(distance <= expected.len());
                    compressed[flags_index] |= 0x80 >> index;
                    compressed.push((((length - 3) << 4) | ((distance - 1) >> 8)) as u8);
                    compressed.push((distance - 1) as u8);
                    for _ in 0..length {
                        expected.push(expected[expected.len() - distance]);
                    }
                }
            }
        }
    }
    let length = expected.len() as u32;
    compressed[..4].copy_from_slice(&(length << 8 | 0x10).to_le_bytes());
    (compressed, expected)
}

fn prepare(service: u8, thumb: bool, destination: u32, compressed: &[u8]) -> (Machine, u32) {
    call_status_data(
        service,
        thumb,
        [ROM_DATA, destination, 0x55aa_1234],
        0x1f,
        compressed,
    )
}

fn complete(machine: &mut Machine, return_pc: u32) {
    machine.step().unwrap();
    reach(machine, return_pc, 500_000);
}

fn verify(service: u8, thumb: bool, destination: u32, compressed: &[u8], expected: &[u8]) {
    let (mut machine, return_pc) = prepare(service, thumb, destination, compressed);
    let before = machine.cpu().clone();
    // Halfword sentinels also work in VRAM without byte-write duplication.
    machine
        .memory_mut()
        .write16((destination - 2) & !1, 0xa55a)
        .unwrap();
    let after = (destination + expected.len() as u32 + 1) & !1;
    machine.memory_mut().write16(after, 0xa55a).unwrap();
    for address in [destination - 1, destination + expected.len() as u32] {
        machine.memory_mut().write16(address & !1, 0xa55a).unwrap();
    }
    machine
        .memory_mut()
        .write32(bios::IRQ_STACK, 0x5a5a_a5a5)
        .unwrap();
    complete(&mut machine, return_pc);
    for (index, value) in expected.iter().enumerate() {
        assert_eq!(
            machine.memory().read8(destination + index as u32).unwrap(),
            *value,
            "output byte {index}"
        );
    }
    assert_eq!(
        machine.memory().read16((destination - 2) & !1).unwrap(),
        0xa55a
    );
    assert_eq!(machine.memory().read16(after).unwrap(), 0xa55a);
    for address in [destination - 1, destination + expected.len() as u32] {
        assert_eq!(
            machine.memory().read8(address).unwrap(),
            if address & 1 == 0 { 0x5a } else { 0xa5 }
        );
    }
    assert_eq!(
        machine.memory().read32(bios::IRQ_STACK).unwrap(),
        0x5a5a_a5a5
    );
    assert_eq!(&machine.cpu().registers()[..15], &before.registers()[..15]);
    assert_eq!(machine.cpu().cpsr(), before.cpsr());
}

fn fail(machine: &mut Machine) -> MachineError {
    for _ in 0..1000 {
        let before = machine.cpu().clone();
        let cycles = machine.cycles();
        let timing = machine.last_timing();
        if let Err(error) = machine.step() {
            assert_eq!(machine.cpu(), &before);
            assert_eq!(machine.cycles(), cycles);
            assert_eq!(machine.last_timing(), timing);
            assert_eq!(machine.step(), Err(error.clone()));
            return error;
        }
    }
    panic!("invalid stream did not produce a bounded diagnostic");
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
fn literals_cross_flag_groups_in_arm_and_thumb_for_both_write_widths() {
    let tokens: Vec<_> = (0..34).map(|value| Token::Literal(value * 7)).collect();
    let (compressed, expected) = stream(&tokens);
    for thumb in [false, true] {
        verify(0x11, thumb, DEST, &compressed, &expected);
        verify(0x12, thumb, VRAM_START + 4, &compressed, &expected);
        verify(0x12, thumb, DEST, &compressed, &expected); // Halfword variant also works in RAM.
    }
}

#[test]
fn literal_and_reference_flags_are_read_most_significant_bit_first() {
    let tokens = [
        Token::Literal(b'A'),
        Token::Literal(b'B'),
        Token::Reference {
            distance: 2,
            length: 3,
        },
        Token::Literal(b'C'),
        Token::Reference {
            distance: 4,
            length: 4,
        },
        Token::Literal(b'D'),
        Token::Literal(b'E'),
        Token::Reference {
            distance: 3,
            length: 6,
        },
        Token::Reference {
            distance: 8,
            length: 4,
        },
        Token::Literal(b'F'),
        Token::Literal(b'G'),
    ];
    let (compressed, expected) = stream(&tokens);
    assert_eq!(compressed[4], 0x29); // References in token positions 2,4,7.
    assert_eq!(expected.len() % 2, 0);
    verify(0x11, false, DEST, &compressed, &expected);
    verify(0x12, true, VRAM_START + 4, &compressed, &expected);
}

#[test]
fn overlapping_references_support_every_run_length() {
    for length in 3..=18 {
        let mut tokens = vec![
            Token::Literal(0x12),
            Token::Literal(0xab),
            Token::Reference {
                distance: 2,
                length,
            },
        ];
        if length % 2 != 0 {
            tokens.push(Token::Literal(0x99));
        }
        let (compressed, expected) = stream(&tokens);
        verify(0x11, false, DEST + 1, &compressed, &expected); // Byte destination need not be aligned.
        verify(0x12, true, VRAM_START + 4, &compressed, &expected);
    }
}

#[test]
fn wram_variant_supports_distance_one_runs() {
    let (compressed, expected) = stream(&[
        Token::Literal(0x5a),
        Token::Reference {
            distance: 1,
            length: 18,
        },
    ]);
    verify(0x11, false, DEST, &compressed, &expected);
    verify(0x11, true, DEST + 1, &compressed, &expected);
}

#[test]
fn full_twelve_bit_displacements_reach_back_4096_bytes() {
    for distance in [4095, 4096] {
        let mut tokens: Vec<_> = (0..4096)
            .map(|index| Token::Literal((index * 37 % 251) as u8))
            .collect();
        tokens.push(Token::Reference {
            distance,
            length: 18,
        });
        let (compressed, expected) = stream(&tokens);
        verify(0x11, false, DEST, &compressed, &expected);
        verify(0x12, true, VRAM_START + 4, &compressed, &expected);
    }
}

#[test]
fn seeded_valid_streams_match_independent_token_expansion() {
    let mut state = 0x1234_abcd_u32;
    for seed in 0..16 {
        let mut tokens = vec![Token::Literal(0x13), Token::Literal(0x57)];
        let mut length = 2;
        for _ in 0..40 {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            if state & 3 == 0 {
                tokens.push(Token::Literal((state >> 16) as u8));
                length += 1;
            } else {
                let run = 3 + (state >> 8) as usize % 16;
                let distance = 2 + (state >> 16) as usize % (length - 1).min(4095);
                tokens.push(Token::Reference {
                    distance,
                    length: run,
                });
                length += run;
            }
        }
        if length % 2 != 0 {
            tokens.push(Token::Literal(0));
        }
        let (compressed, expected) = stream(&tokens);
        verify(0x11, seed & 1 == 0, DEST, &compressed, &expected);
        verify(0x12, seed & 1 != 0, VRAM_START + 4, &compressed, &expected);
    }
}

#[test]
fn compressed_sources_can_be_read_from_either_work_ram_region() {
    let (compressed, expected) = stream(&[
        Token::Literal(0x55),
        Token::Literal(0xaa),
        Token::Reference {
            distance: 2,
            length: 4,
        },
    ]);
    for source in [SOURCE, 0x0300_0100] {
        for service in [0x11, 0x12] {
            let (mut machine, return_pc) = call(service, false, [source, DEST, 0]);
            for (offset, byte) in compressed.iter().enumerate() {
                machine
                    .memory_mut()
                    .write8(source + offset as u32, *byte)
                    .unwrap();
            }
            complete(&mut machine, return_pc);
            for (offset, byte) in expected.iter().enumerate() {
                assert_eq!(machine.memory().read8(DEST + offset as u32).unwrap(), *byte);
            }
        }
    }
}

#[test]
fn decompression_restores_user_mode_masks_and_all_caller_registers() {
    let (compressed, _) = stream(&[
        Token::Literal(0x55),
        Token::Literal(0xaa),
        Token::Reference {
            distance: 2,
            length: 4,
        },
    ]);
    for service in [0x11, 0x12] {
        for thumb in [false, true] {
            for status in [0x10, 0x90, 0x9f, 0xdf] {
                let (mut machine, return_pc) = call_status_data(
                    service,
                    thumb,
                    [ROM_DATA, DEST, 0x12345678],
                    status,
                    &compressed,
                );
                let before = machine.cpu().clone();
                complete(&mut machine, return_pc);
                assert_eq!(machine.cpu().cpsr(), before.cpsr());
                assert_eq!(&machine.cpu().registers()[..15], &before.registers()[..15]);
            }
        }
    }
}

#[test]
fn empty_headers_ignore_type_without_reading_payload_or_accessing_destination() {
    // Original synthetic headers, including the all-zero empty-stream case.
    // The unmapped, odd destination also proves no output alignment/access is needed.
    for service in [0x11, 0x12] {
        for thumb in [false, true] {
            for low_byte in 0..=255 {
                let (mut machine, return_pc) =
                    prepare(service, thumb, 0x0e00_0001, &[low_byte, 0, 0, 0]);
                let before = machine.cpu().clone();
                complete(&mut machine, return_pc); // Header ends at the last supplied ROM byte.
                assert_eq!(&machine.cpu().registers()[..15], &before.registers()[..15]);
                assert_eq!(machine.cpu().cpsr(), before.cpsr());
            }
        }
    }
}

#[test]
fn empty_headers_preserve_ram_video_and_stack_sentinels() {
    for service in [0x11, 0x12] {
        for thumb in [false, true] {
            for destination in [DEST, VRAM_START + 4] {
                for low_byte in [0, 0x10, 0xff] {
                    verify(service, thumb, destination, &[low_byte, 0, 0, 0], &[]);
                }
            }
        }
    }
}

#[test]
fn bad_headers_alignment_and_wrapping_destinations_are_rejected() {
    for header in [0x0100_u32, 0x0200, 0x0211, 0x0220, 0x02ff] {
        let (mut machine, _) = prepare(0x11, false, DEST, &header.to_le_bytes());
        assert_invalid(fail(&mut machine));
        assert_eq!(machine.memory().read32(DEST).unwrap(), 0);
    }
    for source in [0, 0x3ffc, ROM_DATA + 1, ROM_DATA + 2, ROM_DATA + 3] {
        let (mut machine, _) =
            call_status_data(0x11, false, [source, DEST, 0], 0x1f, &[0x10, 4, 0, 0]);
        assert_invalid(fail(&mut machine));
    }
    let (mut machine, _) = prepare(0x12, false, 0xffff_fffe, &[0x10, 4, 0, 0]);
    assert_invalid(fail(&mut machine));
}

#[test]
fn references_before_output_or_beyond_declared_length_are_rejected() {
    for compressed in [
        vec![0x10, 3, 0, 0, 0x80, 0, 0],       // No preceding output.
        vec![0x10, 4, 0, 0, 0x40, b'A', 0, 1], // Only one preceding byte, distance two.
        vec![0x10, 3, 0, 0, 0x40, b'A', 0, 0], // Length three exceeds two remaining bytes.
    ] {
        let (mut machine, _) = prepare(0x11, false, DEST, &compressed);
        machine.memory_mut().write32(DEST, 0xcccc_cccc).unwrap();
        assert_invalid(fail(&mut machine));
        assert_eq!(
            machine.memory().read8(DEST).unwrap(),
            if compressed[4] == 0x80 { 0xcc } else { b'A' }
        );
        assert_eq!(machine.memory().read8(DEST + 1).unwrap(), 0xcc);
    }
}

#[test]
fn vram_variant_rejects_odd_sizes_odd_destinations_and_distance_one() {
    for (destination, compressed) in [
        (VRAM_START + 4, vec![0x10, 1, 0, 0, 0, 0x77]),
        (VRAM_START + 5, vec![0x10, 2, 0, 0, 0, 0x77, 0x88]),
        (VRAM_START + 4, vec![0x10, 4, 0, 0, 0x40, 0x77, 0, 0]),
    ] {
        let (mut machine, _) = prepare(0x12, true, destination, &compressed);
        machine
            .memory_mut()
            .write32(VRAM_START + 4, 0xcccc_cccc)
            .unwrap();
        assert_invalid(fail(&mut machine));
        assert_eq!(
            machine.memory().read32(VRAM_START + 4).unwrap(),
            0xcccc_cccc
        );
    }
}

#[test]
fn truncated_rom_inputs_produce_memory_diagnostics() {
    for compressed in [
        vec![],
        vec![0x10, 2],
        vec![0x10, 2, 0, 0],
        vec![0x10, 2, 0, 0, 0],
        vec![0x10, 3, 0, 0, 0x80, 0],
    ] {
        let (mut machine, _) = prepare(0x11, false, DEST, &compressed);
        assert_eq!(
            fail(&mut machine),
            MachineError::Cpu(MemoryError::Unmapped(ROM_DATA + compressed.len() as u32).into())
        );
    }
}

#[test]
fn halfword_output_never_commits_a_lone_pending_byte_on_failure() {
    for service in [0x11, 0x12] {
        let compressed = [0x10, 2, 0, 0, 0, b'A']; // Second literal is missing.
        let (mut machine, _) = prepare(service, false, DEST, &compressed);
        machine.memory_mut().write16(DEST, 0xcccc).unwrap();
        assert!(matches!(
            fail(&mut machine),
            MachineError::Cpu(CpuError::Memory(_))
        ));
        assert_eq!(
            machine.memory().read16(DEST).unwrap(),
            if service == 0x11 { 0xcc41 } else { 0xcccc }
        );
    }
}

#[test]
fn destination_failure_preserves_preceding_completed_halfword() {
    let (compressed, _) = stream(&[
        Token::Literal(0x12),
        Token::Literal(0x34),
        Token::Literal(0x56),
        Token::Literal(0x78),
    ]);
    let (mut machine, _) = prepare(0x12, false, 0x07ff_fffe, &compressed);
    assert_eq!(
        fail(&mut machine),
        MachineError::Cpu(MemoryError::ReadOnly(0x0800_0000).into())
    );
    assert_eq!(machine.memory().read16(0x07ff_fffe).unwrap(), 0x3412);
}
