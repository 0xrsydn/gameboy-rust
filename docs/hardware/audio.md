# Sound initialization subset

Sound synthesis and host audio output are not implemented.
The core now models disabled sound-register state so firmware can perform a functional sound reset.
This is not a silent-audio compatibility mode. Enabling sound returns an explicit diagnostic.

## Supported state

| Address | Behavior |
| --- | --- |
| `0x04000060..0x04000081` | Programmable sound generator (PSG) registers remain zero; writes have no effect while master sound is disabled |
| `0x04000082` (`SOUNDCNT_H`) | Retains mixing/routing/timer configuration with readback mask `0x770f` |
| `0x04000084` (`SOUNDCNT_X`) | Reads zero; status flags are read-only; a write setting bit 7 fails |
| `0x04000088` (`SOUNDBIAS`) | Retains bias/resolution configuration with mask `0xc3fe`; no analog output is generated |
| `0x04000090..0x0400009f` | Read/write wave RAM in the CPU-accessible inactive bank |

Unused mapped bytes through `0x0400008b` read zero and ignore writes.
The FIFO reset strobes in `SOUNDCNT_H` do not latch. No FIFO data can be queued in this subset.
FIFO addresses and the remaining sound-region holes stay unmapped.
All sound configuration starts at zero in raw memory. RegisterRamReset bit 6 sets the functional bias default to `0x0200`.

With master sound disabled, `SOUND3CNT_L` stays zero. The CPU accesses wave bank 1.
Bank 0 cannot be selected or played and remains zero. Disabling master sound does not erase wave RAM.
The BIOS reset explicitly clears the accessible wave RAM through ordinary stores.
Future bank switching and channel playback need a complete sound engine, not a larger register array.

## Diagnostics and atomicity

A write setting `SOUNDCNT_X` bit 7 returns `MemoryError::UnsupportedIo`, including the address, byte value, and operation.
Status bits and unused bits alone do not enable sound.
Validation occurs before any bytes in the access are written.
Block stores validate every register value before committing earlier writes. Failed CPU and DMA steps remain retryable.

Original tests cover disabled-register writes, masks, byte lanes, wave retention, and BIOS reset selection.
They also check rejected sound activation through byte/word accesses, ARM block stores, and DMA without partial state changes.

## Remaining work

Implement channel clocks, length/envelope/sweep state, wave banking, FIFO consumption, timer-triggered sound DMA, and mixing.
Add a host-independent sample interface before connecting a desktop audio backend.
Do not acknowledge active sound requests without implementing their device behavior.

## References

- [GBATEK sound control registers](https://problemkaputt.de/gbatek-gba-sound-control-registers.htm): disabled PSG behavior, master enable, masks, FIFO reset strobes, and bias.
- [GBATEK](https://mgba-emu.github.io/gbatek/): channel 3 wave-bank access and register layout.
- [mGBA BIOS service implementation at 3a5e34be](https://github.com/mgba-emu/mgba/blob/3a5e34be33dc7f8f707e5bc9db69e8a430046f21/src/gba/bios.c): functional reset defaults. No upstream implementation was copied.

Reset values are source-backed. Exact Nintendo firmware ordering, timing, and analog behavior are not established.
