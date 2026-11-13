# IRQ entry delay experiment

Hardware does not take an interrupt the instant `IE & IF` becomes nonzero. mGBA models this as a fixed
`GBA_IRQ_DELAY` of seven cycles before the request reaches the CPU and before it wakes HALT. Unmasking IME or
CPSR.I while a request is pending resends it with the same delay. This emulator takes an IRQ at the first
instruction boundary after the request, and HALT wakes at once.

On 2026-11-13 a delay model was implemented and scored against the [mGBA test suite](../public-mgba-suite.md).
That first attempt was not adopted, and the sections below record its design and evidence. A second model was
adopted the same day; see "Adopted model" at the end and [timers and IRQ](../hardware/timers-irq.md). The paragraph
above describes the emulator before that.

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

## Second experiment (2026-11-13, after the System-mode service frame)

A temporary switch in `Machine::step` repeated the sweep with a simpler rule: the IRQ line (IME and `IE & IF`) must
have been seen asserted at a step boundary for N cycles before the CPU takes it, and a HALT wake idles N more cycles.
It was not committed. Nothing hung for N up to 4; N = 5 stopped a sub-suite from settling.

| Delay (cycles) | 0 | 2 | 3 | 4 |
| --- | ---: | ---: | ---: | ---: |
| Timers (of 936) | 587 | 656 | 662 | 713 |
| Timer IRQ (of 90) | 44 | 59 | 71 | 75 |

Unlike the first sweep, the counts rise with the delay. The IntrWait hang is gone because services now run with the
caller's IRQ mask.

The Timer IRQ sub-suite runs from IWRAM with one-cycle `nop`s, so it isolates three facts. With all three, it
passes 90 of 90:

1. **Delay 3.** An overflow in the instruction that ends at boundary B is first taken at boundary B + 3.
   With delay 3 alone, every version takes the IRQ at the right instruction.
2. **One more cycle between acceptance and the handler's stop write.** With delay 3 the stop value is `0x51` where
   hardware reads `0x52`. Charging IRQ entry one extra internal cycle fixes it. Letting a stopped timer count one
   more cycle fixes Timer IRQ equally but lowers Timers (596 against 668), so the entry cycle is the better reading.
3. **A reload write does not reach an overflow in the next cycle.** In the `FFFF` row the counter overflows one
   cycle after the test writes reload 0. Hardware then reads 0, 1, 2, 3 where immediate reload gives 1, 2, 3, 4,
   while its IRQ timing follows the first overflow. Reloading that one overflow from the previous reload value
   (so the counter overflows again a cycle later) reproduces both.

What is still missing, and why the switch was not turned into a model:

- The Timers sub-suite's loops use three-cycle loads and branches. The switch dates a request at the end of the
  instruction in which it occurred, up to two cycles late; the failures there are two or three cycles late. The
  model needs the exact overflow cycle inside the step (`Timer::next_overflow` already computes it).
- SIO timing reads three cycles low with facts 1 and 2 (`0x276` against `0x279`), although this firmware's Halt
  exit is three cycles longer than the documented firmware sequence. A HALT wake therefore seems to cost about six
  more cycles than a running CPU's IRQ. The Misc. edge "Hblank" result is also three low.
- Delay 3 moves two Misc. edge "Flip" results that pass today.

Suggested implementation: keep a device clock and a `ready_at` time for the line in `Io`; date staged timer and serial
requests at their exact phase; bound HALT idle by `ready_at`; charge the extra entry cycle; add the reload latch to
`Timer`. Then fit the HALT wake cost against SIO timing and Hblank.

## Adopted model (2026-11-13)

The suggested implementation was built the same day and all of Timers (936), Timer IRQ (90), and SIO timing (4)
pass. Two readings above were corrected on the way:

- Fact 2 is the **timer stop latency**, not an IRQ entry cycle. With exact request dating, an extra entry cycle left
  every multi-IRQ count-up case one off, because it lengthens every handler round trip; stopping the timer one cycle
  late changes only the stop value. The stopped timer must keep its old prescaler for that cycle.
- The earlier Timers comparison between the two readings was confounded by end-of-instruction dating.

Two constants were then fitted, each with a single sharp optimum:

| HALT wake cycles | 0 | 1 | 2 | 3 | 4 | 5 | 6 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| SIO timing (of 4) | 0 | 0 | 0 | 4 | 0 | 0 | 0 |

| IntrWait exit, cycles against the old path | −7 | −6 | −5 | −4 | −3 | −2 | −1 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Timers (of 936), HALT wake 3 | 903 | 920 | 936 | 918 | 911 | 897 | 880 |

Sampling the line earlier inside multi-cycle instructions was also tried and scored lower in every variant.
The same six Misc. edge results fail before and after the model ("Break", "Read", "Hblank", and "Flip" 1 to 3); "Break" and "Read" were fixed later, and the other four are [build-dependent](../public-mgba-suite.md#remaining-failures). The rules are documented in
[timers and interrupts](../hardware/timers-irq.md#request-delay-stop-latency-and-reload-latch).
