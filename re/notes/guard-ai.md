# Guard AI (Gurlukovich soldiers)

Addresses are in the w00a resident code image (loaded at `0x1F1480`, see
`stage-dat.md`). The port is `crates/tanker/src/guard.rs`.

## Actors

| Actor | Role |
|---|---|
| `chara 0xf7f777` | soldier: `-r` route, `-n` first route point (where he starts), `-d x y z map` alert post, `-i d0 d1 d2` vision, `-l` life (default 3000), `-f` (default 9) |
| `chara 0xddf5ca` | enemy manager: phases; `-h` caution time (default 0xE10); `-w proc` creates "wccomm" |
| `chara 0x59eda8` | "wccomm": `-e proc` creates the soldiers |

The soldier's act function is `FUN_00223388`; its controller lives at
guard+0x24d0.

`-i` comes from variables As16:0x1bc/0x1be/0x1c0, which the title screen
(`n_title` main, first statement) sets per difficulty:

| Difficulty (Bs16:0x8) | 0x1bc | 0x1be | 0x1c0 | 0x1c4 | 0x1c6 | life (Bs16:0xf4) |
|---|---|---|---|---|---|---|
| 10 Very Easy | 4000 | 7500 | 5000 | 3000 | 3 | 200 |
| 20 Easy | 4000 | 9000 | 6000 | 3000 | 5 | 150 |
| 30 Normal | 6000 | 9000 | 6000 | 3000 | 9 | 100 |
| 40 Hard | 7000 | 12000 | 8000 | 4000 | 14 | 75 |
| 50 Extreme | 7000 | 12000 | 8000 | 6000 | 19 | 50 |

`-d` is not the start position: `FUN_002233e8` keeps it at controller
+0x2b0 (map at +0x2c0; without `-d`, his own position). The alert brain
(`FUN_00228710`) sends a guard who has lost Snake there. A route of a single
point is a post he stands at.

## Perception: `FUN_002559C0`

- near range d (controller +0x248), far range 1.5d;
- horizontal: ±0x200 (of 4096) around the head yaw; vertical ±0x200;
- a 250 x 1500 x 250 mm box around the guard counts as seen ("bump",
  `FUN_00256C08`);
- line of sight against the level collision;
- result (+0x18E): 3 seen (inside d), 2 glimpse (d..1.5d), 1 blocked, 0 none.

## Suspicion meter: `FUN_00256488`

Meter at controller +0x274, 0..0x400: level 3 → +1 per frame; level 2 →
at least 1 (−1 per frame during an alert); otherwise −16 per frame unless
the tracking flag is set.

## Normal brain: `FUN_00227BC8`

16 states; transitions in `FUN_00227520` (stimulus bits at C+0x268) and
`FUN_00227430`:

- meter > 0 → investigate (state 6), which escalates when meter > 8;
- meter ≥ 2 or seen → discovery (state 8). Its sub-states come from the
  table at 0x3f0c20; sub 2 is the "!", sub 8 is aiming, which sets the meter
  to 0x400.

## Actions: `FUN_002240f8`

The state functions pick an action (body +0xfdc); `FUN_002240f8` maps it to
a gbs.mar motion (+0xfe4) and a step function (+0x1334):

| Action | Motion | Notes |
|---|---|---|
| 2 | `FUN_0025da40`: 92 (236 with flag 8, 357 with 0x4000) | the discovery "!" (174 frames) |
| 4 / 5 | 15 / 16 | |
| 6 | 17 (363 with 0x4000), step `FUN_00260320` | 580 frames; voice 0x11c (or thud 0x36e) at frames 70, 226 and 393 |
| 7 / 8 | 275 / 274 | |
| 9 | 231 | |
| 10 | 6 | |
| 0xb | 20 | |
| 0xc | 232, step `FUN_00260320` | 999 frames |
| 0x15 | 5 | |
| 0x18 / 0x19 | 104 / 93 | |
| 0x1a | none; +0xfb4 = 600 | |

Motion numbers are archive indices: the body's motion table (+0x1328) is
set by `FUN_0025ad08`, and every soldier passes 0, which selects the
identity table at 0x402090 (0..0x179, gbs.mar's 378 entries; 274 on are
1-frame placeholders with one shared id). `FUN_0025d470(body, 0, motion, 0,
0xffffff, 0x28, mode, bank)` starts it (mode 3: `FUN_0025ce90`), and the
flags at +0xf16 add upper-body layers through `FUN_002607a0`: bits 0..1
pick 196 / 177 / 180, bits 2..3 199 / 183 / 186 (the viewer shows these as
hand-to-head, arms-down and sitting poses: injuries and stances, not aims).
Shooting alternates action 4 (motion 15) with action 6 (motion 17) on shot
frames; reload is action 7 (motion 275, a placeholder) and lasts until the
step function sees the motion's end flag. 15 / 16 / 17 are long standing
loops in the viewer (no rifle drawn there), so the port keeps its aim pose
(motion 4) until the layering is understood.

`MGS2_MOTION_LENGTHS=1` logs every archive's motion lengths and
`MGS2_GUARD_SHEET=dir MGS2_SHEET=motion:frame,...` photographs a guard in
given poses. The port plays motion 92 for the "!" and 16 for the
investigation's look-around (sub-state 10 sets action 5).

## Damage: `FUN_0024a1e8`

Life is body +0xf0c, set by `FUN_0025ad08` through `FUN_00247d20` from `-l`
(As16:0x1c4: 3000 / 3000 / 3000 / 4000 / 6000 by difficulty; default 3000).
A lethal hit is matched against three groups of hit zones on the skeleton:
- +0x3a0, three zones (head and neck): life 0 at once;
- +0x640, eight limb zones: -500, and the limb's bit in +0xf16 (zones 0-1,
  2-3, 4-5, 6-7 set bits 1, 2, 4, 8); a second wound on the same pair
  (both arms, both legs) sets life 0. Wounded guards get the +0xf16 layer
  motions;
- +0xd40, one zone (the body): -1000;
- a hit with no zone: -1000.
Life at 0 plays motion 0x71 (113, the fall); otherwise a hit reaction
(motion 0x7b or `FUN_0025db68`). While the counter at +0xf25 lasts, a hit
pings instead (effect 0x27, sparks: `FUN_00248f68`). The port picks the zone
by hit height and distance from the guard's centre line.

## Blows, sleep and knock-outs

Stamina is body +0xf0e (`-f`, default 9). A punch (`FUN_0024b8e8`, motion
0x79, voice 0x114) takes 1; the knock-down blow (`FUN_0024c3a8`, motion
0x7d, voice 0x104) takes 5. At 0 he is knocked out (voice 0x120) for the
enemy manager's `-o` (`FUN_00221970`: default and every stage 1800 frames)
plus `FUN_0022d648(0x40)` (0..63), and stamina refills from +0xf06. A
tranquilized guard (+0xf12 below 0; see darts below) sleeps for the manager's `-z` (18000
frames); the count runs down at +0xf1a.

Darts (`FUN_0024f490`, hit flag 2): resistance +0xf12 starts at +0xf0a
(3000: `FUN_0025ad08` passes 6000, `FUN_00247d20` halves it). A head-zone
dart takes 3000 and counts in +0xf21, a limb dart 500 (+0xf22), a body dart
1000 (+0xf23); every frame `FUN_00258528` takes per dart 3000 (head), and
by difficulty Bs16:0x8 < 20: 3000 for every dart; < 30: 4 per limb and 8
per body dart; otherwise 2 and 4. Below 0 he falls asleep. On Normal one
body dart takes 501 frames, one limb dart 1251.

## Alert brain: `FUN_00229D30`

Seen → meter 0x400. The tracking flag (+0x348 & 0x40) expires 60 frames
after the last sighting (+0x33C).

Shooting (`FUN_00228710` / `FUN_00228F80`): a guard aims (action 4) when
he sees Snake (+0x18E level 3) or Snake is within 0x5dd mm (+0x188), and
`FUN_002285b0` agrees (only when DAT_001ecd40 bit 35 is set: Snake must be
within ±0x400 of facing him); within 1000 mm and seen it switches to
sub-state 6 instead (close quarters: alert action 5 or 9 when +0xf16 & 1,
table `FUN_002281e8`: `FUN_0025f810` with motion 0x55 / `FUN_0025fa48` 0xf3;
for 0x55 a hit box on joint 20 (`FUN_00254248`) rides frames 2-5; when the
motion ends, back to sub-state 3. Ported with an assumed 15 damage). Each shot is action 6. Aim for 21 frames, then fire
every 4 frames with a 1-in-3 chance to stop after each shot; reload after
31 shots.

Bullet: `FUN_0036D5B8(matrix, flags, 1, 0x19, 10, 20000, 1000, 0xf)`:
hit size 0x19, damage 10, range 20000 mm, speed 1000 mm per frame; aim
spread ±0x80 on two axes.

Damage: Snake's life is at player+0x782 (script Bs16:0xf2, maximum
Bs16:0xf4). `FUN_00289320` applies the damage accumulators (e.g.
DAT_001919f4) and revives Snake with an equipped ration: full life on Very
Easy, half otherwise.

## Evasion brain: `FUN_0022C670`

Its case 2 (`FUN_00238BF8`) is the discovery state again: seeing Snake
during evasion restarts discovery. The search pattern itself is not ported
yet; guards visit navigation nodes around the last known position instead.

## Manager: `FUN_00220818` / `FUN_00220430`

- M7 = max(M7 − 16, highest guard meter);
- infiltration/caution → alert when M7 ≥ 0x400;
- alert → evasion when M7 reaches 0; evasion lasts 0x4B0 frames;
- evasion → caution for `-h` frames (default 0xE10), then infiltration.

Phase is mirrored to Bs16:0x112 (0 infiltration, 1 alert, 2 evasion, 3
caution).

## Reinforcements

- The manager's `-a` proc runs when an alert starts; in w00a it is
  `chara #5bb4d4 -e proc:#8dfc0c -w 180 -a 300 -s 1`, a squad actor
  (`cmd_5bb4d4` at 0x29E0A0). Its init (`FUN_0029ddd0`) calls every `-e`
  proc at once (`FUN_0029dcb8` -> `FUN_00152ca0`), which create the
  troopers: `chara #cc9a07 <name> -r <route> -i <vision> -l -f -s -c -q
  proc n -e 0` (route 5 in w00a, entering at its first point). `-w`
  (default 0xf0 frames) is kept at +0x238 and counts down before the squad
  acts; `-a` is read only during evasion (+0x23C).
- The squad's act (`FUN_0029d9a8`) follows the phase (Bs16:0x112): alert
  `FUN_0029d348` / `FUN_0029d040` (leader and followers,
  `FUN_0029d360`), evasion `FUN_0029bd90` / `FUN_0029c1a0`..., caution and
  infiltration end it.
- Port: the troopers arrive 3 s after the alert starts with the alert AI,
  search with the others in evasion, and walk back to their entry and leave
  once caution begins. Their own brain (`cmd_cc9a07`) and the squad's
  formation are not ported.

## US Marines in the holds

The holds have no `0xf7f777` soldiers. Their Marines are members of the
crowd actor `#b3e388` (see `formats.md`): most stand in formation, and
members placed at `0 0 n` keep watch on route `n` (in w04a routes 4 and 6
are single-point posts; Hard adds route 2). Their archive `ushold.mar` has
the soldiers' speeds (walk 39: 2109 mm / 95 frames, run 40: 2164 mm / 32
frames), hands up 46, fall 151, lie 152. The port runs them with the
soldiers' AI and these motions (`guard::Anim::MARINE`).

The crowd actor's `-e` procs are its events (offsets from +0x8c8ac): 0 a
pad flag, 1 / 2 Snake near the `-p` points facing them, 3 a Marine heard
something (`FUN_00224a58`), 4 a Marine saw Snake (`FUN_00224b80`, member
state bit 0x80; the proc gets his position), 5..9 further states (bits
0x10 / 0x1000 / 0x2000, flags 0x400 / 0x800). Each pointer is cleared once
called. In w04a event 4 is `#0b6271`: the "caught" sequence, which ends in
`load "d14t" -g` (game over). The port fires event 4 when a Marine in
formation sees Snake (the 6 m box, 0x100 of his facing, 2 m of height, a
clear line; `-t` is level of detail, not sight); the members' own state machine
(`FUN_002273b8`, actions at +0x5a8) is ported in outline (`crowd::drill`):
idle motions, attention and head turns, the facing drills with their turn
motions, looking about and the step round to face front; the crowd's angles
are 180 degrees from the port's yaw (0 faces -z). The big turns' quarter
offset carried by the motion is not reproduced (they turn straight to the
facing).

## Not yet ported

- radio reports; the troopers' own brain; the strike's real damage;
- the game's precomputed shortest-path matrices in the `.hzx` (paths are
  recomputed from the node links instead).

## Olga (w00b): step tables run, selector simplified

- Stamina +0xfb2 starts at 0x80. Her eleven hit zones (table 0x1F6090,
  0x20 bytes each: joint at +0xc, damage at +0x1c as `(u32)float >> 17`
  unless the actor passes `-c`): two head / neck zones 32, eight limb zones
  8, the body 10. A dart (`FUN_002680e0`) plays 0x1f5 or 0x1f6 and takes
  the zone's damage; on Very Easy (Bs16:0x8 < 0x15) a dart also drains 4
  more every 128 frames down to 64 below where she was (+0x1072).

- Actor `chara 0x7798f6` (`-m org_def_sh_mt -a orga -p -18800 13000 -19300`),
  handler `cmd_7798f6` at 0x25EE10 in **w00b's** resident image (each stage
  carries the actor code it needs; analysed in a separate Ghidra project).
- Act `FUN_0025EC50` calls the step function at actor+0xF84, a pointer into
  step tables (0x1F6280..0x1F6400); steps advance it by 4, `FUN_00267DE0`
  picks the next sequence (partly random, with an 8-entry history at +0xF88).
  Start 0x1F6378 (`FUN_00265E70`) -> 0x1F62F0.
- `olga.rs` reads her step tables from w00b's resident image and runs them
  with Rust versions of the step functions (table in the module docs). Not
  ported: the selectors and cover choice (they depend on cover objects at
  +0x107C and Snake's zone at +0x109C); the stand-in alternates moves to her
  positions (0x1F5D80..0x1F5E30) with the shoot sequences 0x1F6410..0x1F6530
  and reloads (0x1F62A0) when empty. Her positions sit at y 13000, her raised platform.
- Steps decoded so far: most call `FUN_00327BC0(motion ctl, layer, motion, ...)`
  to play an orga.mar motion, set flags at +0xF70 and advance. `FUN_002617E8`
  14 then `FUN_00261878` 15 when she has ammo (+0xFB6 > 0; aims via`
  `FUN_00268660`), `FUN_00261640` fires (`FUN_0025FD28`) unless flag 0x80000;
  `FUN_00261308` 13 and a dash toward the target at +0x1030; `FUN_00262158`
  / `21F0` / `2288` 37 / 38 / 39; `FUN_00265BA0` 1 (walk); `FUN_00260A10` 4
  or 6; `FUN_002661E0` 46 (down; the defeat sequence at 0x1F6338);
  `FUN_00262B00` layer-1 motion 12. `FUN_00268830(n)` returns entry n of a
  16-byte table (positions?) and `FUN_0025FCC0` moves toward one.
  `FUN_00267DE0` picks the next sequence through a condition function from
  0x1F8198 (indexed by +0xF70 & 3), avoiding the last 8 picks (+0xF88).

Cover objects: the array at w00b 0x3ca0b0 (0x70 bytes each, up to 10) is
built by actor `cmd_53209a` (`FUN_00259f30`): w00b's script creates it as
`chara #53209a #c970e3 -m ~obs_box1 ~obs_box2 ~obs_irs -r 0 0 0 -p 0 0 0
...` (three obstacle models, all placed at the origin, so moved later);
`-r` sets each one's turn (+0x68, 10-bit), `-p` its place. Olga's +0x107c
points at them; `FUN_00268d80(cover, point, flags)` grades each (2 bits),
`FUN_002673b0` picks one by those grades and Snake's zone (+0x109c: 0x0,
0x20, 0x60, 0x160, 0x170, 0x1f0, 0x1f2, 0x1fa, 0x1fb, 0x1ff map to cover
masks). Who moves the obstacles is not traced yet.

## Noise (`FUN_0011b630`, `FUN_00220078`, `FUN_0021f958`)

Emitters latch the loudest (level, place) of a frame; soldiers read it the
next frame. Levels 5, 6 and 9 send the nearest soldier within his hearing
range (doubled for 9, within 3000 mm of the noise's height, same area) to
investigate; 11 runs `FUN_0021ff20`; 2, 3, 4, 8, 10 are ignored. Motion
sounds and impacts take their level from the `#b44bbb` byte: Tanker
footsteps are level 0 (silent) except material 4 in w04a (6), rounds
striking are 5 (9 on material 3 in w01b / w01c), sound 55 is 6. The port
dropped its "running is heard" rule for this. The hearing range is the
soldier's `-h` (`FUN_00222f60` -> `FUN_00259618`, guard+0x271c = controller
+0x24c; default 12000, 1000 with controller flag 0x40): w00a 4000, w01b
6000, w02a 4500 (As16:0x1c2 minus A:0xb3a).

Crowd event 3 ("a Marine heard something", `FUN_00224a58`, the member's
position as `$1..$3`) runs from the member update (w04c 0x225xxx) while its
state +0x570 has bit 1; in w04a the proc cuts to a camera on the member and
plays a line. Bit 1 is set by the crowd's noise reader `FUN_002286f0`
(w04c): level 6 marks the nearest member within 10 m (bit 1); level 3 with
Snake within 2.5 m in first person marks one facing him (0x80000); levels
10 and 11 mark every member (0x10) and set DAT_0017f48c bit 1, after
which the update runs event 0 once (w04a: a game over). The port
does the level-6 case (footsteps on w04a's noisy floor, a hard landing).

Approach points (`-p`, w04a one, w04b two: x y z, rotation x y z, one
more value): Snake within 3 m of a point and inside a horizontal cone along
its facing (forward vector (0, 0, 4000) at w04c 0x1f38e0 / 0x1f38f0; half
angle 20 / 25 degrees within 1 m, else 15) runs event 1 / 2 once (the proc
pointer is cleared). Messages 0x42 / 0x43 arm and 0x44 / 0x45 disarm the
points. Skipped while `DAT_001ecd40 & 0x4030`. In w04a the event is a game
over (walking up in front of the formation).

## Stimuli (`FUN_00227520`, C+0x268)

Checks at 0x2307f0..0x2316b0 set the stimulus bits each frame; the
normal-phase transition picks a state per bit (C+0x32c):

| Bit | Set by | State |
|---|---|---|
| 0x1 | heard level 5 / 6 / 8 / 9 (`FUN_00230830` via `FUN_0021e7a8`) | 3, `FUN_002345b0` |
| 0x200 | heard level 3 (`FUN_002307f0`) | 5 |
| 0x800 / 0x1000 | heard level 10 / 11 (unsuppressed shot, explosion) | 8: discovery, `FUN_00238f68` |
| 0x8000 | a book (`bul_book.c`, the default marker group `DAT_001cccc8`) in view (`FUN_002308b0`) | 9 |
| 0x10000 | a footprint (`foot_print.c`, type 0x12) or blood (`blood_sp.c`, type 10) marker group in view (`FUN_00230948`) | 10 |
| 0x4000000 | Snake within the 250 x 1500 x 250 bump box (`FUN_00230c20`) | |
| 0x40 | a comrade down in sight (`FUN_00230a48`; 0x2000000 instead under +0x350 bit 0x8000000) | 2, `FUN_00233120` (below) |
| 0x2000, 0x4000, 0x20000, 0x40000, 0x200000, 0x400000 | 0x230b40, 0x230dd8, 0x2311f0, 0x2316b0, 0x230c20, 0x231418 | various |

Level 8 is the soldier's own "!" call (`FUN_00237128`), so nearby soldiers
hear a discovery. Marker groups register through `FUN_00117798`.

Footprints (`foot_print.c`): leaving a type 0 / 2 named area (`0x839ecc` /
`0x97cb18`; w01a, w01b, w01e have `0x839ecc` at the doors in from the
rain) leaves Snake wet for 3000 frames. Blood (`blood_dp.c` -> `blood_sp.c`):
a hit that leaves him under a third of his life sets 0x1000000 and a drop
falls every 20 frames until life is a third again or a ration is used. A
soldier who sees either marker group goes to the newest marker in view and
marks the older ones (`FUN_00240858`); the port follows that way.

## Drops and dog tags (`FUN_00245e08`, `FUN_00245e98`)

The soldier's `-q` and `-a` each list up to four `proc chance` pairs (kept
at body +0x12b0 / +0x12d0 with a next-index at +0x14 and a count at +0x16).
`FUN_00245e98(scale, list, pos, flags)` tries the next entry: if
`rand % 100 < chance * scale` it calls the proc with (map, x, y, z,
flags & 8, ?, ?, the list's +0x18: the soldier's actor name) and moves on.
`-q` is tried when he goes down (`FUN_0024ae78` / `FUN_0024b578` hit
reactions with 0xf, the sleep and knock-out steps `FUN_00261a40` /
`FUN_00262048`); `-a` when shaken during a hold-up (`FUN_0023acd8`, frame 30
of action 13) once his tags are gone, else `-j` (+0x12f0: proc plus a tag
name per difficulty) gives the tags. The w02a procs pick an item box by
difficulty (Bs16:0x8) and the soldier's name (e.g. `#26539fc`), guarded by
whether Snake can still take that item.

Explosions: `blast.c` makes noise 11, which `FUN_00220078` hands to
`FUN_0021ff20`: if any soldier can still act it records the place and,
60 frames later (`FUN_00220008`), writes 0x400 to the manager's input
(+0x34), which `FUN_00220430` takes as the meter: Alert. More than six
blasts also set manager flag 1. The port does this for stun grenades.

DAT_0017f48c bit 1 holds the phases still: with it set the manager
(`FUN_00220818`) does not let Evasion run out into Caution or Caution into
Infiltration. The holds' crowd sets it (with every member's 0x10, which
turns them to face Snake) when it hears noise 10 or 11.

Other stimuli: 0x2000 (`FUN_00230b40`) Snake aiming at the soldier from
within 3x his near range (the hold-up path); 0x4000 (`FUN_00230dd8`) part of
Snake's skeleton visible (joint mask DAT_001eccb8) while the sight line is
blocked; 0x20000 (`FUN_002311f0`) Snake in the box within 1200 x 1500 x 1200
mm and in front (within 200/4096): state 11, the box inspection
(`FUN_00242448`, actions `FUN_002413d0`). Sight ranges (`FUN_002559C0`):
near +0x248, far 1.5x; in first person (camera mode 6) both x 3/4; in the
box at Alert max(near / 4, 2000).

### Crowd messages (`FUN_00227d..`, handler before `FUN_00227ee0`)

Script messages to the crowd (`#3822c7 <crowd> n`) map to a formation mode:
2-6 -> 0x11-0x15 (facing modes, +0x8cab4: 0x12 / 0x13 / 0x15 turn to 0x400 /
0xc00 / 0x800, 0x14 back to 0, 0x10 / 0x11 the `-c` facings), 7-10 -> 6-9,
11-15 -> 10-14, 16 -> 0x16 (face a point), 17 -> 0x10, 18 -> 0xf; 0x40 sets a
target member, 0x42-0x45 arm / disarm the `-p` points. Modes 10-14
(`FUN_00225690` action 6) set +0x588 to -900 / 900 (10 / 11) and +0x58a to
800 / -800 (12 / 14): the head's turn (`FUN_002282f0` eases +0x58c / +0x58e
toward them by an eighth a frame and multiplies the rotation (x pitch, y yaw)
into the matrix at +0x3b0, joint 12 of the 0x10-byte list at +0x2f0). w04a-c
send 11-15 in proc `#4b58a6` and 14 in `#678089`. Ported (`crowd::Head`;
`MGS2_CROWD_MSG=n` sends one for testing); the axis order and signs are
assumed.

## Notes toward the hold (not ported)

The soldiers' stimuli 0x8000000 / 0x1000000 / 0x2000000 lead to normal
states 0xf (`FUN_00245db0`, action table `LAB_00245880`), 0xd
(`FUN_00244ad0`, table `FUN_00243cf0`) and 0xe (`FUN_00245840`); they are
set from outside the soldier (Snake's actions), and one of them is likely
the hold. Snake's `snake.mar` contact sheet (`MGS2_MOTION_SHEET` with
`--look`) shows arms-at-chest poses at 73-78 and 160-164 and big swings at
177-180, candidates for hold / drag / throw; none confirmed.

## The radio (`radio.c`, w00a 0x265ea0..0x266458)

`FUN_0025a718` (the soldier's body setup) makes the `rad` model actor
(`FUN_00266458(owner body, model, &flags, ?)`) unless his model is
0xc9aaaf / 0x296d9c / 0xd7aaaf / 0xfbbaaf. Its act `FUN_00266168`:
`FUN_00265fe0` places it each frame by the flags word (soldier +0xb4, set
to 1 at setup; < 0 removes the radio): bit 0x1 shown, 0x2 on the left hand
(joint 10, 0x1f3e00 / 0x1f3e10), 0x4 on the right hand (joint 6, 0x1f3e20 /
0x1f3e30), else on the belt (joint 0, 0x1f3e40 offset (-120, 25, -97.5) mm,
0x1f3e50 turns (4028, 3777, 2048)). Joint matrices: model objects
(`FUN_001287f0`) have a 0xc0 header and 0x130 bytes per joint, the matrix
first. `FUN_00125268` translates in the joint's space, `FUN_001253f8` turns
about x, y, z (order of the multiplies not decoded: VU macro code).
`FUN_00265f50`: the radio's own hit box (`FUN_00107da0(.., 0x2000011, ..)`)
struck by 0x800000004801e damage sets flag 0x10000 and plays the
`FUN_002b9ca8` effect (a shot radio; what 0x10000 stops in the soldier,
probably his radio call, is not traced). Ported: the belt place only.
The hand flags come from the radio motions (body action functions):
`FUN_0025ebc0` (action 4, motion 0x23 radio out; 0x31 with the left arm
wounded, +0xf16 & 3 == 2) sets 0x2 (0x4 for the other motion) at frame 28
(7 with `DAT_0017f48c` bit 1) and signals +0xf58 = 3 at the motion's end;
`FUN_0025eda0` loops 0x1b (0x32) while he talks, ending after 600 frames
or when action 5 sets its counter +0xfb4 to 600; `FUN_0025f068` puts it
away (0x1c / 0x33, flag cleared at frame 62). Ported (left hand only).

## A comrade down (normal state 2, `FUN_00233120`)

Stimulus 0x40 (`FUN_00230a48`): `FUN_0021e998` finds a soldier in the
manager's list at +0x7c (count +0xdc, only cleared in w00a's decomp: the
writer is not visible; the port takes the soldiers lying down) whom he
sees at level 3 within his near range. `FUN_00233330` targets him (+0x2f4)
and starts sub-state 5 when he is down (+0xf98 bit 0x4000, or
0x4000000000000, or +0xfb0 bit 4) and not already seen to, else 0. The
sub-states (+0x330, timer +0x338) and their body actions (+0xfdc, table
`FUN_00231948`: 1 motion 6, 2 7, 3 the "!", 4 radio out, 5 radio away, 6
motion 10, 7 0x5b, 8 0x4c, 9 0x2f):

| Sub | Function | What |
|---|---|---|
| 5 | `FUN_002325e8` | mark kind 4, faces him, 61 frames |
| 6 | `FUN_00232660` | runs to him (pace +0x254 = 3) to 0x513 mm; knocked out (+0xffa 2) -> 0x10, asleep -> 0x12, 0x4000000000000 -> 0x17, else 8; a second soldier on the same comrade within 2 m -> 9 / 8 |
| 0x10 | `FUN_00232a60` | waits 91 frames for him to come round (then 0x15 in Infiltration, else 8), else 7 |
| 7 | `FUN_00232b20` | action 8: kneels and shakes him (hit 0x1001000000000000 on frame 22, `FUN_00254580`), then 8 (0x15 if `FUN_002527d0`) |
| 0x12 | `FUN_00232938` | "!", action 9 at frame 60 (the kick); after it his down timer +0xf1a = 20; Evasion: done; else the report |
| 0x17 | `FUN_00232e60` | mark kind 5, 181 frames, target +0x34c bit 4, then 8 |
| 8 | `FUN_00232cb0` | looks at him to frame 181, done |
| 0x15 | `FUN_00232c08` | line 0xc000012, done |
| 10 / 0xc | `FUN_00231d08` / `FUN_00231d88` | the radio: action 4, then line +0x310 (0x1c00000b); at its end action 5 and, for this report, `FUN_0021ee90(-h)` (the caution timer, manager +0x24, which turns Infiltration into Caution); done when the radio is away |
| 0xb / 0xd | `FUN_00231f20` / `FUN_00231fb8` | radio shot (+0xb4 bit 0x10000) or `DAT_0017f48c` bit 0x10: no call (action 6), back to the route |
| 0xf | `FUN_00232128` | the report when the caution timer is still over 3/4 of `-h` or in Evasion (`FUN_00252708`): a "?" for 121 frames |
| 0-4, 9, 0x18 | | a comrade already seen to; another soldier nearby; can't see him from beside him (line 0x144) |

The report's entry is `FUN_00231c28(line, mask)`; the line's length comes
from table 0x400ca8 (`FUN_0021c608`, filled at load), 0xf0 frames when the
line cannot play. Ported in `guard::comrade` (sub-states 5, 6, 7, 8, 0xc,
0xf, 0x10, 0x12, 0x15, 0x17); assumed: 0x4000000000000 = dead, the shake
wakes him; the lines now play (see "Radio lines" below). So only a sleeper's
kick leads to a call and Caution; a knocked-out or dead comrade does not.
`MGS2_DOWN=route:s|k|d@seconds` downs a soldier for testing (w01d r11 / r18).

## Radio lines (`FUN_0021cdb8`, the enemy manager's `-v` / `-y`)

The manager's init (w00a `FUN_00221970`) reads two data blocks
(`-v str[15] -y str[17]` in w00a; `gcx_res` example dumps them):

- `-v`: pairs (VOX clip `0x20000000 + sector`, length in 1/300 s) up to a
  negative id -> 0x401648 (clip) and 0x400ca8 (length / 5 = frames). w00a
  has 464 entries: 8 sets of 0x38 lines (voices 0-3, then 4-7 with other
  wordings) plus 16. Set 0 from 0: "Possible hostile sighted. Send
  backup.", ..., 0xb "It's him, everyone stay on guard.", 0xf
  "Acknowledged.", 0x12 "The situation is normal.", 0x13-0x16 noises and
  footsteps, 0x19-0x1c footprints and blood, 0x23-0x26 the box, 0x27-0x37
  hold-up and challenge lines ("Hold it!", "Why are you unarmed?").
- `-y`: HQ's replies up to a negative word (w00a 0x20005978, 0x20006b4e:
  "Understood."), then -2 and a clip for 0x4014a8 (w00a 0x2000769a "We've
  lost communication with the aft deck."), -3 and one for 0x401638.

`FUN_0021cdb8(line, controller)`: index `line & 0xfff` (+0x1c0 with bit
0x10000); with a soldier and without bits 0x21000, + 0x38 x controller
+0x284 (his voice) + 0xe0 if body +0xf2c & 0x4000. Length: bit 0x8000000
the table's, 0x20000 / 0x1000 other tables (0x401e48 / 0x401f08, options
`f` / `j`), else 0xf0. Clip: bit 0x1000000 0x4014b4 (option `x`, the
area lines of `FUN_0021c670`), 0x2000000 the `-y` replies (0 -> 0x401d5c),
0x100000 0x4014a8, 0x400000 0x401638, else the `-v` clip. The top nibble
is a priority: a line does not cut one of equal or higher priority.
Unless bit 0x40000 effect 99 (the radio click, `FUN_00114b30(0x20, 0x3f,
99)`) plays; bit 0x4000000 pans the stream from the soldier.
`FUN_0021cb40` queues follow-ups for when the line's frames run out: the
report 0x1c00000b (also 0x1c000006..8, 0x1c00000d and a few others)
-> `0x2000000 | n`, HQ's reply n (manager message 3, `FUN_0021f760`; 0
by default); 0x1c000009 / 0xa -> line 0xc00000e, then the reply (or
0x1081400f once the manager's count +0x814 reaches +0x816); 0x2c00000x
and 0x20008003 -> an area line (`FUN_0021c940`).

Investigation (normal state 6, `FUN_0023c470`): sub 0 (`FUN_0023c028`,
line 0x140, a turn, action 3) goes to 5, to 6 in Evasion (action 4, then
5), or to 1 (flag 0x20 with a noise of kind 1); sub 5 (`FUN_0023c108`)
waits 61 frames, says 0xc04001d "Is there somebody there?" (0xc04001e
"Over there?" in Evasion; no click: bit 0x40000) and walks there (sub 8,
`FUN_0023bcf8(.., 0x23bfd8, 350)`).

Ported: the downed-comrade report (voice set `voice()`, set 0 only, line
0xb) and HQ's first reply, "it was nothing" (0xc000012) when a
comrade comes round, and the investigation's line (its pause after the
port's assumed 30-frame turn); they play through the spoken-line player with their
subtitles (assumed shown, as the clips carry them), unplaced. Not ported:
the priority rule, the other lines' callers, the 0xe0 sets (what sets body
flag 0x4000 is not traced), the voice index (+0x284; the port's
`route % 4` is assumed).

