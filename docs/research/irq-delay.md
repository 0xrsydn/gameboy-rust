# IRQ entry delay experiment

Hardware does not take an interrupt the instant `IE & IF` becomes nonzero. mGBA models this as a fixed
`GBA_IRQ_DELAY` of seven cycles before the request reaches the CPU and before it wakes HALT. Unmasking IME or
CPSR.I while a request is pending resends it with the same delay. This emulator takes an IRQ at the first
instruction boundary after the request, and HALT wakes at once.

On 2026-11-13 a delay model was implemented and scored against the [mGBA test suite](../public-mgba-suite.md).
It was not adopted. This note records the design and the evidence so a later attempt can start from them.

## Evidence for some delay

After the [minimal BIOS IRQ dispatch](../hardware/bios.md), the suite's Timer IRQ test stops its timer at
`0x4e` where hardware reads `0x51`, and versions with one to three `nop`s read the counter after the IRQ has
already been taken, while hardware still reads `0001`–`0003`. Interrupts are taken a few cycles too early there.

## Design tried

- An absolute device clock in `Io`, advanced with every device batch, and a `ready_at` time for the current request.
- A request becoming active arms `ready_at = request time + delay`. Staged timer and serial requests are dated at
  the exact overflow or access phase inside the instruction; display, DMA, and keypad requests at the end of their batch.
- The IRQ line, used both for CPU delivery and HALT wake, is asserted once the clock reaches `ready_at`.
  HALT idle stops at that time. Writing IME from 0 to 1, or an instruction that clears CPSR.I, resends a request that
  already reached the CPU, dated from the start of that instruction.
- Host setup writes that create a request make it ready at once, like the [timer start delay](../hardware/timers-irq.md).
- The replacement BIOS's IntrWait checks flags and enters HALT with IME masked, so a wake-up cannot be lost. It
  re-enables IME only briefly afterwards; with any delay the IRQ arrived after IME was masked again, and every
  VBlank wait hung. The experiment added a loop that keeps IME set until `IE & IF` clears.

## Results

Sub-suite pass counts by delay, with the IntrWait loop present in every run:

| Delay (cycles) | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Timers (of 936) | 556 | 577 | 589 | 530 | 563 | 470 | 491 | 431 |
| Timer IRQ (of 90) | 44 | 52 | 59 | 71 | 75 | 65 | 65 | 65 |
| Sum | 600 | 629 | 648 | 601 | 638 | 535 | 556 | 496 |

Without the model, the committed core scores 596 and 44 (sum 640). Timing stays at 1680/2020 and SIO timing at
0/4 for every delay.

The best delay gains 8 results, the curve is not monotonic, and mGBA's seven cycles is clearly worse here. The
timers sub-suite counts busy-loop iterations between an IntrWait and a timer IRQ, so it also measures this
emulator's IntrWait and Halt wake paths, which differ from the real firmware's. Instruction-boundary delivery adds
further error. A single delay constant cannot absorb those differences, and adopting it would have required
rewriting 49 timing-specific test expectations.

## Next steps

1. Measure the replacement BIOS's IntrWait and Halt return paths against the suite independently of the delay,
   and match their cycle costs to the documented firmware behavior with original code.
2. Then repeat this sweep. A clear optimum across Timers, Timer IRQ, and SIO timing would justify the model.
