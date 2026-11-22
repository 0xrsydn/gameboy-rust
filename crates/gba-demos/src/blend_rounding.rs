//! An original hardware test for the rounding of the brightness effects (BLDY).
//!
//! The CPU cannot read back a composed pixel, so the answer has to be seen on a screen. The
//! ROM shows four bands, each with one brightness effect and strength. Each band has seven
//! groups of three adjacent blocks, each one tile wide and four tiles high: the lower
//! prediction, the sample (gray `x` on BG0 with the effect applied), and the higher prediction
//! (both on BG1, without the effect). On hardware the edge between the sample and the prediction it equals
//! disappears; the other edge stays visible. The comparison is made on one screen, so the
//! capture's own color handling cannot change the answer.
//!
//! Exact result `x·(16−EVY)/16` (darken) or `x + (31−x)·EVY/16` (brighten); each group's
//! `x` is chosen so that result is not a whole number, so the two predictions differ by one.
//! Darkening, mGBA rounds the result up, Mesen 2 and ares round it down, and NanoBoyAdvance
//! rounds to nearest (and blends green as 6 bits); brightening, only NanoBoyAdvance differs.
//! See docs/research/blend-rounding.md.
//!
//! The header leaves the logo blank: no Nintendo data is included. Run devkitPro's `gbafix`
//! on a copy before putting it on a cartridge; emulators boot it as it is.

/// Lines at which each band's effect takes over; band 0 starts at VBlank, for the next frame.
const BAND_LINES: [u32; 4] = [160, 40, 80, 120];
const GROUPS: usize = 7;
const ROM_BASE: u32 = 0x0800_0000;
const CODE: usize = 0xc0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Brighten,
    Darken,
}

impl Effect {
    fn bldcnt(self) -> u32 {
        match self {
            // Mode 2 or 3, first target BG0 only.
            Effect::Brighten => 0x81,
            Effect::Darken => 0xc1,
        }
    }
}

/// One band: an effect at one strength (EVY).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Band {
    pub effect: Effect,
    pub evy: u32,
}

pub const BANDS: [Band; 4] = [
    Band {
        effect: Effect::Darken,
        evy: 3,
    },
    Band {
        effect: Effect::Darken,
        evy: 11,
    },
    Band {
        effect: Effect::Brighten,
        evy: 5,
    },
    Band {
        effect: Effect::Brighten,
        evy: 13,
    },
];

/// Which prediction an emulator's formula gives for a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Lower,
    Higher,
}

/// One group: the sample's gray level and the two whole-number neighbors of the exact result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Group {
    pub x: u32,
    pub lower: u32,
    pub higher: u32,
    pub mgba: Side,
    pub mesen: Side,
    pub nba: Side,
}

impl Band {
    /// The amount the effect moves `x`, times 16.
    fn step16(self, x: u32) -> u32 {
        match self.effect {
            Effect::Darken => x * self.evy,
            Effect::Brighten => (31 - x) * self.evy,
        }
    }

    pub fn group(self, x: u32) -> Group {
        let step = self.step16(x);
        let (floor, ceil, remainder) = (step / 16, step.div_ceil(16), step % 16);
        match self.effect {
            // mGBA: x - floor; Mesen/ares: (x(16-e))>>4 = x - ceil; NBA: x - ((xe+7)>>4).
            Effect::Darken => Group {
                x,
                lower: x - ceil,
                higher: x - floor,
                mgba: Side::Higher,
                mesen: Side::Lower,
                nba: if remainder <= 8 {
                    Side::Higher
                } else {
                    Side::Lower
                },
            },
            // mGBA and Mesen/ares: x + floor; NBA: x + (((31-x)e+8)>>4).
            Effect::Brighten => Group {
                x,
                lower: x + floor,
                higher: x + ceil,
                mgba: Side::Lower,
                mesen: Side::Lower,
                nba: if remainder >= 8 {
                    Side::Higher
                } else {
                    Side::Lower
                },
            },
        }
    }

    /// Seven gray levels spread over the range, each with a fractional result.
    pub fn groups(self) -> [Group; GROUPS] {
        let mut chosen: Vec<u32> = Vec::new();
        for wanted in [4, 8, 12, 16, 20, 24, 28] {
            let x = (0..32)
                .filter(|x| self.step16(*x) % 16 != 0 && !chosen.contains(x))
                .min_by_key(|x| x.abs_diff(wanted))
                .expect("a fractional level near each target");
            chosen.push(x);
        }
        chosen.sort_unstable();
        std::array::from_fn(|index| self.group(chosen[index]))
    }
}

fn gray(level: u32) -> u16 {
    (level | level << 5 | level << 10) as u16
}

/// Screen tile rows of band `band`'s groups: rows 1-4 of its five; row 0 is a blank margin
/// under the line where the CPU changes the effect.
fn band_rows(band: usize) -> std::ops::Range<usize> {
    band * 5 + 1..band * 5 + 5
}

/// Tile column of a group's first block; groups are three blocks and a gap wide.
pub fn group_column(group: usize) -> usize {
    1 + group * 4
}

/// Pixel at the middle of a block (0 lower, 1 sample, 2 higher).
pub fn block_center(band: usize, group: usize, block: usize) -> (usize, usize) {
    let x = (group_column(group) + block) * 8 + 4;
    let y = band_rows(band).start * 8 + 16;
    (x, y)
}

const ARM_CODE: [u32; 26] = [
    0xe3a0_0301, // 00 MOV r0,#0x04000000
    0xe28f_905c, // 04 ADD r9,pc,#0x5c (tables at 0x68)
    0xe8b9_000e, // 08 copies: LDMIA r9!,{r1-r3} (source, destination, bytes)
    0xe353_0000, // 0c CMP r3,#0
    0x0a00_0004, // 10 BEQ regs
    0xe8b1_00f0, // 14 copy: LDMIA r1!,{r4-r7}
    0xe8a2_00f0, // 18 STMIA r2!,{r4-r7}
    0xe253_3010, // 1c SUBS r3,r3,#16
    0xcaff_fffb, // 20 BGT copy
    0xeaff_fff7, // 24 B copies
    0xe8b9_0006, // 28 regs: LDMIA r9!,{r1,r2} (address, value)
    0xe351_0000, // 2c CMP r1,#0
    0x0a00_0001, // 30 BEQ bands
    0xe1c1_20b0, // 34 STRH r2,[r1]
    0xeaff_fffa, // 38 B regs
    0xe1a0_a009, // 3c bands: MOV r10,r9
    0xe1a0_900a, // 40 frame: MOV r9,r10
    0xe8b9_000e, // 44 entry: LDMIA r9!,{r1-r3} (line, BLDCNT, BLDY)
    0xe371_0001, // 48 CMN r1,#1 (end of the band table)
    0x0aff_fffb, // 4c BEQ frame
    0xe1d0_40b6, // 50 wait: LDRH r4,[r0,#6] (VCOUNT)
    0xe154_0001, // 54 CMP r4,r1
    0x1aff_fffc, // 58 BNE wait
    0xe1c0_25b0, // 5c STRH r2,[r0,#0x50] (BLDCNT)
    0xe1c0_35b4, // 60 STRH r3,[r0,#0x54] (BLDY)
    0xeaff_fff6, // 64 B entry
];
const TABLES: usize = CODE + 0x68;

/// The test ROM.
pub fn blend_rounding_rom() -> Vec<u8> {
    // Palette: 0 is the black backdrop; every other color gets its own index, and tile n is
    // a solid 8bpp tile of color n.
    let mut colors: Vec<u16> = vec![0];
    let mut index = |color: u16| -> u16 {
        if let Some(found) = colors[1..].iter().position(|&c| c == color) {
            return found as u16 + 1;
        }
        colors.push(color);
        colors.len() as u16 - 1
    };
    let mut map0 = vec![0u16; 32 * 32];
    let mut map1 = vec![0u16; 32 * 32];
    for (band_index, band) in BANDS.iter().enumerate() {
        for (group_index, group) in band.groups().iter().enumerate() {
            let column = group_column(group_index);
            let lower = index(gray(group.lower));
            let sample = index(gray(group.x));
            let higher = index(gray(group.higher));
            for row in band_rows(band_index) {
                map1[row * 32 + column] = lower;
                map0[row * 32 + column + 1] = sample;
                map1[row * 32 + column + 2] = higher;
            }
        }
    }
    assert!(colors.len() <= 256);
    let palette: Vec<u8> = (0..256)
        .flat_map(|i| colors.get(i).copied().unwrap_or(0).to_le_bytes())
        .collect();
    let tiles: Vec<u8> = (0..256u32).flat_map(|n| [n as u8; 64]).collect();
    let bytes = |map: &[u16]| -> Vec<u8> { map.iter().flat_map(|e| e.to_le_bytes()).collect() };
    let blobs = [
        (palette, 0x0500_0000u32),
        (tiles, 0x0600_0000),
        (bytes(&map0), 0x0600_8000), // screen block 16
        (bytes(&map1), 0x0600_8800), // screen block 17
    ];

    let mut tables: Vec<u32> = Vec::new();
    let copies = 4 * 3 + 3;
    let regs = [
        (0x0400_0008, 0x1080), // BG0CNT: priority 0, 8bpp, screen block 16
        (0x0400_000a, 0x1181), // BG1CNT: priority 1, 8bpp, screen block 17
        (0x0400_0000, 0x0300), // DISPCNT: mode 0, BG0 and BG1, no forced blank
    ];
    let reg_words = regs.len() * 2 + 2;
    let band_words = BANDS.len() * 3 + 3;
    let data_start = TABLES + (copies + reg_words + band_words) * 4;
    let mut data = Vec::new();
    for (blob, destination) in &blobs {
        assert_eq!(blob.len() % 16, 0);
        let source = ROM_BASE + (data_start + data.len()) as u32;
        tables.extend([source, *destination, blob.len() as u32]);
        data.extend_from_slice(blob);
    }
    tables.extend([0, 0, 0]);
    for (address, value) in regs {
        tables.extend([address, value]);
    }
    tables.extend([0, 0]);
    for (band, line) in BANDS.iter().zip(BAND_LINES) {
        tables.extend([line, band.effect.bldcnt(), band.evy]);
    }
    tables.extend([u32::MAX, 0, 0]);

    let mut rom = vec![0u8; CODE];
    rom[0..4].copy_from_slice(&0xea00_002e_u32.to_le_bytes()); // B 0xc0
    rom[0xa0..0xac].copy_from_slice(b"BLENDROUND\0\0");
    rom[0xac..0xb0].copy_from_slice(b"ZBRE");
    rom[0xb0..0xb2].copy_from_slice(b"00");
    rom[0xb2] = 0x96;
    let sum = rom[0xa0..0xbd]
        .iter()
        .fold(0u8, |sum, &byte| sum.wrapping_add(byte));
    rom[0xbd] = 0u8.wrapping_sub(sum).wrapping_sub(0x19);
    rom.extend(ARM_CODE.iter().flat_map(|word| word.to_le_bytes()));
    assert_eq!(rom.len(), TABLES);
    rom.extend(tables.iter().flat_map(|word| word.to_le_bytes()));
    assert_eq!(rom.len(), data_start);
    rom.extend(data);
    rom
}

/// The layout as a Markdown table: what each group predicts per emulator.
pub fn blend_rounding_table() -> String {
    let side = |side: Side| match side {
        Side::Lower => "left",
        Side::Higher => "right",
    };
    let mut text = String::from(
        "| Band | Effect | EVY | Group | x | Left | Right | mGBA | Mesen, ares | NanoBoyAdvance |\n\
         | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for (band_index, band) in BANDS.iter().enumerate() {
        for (group_index, group) in band.groups().iter().enumerate() {
            text += &format!(
                "| {} | {:?} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
                band_index + 1,
                band.effect,
                band.evy,
                group_index + 1,
                group.x,
                group.lower,
                group.higher,
                side(group.mgba),
                side(group.mesen),
                side(group.nba),
            );
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_group_separates_the_two_roundings_and_the_bands_separate_all_three() {
        for band in BANDS {
            for group in band.groups() {
                assert_eq!(group.higher, group.lower + 1, "{band:?} {group:?}");
            }
            let groups = band.groups();
            if band.effect == Effect::Darken {
                // NBA agrees with mGBA in some groups and with Mesen in others.
                assert!(groups.iter().any(|g| g.nba == g.mgba));
                assert!(groups.iter().any(|g| g.nba == g.mesen));
            } else {
                assert!(groups.iter().any(|g| g.nba != g.mgba));
            }
        }
    }

    #[test]
    fn the_formulas_match_each_emulator() {
        let pick = |group: Group, side: Side| match side {
            Side::Lower => group.lower,
            Side::Higher => group.higher,
        };
        for band in BANDS {
            for group in band.groups() {
                let (x, e) = (group.x, band.evy);
                match band.effect {
                    Effect::Darken => {
                        assert_eq!(pick(group, group.mgba), x - ((x * e) >> 4));
                        assert_eq!(pick(group, group.mesen), (x * (16 - e)) >> 4);
                        assert_eq!(pick(group, group.nba), x - ((x * e + 7) >> 4));
                    }
                    Effect::Brighten => {
                        assert_eq!(pick(group, group.mgba), x + (((31 - x) * e) >> 4));
                        assert_eq!(pick(group, group.mesen), (x * (16 - e) + 31 * e) >> 4);
                        assert_eq!(pick(group, group.nba), x + (((31 - x) * e + 8) >> 4));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod emulation_tests {
    use super::*;
    use gba_core::{bios, machine::Machine, video::DarkenRounding, video::Framebuffer};

    fn level(pixel: u32) -> u32 {
        // 0x00RRGGBB from a gray RGB555 color; every channel is the same.
        (pixel >> 16 & 0xff) >> 3
    }

    fn frames(machine: &mut Machine, count: usize) -> Framebuffer {
        for _ in 0..count {
            machine.run_until_vblank(2_000_000).unwrap();
        }
        let mut frame = Framebuffer::default();
        assert!(machine.memory().present_frame(&mut frame).unwrap());
        frame
    }

    /// Check every group's blocks; `side` picks the emulator whose prediction the sample shows.
    fn check(frame: &Framebuffer, side: impl Fn(&Group) -> Side) {
        let at = |(x, y): (usize, usize)| level(frame.pixels()[y * 240 + x]);
        for (band_index, band) in BANDS.iter().enumerate() {
            for (group_index, group) in band.groups().iter().enumerate() {
                let [lower, sample, higher] =
                    [0, 1, 2].map(|block| at(block_center(band_index, group_index, block)));
                assert_eq!(
                    (lower, higher),
                    (group.lower, group.higher),
                    "{band:?} {group:?}"
                );
                let expected = match side(group) {
                    Side::Lower => group.lower,
                    Side::Higher => group.higher,
                };
                assert_eq!(sample, expected, "{band:?} {group:?}");
            }
        }
        // The blank margin rows and gaps stay backdrop black.
        assert_eq!(frame.pixels()[0], 0);
        assert_eq!(frame.pixels()[44 * 240 + 4], 0);
    }

    #[test]
    fn this_emulator_shows_its_mgba_rounding_on_the_sample_side() {
        let mut machine = bios::boot(blend_rounding_rom()).unwrap();
        machine.memory_mut().set_scanline_rendering(true);
        // Frame 1 starts without band 0's effect; frame 3 is steady.
        check(&frames(&mut machine, 3), |group| group.mgba);
    }

    #[test]
    fn rounding_down_shows_mesens_side_and_survives_loading_a_state() {
        let mut machine = bios::boot(blend_rounding_rom()).unwrap();
        machine.memory_mut().set_scanline_rendering(true);
        let state = {
            frames(&mut machine, 1);
            machine.save_state().unwrap()
        };
        machine
            .memory_mut()
            .set_darken_rounding(DarkenRounding::Down);
        check(&frames(&mut machine, 2), |group| group.mesen);
        machine.load_state(&state).unwrap();
        check(&frames(&mut machine, 3), |group| group.mesen);
    }

    #[test]
    fn the_header_is_well_formed_except_for_the_logo() {
        let rom = blend_rounding_rom();
        assert_eq!(&rom[0..4], &0xea00_002e_u32.to_le_bytes());
        assert!(rom[4..0xa0].iter().all(|&byte| byte == 0), "no logo data");
        let sum = rom[0xa0..0xbd].iter().fold(0u8, |s, &b| s.wrapping_add(b));
        assert_eq!(sum.wrapping_add(rom[0xbd]).wrapping_add(0x19), 0);
    }
}
