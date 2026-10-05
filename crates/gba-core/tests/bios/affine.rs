use super::*;
use gba_core::{
    bios::INVALID_ARGUMENT_TRAP,
    dma::DMA_BASE,
    io::{BG2PA, DISPCNT},
    memory::{OAM_START as OAM, PALETTE_START as PAL},
    video::{rgb555_to_rgb888 as rgb, Framebuffer},
};

const DATA: u32 = ROM_START + 23 * 4;

#[derive(Clone, Copy)]
struct Bg {
    texture: [i32; 2],
    screen: [i16; 2],
    scale: [i16; 2],
    angle: u16,
}

#[derive(Clone, Copy)]
struct Obj {
    scale: [i16; 2],
    angle: u16,
}

fn bg_bytes(records: &[Bg]) -> Vec<u8> {
    records
        .iter()
        .flat_map(|r| {
            let mut bytes = Vec::new();
            for v in r.texture {
                bytes.extend(v.to_le_bytes());
            }
            for v in r.screen.into_iter().chain(r.scale) {
                bytes.extend(v.to_le_bytes());
            }
            bytes.extend(r.angle.to_le_bytes());
            bytes.extend([0xa5, 0x5a]); // Ignored structure padding.
            bytes
        })
        .collect()
}

fn obj_bytes(records: &[Obj]) -> Vec<u8> {
    records
        .iter()
        .flat_map(|r| {
            let mut bytes = Vec::new();
            for v in r.scale {
                bytes.extend(v.to_le_bytes());
            }
            bytes.extend(r.angle.to_le_bytes());
            bytes.extend([0xa5, 0x5a]);
            bytes
        })
        .collect()
}

// Four-argument call harness. Seed r4-r12 to check nonzero register preservation.
fn prepare(service: u8, thumb: bool, args: [u32; 4], status: u32, data: &[u8]) -> (Machine, u32) {
    let mut code: Vec<u32> = (0..4).map(|r| 0xe59f_0044 | r << 12).collect();
    for r in 4..=12 {
        code.push(0xe3a0_0040 | r << 12 | r);
    }
    code.extend([
        0xe321_f000 | status,
        0xe328_f480, // MSR CPSR_f,#N
        if thumb { 0xe28f_e001 } else { 0xe1a0_e00f },
        if thumb { 0xe12f_ff1e } else { 0xe1a0_0000 },
        if thumb {
            0xe7fe_df00 | u32::from(service)
        } else {
            0xef00_0000 | u32::from(service) << 16
        },
        0xeaff_fffe,
    ]);
    code.extend(args);
    let mut rom = words(&code);
    assert_eq!(rom.len() as u32 + ROM_START, DATA);
    rom.extend(data);
    let mut m = bios::boot(rom).unwrap();
    reach(&mut m, ROM_START + 68, 100);
    (m, ROM_START + if thumb { 70 } else { 72 })
}

fn complete(m: &mut Machine, pc: u32) {
    let before = m.cpu().clone();
    m.memory_mut()
        .write32(bios::IRQ_STACK, 0x1234_5678)
        .unwrap();
    m.step().unwrap();
    reach(m, pc, 3_000_000);
    assert_eq!(&m.cpu().registers()[..15], &before.registers()[..15]);
    assert_eq!(m.cpu().cpsr(), before.cpsr());
    assert_eq!(m.memory().read32(bios::IRQ_STACK).unwrap(), 0x1234_5678);
}

// Independent floating-point table generation and wide-integer matrix arithmetic.
fn coefficients(scale: [i16; 2], angle: u16) -> [i16; 4] {
    let phase = usize::from(angle >> 8);
    let sine = |index: usize| {
        ((index as f64 * std::f64::consts::TAU / 256.0).sin() * 16384.0).trunc() as i64
    };
    let s = sine(phase);
    let c = sine((phase + 64) % 256);
    let convert = |scale: i16, v: i64| (i64::from(scale) * v).div_euclid(16384) as i16;
    [
        convert(scale[0], c),
        convert(scale[0], s),
        convert(scale[1], s),
        convert(scale[1], c),
    ]
}

fn bg_reference(r: Bg) -> Vec<u8> {
    let [pa, sxsin, pc, pd] = coefficients(r.scale, r.angle);
    let x = i64::from(r.texture[0]) - i64::from(pa) * i64::from(r.screen[0])
        + i64::from(sxsin) * i64::from(r.screen[1]);
    let y = i64::from(r.texture[1])
        - i64::from(pc) * i64::from(r.screen[0])
        - i64::from(pd) * i64::from(r.screen[1]);
    let mut bytes: Vec<_> = [pa, sxsin.wrapping_neg(), pc, pd]
        .into_iter()
        .flat_map(i16::to_le_bytes)
        .collect();
    bytes.extend((x as u32).to_le_bytes());
    bytes.extend((y as u32).to_le_bytes());
    bytes
}

fn verify_bg(records: &[Bg], thumb: bool, dest: u32) {
    let (mut m, pc) = prepare(
        0x0e,
        thumb,
        [DATA, dest, records.len() as u32, 0xdead_beef],
        0x1f,
        &bg_bytes(records),
    );
    let end = dest + records.len() as u32 * 16;
    for address in [dest - 4, end] {
        m.memory_mut().write32(address, 0xa55a_1234).unwrap();
    }
    complete(&mut m, pc);
    let expected: Vec<_> = records.iter().flat_map(|r| bg_reference(*r)).collect();
    for (i, byte) in expected.iter().enumerate() {
        assert_eq!(
            m.memory().read8(dest + i as u32).unwrap(),
            *byte,
            "byte={i}"
        );
    }
    for address in [dest - 4, end] {
        assert_eq!(m.memory().read32(address).unwrap(), 0xa55a_1234);
    }
}

fn verify_obj(records: &[Obj], thumb: bool, dest: u32, stride: u32) {
    let data = obj_bytes(records);
    let span = records.len() as u32 * stride * 4;
    let (mut m, pc) = prepare(
        0x0f,
        thumb,
        [DATA, dest, records.len() as u32, stride],
        0x1f,
        &data,
    );
    let mut expected = vec![0xa55au16; span as usize / 2];
    for (i, r) in records.iter().enumerate() {
        let [pa, sxsin, pc, pd] = coefficients(r.scale, r.angle);
        for (j, value) in [pa, sxsin.wrapping_neg(), pc, pd].into_iter().enumerate() {
            expected[(i * 4 + j) * stride as usize / 2] = value as u16;
        }
    }
    for offset in (0..span).step_by(2) {
        m.memory_mut().write16(dest + offset, 0xa55a).unwrap();
    }
    for address in [dest - 2, dest + span] {
        m.memory_mut().write16(address, 0x1357).unwrap();
    }
    complete(&mut m, pc);
    for (i, value) in expected.iter().enumerate() {
        assert_eq!(
            m.memory().read16(dest + i as u32 * 2).unwrap(),
            *value,
            "halfword={i}"
        );
    }
    for address in [dest - 2, dest + span] {
        assert_eq!(m.memory().read16(address).unwrap(), 0x1357);
    }
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
    panic!("invalid affine call did not produce a bounded diagnostic");
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
fn background_identity_translates_texture_and_display_centers() {
    let r = Bg {
        texture: [0x12345, -0x12345],
        screen: [12, -34],
        scale: [256, 256],
        angle: 0,
    };
    let expected = bg_reference(r);
    assert_eq!(&expected[..8], &[0, 1, 0, 0, 0, 0, 0, 1]);
    for thumb in [false, true] {
        verify_bg(&[r], thumb, DEST);
    }
}

#[test]
fn object_cardinal_rotations_write_packed_and_oam_matrices() {
    let records: Vec<_> = [0, 0x4000, 0x8000, 0xc000]
        .into_iter()
        .map(|angle| Obj {
            scale: [256, 512],
            angle,
        })
        .collect();
    for thumb in [false, true] {
        verify_obj(&records, thumb, DEST, 2);
        verify_obj(&records, thumb, OAM + 6, 8);
    }
}

#[test]
fn every_angle_phase_matches_independent_matrix_and_origin_references() {
    let records: Vec<_> = (0..256u16)
        .map(|phase| Bg {
            texture: [0x123456, -0x234567],
            screen: [-123, 234],
            scale: [257, -513],
            angle: phase << 8,
        })
        .collect();
    verify_bg(&records, false, DEST);
    let objects: Vec<_> = records
        .iter()
        .map(|r| Obj {
            scale: r.scale,
            angle: r.angle,
        })
        .collect();
    verify_obj(&objects, true, DEST, 8);
}

#[test]
fn low_angle_byte_and_record_padding_do_not_change_results() {
    let records: Vec<_> = (0..256u16)
        .map(|low| Bg {
            texture: [-12345, 98765],
            screen: [45, -67],
            scale: [319, 711],
            angle: 0x2300 | low,
        })
        .collect();
    let expected = bg_reference(records[0]);
    assert!(records.iter().all(|r| bg_reference(*r) == expected));
    verify_bg(&records, true, DEST);
    let objects: Vec<_> = records
        .iter()
        .map(|r| Obj {
            scale: r.scale,
            angle: r.angle,
        })
        .collect();
    verify_obj(&objects, false, DEST, 2);
}

#[test]
fn signed_scales_zero_reflections_and_halfword_wrap_match_reference() {
    let mut records = Vec::new();
    for sx in [-32768, -32767, -257, -1, 0, 1, 127, 256, 32767] {
        for sy in [-32768, -1, 0, 1, 256, 32767] {
            for angle in [0, 0x100, 0x4000, 0x8000, 0xc000, 0xff00] {
                records.push(Bg {
                    texture: [i32::MIN, i32::MAX],
                    screen: [i16::MIN, i16::MAX],
                    scale: [sx, sy],
                    angle,
                });
            }
        }
    }
    verify_bg(&records, false, DEST);
    let objects: Vec<_> = records
        .iter()
        .map(|r| Obj {
            scale: r.scale,
            angle: r.angle,
        })
        .collect();
    verify_obj(&objects, true, DEST, 2);
}

#[test]
fn pb_negation_occurs_after_signed_rounding() {
    assert_eq!(coefficients([1, 1], 0x100), [0, 0, 0, 0]);
    assert_eq!(coefficients([-1, -1], 0x100), [-1, -1, -1, -1]);
    let records = [
        Obj {
            scale: [1, 1],
            angle: 0x100,
        },
        Obj {
            scale: [-1, -1],
            angle: 0x100,
        },
    ];
    verify_obj(&records, false, DEST, 2); // PB must be 0 then +1, not -1 then 0.
}

#[test]
fn seeded_origins_and_matrices_match_wide_integer_wraparound() {
    let mut state = 0x817b_53e9u32;
    let mut next = || {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        state
    };
    let records: Vec<_> = (0..512)
        .map(|_| Bg {
            texture: [next() as i32, next() as i32],
            screen: [next() as i16, next() as i16],
            scale: [next() as i16, next() as i16],
            angle: next() as u16,
        })
        .collect();
    verify_bg(&records, true, DEST);
}

#[test]
fn custom_even_strides_preserve_gaps_between_coefficients_and_records() {
    let records = [
        Obj {
            scale: [256, 128],
            angle: 0x1200,
        },
        Obj {
            scale: [-257, 513],
            angle: 0xa500,
        },
        Obj {
            scale: [0, -1],
            angle: 0xffff,
        },
    ];
    for stride in [2, 4, 6, 8, 16, 32, 1024] {
        verify_obj(&records, false, DEST, stride);
    }
}

#[test]
fn zero_count_skips_all_buffer_and_stride_validation() {
    for service in [0x0e, 0x0f] {
        let (mut m, pc) = prepare(service, true, [1, 0xffff_ffff, 0, 1], 0x1f, &[]);
        complete(&mut m, pc);
    }
}

#[test]
fn alignment_and_protected_sources_are_rejected_before_output() {
    for service in [0x0e, 0x0f] {
        for source in [0, 0x3ffc, 0x0100_0000, DATA + 1, DATA + 3] {
            let (mut m, _) = prepare(service, false, [source, DEST, 1, 2], 0x1f, &[0; 20]);
            assert_invalid(fail(&mut m));
            assert_eq!(m.memory().read32(DEST).unwrap(), 0);
        }
        let (mut m, _) = prepare(service, false, [DATA, DEST + 1, 1, 2], 0x1f, &[0; 20]);
        assert_invalid(fail(&mut m));
    }
    for (source, dest) in [(DATA + 2, DEST), (DATA, DEST + 2)] {
        let (mut m, _) = prepare(0x0e, false, [source, dest, 1, 2], 0x1f, &[0; 24]);
        assert_invalid(fail(&mut m));
    }
    let mut data = vec![0, 0];
    data.extend(obj_bytes(&[Obj {
        scale: [256, 256],
        angle: 0,
    }]));
    let (mut m, pc) = prepare(0x0f, false, [DATA + 2, DEST + 2, 1, 2], 0x1f, &data);
    complete(&mut m, pc); // OBJ only requires halfword alignment.
    assert_eq!(m.memory().read16(DEST + 2).unwrap(), 256);
}

#[test]
fn invalid_stride_batch_sizes_and_wrapping_ranges_are_diagnostics() {
    for stride in [0, 1, 3, 0x4000_0000, 0xffff_fffe] {
        let (mut m, _) = prepare(0x0f, false, [DATA, DEST, 1, stride], 0x1f, &[]);
        assert_invalid(fail(&mut m));
    }
    for service in [0x0e, 0x0f] {
        for (source, dest, count, stride) in [
            (DATA, DEST, u32::MAX, 2),
            (0xffff_fff8, DEST, 1, 2),
            (DATA, 0xffff_fff8, 1, 2),
            (DATA, DEST, 0x1000_0000, 32),
        ] {
            let (mut m, _) = prepare(service, false, [source, dest, count, stride], 0x1f, &[]);
            assert_invalid(fail(&mut m));
            assert_eq!(m.memory().read32(DEST).unwrap(), 0);
        }
    }
}

#[test]
fn last_record_padding_need_not_be_mapped() {
    for service in [0x0e, 0x0f] {
        let mut data = if service == 0x0e {
            bg_bytes(&[Bg {
                texture: [0, 0],
                screen: [0, 0],
                scale: [256, 256],
                angle: 0,
            }])
        } else {
            obj_bytes(&[Obj {
                scale: [256, 256],
                angle: 0,
            }])
        };
        data.truncate(data.len() - 2);
        let (mut m, pc) = prepare(service, false, [DATA, DEST, 1, 2], 0x1f, &data);
        complete(&mut m, pc);
        assert_eq!(m.memory().read16(DEST).unwrap(), 256);
    }
}

#[test]
fn later_source_failure_preserves_first_record_but_not_partial_new_output() {
    for service in [0x0e, 0x0f] {
        let mut data = if service == 0x0e {
            bg_bytes(
                &[Bg {
                    texture: [0, 0],
                    screen: [0, 0],
                    scale: [256, 256],
                    angle: 0,
                }; 2],
            )
        } else {
            obj_bytes(
                &[Obj {
                    scale: [256, 256],
                    angle: 0,
                }; 2],
            )
        };
        data.truncate(data.len() - 3); // Missing last angle byte in second record.
        let span = if service == 0x0e { 16 } else { 8 };
        let (mut m, _) = prepare(service, false, [DATA, DEST, 2, 2], 0x1f, &data);
        m.memory_mut().write32(DEST + span, 0xcccc_cccc).unwrap();
        assert_eq!(
            fail(&mut m),
            MachineError::Cpu(MemoryError::Unmapped(DATA + data.len() as u32).into())
        );
        assert_eq!(m.memory().read16(DEST).unwrap(), 256);
        assert_eq!(m.memory().read16(DEST + 6).unwrap(), 256);
        assert_eq!(m.memory().read32(DEST + span).unwrap(), 0xcccc_cccc);
    }
}

#[test]
fn destination_failure_keeps_preceding_halfword_stores() {
    for service in [0x0e, 0x0f] {
        let data = if service == 0x0e {
            bg_bytes(&[Bg {
                texture: [0, 0],
                screen: [0, 0],
                scale: [256, 256],
                angle: 0,
            }])
        } else {
            obj_bytes(&[Obj {
                scale: [256, 256],
                angle: 0,
            }])
        };
        let (mut m, _) = prepare(service, false, [DATA, 0x07ff_fffc, 1, 2], 0x1f, &data);
        assert_eq!(
            fail(&mut m),
            MachineError::Cpu(MemoryError::ReadOnly(ROM_START).into())
        );
        assert_eq!(m.memory().read32(0x07ff_fffc).unwrap(), 256);
    }
}

#[test]
fn both_work_ram_regions_can_supply_records() {
    for service in [0x0e, 0x0f] {
        let data = if service == 0x0e {
            bg_bytes(&[Bg {
                texture: [0, 0],
                screen: [0, 0],
                scale: [256, 256],
                angle: 0,
            }])
        } else {
            obj_bytes(&[Obj {
                scale: [256, 256],
                angle: 0,
            }])
        };
        for source in [SOURCE, 0x0300_0100] {
            let (mut m, pc) = prepare(service, true, [source, DEST, 1, 2], 0x1f, &[]);
            for (i, byte) in data.iter().enumerate() {
                m.memory_mut().write8(source + i as u32, *byte).unwrap();
            }
            complete(&mut m, pc);
            assert_eq!(m.memory().read16(DEST).unwrap(), 256);
            assert_eq!(m.memory().read16(DEST + 6).unwrap(), 256);
        }
    }
}

#[test]
fn user_system_status_masks_and_nonzero_caller_registers_are_preserved() {
    for service in [0x0e, 0x0f] {
        let data = if service == 0x0e {
            bg_bytes(&[Bg {
                texture: [-12345, 67890],
                screen: [-123, 321],
                scale: [-257, 519],
                angle: 0xa501,
            }])
        } else {
            obj_bytes(&[Obj {
                scale: [-257, 519],
                angle: 0xa501,
            }])
        };
        for thumb in [false, true] {
            for status in [0x10, 0x50, 0x90, 0xd0, 0x1f, 0x5f, 0x9f, 0xdf] {
                let (mut m, pc) = prepare(service, thumb, [DATA, DEST, 1, 8], status, &data);
                complete(&mut m, pc);
            }
        }
    }
}

#[test]
fn background_service_can_write_live_affine_registers_and_render() {
    let data = bg_bytes(&[Bg {
        texture: [256, 0],
        screen: [0, 0],
        scale: [256, 256],
        angle: 0,
    }]);
    let (mut m, pc) = prepare(0x0e, false, [DATA, BG2PA, 1, 0], 0x1f, &data);
    m.memory_mut().write16(DISPCNT, 0x0403).unwrap();
    m.memory_mut().write16(VRAM_START, 31).unwrap();
    m.memory_mut().write16(VRAM_START + 2, 0x3e0).unwrap();
    complete(&mut m, pc);
    // Affine registers are write-only; verify their effect through rendering.
    let mut f = Framebuffer::default();
    m.memory().render_frame(&mut f).unwrap();
    assert_eq!(f.pixels()[0], rgb(0x3e0));
}

#[test]
fn object_service_writes_oam_matrix_without_changing_sprite_attributes() {
    let data = obj_bytes(&[Obj {
        scale: [-256, 256],
        angle: 0,
    }]);
    let (mut m, pc) = prepare(0x0f, true, [DATA, OAM + 6, 1, 8], 0x1f, &data);
    m.memory_mut().write16(DISPCNT, 0x1040).unwrap();
    m.memory_mut().write16(PAL, 0x7c00).unwrap();
    m.memory_mut().write16(PAL + 0x202, 31).unwrap();
    m.memory_mut().write16(PAL + 0x204, 0x3e0).unwrap();
    for i in 0..128 {
        m.memory_mut().write16(OAM + i * 8, 0x200).unwrap();
    }
    m.memory_mut().write16(OAM, 0x100).unwrap();
    for y in 0..8 {
        for x in (0..8).step_by(4) {
            m.memory_mut()
                .write16(
                    VRAM_START + 0x10000 + y * 4 + x / 2,
                    if x == 0 { 0x1111 } else { 0x2222 },
                )
                .unwrap();
        }
    }
    complete(&mut m, pc);
    assert_eq!(m.memory().read16(OAM).unwrap(), 0x100);
    assert_eq!(m.memory().read16(OAM + 8).unwrap(), 0x200);
    let mut f = Framebuffer::default();
    m.memory().render_frame(&mut f).unwrap();
    assert_eq!(f.pixels()[1], rgb(0x3e0));
    assert_eq!(f.pixels()[7], rgb(31));
}

#[test]
fn output_uses_normal_halfword_and_word_video_bus_writes() {
    verify_bg(
        &[Bg {
            texture: [0x1234, -0x1234],
            screen: [0, 0],
            scale: [256, 512],
            angle: 0,
        }],
        false,
        VRAM_START + 4,
    );
    verify_obj(
        &[Obj {
            scale: [256, 512],
            angle: 0x1234,
        }],
        true,
        PAL + 4,
        2,
    );
}

#[test]
fn dma_and_timers_progress_while_services_defer_irq_delivery() {
    for service in [0x0e, 0x0f] {
        let data = if service == 0x0e {
            bg_bytes(&[Bg {
                texture: [0, 0],
                screen: [0, 0],
                scale: [256, 256],
                angle: 0,
            }])
        } else {
            obj_bytes(&[Obj {
                scale: [256, 256],
                angle: 0,
            }])
        };
        let (mut m, pc) = prepare(service, false, [DATA, DEST, 1, 2], 0x1f, &data);
        m.step().unwrap();
        m.memory_mut().write16(IE, 8).unwrap();
        m.memory_mut().write16(IME, 1).unwrap();
        m.memory_mut().write32(TIMER_BASE, 0x00c0_fff0).unwrap();
        m.memory_mut().write32(SOURCE, 0x8765_4321).unwrap();
        m.memory_mut().write32(DMA_BASE, SOURCE).unwrap();
        m.memory_mut().write32(DMA_BASE + 4, SOURCE + 4).unwrap();
        m.memory_mut().write32(DMA_BASE + 8, 0x8400_0001).unwrap();
        let mut dma = 0;
        for _ in 0..1000 {
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
        assert_eq!(m.memory().read16(DEST).unwrap(), 256);
        assert_eq!(m.step().unwrap(), StepKind::IrqEntry);
    }
}
