use super::*;
use gba_rust::{
    bios::INVALID_ARGUMENT_TRAP,
    dma::DMA_BASE,
    memory::{OAM_START, PALETTE_START},
};
use std::collections::{BTreeMap, VecDeque};

#[derive(Clone)]
enum Tree {
    Leaf(u8),
    Branch(Box<Tree>, Box<Tree>),
}

fn branch(left: Tree, right: Tree) -> Tree {
    Tree::Branch(Box::new(left), Box::new(right))
}

fn pair() -> Tree {
    branch(Tree::Leaf(0x12), Tree::Leaf(0x34))
}

fn balanced(values: &[u8]) -> Tree {
    if values.len() == 1 {
        return Tree::Leaf(values[0]);
    }
    let half = values.len() / 2;
    branch(balanced(&values[..half]), balanced(&values[half..]))
}

fn chain(depth: u8) -> Tree {
    (0..depth).rev().fold(Tree::Leaf(depth), |next, value| {
        branch(next, Tree::Leaf(value))
    })
}

/// Test-only tree serializer and path encoder. Expected output comes directly
/// from requested symbols; no production traversal code is used to decode it.
fn encode(width: u8, tree: &Tree, symbols: &[u8]) -> (Vec<u8>, Vec<u8>) {
    fn paths(tree: &Tree, prefix: &mut Vec<bool>, codes: &mut BTreeMap<u8, Vec<bool>>) {
        match tree {
            Tree::Leaf(value) => {
                assert!(codes.insert(*value, prefix.clone()).is_none());
            }
            Tree::Branch(left, right) => {
                prefix.push(false);
                paths(left, prefix, codes);
                prefix.pop();
                prefix.push(true);
                paths(right, prefix, codes);
                prefix.pop();
            }
        }
    }
    let mut codes = BTreeMap::new();
    paths(tree, &mut Vec::new(), &mut codes);
    let mut table = vec![0, 0]; // Size byte and root.
    let mut queue = VecDeque::from([(1usize, tree)]);
    while let Some((index, node)) = queue.pop_front() {
        match node {
            Tree::Leaf(value) => table[index] = *value,
            Tree::Branch(left, right) => {
                let children = table.len();
                let offset = (children - (index & !1)) / 2 - 1;
                assert!(offset < 64);
                table[index] = offset as u8
                    | if matches!(**left, Tree::Leaf(_)) {
                        0x80
                    } else {
                        0
                    }
                    | if matches!(**right, Tree::Leaf(_)) {
                        0x40
                    } else {
                        0
                    };
                table.extend([0, 0]);
                queue.push_back((children, left));
                queue.push_back((children + 1, right));
            }
        }
    }
    while table.len() % 4 != 0 {
        table.push(0);
    }
    assert!(table.len() <= 512);
    table[0] = (table.len() / 2 - 1) as u8;
    let expected: Vec<u8> = if width == 4 {
        assert!(symbols.iter().all(|v| *v < 16));
        assert_eq!(symbols.len() % 2, 0);
        symbols
            .chunks_exact(2)
            .map(|pair| pair[0] | pair[1] << 4)
            .collect()
    } else {
        symbols.to_vec()
    };
    assert_eq!(expected.len() % 4, 0);
    let mut data = (((expected.len() as u32) << 8) | 0x20 | u32::from(width))
        .to_le_bytes()
        .to_vec();
    data.extend(table);
    let bits: Vec<bool> = symbols
        .iter()
        .flat_map(|v| codes[v].iter().copied())
        .collect();
    for chunk in bits.chunks(32) {
        let word = chunk
            .iter()
            .enumerate()
            .fold(0u32, |word, (i, bit)| word | u32::from(*bit) << (31 - i));
        data.extend(word.to_le_bytes());
    }
    (data, expected)
}

fn prepare(thumb: bool, dest: u32, data: &[u8]) -> (Machine, u32) {
    call_status_data(0x13, thumb, [ROM_DATA, dest, 0xa55a_1234], 0x1f, data)
}

fn complete(m: &mut Machine, pc: u32) {
    m.step().unwrap();
    reach(m, pc, 8_000_000);
}

fn verify(thumb: bool, dest: u32, data: &[u8], expected: &[u8]) {
    let (mut m, pc) = prepare(thumb, dest, data);
    let before = m.cpu().clone();
    let end = dest + expected.len() as u32;
    for address in [dest - 4, end, bios::IRQ_STACK] {
        m.memory_mut().write32(address, 0xa55a_1234).unwrap();
    }
    complete(&mut m, pc);
    for (index, byte) in expected.iter().enumerate() {
        assert_eq!(
            m.memory().read8(dest + index as u32).unwrap(),
            *byte,
            "byte={index}"
        );
    }
    for address in [dest - 4, end, bios::IRQ_STACK] {
        assert_eq!(m.memory().read32(address).unwrap(), 0xa55a_1234);
    }
    assert_eq!(&m.cpu().registers()[..15], &before.registers()[..15]);
    assert_eq!(m.cpu().cpsr(), before.cpsr());
}

fn fail(m: &mut Machine) -> MachineError {
    for _ in 0..10000 {
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
    panic!("invalid Huffman stream did not produce a bounded diagnostic");
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
fn documented_huff_example_decodes_msb_first_words_from_arm_and_thumb() {
    let tree = branch(Tree::Leaf(b'f'), branch(Tree::Leaf(b'H'), Tree::Leaf(b'u')));
    let (data, expected) = encode(8, &tree, b"Huff");
    assert_eq!(
        data,
        [0x28, 4, 0, 0, 3, 0x80, b'f', 0xc0, b'H', b'u', 0, 0, 0, 0, 0, 0xb0]
    );
    for thumb in [false, true] {
        verify(thumb, DEST, &data, &expected);
    }
}

#[test]
fn nibble_symbols_pack_low_nibble_first_into_output_words() {
    let tree = branch(Tree::Leaf(2), Tree::Leaf(13));
    let (data, expected) = encode(4, &tree, &[2, 13, 13, 2, 2, 2, 13, 13]);
    assert_eq!(expected, [0xd2, 0x2d, 0x22, 0xdd]);
    assert_eq!(&data[8..], &0x6300_0000u32.to_le_bytes());
    verify(true, VRAM_START + 4, &data, &expected);
}

#[test]
fn balanced_trees_cover_every_byte_and_nibble_value() {
    for base in [0u8, 128] {
        let symbols: Vec<_> = (0..128u8).map(|n| base + n).collect();
        let (data, expected) = encode(8, &balanced(&symbols), &symbols);
        verify(false, DEST, &data, &expected);
        verify(true, VRAM_START + 4, &data, &expected);
    }
    let symbols: Vec<_> = (0..16u8).collect();
    let (data, expected) = encode(4, &balanced(&symbols), &symbols);
    verify(true, DEST, &data, &expected);
}

#[test]
fn maximum_tree_and_deep_paths_preserve_node_state_across_input_words() {
    let (data, expected) = encode(8, &chain(255), &[255, 254, 0, 127]);
    assert_eq!(data[4], 255); // 512-byte size/table section; 511 actual nodes.
    verify(false, DEST, &data, &expected);
    verify(true, VRAM_START + 4, &data, &expected);
}

#[test]
fn maximum_child_offset_and_unused_tree_padding_are_supported() {
    let mut data = vec![0x28, 4, 0, 0];
    let mut table = vec![0xff; 512]; // Unused bytes are not decoded or validated.
    table[0] = 255;
    table[1] = 0xff; // Both leaves, offset 63 -> section bytes 128 and 129.
    table[128] = 0x12;
    table[129] = 0x34;
    data.extend(table);
    data.extend(0x6000_0000u32.to_le_bytes()); // 0,1,1,0
    verify(false, DEST, &data, &[0x12, 0x34, 0x34, 0x12]);
}

#[test]
fn seeded_variable_length_paths_match_original_symbols() {
    let mut seed = 0x392b_175eu32;
    for case in 0..48 {
        let width = if case % 2 == 0 { 4 } else { 8 };
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let depth = 2 + (seed % if width == 4 { 14 } else { 62 }) as u8;
        let symbols: Vec<_> = (0..64)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed % (u32::from(depth) + 1)) as u8
            })
            .collect();
        let (data, expected) = encode(width, &chain(depth), &symbols);
        verify(case % 3 == 0, DEST, &data, &expected);
    }
}

#[test]
fn output_byte_length_uses_upper_header_bits_for_both_symbol_widths() {
    for width in [4, 8] {
        let tree = branch(Tree::Leaf(1), Tree::Leaf(2));
        let symbols: Vec<_> = (0..65536 * (8 / width as usize))
            .map(|i| if i % 3 == 0 { 1 } else { 2 })
            .collect();
        let (data, expected) = encode(width, &tree, &symbols);
        assert_eq!(&data[..4], &[0x20 | width, 0, 0, 1]);
        verify(false, DEST, &data, &expected);
    }
}

#[test]
fn streams_can_be_read_from_both_work_ram_regions() {
    let (data, expected) = encode(8, &pair(), &[0x12, 0x34, 0x12, 0x34]);
    for source in [SOURCE, 0x0300_0100] {
        let (mut m, pc) = call(0x13, true, [source, DEST, 0]);
        for (index, byte) in data.iter().enumerate() {
            m.memory_mut().write8(source + index as u32, *byte).unwrap();
        }
        complete(&mut m, pc);
        for (index, byte) in expected.iter().enumerate() {
            assert_eq!(m.memory().read8(DEST + index as u32).unwrap(), *byte);
        }
    }
}

#[test]
fn user_system_status_masks_and_caller_registers_are_restored() {
    let (data, _) = encode(8, &pair(), &[0x12; 4]);
    for thumb in [false, true] {
        for status in [0x10, 0x50, 0x90, 0xd0, 0x1f, 0x5f, 0x9f, 0xdf] {
            let (mut m, pc) =
                call_status_data(0x13, thumb, [ROM_DATA, DEST, 0x1234_5678], status, &data);
            let before = m.cpu().clone();
            complete(&mut m, pc);
            assert_eq!(&m.cpu().registers()[..15], &before.registers()[..15]);
            assert_eq!(m.cpu().cpsr(), before.cpsr());
        }
    }
}

#[test]
fn zero_output_length_does_not_read_tree_or_bitstream() {
    for header in [0x24, 0x28] {
        let (mut m, pc) = prepare(false, 0x0e00_0000, &[header, 0, 0, 0]);
        complete(&mut m, pc);
    }
}

#[test]
fn unsupported_type_or_symbol_width_headers_are_rejected() {
    for header in 0u8..=255 {
        if [0x24, 0x28].contains(&header) {
            continue;
        }
        let (mut m, _) = prepare(false, DEST, &[header, 4, 0, 0]);
        assert_invalid(fail(&mut m));
        assert_eq!(m.memory().read32(DEST).unwrap(), 0);
    }
}

#[test]
fn source_destination_length_and_bitstream_alignment_are_checked() {
    let (data, _) = encode(8, &pair(), &[0x12; 4]);
    for (source, dest) in [
        (0, DEST),
        (0x3ffc, DEST),
        (0x0100_0000, DEST),
        (ROM_DATA + 1, DEST),
        (ROM_DATA + 2, DEST),
        (ROM_DATA + 3, DEST),
        (ROM_DATA, DEST + 1),
        (ROM_DATA, DEST + 2),
        (ROM_DATA, DEST + 3),
        (ROM_DATA, 0xffff_fffc),
    ] {
        let (mut m, _) = call_status_data(0x13, false, [source, dest, 0], 0x1f, &data);
        assert_invalid(fail(&mut m));
    }
    for length in [1, 2, 3, 5, 6, 7] {
        let (mut m, _) = prepare(true, DEST, &[0x28, length, 0, 0]);
        assert_invalid(fail(&mut m));
    }
    for size in [0, 2, 128, 254] {
        let (mut m, _) = prepare(false, DEST, &[0x28, 4, 0, 0, size]);
        assert_invalid(fail(&mut m)); // Table end must be aligned, not rounded silently.
    }
}

#[test]
fn traversed_out_of_table_children_fail_without_reading_them() {
    for root in [0xc1, 0xc2, 0xff, 0] {
        // Size=1 permits only root + two bytes. Root=0 treats a child as another
        // internal node; its next edge must also stay inside the declared table.
        let data = [0x28, 4, 0, 0, 1, root, 0, 0, 0, 0, 0, 0];
        let (mut m, _) = prepare(false, DEST, &data);
        assert_invalid(fail(&mut m));
        assert_eq!(m.memory().read32(DEST).unwrap(), 0);
    }
}

#[test]
fn invalid_nibble_leaves_discard_pending_word_but_keep_prior_output() {
    let mut data = vec![0x24, 8, 0, 0, 1, 0xc0, 2, 0x10];
    data.extend(0x0040_0000u32.to_le_bytes()); // Eight left leaves, then left, then invalid right.
    let (mut m, _) = prepare(true, DEST, &data);
    m.memory_mut().write32(DEST + 4, 0xcccc_cccc).unwrap();
    assert_invalid(fail(&mut m));
    assert_eq!(m.memory().read32(DEST).unwrap(), 0x2222_2222);
    assert_eq!(m.memory().read32(DEST + 4).unwrap(), 0xcccc_cccc);
}

#[test]
fn unused_invalid_leaf_does_not_block_valid_decoding() {
    let data = [0x24, 4, 0, 0, 1, 0xc0, 2, 0xff, 0, 0, 0, 0];
    verify(false, DEST, &data, &[0x22; 4]);
}

#[test]
fn truncated_headers_tables_and_words_return_memory_diagnostics() {
    let (data, _) = encode(8, &pair(), &[0x12; 4]);
    for length in 0..data.len() {
        let (mut m, _) = prepare(false, DEST, &data[..length]);
        // Input words are loaded before the first node. Missing table storage
        // therefore faults at the declared bitstream address, not at a tree pre-scan.
        let address = ROM_DATA
            + if (5..8).contains(&length) {
                8
            } else {
                length as u32
            };
        assert_eq!(
            fail(&mut m),
            MachineError::Cpu(MemoryError::Unmapped(address).into()),
            "length={length}"
        );
    }
}

#[test]
fn input_word_refill_failure_does_not_commit_incomplete_output() {
    for width in [4, 8] {
        let symbols = vec![3; if width == 4 { 24 } else { 12 }];
        let (mut data, _) = encode(width, &chain(3), &symbols);
        let bitstream = 4 + (usize::from(data[4]) + 1) * 2;
        data.truncate(bitstream + 4); // Only 32 of the required branch bits exist.
        let (mut m, _) = prepare(false, DEST, &data);
        for offset in [0, 4, 8] {
            m.memory_mut().write32(DEST + offset, 0xcccc_cccc).unwrap();
        }
        assert_eq!(
            fail(&mut m),
            MachineError::Cpu(MemoryError::Unmapped(ROM_DATA + data.len() as u32).into())
        );
        let word = if width == 4 { 0x3333_3333 } else { 0x0303_0303 };
        assert_eq!(m.memory().read32(DEST).unwrap(), word);
        assert_eq!(
            m.memory().read32(DEST + 4).unwrap(),
            if width == 4 { 0xcccc_cccc } else { word }
        );
        assert_eq!(m.memory().read32(DEST + 8).unwrap(), 0xcccc_cccc);
    }
}

#[test]
fn destination_failure_preserves_preceding_completed_word() {
    let (data, _) = encode(8, &pair(), &[0x12; 8]);
    let (mut m, _) = prepare(false, 0x07ff_fffc, &data);
    assert_eq!(
        fail(&mut m),
        MachineError::Cpu(MemoryError::ReadOnly(ROM_START).into())
    );
    assert_eq!(m.memory().read32(0x07ff_fffc).unwrap(), 0x1212_1212);
}

#[test]
fn word_output_works_in_video_palette_and_object_memory() {
    let (data, expected) = encode(8, &pair(), &[0x12, 0x34, 0x34, 0x12]);
    for dest in [VRAM_START + 4, PALETTE_START + 4, OAM_START + 4] {
        verify(false, dest, &data, &expected);
    }
}

#[test]
fn declared_output_end_ignores_remaining_bits_and_trailing_words() {
    let (mut data, expected) = encode(8, &pair(), &[0x12; 4]);
    data[8..12].copy_from_slice(&0x0fff_ffffu32.to_le_bytes());
    data.extend([0xff; 4]);
    verify(true, DEST, &data, &expected);
}

#[test]
fn device_clocks_and_dma_continue_while_huffman_masks_irq_delivery() {
    let (data, expected) = encode(8, &pair(), &[0x12; 32]);
    let (mut m, pc) = prepare(false, DEST, &data);
    m.step().unwrap();
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
