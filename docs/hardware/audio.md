# Direct Sound subset

The core models timer-driven Direct Sound A/B playback and a bounded sound-DMA configuration.
It does not produce desktop audio. Programmable sound generator (PSG) synthesis remains unsupported.
`Memory::audio_level()` exposes the current digital stereo level for deterministic inspection, not a continuous sample stream.

## Registers and inactive PSG state

| Address | Behavior |
| --- | --- |
| `0x04000060..0x04000081` | Idle PSG configuration and documented read masks; writes are ignored while master sound is disabled |
| `0x04000082` (`SOUNDCNT_H`) | Mixing, routing, timer selection, and FIFO reset strobes; readback mask `0x770f` |
| `0x04000084` (`SOUNDCNT_X`) | Master enable in bit 7; read-only PSG activity flags remain zero |
| `0x04000088` (`SOUNDBIAS`) | Bias/resolution configuration with mask `0xc3fe` |
| `0x04000090..0x0400009f` | CPU wave RAM window, opposite the bank selected by `SOUND3CNT_L` bit 6 |
| `0x040000a0..0x040000a7` | FIFO A/B writes with byte, halfword, and word access boundaries preserved |

Unused mapped register bytes read zero and ignore writes.
FIFO reads and the `0x0400008c..0x0400008f` gap remain explicit unmapped-access errors; no FIFO open-bus value is invented.
Raw memory starts with zero sound state. RegisterRamReset bit 6 sets the functional bias default to `0x0200`.

Master disable clears PSG configuration and the FIFO queues, but retains wave RAM and Direct Sound routing/bias.
The CPU then accesses wave bank 1. Idle bank selection can expose bank 0 after master enable.
The original BIOS reset clears bank 1 and both FIFO queues. It retains bank 0 and any separate in-flight playback word.
This is a functional reset subset, not a claim of complete firmware ordering or sound-reset equivalence.

An enabled PSG channel trigger returns `MemoryError::UnsupportedIo` with the exact address and value.
This applies even when the configured channel would be inaudible. Disabled PSG trigger writes are ignored.
Length, envelope, sweep, frequency, and wave-bank playback clocks are not implemented.
Do not interpret successful idle configuration writes as PSG synthesis.

## FIFO and timer behavior

Each channel has a seven-word queue, a separate four-byte playback word, and a held signed sample.
Timer 0 or timer 1 overflow clocks the selected channel while master sound is enabled.
Timer 1 cascade overflows work. Timer IRQ enable and stereo routing do not gate sample consumption.
Timers 2 and 3 cannot clock Direct Sound.

On each selected overflow:

1. Request sound DMA if the queue contains at most three words, before loading a playback word.
2. Load a queued word if the playback word is empty.
3. Latch the next signed eight-bit sample in little-endian order.

The held sample remains unchanged between overflows. An empty playback word supplies zero on the next overflow.
This underflow policy follows both consulted emulator implementations; the cited hardware report leaves it uncertain.
A bulk advance consumes all available samples and then reaches zero without looping over every empty overflow.

A partial FIFO write updates the addressed lanes of the next queue slot and enqueues that whole word.
Other lanes retain that slot's previous bytes. A write to a full queue clears the queue.
Reset strobes clear queues without changing the held sample or remaining playback bytes.
Queue clearing also zeros stored queue slots in this nominal model.
Exact stale-lane behavior across overflow/reset remains unverified on hardware.

HALT continues timer/audio progress. STOP freezes it.
Master disable stops sample consumption; route disable only mutes the mixer path.

## Sound DMA

Special timing supports DMA1 to FIFO A and DMA2 to FIFO B.
Each request transfers four 32-bit words with a fixed destination, regardless of programmed count, width, or destination mode.
Source address control, channel priority, repeat enable, completion IRQ, and retained DMA data still apply.
Without repeat, the channel disables after the fourth word. With repeat, it waits for another timer request.

Other channel/FIFO pairings and non-FIFO destinations remain explicit `DmaError::UnsupportedControl` cases.
The references disagree about request routing outside the supported pairing; hardware reports permit non-FIFO destinations.
The core does not silently guess those configurations. Video capture and Game Pak DRQ remain unsupported.

Requests become visible at successful machine-step boundaries. Active DMA blocks ignore further requests.
Device-only bulk advances coalesce unserviced requests; they do not execute or replay DMA blocks.
Sub-instruction DMA arbitration and exact refill latency are not modeled.

## Mixing and timing ownership

Each routed sample contributes `sample * 2` at 50% volume or `sample * 4` at 100% volume.
The mixer adds the ten-bit bias, clips each side to `0..1023`, and subtracts 512.
`StereoLevel` therefore contains signed levels in `-512..511`. Master disable returns zero.
Bias resolution bits retain their values but do not yet drive pulse-width modulation (PWM) sampling or quantization.
There is no resampling, analog filtering, sample queue, or host output backend.
Polling once per video frame will not reconstruct audio.

Timer and sound state share the staged CPU/DMA clock.
Reads and writes observe their nominal bus-completion phase, after earlier timer overflows.
Successful steps commit once. Failed steps discard FIFO consumption, held samples, register writes, and refill requests.
Block-store validation simulates sound enable/disable in a temporary state before committing any RAM or I/O writes.
Other devices retain their existing scheduling limits.

## Validation and remaining work

Original tests cover signed samples, byte order, queue capacity, partial writes, resets, underflow, stereo routing, and clipping.
They also cover cascaded timers, DMA block/repeat behavior, source progress, completion IRQs, HALT/STOP, and clock batching.
CPU/DMA failure tests verify transactional rollback and sound writes at bus completion.
BIOS tests cover disabled FIFO reset and the supported wave-bank policy.

These tests validate the documented nominal model, not hardware audio fidelity.
Next work: PSG clocks and synthesis, a timestamped or fixed-rate output stream, PWM/mixer sampling, and a Darwin host backend.
The local Emerald startup now passes master enable and stops at a PSG channel 1 trigger.
See [the local runtime result](../research/emerald-reset.md).

## References

- [GBATEK Direct Sound](https://problemkaputt.de/gbatek-gba-sound-channel-a-and-b-dma-sound.htm): registers, timer selection, signed samples, and DMA blocks.
- [GBATEK sound control](https://problemkaputt.de/gbatek-gba-sound-control-registers.htm): master enable, read masks, reset strobes, routing, and bias.
- [GBATEK mirror](https://mgba-emu.github.io/gbatek/): wave-bank access and register layout.
- [gbadev Direct Sound overview](https://gbadev.net/gbadoc/audio/directsound.html): introductory FIFO/timer/DMA behavior; not authority for queue edge cases.
- [Gericom's hardware findings, mGBA issue 1847](https://github.com/mgba-emu/mgba/issues/1847): seven-word queue, separate playback buffer, request order, partial writes, and overflow observations.
- [mGBA audio at 3a5e34be](https://github.com/mgba-emu/mgba/blob/3a5e34be33dc7f8f707e5bc9db69e8a430046f21/src/gba/audio.c): playback, reset, mixing, and destination-based request routing comparison.
- [NanoBoyAdvance APU at 55b5cf0a](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/hw/apu/apu.cc): master gating, playback word, underflow, and mixing.
- [NanoBoyAdvance FIFO](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/hw/apu/channel/fifo.hh), [registers](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/hw/apu/registers.cc), and [DMA](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/hw/dma/dma.cc): queue/reset policy and channel-based request routing comparison.
- [mGBA BIOS at 3a5e34be](https://github.com/mgba-emu/mgba/blob/3a5e34be33dc7f8f707e5bc9db69e8a430046f21/src/gba/bios.c): functional reset defaults.

The Rust implementation and regression programs are original. No upstream implementation or external test ROM was copied into the repository.
