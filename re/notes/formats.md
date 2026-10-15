# Asset formats (Stage 3)

Byte-level layouts live in the doc comments of each `mgs2-formats` module;
this file records how they were found and what is still open.

File handlers are registered by `FUN_00140ae8(letter, handler)` (bit 0x100 on
the letter marks a handler that also runs on unload):

| letter | handler | format | module |
|---|---|---|---|
| b | `0x105698` | resident code image | — |
| c | `0x129b38` | cv2 | — |
| e | `0x129bd8` | evm | — |
| f | `0x11c9e0` | far | — |
| g | `0x153168` | gcx script | `gcx` |
| h | `0x144510` | hzx collision | `hzx` |
| k | `0x129ac0` | kms model | `kms` |
| l | `0x129bb0` | lt2 lighting | — |
| m | `0x11c8f8` | mar motion | — |
| p | `0x156428` | ? | — |
| s | `0x11c990` | sar | — |
| t | `0x129e48` | tri texture set | `tri` |
| z | `0x129c00` | zms (magic `0x1BA13`, table of kms) | — |

## tri (textures): done

- Raw GS VRAM upload: image block then CLUT block, both PSMCT32, 64 wide.
- Textures are PSMT4 / PSMT8 with CSM1 32-bit CLUTs; several textures share
  one CLUT block and select a 16-colour palette with `CSA`.
- Each entry's `CLAMP_1` region clamp gives its rectangle in the atlas.
- Verified visually; all 15,919 tanker textures decode.

## kms (models): done for static geometry and bind pose

- Triangle strips wind clockwise as seen from the front (in our right-handed,
  y-up space): culling Bevy's "front" faces gives the game's look, where
  room ceilings vanish under the overhead stage cameras.

- Triangle strips; restart flag in normal.w bit 15.
- Positions are `i16` millimetres, Y up; UVs are relative to the texture's
  rectangle (0..1, scale 4096).
- Objects form a bone hierarchy; `+0x20` is the offset from the parent.
  Vertices are in their bone's space. Position `w` is the vertex's weight on its
  own bone versus the parent. In bind pose, weight-0 seam vertices land on the
  parent's surface; this was checked numerically on the Gurlukovich soldier
  models.
- Object flag 0x4000 (seen as 0x4801, 74 objects over all Tanker models):
  an overlay of its parent (Snake's hair layer, sleeves): its offset repeats
  the parent's, and it is drawn with the parent's matrix (no further
  offset). Taking the offset again left Snake's hair floating 115 mm above
  his head. `cargo run -p mgs2-formats --example kms_info -- <file.kms>`
  lists a model's objects (several files: only unusual flags).
- Open: other object flags, mesh flags
  (0x48/0x49/0x4C/0x2F8), the second/third texture slots (lightmaps?), and
  header `+0x04/+0x10/+0x14`.

## hzx (collision): geometry done

- Areas are 3D grids of groups (cells). Element coordinates are `i16`
  relative to the group origin.
- Walls are point lists with a height; floors are 3- or 4-gons with the plane
  normal * 32000 packed into the vertices' `w`.
- Verified by overlaying on the `w00a` level mesh: the walls trace the hull
  and superstructure.
- The fifth per-group list holds trap boxes: 20-byte records `i16 min x y z,
  pad, i16 max x y z, pad, u32 name` relative to the group centre (the pads
  differ between copies of a box in different cells). Scripts name them in
  `trap <box> <actor> -m <mode> -e {...}`; e.g. w00a `r_boat` covers the
  lifeboats on the 2F deck.
- The area's +04/+0C pointers are navigation nodes (see Guards, and
  `hzx.rs`).
- Railings in scripts: stages also call proc `#702569 <trap> x $6 z0 z1 k`
  from a trap on Snake ("beyond_set"); with the action button held and
  Snake facing 0xc00 / 0x400 (±0x100) it sends him `mesg #28dce6 0` with
  mode 5 / 6, the facing, and his place (x + 500 / + 250, z clamped to
  z0 + 450 .. z1 - 450), and limits him to x ± 2000, z0..z1
  (`#5686ae -x -z`). The holds (w04a/b), which have no 0x800 walls, get
  their hang ledges only this way; the port builds ledges from these calls.
- Hang ledges: second-set walls with flag 0x800 run 200-250 mm below a deck
  edge under its railing (top = the deck floor); 24 in w00a.
- Element flags (+0x14 of each wall/floor element) are masked by the
  collision queries: `FUN_0014f230(coll, a, b, mode, wall_mask, floor_mask)`
  (line of sight) tests only walls with `flags & mask == 0` (`FUN_0014e128`;
  floors `FUN_0014e808`), and adds 0x40000 / 0x80000 to the masks unless
  `mode` has 0x100 / 0x200. The w04c camera passes 0x10004, so the holds'
  railings and RAY's walls (flags 0x337f0, 0x377f0, 0x13bc0: bit 0x10000)
  do not block photos.
- Open: the rest of the second wall/floor set (ids +0x400), the meaning of
  most flag bits, header table B, and the precomputed shortest-path
  matrices.

## gcx (GCL scripts): done (disassembly)

- VM spec from the interpreter: statements `0x60` command / `0x70` call, the
  value types, RPN expressions with operators 1..0x17, and typed variables in
  two banks.
- Tanker scripts use only these commands: `eval chara call command if mesg
  trap print return load restart`.
- Gameplay lives in the actor types created by `chara <type> <name> ...`. The
  type hashes match the resident image's command table (`cmd_<hash>` in
  Ghidra).
- `Bs16:0x8` is the difficulty (10..50).
- Name lookup: built-ins, identifiers harvested from the stage, and JoyDict
  file names (git-ignored local copy, shown as `~name`).

## Level placement (from scripts)

Static level geometry is not found by file name. The stage script creates it
with the map-piece actor:

```
chara 0x35a2cf <instance> -k <kms> [x y z] [-k <kms> ...] -h <hzx> <area> -l <lt2>
```

One piece can list several models. The same model can appear in several
pieces with different collision areas (e.g. `w04a1` in w04a). Cutscene stages
(`d0*t`, `d00*p*`) have none.

Open: the viewer orientation may be mirrored (handedness unverified). Check
it against an asymmetric reference such as Snake's holster side once his
model is located (it is not in the w00a files).

## Script actors used by the game

| Actor | Meaning | Options |
|---|---|---|
| `0x35a2cf` | map piece | `-k model [x y z]... -h hzx area -l lt2` |
| `0x7a8ec1` | static scenery (floors, furniture, passages) | `-m model -r rx ry rz -p x y z -s sx sy sz (%) -f flags`; `-f` bit 0x100 = never drawn (helper poles) |
| `0xd1279a` | plant (breakable) | `-m model -b broken -r -p -s` |
| `0xcba568` | breakable glass | `-m model broken pieces -p` |
| `0x9aff54` | item box | `<name> -p x y z -k model -s shadow -l label -f kind (0 item, 1 ammo, 2 grenade) -i id -n count -x {on pickup}` |
| `0xebec24` | deck door (hatch) | `-k door handle -p -r -O proc (opened; $1 = 0) -h x y z x y z (handle line)` |
| `0x55b942` | sliding door | `-m model -d rx ry rz -p -s width -t -b` |
| `0x128946` | player | `-m sna_def -a snake -p x y z -d 0 dir`, name `0x28dce6` |
| `0xf7f777` | soldier | see Guards |
| `0x43f718` | camera (set from traps) | `-c type -p pos -t target ...` (not used yet) |
| `delay` | timer | `-t frames -e {block}` |
| `0xe22388` | fade | `-t frames -p proc` (proc runs when done) |

Stage exits: a door's `-O` proc (or a trap block) calls a proc that sets
Snake's next position (`Au32:0xbc/0x7f4/0xc0`, facing `0x7f8`), the story
marker `Au32:0xc4`, and then `load "<stage>"`, usually behind a `delay` or a
fade. The game runs these with the timers and fades firing immediately.
`Abit:0x7e8.5` is set once Snake's entrance walk-in is over; exit traps
check it so Snake does not leave through the door he came in by.

## trap events (`FUN_0014A448`)

Each frame the trap boxes containing an actor are compared with the
previous frame's: new boxes send `0x670d6b` (enter), kept ones `0x54a56b`
(stay), lost ones `0x61d56b` (leave). A handler's `-m` mode filters them:
an event code itself, `0x14c9` (enter and leave) or `0x1516` (all three).
The block gets `$1` box, `$2` actor, `$3` event, `$4..$6` actor position.
Handler conditions (`FUN_00149FE8`) include `-b` (button), `-d dir range`
(facing) and `-s` (stance; not modelled yet).

## CODEC.DAT (Codec conversations)

- Not encrypted: a run of GCL script records (same bytecode as stage`.gcx`),
  each starting with the stamp `0x3bb29171` and padded to 16 bytes; 451 in
  the NTSC-U disc. `mgs2-extract <iso> codec` writes them as text.
- Stage scripts name a record by id: low 24 bits x 16 = byte offset in
  CODEC.DAT (high byte: flags). `command #4a243a -o <event> <freq> <id>`
  sets the conversation for calling `freq` in that area (on enter/stay, off
  on leave); `-c <freq> <id> <mode>` is an incoming call; `-r` resets.
  14112 = 141.12 (Otacon).
- A record's `main` (`#57c8d1 "codec" ... {...}`) branches on story
  variables and calls procs whose `#01e658 <voice id> {...}` blocks hold
  the lines `#3b91eb <speaker> <?> -m str[n]`. Speakers: `0x4ab569`
  Otacon, `0x28dce6` Snake (the player actor's name). Faces (`face`,
  `#6b11d5` lip sync) and voices (VOX.DAT, the voice id) are not used yet.

## The holds' photo finale (w04c)

- `command #661bee -p <4 corners> -f <fill %> -R proc [-x pitch] [-y yaw]`
  (`cmd_661bee`): a camera target, a 0x70-byte record: corners, normal
  `(c2-c1) x (c1-c0)`, centre, fill, proc, `-x` (default 640) and `-y`
  (default 320) angle limits in 4096ths.
- `FUN_0031cc18` grades every target on each photo and calls its proc with
  the grade: 0 if the camera is behind the quad or more than `-y` (yaw) /
  `-x` (pitch) off its normal; else it projects the corners: all four on
  screen and unblocked = 4 if the quad covers `-f`% of the screen (NDC area
  * 25) else 3; on screen but some blocked = 2; all off screen with the
  quad covering the whole frame = 1; partly off screen = 1 if the centre is
  within 0.3 of the middle, else 0. Point-in-quad `FUN_0031c968`, area
  `FUN_0031c8f0`.
- `#ec40c7 -n <actor> -j <joint> -f <fill> -p proc`: a character target
  (a marine's head), graded 1/0 from the joint's box. Not modelled yet.
- The procs map grades to codes (10 front, 20 front-right, 30 front-left,
  40 "MARINES" lettering, 50-120 poorer RAY shots, 130 a marine, 170 the
  commandant) and keep the lowest in As16:0x50; `#8b12d1 -p` (after each
  photo) stores it in the album As16:0x52[Au32:0x4c] (10000 = empty).
- The terminal (trap #8bc63b with the action button) plays CODEC.DAT
  record 125 through `#db03a6 -c 0x0e046d22 -p #c78b20`: Otacon goes through
  the album (`#89a17e { ... }` repeats while its block returns non-zero),
  sets Abit:0x3c.5/.3/.4/.6 for codes 10/20/30/40 and, with all four,
  Abit:0x62.2. `#c78b20` then restarts the stage with marker #9d2cb1, whose
  path plays the speech's last line (`#7aa13a 0x2000a808 -p #603f27`) and
  loads d12t; otherwise it empties the album to try again.
- `chara #b3e388 ... -a <11 values per Marine>` (`FUN_0022a5d8`): name,
  x y z, rotation, variant (% 22), unused, idle variation (% 6), flags.
  Variants < 18: `us_def_1` with parts swapped for `w04a_us_def` parts by
  the table at 0x1f73e0 (0x54 bytes = 21 part indices, -1 keeps the part;
  +18 / +36 for the two lower LODs `us_mid_5_s`/`us_low_5_s`); 19 the
  commandant `sco_def`; 20-21 `us_def_pants`. Flags: 0x70 rifle `m4a_nm`,
  0x1 `obj_cap`, 0x4 `obj_helmet_def_s`, 0x80 `sco_hat`, 0x100
  `obj_headphone_def`, 0x2 `obj_glass`.
- `#7aa13a <id> -p proc`: a spoken line (ids 0x2000xxxx, not Codec); the
  commandant's speech is a chain of these, each proc picking the next, and
  its end is the time limit.

## VOX.DAT (spoken lines): partly understood

- Clips start on a sector with the DEMO.DAT chunk layout: `0x10` header
  chunks (the second chunk's payload has `0x44ac` = 44100?), kind 1 audio
  (16 KiB interleaved SPU ADPCM), kind 5 event packets every 5 frames
  (some clips), kind 4 subtitles (same records as DEMO.DAT), and an end
  chunk kind `0xf0` whose frame field is the clip's length. 3407 clips;
  `examples/vox_index.rs` lists them.
- `#7aa13a` ids are `0x20000000 + sector`: w04c's "caught" line
  `0x2000ddaa` is the clip at sector 0xddaa of VOX.DAT ("Huh!? / Stop!! /
  Freeze!!"). The commandant's speech ids (0x200003b4, ..., 0x2000ab08) are
  clips in MOVIE.DAT instead: the same chunk layout with four header
  chunks, an IPU video stream (kind 0xf, payload magic `ipum`), 44100 Hz
  stereo ADPCM (kind 1), lip sync (kind 5) and subtitles. No sector holds a
  clip in both files, so the port tries VOX.DAT, then MOVIE.DAT.
- The debug stage `select` lists every VOX.DAT clip by name (`chara
  #fe6730 "test" -s "vc000000" 0x20000000 ...`, 3407 names). All are `vc`
  voice clips; the long stereo ones are long conversations, not music.
- Subtitle chunks: the leading u32 counts the record bytes after it
  (records start at chunk +20). In the text, `0x80 '|'` breaks the line and
  `0x80 '#' xx xx` is a control code.

- Codec voices: a record's `#01e658 <id> { ... }` id is `0x20000000 +
  sector` of a VOX.DAT clip (checked for several record-125 lines).
- Audio header (kind 1, 0x20 bytes): big-endian u32 total ADPCM bytes
  (matches the data chunks exactly), `00 7f`, big-endian u16 rate field
  (VOX 0xac44 = 44100, DEMO 0xbb50 = 48000), a channel byte (VOX 1 or 2,
  DEMO 2). Data are 16 KiB SPU-ADPCM chunks (`29 02 ...` blocks).
- Format byte (header +10, `FUN_00118cd0` in sdstream.c): 0 SPU ADPCM, 1
  the DEMO.DAT format, 2 a third (`DAT_000e1fe8` on the IOP). The EE sends
  the header to the IOP driver (iop_main.irx in MODULE.IPK) as commands
  `0xf0/0xf2 size>>4`, `0xf5/0xf4 format<<20 | channels<<16 | rate`;
  stream 0 only plays ADPCM, stream 1 all three.
- Times: chunk frame fields and subtitle spans count 300ths of a second
  (sdstream.c computes lengths as `bytes * 300 / (rate * channels)`); an
  end frame of 60793 is the 202.6 s of the opening's audio.
- Playback: SPU ADPCM, stereo clips interleave 0x800-byte blocks per
  channel (each channel's last block carries the end flag; the two are
  0x800 apart), at the header's rate (the IOP sets pitch `rate * 4096 /
  48000`). VOX clips are 44100 Hz: the earlier fit of the "caught" shouts
  to their subtitles (about 9375 Hz with 60 ticks a second) is 46.9 kHz
  with the real 300. `mgs2_formats::vox` decodes clips; Codec calls play
  their voice blocks (`#01e658 <id>`).
- DEMO.DAT audio (format 1, rate field 0xbb50, 2 channels): the IOP's PCM
  path (`FUN_00018028`) feeds the SPU2's 48 kHz input. Bytes alternate
  left/right; each is a delta added to that channel's 16-bit sample
  (`FUN_00019ae4`): `m = b & 0x78`; if `b & 7 == 7` the delta is `m << 1`,
  else `((m | 0x80) << 7) >> (b & 7)`; bit 7 negates it. So `07` is
  silence. A demo's audio is voices, music and effects mixed.

## DEMO.DAT (cutscenes): cameras and actors decoded

- Not encrypted. Cutscene stages start demos with `chara "demo" <name> -i
  0x40000000 + n`; n is the demo's first sector (2048 bytes).
- A demo is a chain of chunks `u32 kind, u32 size, u32 frame, u32 0`:
  kind-0x10 headers, kind 2 (event packets every 5 ticks, 300 a second), kind 1 (16 KiB
  streamed audio), kind 4 (subtitles: `u32 size`, then records `u32 start,
  u32 end, u32 0, u32 length`, text with `0x80 '|'` line breaks) and 0xf0
  (end; its frame is the length).
- The demo player is the `demo` actor (`cmd_33a20f`) in each cutscene
  stage's resident image (analysed for d05t in `re/ghidra-d05t`).
  `FUN_00216308` dispatches packet sub-records `u32 type, u32 size, u32
  serial, ...` on `type & 0x7fffffff` through the table at 0x1f3198:
  - 2 (`FUN_00219020`): messages. Sub-kind 0 sends `1, value (+0x18)` to
    the model loaded by serial +0x14 (`FUN_00217ce0`): models start
    hidden (flag 0x3000), value 0 shows, anything else hides. Sub-kind 1
    removes the model; 3 creates an effect.
  - 3 (`FUN_002190e0` -> camera actor `FUN_00219ac8`): +0x14 zoom, +0x18
    roll (radians), +0x20 eye, +0x30 target (f32 mm).
  - 4 (`FUN_002163f8` -> `FUN_00217a80`): a model object keyed by the
    record's serial: +0x1c kms file id (type 0x0a), +0x18 name, +0x10 an
    optional `.evm` skinned mesh on top (`0x04xxxxxx`), +0x14 flags.
  - 6 (`FUN_002167f8`): other actors by constructor id (+0x10).
  - 7 (`FUN_00219718`): the actor stream, ids = model serials. `u32 n` at
    +0x10; entries at +0x20 `u32 id, u16 flags, u8, u8 joints, u16 data
    offset, u16`. Data: flag 1 position (16 bytes, f32 xyz), flag 2 root
    angles (u16 x y z in 4096ths + u16), then per joint a control word (low
    byte = joints it covers, bit `0x100 << k` = value k absent) and the
    values: quaternion x y z w as i16 / 16383, translation as half floats
    (`FUN_00219a88`, scaled by 128). Absent values keep their last value; a
    word's values serve all the joints it covers. Joint 0's translation is
    the hips' position; joints past the kms object count belong to the
    `.evm` mesh.
- Scenery is not in the demo: the cutscene stage's script places it with
  the usual prop actor `0x7a8ec1` (the deck pieces), a sky (`0x6273b5`) and
  sea pieces (`0xa895c4`).
- `.evm` (file type 4, `mgs2_formats::evm`, loader `FUN_00129bd8`): the
  cutscene characters' skinned meshes. Header: animated bone count, bone
  records, bbox, texture set (+0x24), mesh count / offset (+0x28/+0x2c).
  Bones (0x40): parent, offset from parent, bind position. Meshes (0x40):
  flags, 3 textures, vertex count, up to 8 bone indices (+0x18), data
  offsets (+0x20): positions `i16 x y z w` in 1/16 mm in model space (w bit
  15 = strip restart), normals `i16` / 4096, UVs `i16 u v 4096 0` / 4096,
  skin 8 bytes a vertex (4 weights u8 / 128, 4 slots u8 = 4 x index into
  the mesh's bones). The demo's joints rotate the evm bones (the first
  `animated` ones); a vertex moves from its bones' bind positions.
- Open: type-6 actors and effects, audio.
## mar (motions): joint rotations and root height done

Full layout in `mgs2-formats/src/mar.rs`. Player code is in the resident
image (w00a addresses `0x3AF000..0x3B5000`).

- Joint rotation keys hold quaternions packed as 4 x 12-bit fields and are
  played back with squad. Every key of every tanker motion decodes to a unit
  quaternion (`cargo test`). A key's duration is the length of the segment
  ending at it.
- Quaternions apply directly as local joint rotations; the conjugate gives
  visibly broken poses.
- Root track (flag 0x20): fp16 values in units of 100 mm; hip heights
  0.8 / 0.42 / 0.14 m (stand / crouch / prone).
- Not decoded: tangents of the root curve, flags 0x01 / 0x04 / 0x10 / 0x40
  tracks and the `.var` (motion lists) files. The `.sar` holds the motion
  sound events (see below).
- Snake (`snake.mar`, in `r_tnk0`) motions identified from root motion:
  0 idle, 5 crouch idle, 7 prone idle, 17 walk (1.25 m/s), 18 run
  (3.05 m/s), 53 crouch walk. `re/scripts/mar_root.py` prints all of them.
- Some `m`-type files are `.mdl` physics models (magic `Phys`), not motions.

## Coordinate fixes found while making Snake walk

- `.kms` header `+0x34` is the model origin. Level geometry is stored centred
  and must be translated; found by correlating mesh and collision heights
  (best match equals the header value exactly in w00a and w04a).
- `.hzx` group centres are stored x, z, y. Confirmed by the area's grid
  dims/origin and by script entry points landing on floors and stairs.
- Orientation is not mirrored: Snake's holster is on his right thigh.
- Player entry points: the procs set `Au32:0xbc / 0x7f4 / 0xc0` (x y z, feet
  on a collision floor) and `0x7f8` (facing, 4096 = 360°); the player actor is
  `chara 0x128946 ... -m sna_def -a snake -p <x y z> -d 0 <dir>`.
- `r_tnk0` is the Tanker resident stage (Snake, weapons, menus, Codec, maps),
  loaded alongside every Tanker stage.

## Guards (Stage 5b)

- Actor `chara 0xf7f777 <id> -r route -n node -d x y z map -s flags -h range
  -i d0 d1 d2 -l .. -f .. -j <dogtag proc + 5 names> -q/-a <procs>`.
  Handler `cmd_f7f777` (0x223590 in w00a's resident image), options parsed by
  `FUN_00222F60`: `-i` defaults 6000, `-l` 3000, `-f` 9, `-h` 12000. Model
  `gbs_def` (0x613791; 0xd83aaf for a variant flag), motions `gbs.mar`.
  Vision set up by `FUN_00259618(vision, 0, 0x200, i[0], h, 6000)`.
- Guards are created by script proc `#ddf4ef`, reached through:
  `main` -> `command ... -s {setup procs}` -> proc `#d2b8e8` -> actor
  `0xddf5ca` (`-w proc`) -> proc `#bdd3aa` -> actor "wccomm" `0x59eda8`
  (`-e proc`) -> `#ddf4ef`. It branches on difficulty (`Bs16:0x8`) and the
  story marker `Au32:0xc4`.
- `Au32:0xc4` is set by the stage that loads this one. `d01t` (after the
  opening) sets `#8744ee` and puts Snake at (6406, 0, 16224) facing 2048.
- Routes are the `.hzx` header table at +08: `{u32 count, u32 offset, 0}`,
  40-byte points `f32 x z y, look_x; f32 look_z look_y; i16 ?, ?, ?, ?,
  flags (4 = look-at point), wait (frames, -1 = forever), direction, ?,
  action, ...`.
- Header table at +10 (0x30-byte records) holds boxes, probably event areas.
- `gbs.mar`: 0 idle, 1 walk (1.33 m/s), 2 run (4.05 m/s), 113 fall prone,
  114 lying (asleep / knocked out).
- `snake.mar`: 13 collapse onto his side (death), 14 fall face down, 176
  knocked onto his back.
- The guard AI port is described in `guard-ai.md`.

## Not started

`lt2`, `cv2`, `evm`, `far`, `o2d`, `sar`, `var`, `zms`, `r*`, and the `anm`
sound-bank pages.

## GCL expressions (`FUN_00154080`, `FUN_00153f40`)

- RPN: values, then `0x31 op` tokens; `0x31 0` ends. Every operator pops
  two values, (second from top, top) (`FUN_00153f40(op, a, b)`): the unary
  ones (1 neg, 2 !, 3 ~) use only the top, and the compiler pushes a dummy 0
  before their operand (`0 x !`). 0x16 assigns, 0x17 returns its right
  operand. The result is the top value. (Treating ! as popping one left the
  dummies behind and dropped terms: the holds' speech bug below.)
  `examples/gcx_rpn.rs` dumps the raw RPN of expressions reading a variable.
- `Bu32:0xb4` holds the current stage's name hash (the holds' shared script
  branches on it). A new game runs n_title's `#b22659 difficulty`, which sets
  story variables and `Au32:0x70 = 25200`, the holds' time limit in frames.
- `chara #9b65f0 -t frames -p proc -s flags` is the holds' timer (w04
  residents `FUN_0028fe00` / `FUN_0028f890`): counts 60 a second while
  running (`mesg #9b65f0 #000001` runs, `#000000` pauses, `-s` bit 0 starts
  paused), shows mm:ss:cc, ticks with effect 0x585 each second (0xcf under
  600 frames), runs `-p` at zero; `command #450c64` reads the frames left.- The holds' speech: w04a's `#86e3c2` tests `#450c64 < 600 && (Bu32:0xb4 != "w04b" || !Abit:0xb63.5)` (the closing line only in the last 10 seconds). `#450c64` returns the resident global the timer actor keeps (`DAT_003afbbc` in w04c), so a timer created earlier in the same script run already answers it.
- `GCL_TRACE=1` makes the port log every proc call.
## FACE.DAT (Codec faces)

- Header sector: `u32 stamp, u16 1, u16 1, u16 count`, entries from +16 of
  `char name[8], u32 sector`: `node photo capture mobile` (menus) and one
  per Codec cast (`f00a` the Tanker's Snake and Otacon; `f01a`..`f06a`
  the Plant's Raiden, Rose, Campbell...). Each entry is a plaintext
  stage-style page list (`u32 n`, pairs `a, b`; a `0x7F000002` page with
  its size, then `type << 24 | hash` files at offsets, listed by type).
- Contents (names from JoyDict): `sna_radio_mh_mt.evm/.tri` (Snake's
  head), `otc_radio_mh_mt.evm`, `otc_hair_mh.evm`, `otc_glasses_mt.kms`,
  `otc_dummy.kms`, `cdc_snake/cdc_otacon.mar/.sar` (motions),
  `lip_*_nom_eye_toji.rmt`, `save_load.o2d`, `tnkicon.ric`. The heads use
  the standard skeleton: bone 12 is the head (Otacon's at y 578 mm),
  bones 21+ the face; kms parts hang from bone 12. `mgs2_formats::face`
  reads an entry; `examples/face_dump.rs` lists them and writes textures.- Voice clips' kind-5 chunks are the speakers' face animation: records
  `u32 0x1000, u32 size, u32 0, u32 count`, 12-byte entries `u32 actor,
  u16 flags, u8, u8 joints, u16 offset, u16`, then the cutscene actor-stream
  joint data (offsets from the end of the entries). Actors are speaker ids
  (`0x28dce6` Snake, `0x4ab569` Otacon) with 32 joints = the heads' face
  bones 21-52, and `id + 1` (21 joints) the body bones 0-20; keys every 5
  ticks (`mgs2_formats::demo::face_stream`).- DEMO type-6 records: `+0x10` constructor id, looked up in the cutscene
  resident's command table (entries with high byte 1, e.g. d05t: 0x1000000
  fog, 0x1000006 ambient colour (RGB ints -> engine `0x190af0`, `FUN_00125eb0`),
  0x1000007 light (RGB ints, then a float direction at param 4..6, normalised
  x 4096 into the light matrix `0x190a80` via `FUN_00125ee0`, colour to
  `0x190ac0`); the port shades characters ambient + colour x max(0, n . dir)
  with 0x80 = 1.0 (the sign of dir is assumed),
  0x1000008 / 0x1000003 screen fade (`d_fade_io_prog.c`: RGBA from -> to,
  alpha 0x80 opaque, over the difference of two 60 Hz frame stamps),
  0x10fff00.. demo control); `+0x14` 0 runs it once with the parameters
  from `+0x20`, 1 creates an actor.
## lt2 lighting

See `crates/mgs2-formats/src/lt2.rs` for the layout. The per-vertex formulas come from the VU0 micro programs that `FUN_0012b1d0` calls (MPG of 190 instructions in the VIF0 packet at `0x196020`; `vcallms` 0x00 directional + ambient, 0x10 point, 0x20 spot, 0x50 strip, 0x30 shadow sphere). Constants: `0x1b02a0` = (1, 1, 1, 255) (axis weights, clamp), `0x1b02b0` = (1.5, -0.75) (the 1.5 point/spot gain). Model normals point inwards (checked on Snake's parts), so lights use `n . (vertex - light)` and the directional `n . dir` with dir the direction the light travels. Map models have no vertex colours of their own.

## Script command `#944e11` (`cmd_944e11`, main executable 0x105850)

Switches game features off and on around scripted scenes: each option takes
"on" (str_code 0xe4e) or "off" (0x1c926). "off" sets, "on" clears, bits of
DAT_0017f498: `-m` 3 (weapon 1 and item 2 windows: the menus check 0x201 /
0x102), `-r` 4 (the radar: `FUN_00393318` returns 0 while 0x704 is set),
`-g` 8, `-s` 0x20, `-A` 0x40000, `-v` 0x80000, `-d` 0x30000, `-n` 0x4000
(inverted); `-p` sets DAT_0017f490 0x200 (no pausing), `-j` clears 0x80. The
stages' event procs use it in pairs (w04c `#fe0810` off, `#be57f5` on). The
port hides the radar and the weapon / item windows accordingly.

Other frequent unhandled commands, by handler: `#6a725a` 0x3a59d0 (options
w r m p c), `#b44bbb` 0x11cca8 (i t s d c), `#56ef97` 0x10ebb0 (runs its
`-s` block under a mask; the port runs the block), `#0acf09` 0x2d7d08,
`#fbeb7f` 0x105d48. The command table is the resident's (header +0xC).

## Script walls (`chara #f91b9f`, `dsegment.c`)

`chara #f91b9f <name> -p x1 y1 z1 h1 x2 y2 z2 h2 -a flags [-d]` adds a wall
from point 1 to point 2 (feet height y, height h above it) to the map's
collision (`FUN_00292160` -> `FUN_00150bc8`; flags `-a`, plus 0x27800
without `-d`, plus `-N` << 28). Messages (`FUN_00291e30`): 0 removes it, 1
clears flag bit 8 (on), 2 sets it (off), 3 moves it to new points, 5
shifts it. The stages place them in setup and toggle them in events (w04a
14, w01f 8...). The port pushes Snake out of the ones that are on
(`MGS2_NO_SCRIPT_WALLS=1` ignores them).

## Motion sounds (`.sar`, `#b44bbb`)

The `.sar` named like a `.mar` lists, per motion (same index), sounds keyed
to motion time (`frame x step`): a table `u32 n, n x {u16 sound, u8 mode,
u8 joint, u32 ?}` and a stream `{u16 time, u8 length, u8 table index}` up
to time 0xffff (`FUN_003b3568`, `FUN_003b33b0`; `mgs2_formats::sar`).
The soldiers' run (gbs `#01a8b3` motion 18, 53 frames) steps at 5 and 135 with sounds 2 / 3 on
joints 20 / 16 (the feet). The characters' handler (`FUN_003b3768`) maps the
sound through the tables `#b44bbb -i <table> -t <which> -s <sound> -d (id,
byte) x 8` registers (`cmd_b44bbb`, 256 slots at 0x1ccd60, keyed by table
and sound; `-c` clears): the id is `ids[material]` (`-t 0`; `-t 1` reads a
second byte), where the material is the floor element's flags >> 28 and the
table is 0 for Snake (`FUN_002898d0`) and 1 + voice for soldiers
(`FUN_00257bc0`). Without a table the raw sound plays. w00a: Snake's steps
2 / 3 become 422..437, the soldiers' 358..421.

Snake's shots (bullet flag 2: the M9's darts and the USP; soldiers' rounds
pass 1 and skip this) use the same tables: the struck element's flags >> 28
picks a hook the stage set per material (`FUN_0036c630` -> `FUN_003a5c40`):
`#41cf4e -n <material>` plays that material's variant of sound 0x27 from
table 0 (`FUN_003a5cf8`; w00a 446..453), `#0acf09 -n` registers
`0x2d7498` (sparks, not ported), and a few stages hook other actors on
5..7.

Snake's `snake.sar` (r_tnk0) keys his wall knocks to sound 55 on joint 6
(motion 67 at time 50, the crouched and other knocks 68-78, 91-94,
148-151) and the wall sidesteps (57 / 58) to sounds 18 / 19. Sound 55 is a
wall-keyed (`-t 1`) table: the knock for the wall's material, noise 6. Soldiers
hear knocks this way; there is no separate knock code.
