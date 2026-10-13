# Sound driver (iop_main.irx)

The IOP module `iop_main.irx` (in `MODULE.IPK`, decrypted per entry with the
key 0x7E8A6B4C + 0xA84E6B2E per word) is the MGS1 `sd` driver grown for the
PS2. Decompile: `re/iop/iop_main.c` (not committed). Port:
`mgs2_formats::sd` (music only).

## Commands from the EE (`FUN_001603f0` -> `FUN_000109cc` / `FUN_0000bd70`)

- `0x0100000n`: play song n of the loaded song data.
- `0xff00010n`: interactive music level n (tracks with level mode 1 fade to
  their level-n volume); `0xff0000ff` and others: sync flags.
- `0x01ffff03..0d`, `0x01fffffd/e/f`: fades and stops.
- `0x02xxxxxx`: sound effects; `0xf0..0xf5`: streams (see VOX/DEMO in
  `formats.md`).
- GCL `command #f2eedc -c <value>` sends a raw command; `command #d9c789`
  returns the current song.

## Timing

A hardware timer fires every 2.5 ms (`FUN_000007fc`: `USec2SysClock(2500)`)
and wakes the driver thread (`FUN_0000f6e0`), which runs the sequencer every
other wake: 200 ticks a second. Each tick a track adds its tempo to a
counter and advances one step when the counter passes 0x100.

## Stage sound pack (page 0x7F000010)

Header: four `u32 start sector, u32 id` (0x800-byte sectors). The driver
loads (`FUN_00000d20`) a wave bank, a second wave bank, sound effect data and
song data, skipping parts whose id matches what is already loaded.

- Wave bank (`FUN_00001928`): big-endian `u32 tone table offset, u32 table
  size`, the table at +0x10 (copied to tone `offset / 16` on), then
  big-endian `u32 SPU address, u32 size` and, 0x10 later, ADPCM data
  DMA'd to SPU 0x20000 + address.
- Song data (`FUN_000016cc`, loaded to 0xa2630): `u32 count`, `u32`
  offsets of each song (1-based); a song is 32 `u32` track offsets (24 bits,
  0 = unused), from the section's start.
- Tone (16 bytes, `FUN_0000ad58`): `u32` SPU address (+0x20000), base note,
  fine tune, attack exponential flag, attack (0x7f - rate), decay (0xf -
  rate), sustain mode (0 linear down, 1 exponential down, 2 linear up, else
  exponential up), sustain (0x7f - rate), sustain level, release
  exponential flag, release (0x1f - rate), pan (x2 = 0..40), volume
  decrement. Tones 0x200 on (drum notes 0x48+: tone note + 0x1b8) come from
  a resident table at 0x99c80.

## Tracks (`FUN_000033a0`, `FUN_000037c8`)

4-byte events, byte 3 the command, bytes 2, 1, 0 its parameters. Under 0x80:
a note (length in steps, gate %, velocity). Controls (table at 0x1a720):

| cmd | meaning |
|---|---|
| d0 / d1 | tempo / tempo slide |
| d2-d4, fd | tone (fd: + 0x100) |
| d5 / d6 | volume / volume slide |
| d7, d8, d9, f1 | ADSR rates and modes |
| da / db | wait for another track's signal (ef) / jump back |
| dc | move the sample start |
| dd / de | pan / pan slide (centre 0x14) |
| df / e0 | transpose / detune |
| e1 / e2 | vibrato / vibrato fade-in |
| e3 | random pitch |
| e4 | pitch sweep to a note (read ahead after a note) |
| e5 / e6 | sweep from an offset / portamento |
| e7-e8, e9-ea | counted loops (with volume and pitch change per pass) |
| eb / ec | song loop point / jump to it (end if unset) |
| ed / ee | first/second ending repeat |
| f2 / f3 | rest / tie |
| f6 / f7 | reverb send on / off |
| f8-fc, cd-cf | interactive levels: volume and fade time per level 1-8; f8 also the mode (1 = game level) |
| ff | end of track |

Pitch: semitone << 8 (+ fine tune), then the tone's base note, through the
interpolated table at 0x1ab7c (0x1000 = 48 kHz). Volume: track volume x
velocity / 256, minus the tone's decrement, through the 41-step pan curve at
0x1a9d4, times the track's level volume.

## Music in the stages (`bgmanage.c`, `cmd_70a68a`)

Its update (`FUN_00230330`) follows the alert phase (`+0x112`): sneaking ->
song 5 + level 1; alert -> SE 0x2a, song 2 + level 2; evasion -> song 3 +
level 3; caution -> song 4; with flag `DAT_0017f48c & 1` song 1 + level 4.
`bgmfader.c` (`cmd_62b1fd`, update `FUN_00246af8`) places a song track in up to
8 boxes (`-t track -a cx cy cz hx hy hz "~stage" ...`, `-s` bit 1: volume only):
while sneaking it takes the box point nearest the camera, turns its
distance and direction into volume and pan (`FUN_00114bf8`), eases both by
16 a frame and sends `0xfb000000 | track << 16 | pan << 8 | volume`. The
driver stores pan in 0x9f8ec and the volume (x 0x410) as the track's level;
tracks in pan mode 2 (`dd 2`) pan with it through the 65-step curve. In
w00a, song 5 (sneaking) has tracks 4-13 in pan mode 2 and faders on 6-8;
songs 1-3 are one piece whose `f8` commands raise different track sets.

## Sound effects

- Command `pan << 18 | volume << 12 | id` (high byte 0), queued to
  `FUN_00010004`: ids under 0x100 use the resident table (`r_tnk0`'s pack,
  whose effect section starts with an `fe fe` map), ids from 0x100 the stage
  map (section 2's first 0x800 bytes: id - 0x100 -> entry). Entry (16
  bytes): priority, track count (up to 3), kind (nonzero: volume and pan
  from the command through the 65-step curve at 0x1aa78), group, three
  sequence offsets (from 0x1000 after the table). They play on 12 effect
  tracks (0x20-0x2b) with the song engine.
- Wave banks with a table offset of 0x1000 and up are kept in IOP memory
  (0x1ba70) and streamed into SPU 0x9020 + channel * 0x1000 when an effect
  track keys one on (`FUN_000121e0`); the resident bank (offset 0) fills tones
  0..0x93, and its entries past the first address drop become the drum
  tones 0x200+.
- Game calls: `FUN_00114b30(pan, volume, id)` direct, `FUN_00114ce8(id,
  position, mode)` positional (pan and volume from the camera,
  `FUN_00114a10`). Tanker ids seen: usp.c fires 0x52 (weapon 0, the M9),
  1 (USP/SOCOM), 0x30, 0xa7-0xaa by weapon, 4 when empty; wt_door.c 0x6f;
  raiden.c 7 / 8 (footsteps; 0x589 / 0x58a under a flag), 0x19 / 0x1a / 0x25
  when hurt; item_box.c 0x16 taken, 0xb can't carry, 0xd / 0x48 / 0xa1 box
  sounds; rain.c starts the rain loop 0x1d4 once (raw command); bgmanage 0x2a
  on alert; menus 0x14 / 0x15 / 0x17 / 0x22 / 0x23. Guards' gunfire was not
  found among these calls (the port uses the USP's 1).- Guards' voices are stage effects at `base + voice` (voice = a per-guard
  value mod 4, `*(short *)(guard+0xf48 + 0x1e)`), from the soldier's state
  functions (reached through state tables): 0x100 (hit, with noise 8),
  0x104 / 0x10c / 0x120 (several reactions; 0x120 when the last punch
  knocks him out, `FUN_0024d020`), 0x108 (falls, motions 0x89 / 0x99),
  0x110 (hit), 0x114 (punched, motion 0x79, `FUN_0024b8e8`), 0x118 (motion
  0x74), 0x11c (falling asleep, `FUN_00260320`, then the thud 0x36e).
- Guards' spoken reactions go through `FUN_0021c0a0(base, guard)` (w04c resident; one line at a time, played by `FUN_0021cd20` as `base + voice`), each next to a head-mark command `FUN_0023d800(guard, 0xc, 0x8000 | kind)`: 0x14c with kind 0 (the counted "!", discovery), 0x144 with kind 5 ("?", not in Evasion), 0x140 with kind 4, 0x158 with kind 6, and 0x150 / 0x154 / 0x160 elsewhere. `0x8f + voice` plays on a frame counter (mode 5). In w00a (`FUN_0021c620` there):
  - investigation (normal state 6, `FUN_0023c470`): 0x140 on the first
    frame of sub-state 0 (`FUN_0023c028`, entered by `FUN_0023c568`), 0x144
    at frame 120 of sub-state 3 (`FUN_0023c370`, after the walk; skipped in
    Evasion);
  - state 5 (`FUN_0023b580`, stimulus 0x200 = message kind 3 from
    `FUN_0021e7a8`; in Evasion the same stimulus starts discovery): 0x150
    after 31 frames when `FUN_002527d0` is 0, 0x154 on entering sub-state
    0xd (`FUN_0023ac28`);
  - 0x158 with kind 6 when the noise source's flags have 0x20
    (`FUN_0023df78`);
  - discovery (normal state 8, table 0x3f0c20): sub-state 2
    (`FUN_00237128`, and 22 `FUN_00237320`) says 0x148 on its first frame
    with the mark command kind 0, plays effect 0x53 at the guard
    (`FUN_00114ce8(0x53, .., 1)`, unless DAT_0017f48c & 1) and counts the
    discovery (`FUN_00220d60`). 0x14c (`FUN_002321b8`) belongs to another
    state that checks a second controller (+0x2f4).
  The port plays 0x148 and 0x53 on discovery, 0x140 when an investigation starts and
  0x144 120 frames after the guard arrives.
