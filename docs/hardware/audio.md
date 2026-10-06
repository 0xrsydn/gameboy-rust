# Audio device subset

The core models Direct Sound A/B, bounded sound DMA, and programmable sound generator (PSG) pulse channel 1.
It does not produce desktop audio. PSG channels 2–4 remain unsupported.
`Memory::audio_level()` exposes the current digital stereo level for deterministic inspection, not a continuous sample stream.

## Registers and PSG state

| Address | Behavior |
| --- | --- |
| `0x04000060..0x04000065` | Pulse channel 1 sweep, duty, length, envelope, frequency, and trigger |
| `0x04000066..0x04000081` | Idle channel 2–4 configuration and PSG mixer controls |
| `0x04000082` (`SOUNDCNT_H`) | Mixing, routing, timer selection, and FIFO reset strobes; readback mask `0x770f` |
| `0x04000084` (`SOUNDCNT_X`) | Master enable in bit 7; read-only channel 1 activity in bit 0; channels 2–4 remain inactive |
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

An enabled channel 2–4 trigger returns `MemoryError::UnsupportedIo` with the exact address and value.
This applies even when the configured channel would be inaudible. All PSG writes are ignored while master sound is disabled.
Reserved PSG volume selection 3 is rejected while master sound is enabled, including enable after disabled configuration.
Idle channel 2–4 writes do not imply playback support.

## Pulse channel 1

Channel 1 has four duty patterns: 12.5%, 25%, 50%, and 75%.
Its eight-position waveform advances every `16 * (2048 - frequency)` system clocks.
No general-purpose timer must be enabled for PSG playback.
Frequency writes change the next oscillator reload, not the remaining current interval.
A trigger reloads the oscillator timer, envelope volume, envelope timer, and sweep shadow/timer state.
Retrigger preserves the duty position. Master disable resets it.

The modulation sequencer receives a 512 Hz clock, once per 32768 system clocks:

| Sequencer step | Event |
| --- | --- |
| 0, 2, 4, 6 | Length counter clock: 256 Hz |
| 2, 6 | Sweep clock: 128 Hz |
| 7 | Envelope clock: 64 Hz |

The divider starts at synthetic phase zero and continues while master sound is disabled.
Master enable sets the next sequencer step to zero without resetting that divider.
Channel trigger does not reset the shared sequencer. HALT continues its clocks; STOP freezes them.
Waveform advancement uses arithmetic batches between sequencer events, not a loop for every system clock.

Length writes load `64 - length_field`. Enabled length expiry clears the channel activity flag.
Enabling length before a sequencer step that does not clock length applies an extra decrement.
A trigger reloads an empty counter to 64, or 63 when that extra-clock condition applies.
Nonempty length counters retain their values on trigger.

The envelope changes volume by one after each selected number of 64 Hz clocks.
Period zero holds volume. Saturation at zero or fifteen stops envelope changes without clearing channel activity.
Live envelope writes retain current volume and the remaining active countdown.
Reactivating a stopped envelope loads its new countdown. Trigger loads its configured initial volume.
Clearing envelope bits 3–7 disables the channel; setting them again requires a trigger to restart it.
This is the logical gate often called DAC enable in Game Boy references, not a separate physical GBA DAC.

Sweep uses a shadow frequency independent of later frequency-register writes.
A nonzero shift checks overflow immediately at trigger.
Timed sweep updates check overflow both before and after committing the new frequency.
A result above 2047 disables the channel. Period zero suppresses periodic frequency changes, not the trigger-time check.
Shift zero performs timed overflow checks without committing a changed frequency.
Clearing the subtraction direction after a subtraction calculation disables the channel until a new trigger.

These rules define a nominal digital model, not exact hardware edge timing.
First-trigger output suppression, trigger sub-divider alignment, envelope trigger-edge delays, and revision-specific envelope write effects are not modeled.
The model samples the selected duty position immediately after trigger and uses a full-period timer reload.
The consulted implementations differ on some initial phases and envelope/sweep edge cases; no GBA hardware audio recording was used as an oracle.

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

Each routed Direct Sound sample contributes `sample * 2` at 50% volume or `sample * 4` at 100% volume.
Pulse channel 1 contributes a centered signed amplitude, `+volume` or `-volume`, while active.
`SOUNDCNT_L` routes it independently left/right and multiplies each side by its volume field plus one.
`SOUNDCNT_H` then applies the PSG ratio: 25%, 50%, or 100%, using an arithmetic right shift.
At full volume, channel 1 therefore spans `-120..120` before mixing with Direct Sound.
This centered mixer follows GBATEK's signed range and the NanoBoyAdvance comparison; exact analog offset is not modeled.
The mixer adds all supported channels and the ten-bit bias, clips each side to `0..1023`, and subtracts 512.
`StereoLevel` therefore contains signed levels in `-512..511`. Master disable returns zero.
Bias resolution bits retain their values but do not yet drive pulse-width modulation (PWM) sampling or quantization.
There is no resampling, analog filtering, sample queue, or host output backend.
Polling once per video frame will not reconstruct audio.

Timer and sound state share the staged CPU/DMA clock.
Reads and writes observe their nominal bus-completion phase, after earlier timer overflows.
Successful steps commit once. Failed steps discard FIFO consumption, held samples, PSG clocks/state, register writes, and refill requests.
Block-store validation simulates sound enable/disable in a temporary state before committing any RAM or I/O writes.
Other devices retain their existing scheduling limits.

## Validation and remaining work

Original tests cover signed samples, byte order, queue capacity, partial writes, resets, underflow, stereo routing, and clipping.
They also cover cascaded timers, DMA block/repeat behavior, source progress, completion IRQs, HALT/STOP, and clock batching.
CPU/DMA failure tests verify transactional rollback and sound writes at bus completion.
BIOS tests cover disabled FIFO reset and the supported wave-bank policy.
Pulse tests cover every frequency/duty pair, waveform batching, status, length, envelope, sweep, mixing, and reserved-volume diagnostics.
They also check bus-completion status reads, retrigger timing, clock ownership, and rollback with a running pulse channel.

These tests validate the documented nominal model, not hardware audio fidelity.
Next work: PSG channels 2–4, a timestamped or fixed-rate output stream, PWM/mixer sampling, and a Darwin host backend.
The local Emerald startup now passes the channel 1 trigger and stops at a channel 2 trigger.
See [the local runtime result](../research/emerald-reset.md).

## References

- [GBATEK channel 1](https://mgba-emu.github.io/gbatek/): register fields, duty ratios, and modulation rates.
- [Pan Docs audio details](https://gbdev.io/pandocs/Audio_details.html): duty counters, shadow sweep, length edge rules, and GBA digital-mixer differences. GB-only edge details are not treated as verified GBA measurements.
- [GbdevWiki sound hardware](https://gbdev.gg8.se/wiki/articles/Gameboy_sound_hardware): background sequencer and sweep descriptions.
- [mGBA shared PSG at 3a5e34be](https://github.com/mgba-emu/mgba/blob/3a5e34be33dc7f8f707e5bc9db69e8a430046f21/src/gb/audio.c): GBA-style sequencer, length, envelope, sweep, and waveform comparison.
- [NanoBoyAdvance pulse at 55b5cf0a](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/hw/apu/channel/quad_channel.cc), [base channel](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/hw/apu/channel/base_channel.hh), [length](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/hw/apu/channel/length_counter.hh), [envelope](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/hw/apu/channel/envelope.hh), and [sweep](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/hw/apu/channel/sweep.hh): ordinary pulse behavior and documented uncertainty. The adjacent `quad_channel.hh` defines frequency scaling.
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
