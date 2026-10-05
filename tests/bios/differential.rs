use super::*;
use gba_rust::{bios::INVALID_ARGUMENT_TRAP, dma::DMA_BASE, memory::OAM_START};

fn filtered(service: u8, samples: &[u16]) -> (Vec<u8>, Vec<u8>) {
    let halfword = service == 0x18;
    let mask = if halfword { 65535i64 } else { 255 };
    let length = samples.len() * if halfword { 2 } else { 1 };
    let mut source = ((length as u32) << 8 | if halfword { 0x82 } else { 0x81 })
        .to_le_bytes()
        .to_vec();
    let mut expected = Vec::new();
    let mut previous = 0i64;
    // Encode differences from original samples, rather than duplicating decoder accumulation.
    for &sample in samples {
        assert!(i64::from(sample) <= mask);
        let difference = ((i64::from(sample) - previous) & mask) as u16;
        if halfword {
            source.extend(difference.to_le_bytes());
            expected.extend(sample.to_le_bytes());
        } else {
            source.push(difference as u8);
            expected.push(sample as u8);
        }
        previous = i64::from(sample);
    }
    (source, expected)
}

fn prepare(service: u8, thumb: bool, dest: u32, data: &[u8]) -> (Machine, u32) {
    call_status_data(service, thumb, [ROM_DATA, dest, 0x55aa_1234], 0x1f, data)
}

fn complete(m: &mut Machine, pc: u32) {
    m.step().unwrap();
    reach(m, pc, 4_000_000);
}

fn verify(service: u8, thumb: bool, dest: u32, data: &[u8], expected: &[u8]) {
    let (mut m, pc) = prepare(service, thumb, dest, data);
    let before = m.cpu().clone();
    let end = dest + expected.len() as u32;
    for address in [(dest - 1) & !1, end & !1] {
        m.memory_mut().write16(address, 0xa55a).unwrap();
    }
    m.memory_mut()
        .write32(bios::IRQ_STACK, 0x5a5a_a5a5)
        .unwrap();
    complete(&mut m, pc);
    for (index, byte) in expected.iter().enumerate() {
        assert_eq!(
            m.memory().read8(dest + index as u32).unwrap(),
            *byte,
            "service={service:x}, byte={index}"
        );
    }
    for address in [dest - 1, end] {
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
    panic!("invalid filter did not produce a bounded diagnostic");
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
fn first_unit_is_original_and_following_units_are_deltas() {
    for service in [0x16, 0x17, 0x18] {
        let (data, expected) = filtered(service, &[10, 11, 12, 13, 14, 15]);
        if service != 0x18 {
            assert_eq!(&data[4..], &[10, 1, 1, 1, 1, 1]);
        }
        for thumb in [false, true] {
            verify(
                service,
                thumb,
                if service == 0x16 {
                    DEST
                } else {
                    VRAM_START + 4
                },
                &data,
                &expected,
            );
        }
    }
}

#[test]
fn byte_filters_cover_every_value_and_wrap_in_both_directions() {
    let mut samples: Vec<_> = (0..=255u16).collect();
    samples.extend((0..=255u16).rev());
    samples.extend([255, 0, 255, 128, 0, 127, 0, 0]);
    for service in [0x16, 0x17] {
        let (data, expected) = filtered(service, &samples);
        for thumb in [false, true] {
            verify(
                service,
                thumb,
                if service == 0x16 {
                    DEST + 1
                } else {
                    VRAM_START + 4
                },
                &data,
                &expected,
            );
        }
    }
}

#[test]
fn halfword_filter_covers_every_sample_and_uses_byte_length() {
    let samples: Vec<_> = (0..=65535u32)
        .map(|i| i.wrapping_mul(40503) as u16)
        .collect();
    let (data, expected) = filtered(0x18, &samples);
    assert_eq!(&data[..4], &[0x82, 0, 0, 2]); // 131072 bytes, not 65536 units.
    verify(0x18, true, DEST, &data, &expected);
}

#[test]
fn modular_accumulation_survives_full_register_wrap_and_large_byte_lengths() {
    for (service, length) in [(0x16, 65536usize), (0x17, 65536), (0x18, 65538 * 2)] {
        let mut data = ((length as u32) << 8 | if service == 0x18 { 0x82 } else { 0x81 })
            .to_le_bytes()
            .to_vec();
        data.extend(std::iter::repeat_n(255, length));
        let expected: Vec<u8> = if service == 0x18 {
            (1..=length / 2)
                .flat_map(|i| (((i as u64 * 65535) % 65536) as u16).to_le_bytes())
                .collect()
        } else {
            (1..=length)
                .map(|i| ((i as u64 * 255) % 256) as u8)
                .collect()
        };
        verify(service, false, DEST, &data, &expected);
    }
}

#[test]
fn seeded_original_samples_round_trip_through_all_filters() {
    let mut seed = 0x415c_a927u32;
    for case in 0..64 {
        for service in [0x16, 0x17, 0x18] {
            let samples: Vec<_> = (0..(case + 1) * 2)
                .map(|_| {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    (seed >> 16) as u16 & if service == 0x18 { 65535 } else { 255 }
                })
                .collect();
            let (data, expected) = filtered(service, &samples);
            verify(service, case % 2 == 0, DEST, &data, &expected);
        }
    }
}

#[test]
fn smallest_outputs_and_odd_byte_output_use_exact_boundaries() {
    for samples in [vec![0x12], vec![0x12, 0x34, 0x56]] {
        let (data, expected) = filtered(0x16, &samples);
        verify(0x16, true, DEST + 1, &data, &expected);
    }
    let (data, expected) = filtered(0x17, &[0x12, 0x34]);
    verify(0x17, false, DEST, &data, &expected);
    let (data, expected) = filtered(0x18, &[0xabcd]);
    verify(0x18, true, VRAM_START + 4, &data, &expected);
}

#[test]
fn sources_can_be_read_from_either_work_ram_region() {
    for service in [0x16, 0x17, 0x18] {
        let (data, expected) = filtered(service, &[0, 255, 128, 1]);
        for source in [SOURCE, 0x0300_0100] {
            let (mut m, pc) = call(service, false, [source, DEST, 0]);
            for (index, byte) in data.iter().enumerate() {
                m.memory_mut().write8(source + index as u32, *byte).unwrap();
            }
            complete(&mut m, pc);
            for (index, byte) in expected.iter().enumerate() {
                assert_eq!(m.memory().read8(DEST + index as u32).unwrap(), *byte);
            }
        }
    }
}

#[test]
fn caller_status_masks_and_registers_are_restored() {
    for service in [0x16, 0x17, 0x18] {
        let (data, _) = filtered(service, &[0x12, 0x34, 0xff, 0]);
        for thumb in [false, true] {
            for status in [0x10, 0x50, 0x90, 0xd0, 0x1f, 0x5f, 0x9f, 0xdf] {
                let (mut m, pc) =
                    call_status_data(service, thumb, [ROM_DATA, DEST, 0xa55a_1234], status, &data);
                let before = m.cpu().clone();
                complete(&mut m, pc);
                assert_eq!(&m.cpu().registers()[..15], &before.registers()[..15]);
                assert_eq!(m.cpu().cpsr(), before.cpsr());
            }
        }
    }
}

#[test]
fn zero_length_reads_no_payload_and_does_not_access_destination() {
    for service in [0x16, 0x17, 0x18] {
        let (data, _) = filtered(service, &[]);
        let (mut m, pc) = prepare(service, false, 0x0e00_0000, &data);
        complete(&mut m, pc);
    }
}

#[test]
fn all_incorrect_type_or_unit_headers_are_rejected_before_output() {
    for service in [0x16, 0x17, 0x18] {
        for header in 0u8..=255 {
            if header == if service == 0x18 { 0x82 } else { 0x81 } {
                continue;
            }
            let (mut m, _) = prepare(service, false, DEST, &[header, 4, 0, 0]);
            assert_invalid(fail(&mut m));
            assert_eq!(m.memory().read32(DEST).unwrap(), 0);
        }
    }
}

#[test]
fn protected_sources_alignment_odd_lengths_and_output_wrap_are_diagnostics() {
    for service in [0x16, 0x17, 0x18] {
        let (data, _) = filtered(service, &[0x12, 0x34]);
        for source in [
            0,
            0x3ffc,
            0x0100_0000,
            ROM_DATA + 1,
            ROM_DATA + 2,
            ROM_DATA + 3,
        ] {
            let (mut m, _) = call_status_data(service, false, [source, DEST, 0], 0x1f, &data);
            assert_invalid(fail(&mut m));
        }
        let (mut m, _) = prepare(service, false, 0xffff_fffe, &data);
        assert_invalid(fail(&mut m));
        if service == 0x16 {
            continue;
        }
        let (mut m, _) = prepare(service, false, DEST + 1, &data);
        assert_invalid(fail(&mut m));
        let mut odd = data.clone();
        odd[1] = 3;
        let (mut m, _) = prepare(service, true, DEST, &odd);
        assert_invalid(fail(&mut m));
        assert_eq!(m.memory().read32(DEST).unwrap(), 0);
    }
}

#[test]
fn every_truncated_source_prefix_returns_a_memory_diagnostic() {
    for service in [0x16, 0x17, 0x18] {
        let (data, _) = filtered(service, &[0x12, 0x34, 0x56, 0x78]);
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
fn failed_source_reads_keep_completed_units_but_not_pending_halfwords() {
    for service in [0x16, 0x17, 0x18] {
        let (mut data, _) = filtered(service, &[0x12, 0x34, 0x56, 0x78]);
        // Three byte samples or one complete halfword plus one byte are available.
        data.truncate(7);
        let (mut m, _) = prepare(service, false, DEST, &data);
        m.memory_mut().write32(DEST, 0xcccc_cccc).unwrap();
        assert!(matches!(
            fail(&mut m),
            MachineError::Cpu(CpuError::Memory(_))
        ));
        assert_eq!(
            m.memory().read32(DEST).unwrap(),
            match service {
                0x16 => 0xcc56_3412,
                0x17 => 0xcccc_3412,
                _ => 0xcccc_0012,
            }
        );
    }
}

#[test]
fn destination_failure_preserves_preceding_completed_halfword() {
    for service in [0x17, 0x18] {
        let (data, _) = filtered(service, &[0x12, 0x34, 0x56, 0x78]);
        let (mut m, _) = prepare(service, false, 0x07ff_fffe, &data);
        assert_eq!(
            fail(&mut m),
            MachineError::Cpu(MemoryError::ReadOnly(ROM_START).into())
        );
        assert_eq!(
            m.memory().read16(0x07ff_fffe).unwrap(),
            if service == 0x17 { 0x3412 } else { 0x12 }
        );
    }
}

#[test]
fn output_uses_normal_byte_and_halfword_video_bus_rules() {
    for service in [0x16, 0x17, 0x18] {
        let (data, _) = filtered(
            service,
            if service == 0x18 {
                &[0x3412]
            } else {
                &[0x12, 0x34]
            },
        );
        for dest in [VRAM_START, OAM_START] {
            let (mut m, pc) = prepare(service, false, dest, &data);
            m.memory_mut().write16(dest, 0xcccc).unwrap();
            complete(&mut m, pc);
            assert_eq!(
                m.memory().read16(dest).unwrap(),
                if service != 0x16 {
                    0x3412
                } else if dest == OAM_START {
                    0xcccc
                } else {
                    0x3434
                }
            );
        }
    }
}

#[test]
fn declared_end_does_not_consume_trailing_payload() {
    for service in [0x16, 0x17, 0x18] {
        let (mut data, expected) = filtered(service, &[0x12, 0x34]);
        data.extend([0xff; 8]);
        verify(service, false, DEST, &data, &expected);
    }
}

#[test]
fn dma_and_timers_continue_while_filter_masks_cpu_irq_delivery() {
    for service in [0x16, 0x17, 0x18] {
        let (data, expected) = filtered(service, &[0x55; 32]);
        let (mut m, pc) = prepare(service, false, DEST, &data);
        m.step().unwrap(); // Enter SWI before requesting IRQs.
        m.memory_mut().write16(IE, 8).unwrap();
        m.memory_mut().write16(IME, 1).unwrap();
        m.memory_mut().write32(TIMER_BASE, 0x00c0_fff0).unwrap();
        m.memory_mut().write32(SOURCE, 0x8765_4321).unwrap();
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
        assert_eq!(m.memory().read32(SOURCE + 4).unwrap(), 0x8765_4321);
        assert_eq!(m.memory().read16(IF).unwrap() & 8, 8);
        assert_eq!(m.memory().read16(IME).unwrap(), 1);
        assert_eq!(m.memory().read16(IRQ_FLAGS).unwrap(), 0);
        for (index, byte) in expected.iter().enumerate() {
            assert_eq!(m.memory().read8(DEST + index as u32).unwrap(), *byte);
        }
    }
}
