# Guard AI (Gurlukovich soldiers)

Addresses are in the w00a resident code image (loaded at `0x1F1480`, see
`stage-dat.md`). The port is `crates/tanker/src/guard.rs`.

## Actors

| Actor | Role |
|---|---|
| `chara 0xf7f777` | soldier: `-r` route, `-n` first route point, `-d x y z` start, `-i d0 d1 d2` vision, `-l` (hearing?), `-f` |
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

`MGS2_MOTION_LENGTHS=1` logs every archive's motion lengths and
`MGS2_GUARD_SHEET=dir MGS2_SHEET=motion:frame,...` photographs a guard in
given poses. The port plays motion 92 for the "!".

## Alert brain: `FUN_00229D30`

Seen → meter 0x400. The tracking flag (+0x348 & 0x40) expires 60 frames
after the last sighting (+0x33C).

Shooting (`FUN_00228710` / `FUN_00228F80`): aim for 21 frames, then fire
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
formation sees Snake (within `-t` near, or far while Snake stands; ±0x200
of his facing; clear line); the members' own state machine is not ported.

## Not yet ported

- the per-guard "!" length (from the action's motion);
- close-quarters sub-states, radio reports; the troopers' own brain;
- noise, footprints and bodies as stimuli;
- the game's precomputed shortest-path matrices in the `.hzx` (paths are
  recomputed from the node links instead).

## Olga (w00b): step tables run, selector simplified

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
  and reloads (0x1F62A0) when empty. Darts take 15 (head 30) of 100 stamina
  (not from the game). Her positions sit at y 13000, her raised platform.
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
