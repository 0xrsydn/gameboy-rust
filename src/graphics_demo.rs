//! Original ARM program for CPU-driven Mode 3 graphics and polled GBA input.
//! ARM code uses DMA3 for the background fill, then redraws during VBlank.
//! The optional original BIOS supplies VBlankIntrWait and IRQ dispatch.
//! The host runs between display events, not RAM update-counter changes.

use std::{error::Error, fmt};

use crate::{
    bios,
    cpu::CpuError,
    dma::DmaError,
    input::Buttons,
    io::DISPCNT,
    machine::{FrameRunError, Machine, MachineError},
    memory::{MemoryError, ROM_START},
    video::{Framebuffer, VideoError},
};

pub const DEMO_STATE: u32 = 0x0200_0000; // completed updates, x, y, color (debug state)
pub const SQUARE_SIZE: usize = 16;
pub const BACKGROUND: u16 = 0x4000;
const FRAME_STEP_LIMIT: usize = 200_000;

#[derive(Debug)]
pub enum GraphicsError {
    Cpu(CpuError),
    Dma(DmaError),
    Memory(MemoryError),
    Video(VideoError),
    FrameTimeout,
    FrameUnavailable,
}

impl fmt::Display for GraphicsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cpu(error) => write!(f, "{error}"),
            Self::Dma(error) => write!(f, "{error}"),
            Self::Memory(error) => write!(f, "{error}"),
            Self::Video(error) => write!(f, "{error}"),
            Self::FrameUnavailable => write!(f, "no complete scanline frame is available"),
            Self::FrameTimeout => write!(
                f,
                "graphics demo exceeded {FRAME_STEP_LIMIT} steps without completing a frame"
            ),
        }
    }
}

impl Error for GraphicsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cpu(error) => Some(error),
            Self::Dma(error) => Some(error),
            Self::Memory(error) => Some(error),
            Self::Video(error) => Some(error),
            Self::FrameTimeout | Self::FrameUnavailable => None,
        }
    }
}

impl From<MachineError> for GraphicsError {
    fn from(error: MachineError) -> Self {
        match error {
            MachineError::Cpu(error) => Self::Cpu(error),
            MachineError::Dma(error) => Self::Dma(error),
        }
    }
}

pub struct GraphicsDemo {
    machine: Machine,
    primed: bool,
}

impl GraphicsDemo {
    pub fn new() -> Result<Self, MemoryError> {
        Self::with_program(program())
    }

    pub(crate) fn with_program(rom: Vec<u8>) -> Result<Self, MemoryError> {
        Ok(Self {
            machine: bios::boot(rom)?,
            primed: false,
        })
    }

    pub fn machine(&self) -> &Machine {
        &self.machine
    }

    /// Supply input, run to the next VBlank entry, then present the captured image.
    /// The first call also runs startup under forced blank and primes the VBlank wait.
    /// Emulated CPU code and DMA write display registers, VRAM, and demo state.
    /// Errors retain completed machine steps; the output buffer is not updated.
    pub fn frame(
        &mut self,
        buttons: Buttons,
        output: &mut Framebuffer,
    ) -> Result<usize, GraphicsError> {
        self.frame_with_control(buttons, output, 0x403)
    }

    pub(crate) fn frame_with_control(
        &mut self,
        buttons: Buttons,
        output: &mut Framebuffer,
        ready_control: u16,
    ) -> Result<usize, GraphicsError> {
        self.machine.memory_mut().set_scanline_rendering(true);
        self.machine.memory_mut().set_buttons(buttons);
        let mut steps = 0;
        if !self.primed {
            // Suppress incomplete startup images. This watches a real display
            // register, not a software mailbox or an instruction-address breakpoint.
            while self
                .machine
                .memory()
                .read16(DISPCNT)
                .map_err(GraphicsError::Memory)?
                != ready_control
            {
                if steps == FRAME_STEP_LIMIT {
                    return Err(GraphicsError::FrameTimeout);
                }
                self.machine.step().map_err(GraphicsError::from)?;
                steps += 1;
            }
            steps += self.wait_vblank(FRAME_STEP_LIMIT - steps)?;
            self.primed = true;
        }
        // Drawing occurs in the preceding VBlank. At this next VBlank entry
        // the completed visible frame is ready; the new update has not begun.
        steps += self.wait_vblank(FRAME_STEP_LIMIT - steps)?;
        if !self
            .machine
            .memory()
            .present_frame(output)
            .map_err(GraphicsError::Video)?
        {
            return Err(GraphicsError::FrameUnavailable);
        }
        Ok(steps)
    }

    fn wait_vblank(&mut self, max_steps: usize) -> Result<usize, GraphicsError> {
        self.machine
            .run_until_vblank(max_steps)
            .map_err(|error| match error {
                FrameRunError::Cpu(error) => GraphicsError::Cpu(error),
                FrameRunError::Dma(error) => GraphicsError::Dma(error),
                FrameRunError::StepLimit(_) => GraphicsError::FrameTimeout,
            })
    }
}

fn program() -> Vec<u8> {
    let mut code = vec![
        0xe3a0_8406, // MOV r8, #0x06000000 (VRAM)
        0xe3a0_9301, // MOV r9, #0x04000000 (DISPCNT)
        0xe289_ac01, // ADD r10, r9, #0x100
        0xe28a_a030, // ADD r10, r10, #0x30 (KEYINPUT)
        0xe3a0_b402, // MOV r11, #0x02000000 (demo state)
        0xe3a0_0b01, // MOV r0, #0x400
        0xe380_0083, // ORR r0, r0, #0x83 (Mode 3, BG2, forced blank)
        0xe1c9_00b0, // STRH r0, [r9]
        0xe3a0_0c01, // MOV r0,#256 (identity BG2 transform)
        0xe1c9_02b0, // STRH r0,[r9,#0x20] (BG2PA)
        0xe1c9_02b6, // STRH r0,[r9,#0x26] (BG2PD)
        0xe3a0_6901, // MOV r6, #0x4000 (blue background)
        0xe1cb_61b0, // STRH r6, [r11, #16] (DMA fill source)
        0xe289_00d4, // ADD r0, r9, #0xd4 (DMA3SAD)
        0xe28b_1010, // ADD r1, r11, #16
        0xe580_1000, // STR r1, [r0] (source)
        0xe580_8004, // STR r8, [r0, #4] (VRAM destination)
        0xe3a0_1c96, // MOV r1, #38400 (halfwords)
        0xe3a0_2481, // MOV r2, #0x81000000 (enable, fixed source)
        0xe181_1002, // ORR r1, r1, r2
        0xe580_1008, // STR r1, [r0, #8] (count/control; CPU pauses for DMA)
    ];
    let irq_pointer_load = code.len();
    code.extend([
        0,           // LDR r0,=irq_handler (patched below)
        0xe3a0_1403, // MOV r1,#0x03000000
        0xe281_1c7f, // ADD r1,r1,#0x7f00
        0xe581_00fc, // STR r0,[r1,#0xfc] (BIOS IRQ callback pointer)
        0xe3a0_0008, // MOV r0, #8 (VBlank request enable)
        0xe1c9_00b4, // STRH r0, [r9, #4] (DISPSTAT)
        0xe289_1c02, // ADD r1, r9, #0x200 (IE)
        0xe3a0_0001, // MOV r0, #1
        0xe1c1_00b0, // STRH r0, [r1] (IE=VBlank; wait service will enable IME)
        0xe3a0_4070, // MOV r4, #112 (x)
        0xe3a0_5048, // MOV r5, #72 (y)
        0xe3a0_7000, // MOV r7, #0 (completed frames)
        0xe3a0_0b01, // MOV r0, #0x400
        0xe380_0003, // ORR r0, r0, #3 (Mode 3 + BG2, no forced blank)
        0xe1c9_00b0, // STRH r0, [r9]
    ]);
    // VBlankIntrWait discards the old BIOS flag and waits for a new IRQ callback.
    let frame = code.len();
    code.push(0xef05_0000); // ARM SWI #0x050000 (VBlankIntrWait)
    code.push(0xe3a0_6901); // MOV r6, #0x4000 (erase old square)
    let erase_call = code.len();
    code.push(0); // BL draw_square, patched below
    code.extend([
        0xe1da_30b0, // LDRH r3, [r10] (sample active-low KEYINPUT once)
        0xe313_0010, // TST r3, #Right
        0x0284_4002, // ADDEQ r4, r4, #2
        0xe313_0020, // TST r3, #Left
        0x0244_4002, // SUBEQ r4, r4, #2
        0xe354_0000, // CMP r4, #0
        0xb3a0_4000, // MOVLT r4, #0
        0xe354_00e0, // CMP r4, #224
        0xc3a0_40e0, // MOVGT r4, #224
        0xe313_0040, // TST r3, #Up
        0x0245_5002, // SUBEQ r5, r5, #2
        0xe313_0080, // TST r3, #Down
        0x0285_5002, // ADDEQ r5, r5, #2
        0xe355_0000, // CMP r5, #0
        0xb3a0_5000, // MOVLT r5, #0
        0xe355_0090, // CMP r5, #144
        0xc3a0_5090, // MOVGT r5, #144
        0xe313_0008, // TST r3, #Start
        0x03a0_4070, // MOVEQ r4, #112
        0x03a0_5048, // MOVEQ r5, #72
        0xe3a0_6c7f, // MOV r6, #0x7f00
        0xe386_60ff, // ORR r6, r6, #0xff (white)
        0xe313_0002, // TST r3, #B
        0x03a0_6e3e, // MOVEQ r6, #0x03e0 (green)
        0xe313_0001, // TST r3, #A
        0x03a0_601f, // MOVEQ r6, #0x001f (red, A takes priority)
    ]);
    let draw_call = code.len();
    code.push(0); // BL draw_square
    code.extend([
        0xe58b_4004, // STR r4, [r11, #4]
        0xe58b_5008, // STR r5, [r11, #8]
        0xe58b_600c, // STR r6, [r11, #12]
        0xe287_7001, // ADD r7, r7, #1
        0xe58b_7000, // STR r7, [r11] (debug counter only; host does not wait on it)
    ]);
    branch(&mut code, frame, 14, false);

    // Draw a 16x16 square. r4=x, r5=y, r6=color. Clobbers r0-r2 only.
    let draw_square = code.len();
    code.extend([
        0xe065_0205, // RSB r0, r5, r5, LSL #4 (y * 15)
        0xe1a0_0200, // MOV r0, r0, LSL #4 (y * 240)
        0xe080_0004, // ADD r0, r0, r4
        0xe088_0080, // ADD r0, r8, r0, LSL #1 (pixel byte address)
        0xe3a0_1010, // MOV r1, #16 (rows)
    ]);
    let row = code.len();
    code.push(0xe3a0_2010); // MOV r2, #16 (columns)
    let pixel = code.len();
    code.extend([
        0xe0c0_60b2, // STRH r6, [r0], #2
        0xe252_2001, // SUBS r2, r2, #1
    ]);
    branch(&mut code, pixel, 1, false);
    code.extend([
        0xe280_0d07, // ADD r0, r0, #448 (remaining bytes in row)
        0xe251_1001, // SUBS r1, r1, #1
    ]);
    branch(&mut code, row, 1, false);
    code.push(0xe12f_ff1e); // BX lr
    patch_branch(&mut code, erase_call, draw_square, 14, true);
    patch_branch(&mut code, draw_call, draw_square, 14, true);

    // BIOS dispatch saves r0-r3/r12/lr. The callback must acknowledge IF and
    // record its requests in BIOS IRQ_FLAGS, then return to the dispatcher.
    let irq_handler = code.len();
    code.extend([
        0xe280_0c02, // ADD r0,r0,#0x200 (dispatcher supplies I/O base)
        0xe1d0_10b2, // LDRH r1,[r0,#2] (IF)
        0xe1c0_10b2, // STRH r1,[r0,#2] (acknowledge)
        0xe3a0_0403, // MOV r0,#0x03000000
        0xe280_0c7f, // ADD r0,r0,#0x7f00
        0xe1d0_2fb8, // LDRH r2,[r0,#0xf8] (BIOS IRQ_FLAGS)
        0xe182_2001, // ORR r2,r2,r1
        0xe1c0_2fb8, // STRH r2,[r0,#0xf8]
        0xe12f_ff1e, // BX lr
    ]);
    let offset = (code.len() - irq_pointer_load - 2) * 4;
    assert!(offset <= 0xfff);
    code[irq_pointer_load] = 0xe59f_0000 | offset as u32;
    code.push(ROM_START + irq_handler as u32 * 4);
    code.into_iter().flat_map(u32::to_le_bytes).collect()
}

fn branch(code: &mut Vec<u32>, target: usize, condition: u32, link: bool) {
    let index = code.len();
    code.push(0);
    patch_branch(code, index, target, condition, link);
}

fn patch_branch(code: &mut [u32], index: usize, target: usize, condition: u32, link: bool) {
    let displacement = target as i32 - index as i32 - 2;
    assert!((-0x80_0000..0x80_0000).contains(&displacement));
    code[index] = (condition << 28)
        | 0x0a00_0000
        | (u32::from(link) << 24)
        | (displacement as u32 & 0x00ff_ffff);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{cpu::Cpu, io::DISPSTAT, memory::Memory};

    #[test]
    fn frame_runner_does_not_require_a_ram_counter_update() {
        let mut memory = Memory::new(0xeaff_fffe_u32.to_le_bytes().to_vec()).unwrap();
        memory.write16(DISPCNT, 0x403).unwrap();
        let mut demo = GraphicsDemo {
            machine: Machine::new(Cpu::new(ROM_START), memory),
            primed: true,
        };
        let mut frame = Framebuffer::default();
        for expected in 1..=2 {
            demo.frame(Buttons::default(), &mut frame).unwrap();
            assert_eq!(demo.machine().memory().read32(DEMO_STATE).unwrap(), 0);
            assert_eq!(demo.machine().memory().display_position().vblanks, expected);
            assert_eq!(demo.machine().memory().display_position().scanline, 160);
        }
    }

    #[test]
    fn arm_startup_uses_dma3_for_every_background_pixel() {
        use crate::{
            dma::DMA_BASE,
            machine::StepKind,
            memory::VRAM_START,
            video::{HEIGHT, WIDTH},
        };

        let mut demo = GraphicsDemo::new().unwrap();
        let mut transfers = 0;
        let mut ready = false;
        for _ in 0..50_000 {
            let before = demo.machine.cpu().clone();
            if let StepKind::Dma { channel } = demo.machine.step().unwrap() {
                assert_eq!(channel, 3);
                assert_eq!(demo.machine.cpu(), &before);
                assert_eq!(demo.machine.memory().read16(DISPCNT).unwrap(), 0x483);
                transfers += 1;
            }
            if demo.machine.memory().read16(DISPCNT).unwrap() == 0x403 {
                ready = true;
                break;
            }
        }
        assert!(ready);
        assert_eq!(transfers, WIDTH * HEIGHT);
        assert_eq!(
            demo.machine
                .memory()
                .read16(DMA_BASE + 3 * 12 + 10)
                .unwrap()
                & 0x8000,
            0
        );
        for index in 0..WIDTH * HEIGHT {
            assert_eq!(
                demo.machine
                    .memory()
                    .read16(VRAM_START + index as u32 * 2)
                    .unwrap(),
                BACKGROUND
            );
        }
    }

    #[test]
    fn dma_error_keeps_output_unchanged() {
        use crate::dma::DMA_BASE;

        let mut demo = GraphicsDemo::new().unwrap();
        // An unsupported special-mode request fails before any CPU instruction.
        demo.machine
            .memory_mut()
            .write16(DMA_BASE + 10, 0xb000)
            .unwrap();
        let mut frame = Framebuffer::default();
        frame.clear(0x1f);
        assert!(matches!(
            demo.frame(Buttons::default(), &mut frame),
            Err(GraphicsError::Dma(_))
        ));
        assert_eq!(demo.machine().cycles(), 0);
        assert!(frame.pixels().iter().all(|&pixel| pixel == 0xff0000));
    }

    #[test]
    fn graphics_uses_bios_wait_and_irq_dispatch_before_each_redraw() {
        use crate::{
            cpu::Mode,
            io::{IE, IF, IME},
            machine::StepKind,
        };

        let mut demo = GraphicsDemo::new().unwrap();
        let mut frame = Framebuffer::default();
        for expected in 1..=3 {
            demo.frame(Buttons::default(), &mut frame).unwrap();
            assert_eq!(
                demo.machine().memory().read32(DEMO_STATE).unwrap(),
                expected
            );
            assert_eq!(demo.machine().memory().display_position().line_cycle, 0);
            assert_eq!(demo.machine().memory().read16(IE).unwrap(), 1);
            assert_eq!(demo.machine().memory().read16(IME).unwrap(), 0);
            assert_eq!(demo.machine().memory().read16(IF).unwrap(), 1);
            assert!(demo.machine().last_timing().idle_cycles > 0);
            assert_eq!(demo.machine().last_timing().code_cycles, 0);
            assert_eq!(demo.machine().cpu().mode(), Mode::Supervisor);
            assert_eq!(demo.machine().cpu().cpsr() & 0x80, 0);
            assert!(!demo.machine().halted()); // VBlank wakes HALT while the wait service has IME=0.
            let mut entries = 0;
            for _ in 0..200 {
                if demo.machine().cpu().mode() == Mode::System {
                    break;
                }
                if demo.machine.step().unwrap() == StepKind::IrqEntry {
                    entries += 1;
                }
            }
            assert_eq!(entries, 1);
            assert_eq!(demo.machine().memory().read16(IF).unwrap(), 0);
            assert_eq!(demo.machine().memory().read16(bios::IRQ_FLAGS).unwrap(), 0);
            assert_eq!(demo.machine().memory().read16(IME).unwrap(), 1);
            assert_eq!(demo.machine().cpu().mode(), Mode::System);
            assert_eq!(demo.machine().cpu().cpsr() & 0x80, 0);
            assert!(demo.machine().cpu().pc() >= ROM_START);
        }
    }

    #[test]
    fn arm_program_completes_one_update_per_vblank_while_status_is_set() {
        let mut demo = GraphicsDemo::new().unwrap();
        let mut updates = 0;
        let mut previous_vblank = None;
        for _ in 0..300_000 {
            demo.machine.step().unwrap();
            let counter = demo.machine.memory().read32(DEMO_STATE).unwrap();
            if counter != updates {
                assert_eq!(counter, updates + 1);
                assert_eq!(demo.machine.memory().read16(DISPSTAT).unwrap() & 1, 1);
                let vblank = demo.machine.memory().display_position().vblanks;
                if let Some(previous) = previous_vblank {
                    assert_eq!(vblank, previous + 1);
                }
                previous_vblank = Some(vblank);
                updates = counter;
                if updates == 3 {
                    break;
                }
            }
        }
        assert_eq!(updates, 3);
    }

    #[test]
    fn instruction_error_keeps_output_unchanged() {
        let mut demo = GraphicsDemo {
            machine: Machine::new(
                Cpu::new(ROM_START),
                Memory::new(0xf000_0000_u32.to_le_bytes().to_vec()).unwrap(),
            ),
            primed: false,
        };
        let mut frame = Framebuffer::default();
        frame.clear(0x1f);
        assert!(matches!(
            demo.frame(Buttons::default(), &mut frame),
            Err(GraphicsError::Cpu(_))
        ));
        assert_eq!(demo.machine().cycles(), 0);
        assert!(frame.pixels().iter().all(|&pixel| pixel == 0xff0000));
    }

    #[test]
    fn stalled_program_returns_timeout_instead_of_hanging_the_window() {
        let mut demo = GraphicsDemo {
            machine: Machine::new(
                Cpu::new(ROM_START),
                Memory::new(0xeaff_fffe_u32.to_le_bytes().to_vec()).unwrap(),
            ),
            primed: false,
        };
        let mut frame = Framebuffer::default();
        assert!(matches!(
            demo.frame(Buttons::default(), &mut frame),
            Err(GraphicsError::FrameTimeout)
        ));
        assert!(demo.machine().cycles() > 0);
        assert!(frame.pixels().iter().all(|&pixel| pixel == 0));
    }
}
