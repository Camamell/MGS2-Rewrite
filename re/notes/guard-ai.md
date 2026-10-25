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
pings instead (effect 0x27, sparks: `FUN_00248f68`).

The zones (`FUN_0025b608`, the body setup): 0xc0-byte collision records made
by `FUN_00107da0(zone, 0x2004011, 1, 2, half extents, offset)` and linked as
groups to the soldier's main box (+0x10, kind 0x2000010) by `FUN_00108098(main,
group, count, order)`. Each frame `FUN_00258028` copies a joint's matrix
into every record (`FUN_00107f38(zone, *(body+0xf44) + 0xc0 + 0x130 * joint)`):

| Group | Zone | Joint | Half extents (mm) | Offset (joint space) |
|---|---|---|---|---|
| +0x100 (order 0) | 3 zones | 0, 12, 12 | | always flag 0x80000 |
| +0x3a0 head (1) | head | 12 | 96 130 96 | 0 30 0 |
| | heart (left chest) | 2 | 50 65 165 | 100 100 0 |
| | groin | 0 | 100 65 150 | 0 -150 0 |
| +0x640 limbs (2) | right arm 0, 1 | 4, 5 | 75 150 75 | 0 -130 0 |
| | left arm 2, 3 | 8, 9 | 75 150 75 | 0 -130 0 |
| | right leg 4, 5 | 13, 14 | 85 200 120 / 85 250 120 | -10 -200 -10 / -20 -250 -20 |
| | left leg 6, 7 | 17, 18 | as the right | |
| +0xd40 body (3) | body | 2 | 200 300 200 | 0 0 0 |

(Joints of `gbs_def`: 0 hips, 2 chest, 4 / 5 and 8 / 9 the upper and lower
arms on -x and +x, 12 head, 13 / 14 and 17 / 18 thighs and shins; the model
faces +z, so -x is his right.) The heart and groin zones get flag 0x80000
under body +0xf2c bit 0x1000 (not traced). Kind bit 0x4000 turns the offset
by the joint's matrix and tests in the zone's own space (`FUN_00108f28`,
`FUN_001094c8`): boxes on the joints. A round is a swept segment (its box
+0x1c0 gets the frame's start and end, `FUN_00108050`; kind bit 0x10000);
the collision pass `FUN_00109fb0`, once the segment meets a box with linked
groups (bit 0x40000), walks the groups by order, skips zones flagged
0x80000, tests the bare segment against each zone (`FUN_00109b10` ->
`FUN_001094c8`) and stops after the first group with a strike
(DAT_001b7210). So a round through the head, heart or groin kills even
when it also passes the body. A round's kind is 0x5010400 (`FUN_0036d450`
region, 0x313067: 0x5050400 with its flag 0x100): bit 0x4000000 makes
every hit shorten the segment to its entry and remember the record
(DAT_001b7200, `FUN_00109760` / `FUN_00109b10`), and only that nearest one
is struck at the end (`FUN_00109fb0`'s tail). So one zone of the group is
struck (the nearest), and of a soldier and his radio the nearer. His main
box (+0x10, kind 0x2000010, half extents 400 x 1000 x 400 at w00a
0x1f3b80, on his position) always holds the radio, and the body zone (400
mm deep) nearly does: the radio is struck from behind only where it
stands out past the body zone. (Which of the two is tested first is not
traced; it matters only for a head-group zone behind the radio.) Ported (`bullet::zone_hit`) for
models with the soldier's skeleton (others, if any, count a body hit).

Olga (w00b `FUN_0025f510`) builds hers from a table: eleven 0x20-byte
entries at 0x1f6090 (half extents, the joint as a float in +0xc, the
offset, and a float at +0x1c whose integer >> 17 is the dart's damage,
low 16 bits signed by 0x10000 kept beside it), kind 0x2014011, in groups
of 2 / 8 / 1 (counts at 0x3bb778; option 99 of her script can override
them, and `FUN_00152920` the damage: not read). Head (joint 12) and heart
(joint 2) 32, the limbs 8 (arms 45 x 150 x 65), the body on joint 1
(200 x 300 x 100) 10. Ported (`bullet::OlgaZones`, read from the resident
image).

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
within ±0x400 of facing him); within 1000 mm and seen, with Snake on his
feet (DAT_001ecd40 & 0x620 clear: not prone, thrown down or lying; every
way in tests it, `FUN_002284b8` / `FUN_002290b0` also the soldier's legs,
+0xf16 & 0xc), it switches to sub-state 6 instead (close quarters: alert action 5 or 9 when +0xf16 & 1,
table `FUN_002281e8`: `FUN_0025f810` with motion 0x55 / `FUN_0025fa48` 0xf3;
for 0x55 a hit box on joint 20 (`FUN_00254248`) rides frames 2-5; when the
motion ends, back to sub-state 3). The hit (`FUN_00254248`, frames 2-5
of 0x54-0x57, via `FUN_0025f810`): a box of half-extents 500 mm (w00a
0x1f3c40) on joint 20 (0x14 for motion 0x55; 0x57 / 0xf6 use 10, 0x56 6,
0x53 0xe, 0x54 / 0x4c 0x10, 0x82-0x85 / 0xf3 / 0x173 joint 0 with flag
0x40000000000000), a push (0, -100, 100) turned by his facing (0x1f3c30),
and the hit record `FUN_001081d0(+0x1290, 0, 0xff, 0, 5, push)`: damage
5 at +0x14 (the field the rifle round's 10 and the blasts' damage fill;
an instant kill puts Snake's maximum life there). Boxes meet when their
centres are within the sum of half-extents on every axis
(`FUN_00108e38`). Ported: damage 5, the 500 mm box on joint 20 against
Snake's collision cylinder as a box (assumed: his own hit box is not
read), once per strike, and the alert's action 9 for a soldier with a
wounded arm (+0xf16 & 1: `FUN_0025fa48`, motion 0x54 with the box on joint
16; 0x153 under body flag 0x4000, not traced); the push too (below). Each shot is action 6. Aim for 21 frames, then fire
every 4 frames with a 1-in-3 chance to stop after each shot; reload after
31 shots.

Bullet: `FUN_0036D5B8(matrix, flags, 1, 0x19, 10, 20000, 1000, 0xf)`:
hit size 0x19, damage 10, range 20000 mm, speed 1000 mm per frame; aim
spread ±0x80 on two axes.

Damage: Snake's life is at player+0x782 (script Bs16:0xf2, maximum
Bs16:0xf4). `FUN_00289320` applies the damage accumulators (e.g.
DAT_001919f4) and revives Snake with an equipped ration: full life on Very
Easy, half otherwise.

### Snake's reactions to hits (`FUN_002713e8`, `FUN_00271af0`)

`FUN_002713e8` takes a hit on Snake: the record's damage (+0x14) is
halved for flags 0x800000004801c when +0x9c8 == 7 and raised by half
under DAT_001ecbe8 & 0x8000000000000002 (neither traced), added to the
accumulator (PTR_DAT_00191910 + 0x13e, capped at 30000) and the frame's
hit bit DAT_001ecd40 0x200 set. For flags meeting 0x440023000203e0 the
record's vector is his push (+0xf30) and he is turned to face where it
came from (+0x62 = its heading + 0x800; flags 0x4000000000000 /
0x2000000000 pick front / back variants, bits +0xf90 0x2000-0x20000).
`FUN_00271af0` then picks the state:
- flags in that mask (a rifle butt 0x100000000, a blast 0x20000): voice
  0x18 and the knock-down `FUN_00285690`: motion slot +0x1a (+0x10 /
  +0x12 / +0x14 / +0x16 by the +0xf90 variant; +0x1c when +0xa70 = 1),
  sliding by the push, which eases by an eighth a frame
  (`FUN_00142008(+0xf30, 0, 3)`); at the motion's end `FUN_00285b28`
  (slot +0x1e, +0x22 face down): +0xa70 counts 1 a frame (5 while the pad
  is worked) and past 60 any button press gets him up (`FUN_00285d30`,
  slot +0x20 / +0x24; motions 0x15e / 0x15f when +0x9b0 is 5, 6, 0xf or
  0x12), then standing (`0x2733d0`). Life at 0 goes to `FUN_00285eb0`.
- others (rounds): voice 0x19 and the flinch `FUN_002852c0`: slot +0xe /
  +0xc by the frame clock's parity, +0x26 prone (+0xf40 == 2); then his
  stance's state. (`FUN_00285430` instead with item 0x47bfe3 and flag 0x40,
  `FUN_00285570` / `0x278c58` with flag 2: not traced.)
The slot table (`FUN_00269458`: player+0x420 = w00a 0x1f4b30, shorts):
+0xc 11, +0xe 12, +0x10 206, +0x12 207, +0x14 338, +0x16 339, +0x18 13
(death), +0x1a 14, +0x1c 176, +0x1e 15, +0x20 16, +0x22 115, +0x24 116,
+0x26 37. Ported (`player::React`): the flinch (11 / 12, 37 prone), the
knock-down from the front (14, the push, 15, 60 frames and a key, 16) for
the rifle butt and the soldiers' grenades. The push sums the hits'
records into Snake's (`FUN_00108468` -> `FUN_001082f0`): a record of
type 0 adds its vector (the butt's), type 4 (a blast, `FUN_003724d0`:
x = half its box size / 8) pushes that far along the line from the
blast to him (atan2 of the boxes' centres): 125 mm a frame for the
soldiers' 2000 mm grenade. The front / back variants (+0xf90 0x2000 /
0x4000 / 0x10000 / 0x20000) are chosen only for hit flags
0x4000000000000 / 0x2000000000 (`FUN_002713e8`, by the push's heading
within 0x400 of his facing), which neither the butt nor a blast carries:
the plain one (`+0x62 = heading + 0x800`) is the Tanker's. Not
ported: face-down lying, the damage scaling,
flag 2's and the item's states, 0x15e / 0x15f.

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

- The manager's init (`FUN_002181f0`) runs every `-w` proc and then
  every `-a` proc at once (`FUN_0021a688` -> `FUN_00152ca0`), not on the
  alert (corrected 2026-10-21; the port still re-runs `-a` when an alert
  starts for the troopers, whose squad waits for it). An `-a` such as
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
- Leads (w00a dump, not ported): the trooper (`cmd_cc9a07`, 0x3310-byte
  actor, update `FUN_0029b228`, init `FUN_0029b2f0`) is a soldier body at
  +0x2ed0 run by the common `FUN_00257bc0` / `FUN_00257f50` /
  `FUN_00258028`, with its own brain chosen at init from `-s` (+0x3220)
  and the phase: bit 0x800000 -> `FUN_00227e00` (the route walker);
  bit 0x40000000 (needs `-b`, a manager link at +0x3098) -> `FUN_003c5c40`
  in Caution, else `FUN_003bbaf0`; otherwise `FUN_003c2fb8` in Caution,
  else `FUN_003bbaf0`. Options: `-r` route (required), `-n`, `-l` (default
  3000), `-f` (default 9), `-h` (default 12000), `-i` vision boxes (6000),
  `-v` voice (default name % 4), `-e` kind (4 = 0xfbbaaf / 0xe8419b models,
  1-2 a second weapon, 3 another), `-d` post, `-o` a second point. The
  squad's act (`FUN_0029d9a8`) keeps its state at +0x228 (0 none, 1 alert,
  2 evasion) from Bs16:0x112; in the alert `FUN_0029d040` counts members
  seeing Snake (+0x18e / +0x1b6 == 3) into +0x20a and runs the leader /
  follower planners `FUN_0029c1a0`, `FUN_0029caa8`, `FUN_0029cb40`,
  `FUN_0029ce20`, `FUN_0029cff8`, `FUN_0029bfa0`; the evasion runs
  `FUN_0029c1a0`, `FUN_0029bfa0`, `FUN_0029bec0` with `-a` at +0x23c.

## US Marines in the holds

The holds have no `0xf7f777` soldiers. Their Marines are members of the
crowd actor `#b3e388` (see `formats.md`): most stand in formation, and
members placed at `0 0 n` keep watch on route `n` (in w04a routes 4 and 6
are single-point posts; Hard adds route 2). Each is a full soldier actor
(`FUN_002302a8` -> `FUN_0022fbe8`, update `FUN_0022f9a8`, brain switch
`FUN_0022f2d8` on +0x32c) made by the crowd's constructor for members whose
x and y are 0. Its motion table maps slot k to `ushold.mar` motion k + 0x26
(0x17a slots), then the pairs at w04c 0x1f37a0: slot 0 -> 0x1f (idle, 119
frames), 1 -> 0x20 (walk: 1133 mm / 59 frames, 1152 mm/s), 5 -> 0x21 (499
frames). `ushold` 38.. are the soldiers' `gbs.mar` by slot (same motion
ids for half of them; the other half are the Marines' own versions, and
the aim (42), the look about (54) and every radio motion are a one-frame
placeholder, id 0x5fc4). So run 40, hands up 46, fall / lie 151 / 152, the
"!" 130, knocked down / up 161 / 168 and the two get-ups 170 / 171 match
the soldiers' slots. The port runs them with the soldiers' AI and these
motions (`guard::Anim::MARINE`).

The route walk (state 0, `FUN_0022e770`): sub 0 waits 61 frames, 1
(`FUN_0022e040`) walks to the point, 3 (`FUN_0022e0a8`) arrives (within
0x15e) and reads the point's action / wait / direction (+0x900 / +0x980 /
+0xa00 tables), 2 (`FUN_0022e1b8`) turns to the direction with the head
leading, 4 (`FUN_0022e2c0`) waits (action 5 for point action 3), then the
next point. `FUN_0022e928` (Snake seen, level >= 2, or +0x268 & 0x1800)
goes to state 8 (`FUN_0022f7b8`: sub 8 `FUN_0022e670` the "!" with mark
0x53, then 9 `FUN_0022e710` facing him and `FUN_0021e200(1)`, the crowd's
"caught" event).

The commandant (`#8af8fd` in w04c, variant 19 at `0 0 0`) is such a
walker with member flags | 0x21: his slots 0 / 1 / 3 / 0x3d become 0x23
(idle, 139 frames) / 0x22 (walk, 912 mm in 157 frames: 349 mm/s) / 0x25 /
0x24; his update hides two models (his rifle and the object at +0x2358);
at a point's end he plays action 6 for 10 frames (sub 0xb, `FUN_0022e388`)
before the next walk; the soldiers' perception switch (`FUN_0023e318`) is
replaced by `FUN_0022f4a8` (a hit, it seems) -> state 8. He paces w04c
route 0 on the stage (x -4000..4000 at y -14750, waits of 1200 / 600
frames facing the formation). His `sco_def` is an `.evm` without a kms:
the port draws it skinned over the Marines' skeleton (`body::EvmBody`,
hips 1047.5 mm up from the evm's bones). Not ported: action 6 at his
points, his `FUN_0022f4a8` test (he sees as the walkers do), flag 0x80's
60-frame count to `FUN_0021e200(1)` (+0x25e4 is not traced; his `-a`
flags are 0x90).

The `-c x y z dir member actor` entries (`FUN_00230938`, update 0x230640)
are TV cameras on tripods, not speakers: each loads two models
(0xc6cbc3 the camera, 0xc0b045 its tripod, `FUN_00355268(.., 0x11)`) 145 mm above `x y z`, turned
toward the actor named (`FUN_00230440`: x = atan2(horizontal distance,
dy) - 0x400, y = atan2(dx, dz), `FUN_0016cee0` being newlib's
`atan2f(y, x)`; so the camera's lens, +z, points at him; w04c: the
commandant's feet, his position) unless its member's state has 0x60000, and marks the member
(flags | 0x40, +0x5bc = 6, +0x5e8 = 9, +0x5ea = 1). In w04c two Marines
(23826964 / 40604180 at +-5500, -15000, -1200) work the cameras at
+-5250, -14000, -2000 (the port had put the two Marines there; they stand
at their own records). The update (0x230640, decompiled headless; the dump
missed it) has two states at +0xcc: 0 films the actor (`FUN_00230488`)
and tests Snake (`FUN_002304e8`); 1 films Snake (`FUN_00230600`, aimed at
`DAT_001ece00`, no 0x60000 test). `FUN_002304e8`: Snake on the upper level
(y -15000..-12000, z -15000..-12000; x -7000..0 for camera 0, the first
`-c` entry at x +5250; 0..7000 for camera 1) sets crowd flag (+0x8c878)
0x800 (camera 0) / 0x400 (camera 1), runs `FUN_002303d0` (a "!" mark,
`FUN_0029ca50` kinds 0 and 0x4000, at the head's matrix applied to
(0, 333, 0), w04c 0x1f3900; kind 0 also counts toward the times spotted),
sets +0xcc = 1 and `DAT_0017f48c` bit 0. The crowd's update
(`FUN_00228c30` tail) runs event 6 for 0x400, event 7 for 0x800 and event
0 for `DAT_0017f48c & 1`, each once. In w04c events 6 / 7 (`#29827f` /
`#a98232`) set a stage camera from the camera's side (`chara 0x43f718 -c 1
-p +-5421 -12924 -314`), event 0 (`#2d274b`) is the caught sequence.
+0xd4 counts frames and flips +0xd0 between 0x80000001 / 0x80000002 every
0x800 frames (camera 1 the other way round), the head model's
`FUN_0029bdd8` link (probably its lamp; not ported). Ported: the camera
(0xc6cbc3, turned) and its tripod (0xc0b045, at the `-c` facing),
`crowd::FilmCamera`, the sighting, its events and the "!" (shown 1.5 s:
the mark's lifetime is not decoded, an assumption). Not ported: the
operator's 0x60000 test, the times-spotted count.

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

- the troopers' own brain (radio reports and the rifle butt's push are
  ported: see their sections);
- the game's precomputed shortest-path matrices in the `.hzx` (paths are
  recomputed from the node links instead).

## Olga (w00b): step tables and selectors run

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

Selector structure (leads, 2026-10-25; not ported). `FUN_00267DE0`: when
`FUN_00104950()` is true and stamina +0xfb2 > 0 she just heads for point
0xb (`FUN_00268830(0xb)`, `FUN_0025fcc0`). Otherwise the condition
function at 0x1F8198[+0xf70 & 3] (w00b: 0x2679f0, 0x266448, 0x266348,
0x266348) fills a 0-ended list of step-table candidates and returns 0
(take the first as is) or 1 (choose): candidates found in the 8-entry
history +0xf88 (ring index +0xfa8) score by age and the oldest such is
the fallback; any candidate not in the history makes a bit of a mask, a
random set bit of which is picked (`DAT_0019227c = x * 0x5d588b65 + 1`,
ANDed until zero, then its lowest bit) and written to the history.
- `FUN_002679F0` (mode 0, choosing where to go): sets +0xf70 | 0x201
  (or 0x202 on 3 of 4 random draws while +0xf70 & 0xc < 8), clears
  +0xfaa / +0xfa9 / +0xf7c, draws a cover wish (2 random bits; 1 bit while
  +0xf7e != 0; | 2 under +0xf74 & 0x70) and asks `FUN_002673b0` for cover.
  Found: the destination +0x1030 within 100 mm on x or z of her -> table
  0x1F7DA8, else 0x1F6368 (returned as is). Not found: +0xf74 & 0x8000 as
  found; & 0x20000 -> 0x1F7EE8; else by +0xf70 0x8400, 0x200000, 0x80000,
  the two cover objects +0x1074 / +0x1084 (their +0x6a state 0 / 1 / 2),
  +0x1066 >> 12 (< 6) and +0xf7e: 0x1F7E60, 0x1F7CB0, 0x1F7DE8, 0x1F7E30,
  0x1F7E48, 0x1F7DA8 / 0x1F7DC8 (by +0xf70 & 0xc < 5), the difficulty's
  low bit (DAT_001e6924) in one branch. +0xfae = the cover wish.
- `FUN_00266448` (mode 1, attacking): +0xf70 | 0x40, +0xfaf counts picks;
  no rounds (+0xfb6 < 1) -> 0x1F68E8[+0xfae] (reloads); at one of eleven
  fixed points (0x1F5D50..0x1F6040) or +0xf7a & 0x8000 -> 0x1F6368; then
  by +0x108c (>= 20), +0xf70 0xc0000 / 0x1000 / 0x800 / 0x400, Snake's
  zone fields +0x1098 (& 0x3f > 2, & 0x8000) / +0x109a (< 4), the frame's
  player flags DAT_001ecd40 & 0x801 (crouch-free / aiming), rounds
  (> 3, < 10) and +0xfaf (11 -> 0x1F68F8, 12 -> 0x1F6368 and
  +0xf70 | 0x80000): shoot tables 0x1F7A20..0x1F7AC8, 0x1F7B00..0x1F7BD8,
  0x1F7210 / 0x1F7470 / 0x1F7690, pairs from 0x1F6810 / 0x1F7360 /
  0x1F77E0, 0x1F7C80 (rest of the function, past 0x2671d8, not read yet).
- `FUN_00266348` (modes 2 / 3): toggles +0xf70 bits 0 / 1 (| 0x40),
  then 0x1F6E70 (+0xf70 & 0x400 with +0xfae 0) or 0x1F6EA0 / 0x1F6EE0
  (by 0x1000 / 0x800) [+0xfae, + 4 under 0x400000]; runs her `-h`-style
  proc +0xeb0 if set (`FUN_00152ca0`).
Her per-frame sense, `FUN_00269028` (w00b dump line ~151159), writes
most of them: stamina bands -> +0xf70 0x300000 / 0x200000 / 0x100000 and
+0xfac (< 0x16 / 0x26 / 0x46); +0xf70 & 0xc from +0x1070 (counts to 8 on
frames where DAT_001e6924's low byte is 0) and Bs16:8 (> 0x27 / 0x1d);
+0x1066 (a 4-bit stage << 12 and a 12-bit count, `FUN_002685c8`);
+0x1098 (count & 0x3f each 64 frames, 0x4000 past 1, 0x3000 steps,
reset with point 0x15 copied to +0x1000), +0x109a (frames since player
flag 0x80000000000), +0xf7a (point 0xb in line of sight of Snake: 1, or
0x8000 within 1.5 m); +0x109c, Snake's zone: nine side-of-line tests of
point 0x15 (`FUN_00268830` / `FUN_00268748`) against corners at
0x3bb860..0x3bb8fc (bits 1..0x100). The corners (x, z): 0x3bb860..878
obs_box1's (-14000/-13500, -13750/-14750), 0x880..898 obs_box2's
(-13500/-12500, -17600/-18600), 0x8a0..8b8 obs_irs' (-17600/-16400,
-15450/-18550: narrower than its model), 0x8c0..8d8 a turned crate
(-11303,-11867) (-10932,-12903) (-9897,-12532) (-10267,-11497), 0x8e0
(-10890,-16542), 0x8e8 (-9808,-18954), 0x8f0 (-10753,-18391), 0x8f8
(-9842,-16210): each bit says which side of the line through a cover
corner and a crate corner (cross-product sign) Snake is on, i.e. which
covers the crates' sight lines leave open; then the cover objects
(0x3ca0b0, `FUN_0025c5f8`, from +0xf70 & 0xc0000; rest unread).
Her "points" (`FUN_00268830(n)`): her remembered Snake +0x1000 while
+0x1098 & 0x8000 or +0xf70 & 0x40000 (and +0xf74 == 0), else
`FUN_00268748(n)`: Snake himself: n = 0x15 his position (DAT_001ecd00),
other n his joint n (`*DAT_001ecdf0 + 0xf0 + n * 0x130`; 0xb the head);
crouched (DAT_001ecd40 & 0x10) the joint block's +0xf0 when DAT_001ecd50
is within 550 mm above him (and not aiming under +0xf70 & 0x400000),
else DAT_001ecd50, as also with flag 1.

`FUN_002673b0(olga, wish)` (cover choice): with +0xf74 & ~0x170 == 0,
a mask of allowed cover sides from Snake's zone +0x109c (0 -> 5; 0x20 ->
5, or 0x18 when point 0x15's z > -17400; 0x60 -> 0x30; 0x160 -> 0x32;
0x170 -> 0x1e; 0x1f0 / 0x1f2 -> 0x1c; 0x1fa -> 0x20; 0x1fb -> 0x22;
0x1ff -> 0x2a; else 0x3f), narrowed to & 0x3c under +0xf70 0x200000 or
0x40000 and & 0x30 when her destination is 0x1F5E00. The three cover
objects from +0x107c (0x70 bytes apart) are graded by `FUN_00268d80
(cover, point 0x15, wish)` (2 bits each, packed object 2 first): it sets
the two hiding spots +0x20 / +0x30 at height +0x10 - 850 (550 with wish
bit 2: crouched) and tests each side (`FUN_00268af8` with +-300 mm
offsets) against Snake: bit 1 / 2 = that side hides her. No allowed
side: returns 1 (no cover). Else a random allowed bit picks the object
and side (both sides: a coin flip on bit 0x100 of the next draw): her
destination +0x1030 = spot +0x20 (+0xf70 | 0x1000) or +0x30 (| 0x800);
| 0x400 when the object's +0x64 < 1200 (a low cover?), | 0x8000 when it
is the object at +0x1084 (the one she is at). With other +0xf74 bits
(0x4000, 1, 2, 0x1000, 0x20000, 0x800, 0x200, 0x2000, 8, 4, 0x10a0
...) it picks fixed points instead (0x1F5E70 + 16 x stage, 0x1F5E00,
0x1F5E10, 0x1F6040 / 0x1F6030, 0x1F5E50 / 0x1F5E40, 0x1F5D60 / 70,
0x1F5DE0 / F0 with a z solved against Snake by `FUN_00268af8(+-50,
..)` from 0x1F8178..0x1F8190, 0x1F5E20, cover 3's spot at +0x110,
0x1F5E60 with 0x1F5DA0 / B0, 0x1F5D50). Chosen: +0x1078 = old +0x1074,
+0x1074 = old +0x1084 (the cover she was at), +0x1084 = the new cover
(0 for a fixed point), +0x108c = +0x1060 = 0, the destination clamped
(`FUN_00268a88`), returns 0; no allowed side returns 1.
`FUN_002679F0` then: 1 (no cover) -> 0x1F7DA8 if she is within 100 mm
of her destination on x or z, else 0x1F6368 as is; 0 -> by +0xf74 0x8000
(as 1), 0x20000 (0x1F7EE8), the same cover again (+0xf70 0x8000: low,
| 0x400 -> 0x1F7CB0; high with a crouched wish -> 0x1F7E60), stamina
under 0x26 (+0xf70 0x200000) -> 0x1F7DA8, a move between covers 0 <-> 1
or from 2 (by the old and new covers' +0x6a) -> 0x1F7E30 (+0xf7e 0) /
0x1F7E48, plus 0x1F7DE8 from stage (+0x1066 >> 12) 6, else 0x1F7DA8 /
0x1F7DC8 by +0xf70 & 0xc (< 5), or 0x1F7CB0 from stage 6 on even
difficulty.
The cover objects (`FUN_00259f30`, w00b dump line ~142350): the script
gives all three `-p 0 0 0` and `-r 0 0 0` (w00b.gcl line 977), so each
one's place is its model's own: per `-m` model (`FUN_001287f0(.., 0x11)`)
+0x60 = the object; `-r` turns (+0x68, 10-bit, the model rotated by it);
`-p` sets the object's position (+0x30 of the model object); then from
the model header (+0x44: box min +0x1c..+0x24, max +0x28..+0x30, offset
+0x34..+0x3c) half-extents +0x50 / +0x54 / +0x58, +0x64 = twice the
half-height (the "under 1200 mm" test of `FUN_002673b0` is a low cover),
+0x6a = its index (0 obs_box1, 1 obs_box2, 2 obs_irs), +0x68 += 0xc00,
centre +0x40 = the box's middle + offset; corners (x, z) +0x0 (cx + hx,
cz - hz), +0x8 (cx + hx, cz + hz), +0x10 (cx - hx, cz - hz), +0x18
(cx - hx, cz + hz); spots +0x20 / +0x30 start at (cx - hx - 850, cy).
`FUN_00268d80` then puts both spots' x at the -x edge less 850 (550
crouched) and asks `FUN_00268af8(+-300, x, corner, corner, Snake)` for
each spot's z (+0x28 from corners +0x0 / +0x10 with +300, +0x38 from
+0x8 / +0x18 with -300), 0 when no z hides her. `FUN_00268af8(off, x, A,
B, snake)`: the rays from Snake through corners A and B (x, z) meet the
line at x; each crossing's z is shifted by `off` along the ray (divided
by the ray's x component), and the nearer one to the cover's middle is
kept (min for off > 0, max otherwise); a z beyond the deck (< -19150, or
> (x + 12000) / 4 - 12850, the slanted rail) gives 0; when x is within
551 mm of A's x the corner's own z wins if it is further out.
`FUN_00268a88` clamps the spots into the same deck bounds. So: she always takes cover
on a box's -x side, the side away from the deck's +x (where Snake
comes from). So `FUN_002679F0`'s tests of
+0x1074 / +0x1084 `+0x6a` are "which cover she was / is at".
The obs_* models' boxes (kms header): obs_box1 x -14000..-13500, y
12750..15000, z -14750..-13750; obs_box2 x -13500..-12500, y ..13750,
z -18600..-17600 (1000 tall: the low cover); obs_irs x -18095..-15905,
y ..14380, z -18550..-15450.
Ported (2026-10-25): the cover objects drawn and walled (`main::
obstacles`, `olga::Cover`), Snake's zone (`olga::zone`, the table read
from the image), the spots (`Cover::spots`, `spot_z`, `on_deck`) and
`FUN_002673b0`'s plain path (`cover_choice`) for where her moves go;
not ported: the +0xf74 orders' fixed points, her remembered Snake
(+0x1000), the 0x40000 / 0x1F5E00 narrowing, and the selectors that
choose the tables. Still to read: the rest of `FUN_00269028` and of
`FUN_00266448`.

### The selectors ported (2026-10-25, `olga::select`)

Decompiled with headless Ghidra: every step function the selectors' 115
tables use (`re/ghidra-w00b/olga_steps.c`, git-ignored; 104 functions
the dump had missed). Ported: `FUN_00267DE0` exactly (history ring
+0xf88 / +0xfa8: a candidate found in it scores its age
`(slot - ring) & 7`, the smallest, i.e. oldest, is the fallback; the
others form a mask narrowed by `DAT_0019227c` draws), the three
selectors and the steps of every table they reach in the port.
- Modes (+0xf70 & 3): 0 `FUN_002679F0` -> sets 0x201 (attack next) or,
  on 3 of 4 draws while +0xf70 & 0xc < 8, 0x202 (looks next); 2 / 3
  `FUN_00266348`: bit 0 clear -> (& ~2) | 0x41 (attack next), set -> & ~3
  (move next); 1 `FUN_00266448`: `& ~1` (move next) on several paths,
  and every fifth pick (+0xfaf) 3 of 4 draws `& ~1`, else `| 2` (mode
  3, the looks then a move) unless +0xf7c > 0 or +0xf7e != 0 (the
  inner draw only under +0xf70 & 0xc < 4; from 4, not under 0x400000).
  Mode 1 also ends: +0xf78 = 0, +0xf7a &= 0x7fff.
- Mode 1's main path (no orders, Snake not remembered): no rounds ->
  0x1F68E8[+0xfae] (reload: `FUN_00262A78` / `FUN_00262B00`, both
  motion 0xc) taken as is; at a fixed point, +0xf7a & 0x8000 or +0x108c
  >= 20 -> 0x1F6368 taken, mode 0; under +0xf70 & 0xc >= 4 with +0x109a
  >= 4 -> (& ~1) | 0x20000. Stance +0xfae bit 1 clear and +0xf7e == 0:
  i = (+0xf70 & 0xc) >> 2; low cover (0x400): 0x1F7C80 when rounds > 3,
  0x1F6810[2i, 2i+1], then the side's table (0x1000 -> 0x1F77E0, 0x800
  -> 0x1F7360) [2i, 2i+1] and [2i+8, 2i+9]; high cover: the side's
  [2i, 2i+1], + 8 when +0xfae bit 0. +0xf7e != 0: 0x1F7920 / 0x1F79A0 /
  0x1F7960 / 0x1F79E0 (difficulty < 0x1f, +0xf7e < 2, | 0x20000) or
  0x1F7820 / 0x1F78A0 / 0x1F7860 / 0x1F78E0, then +0xf7e - 1. Stance bit
  1 set: & ~1, 0x1F7B00 / 0x1F7B90 / 0x1F7B48 / 0x1F7BD8 (crouched).
- Mode 2 / 3 tables: 0x1F6EA0 (side 0x1000) / 0x1F6EE0 (0x800)
  [+0xfae + 4 under 0x400000], or 0x1F6E70 at a low cover standing.
- `FUN_002673b0` clears +0xf70 0x9c00 before setting the side, 0x400
  (cover +0x64 < 1200) and 0x8000 (the same cover as +0x1084).
- Settle `FUN_00265CA0`: the idle motion is +0xfae itself (0..3); at a
  low cover +0xfae = (& ~2) | 1; `FUN_0025fe90(0, 0x20250)` (mirror and
  layer 1 off); no rounds -> (& 0xbffffe7d) | 1 (mode 1, which
  reloads); crouched (+0xfae bit 1) she turns to the cover's +0x68 (all
  0xc00) instead of Snake. `FUN_00265E38` (table 0x1F6368) first adds a
  miss to +0xf7e.
- Firing: the steps set +0xfcc = 1.0 (her gun, `FUN_0026ddd8` with
  +0xfb6, fires a round); 261878 (motion 0xf), 261A78 (layer-1 0x1b),
  261710 / 262F70 (layer-1 0xf), 263FA8 (0x18, at the head); refused
  (+0xf7e + 1) without rounds, under 0x80000, or (lean shots) with
  +0xf7a & 0x8000. `FUN_00264088` loops back 8 bytes to the shot while
  she has rounds and +0xf7e < 10 (Ghidra's decompile showed the jump
  overwritten; the disassembly at 0x2640b8 returns after it).
- `FUN_00327bc0(ctl, layer, motion, start, joint mask, blend)`: layer 0
  0x1fffff the whole body, layer 1 0x1ffe / 0x7f8 the upper body / arms;
  motion -1 on layer 1 clears it; the motion's end is ctl+4 -> +0x20 & 4.
  Steps that add 0x300 to the model flags (ctl +0x10) play the motion
  mirrored: see "Mirrored motions" below.
- Sense (`FUN_00269028`): `DAT_001e6924` is the frame clock (the notes
  above called its low bit the difficulty: wrong; it is a frame parity).
  +0x1070 counts 256-frame periods up to 8; +0xf70 & 0xc: < 2 -> 0xc,
  < 4 or difficulty > 0x27 -> 8, < 6 or difficulty > 0x1d -> 4. +0x109a
  counts 256-frame periods since Snake's flag 0x80000000000 (set by
  several of his action handlers, `FUN_00272cf8`, `FUN_0027a420`,
  `FUN_0027be10`, `FUN_0027cf10`: which action is open; the port never
  sets it, so after ~17 s of the fight on Normal she leaves cover more).
  `FUN_00268880`: with Snake's flags 0x800 (aiming) and 1 (not known;
  the port uses aiming alone), within 1400 mm of his line -> 0x400000;
  leaning out (+0xf70 & 0xc0080 == 0x80) +0xf78 = the mask of her zones
  the line meets. Sight check under 0x100 (set by the lean / run steps):
  `FUN_0014f230` from her head (+0x1c0 + 0x810) to point 0xb: clear ->
  +0xf7a 1, blocked within 1.5 m (the hit point to her head) -> 0x8000.
  +0x108c (attacking at a cover, +0xf70 & 0x40): +1 a frame while she is
  exposed (`FUN_00268cf0`, the spot z for each side with +-300: exposed
  when +0x1030's z <= zA - 60, or (A clear) >= zB + 60, or neither side
  hides), reset otherwise and under 0x80200 or Snake's hit flag 0x200.
  +0xf70 0x80000 comes only from the orders (+0xf74 & 8, sense line
  ~151470), 0x40000 from +0x1098 & 0x4000.
- +0x1098 (her memory of Snake) is written by her message handler
  `FUN_0026ae58`: message 0 clears 0x8000, message 1 sets it (and the
  value << 6; copies Snake's place to +0x1000 when & 0x3000 == 0x3000).
  Who sends them is not traced (a lead: her sight, or the script).
- Not ported: the orders +0xf74 (the rest of the sense, from line
  ~151600: fixed points 0x1F5D50.., the stage count +0x1066 and
  `FUN_002685c8`, +0x10a0), the remembered Snake, `-h` procs the
  selectors and steps run (+0xe98.. +0xeec: voices / script), the
  upper-body layer (the recoil is a wait of its length; the arms' idle
  overlay 0x7f8 is not drawn), the voice ids steps put in +0x1064 /
  +0x1068. Assumed: the crouched walk (8) and the aiming run (0x10)
  move at the walk's / run's speed; the side step at the walk's.

### Her taunts and the fight's camera events (ported 2026-10-25, `olga::say`)

- Her init reads `-h` into +0xe90 (25 procs: +0xe90 + 4n = h[n]; h[1]
  +0xe94 the defeat) and `-v` into +0xef4 (6 VOX ids).
- +0x1066 is her taunt progress: the high nibble the line reached, the
  low 12 bits a count (+1 every 64 frames while the nibble < 6 and no
  voice plays, +0xf0c == 0). With a count and the nibble not 0 or 5,
  and her motion not 0x28..0x2f, `FUN_002685c8` plays `-v`[nibble]
  (`FUN_001183f8`, handle in +0xf0c) and the nibble goes up. At nibble
  0 or 5 with the count >= 8 the sense gives the order +0xf74 0x4000:
  she goes to point 0x1F5E70 + 16 x nibble, and mode 1 there runs
  h[18] (+0xed8, `#2cba9c`: the camera event `#de7ce7` sweeping to
  her, a `#9c5a50` camera) with table 0x1F80E8 (nibble 0), or h[20]
  (+0xee0, `#ecba91`) with 0x1F8110 (nibble 5). So the selectors'
  "stage >= 6" branches are "all her lines said".
- Other procs the steps / selectors call: h[2] (+0xe98, `#71f807`, sets
  Abit:0x2a.1) from `FUN_00263110` / `FUN_00263690` and mode 1's
  0x80000 path, h[3] (+0xe9c, empty) from `FUN_00263EF8`, h[6] (+0xea8,
  `#563a2a`: a camera event by Au32:0xa5c) from `FUN_002611A0`, h[8]
  (+0xeb0, empty) from mode 2 / 3, h[11] (+0xebc) / h[12] (+0xec0) /
  h[22] (+0xee8) from mode 1's 0x80000 path and the message handler.
- Ported: the count and lines 1-4 in the sense, the order 0x4000 (the
  sense sends her back through her init when her destination is not the
  line's point; `FUN_002673b0`'s order branch picks the point, +0xfae 0
  or 2 from line 3), mode 1's event (h[18] / h[20] and the table taken
  as is), the event steps (`FUN_00265890` / `265A50` say the line,
  `265998` / `265980` / `265A98` wait for its end then `FUN_002658c0`:
  +0xf74 = 0, the proc h[19] / h[21], table 0x1F6368, the next line),
  `265C20`, `265728` / `2657B0`. The lines play as spoken lines with
  their subtitles; "over" = the stage no longer plays it. Run (w00b,
  Normal): line 0 at ~9 s, lines 1-4 by ~32 s, line 5's event at ~43 s.
  The procs run, but their camera event (`#de7ce7`, the `#43f718`
  cameras) does not move the port's camera yet; the skip by button
  (`DAT_0019165e & 0x840` under +0xf70 0x4000000) is not ported.
- `FUN_00104950` (the "busy" test at the top of the selector and the
  sense: she heads for point 0xb) is true while `DAT_0017f48c |
  DAT_0017f490` is negative, `DAT_0017f4e4` set or `DAT_0017f4e0` set
  (a demo / pause state; not traced).

### Mirrored motions (`FUN_003b2f58`, model flags 0x300)

The engine mirrors a posed skeleton before composing it: 0x100 the
upper body, 0x200 the root and legs (and the root's x translation
negated, `FUN_003af528`). Every listed joint's rotation quaternion has
x and w negated (`vsub.xw vf1, vf0*0, vf1` at 0x3b3058: the reflection
in the model's x = 0 plane), then pairs swap rotations. By model flags
& 0xf0000: 0x10000 (16 joints) masks 0x3fe / 0xfc01, pairs at
0x1fc070 (7,2) (8,3) (9,4) and 0x1fc080 (13,10) (14,11) (15,12);
0x20000 (21 joints: soldiers, Olga) masks 0x1ffe / 0x1fe001, pairs at
0x1fc090 (7,3) (8,4) (9,5) (10,6) and 0x1fc0a0 (17,13) (18,14) (19,15)
(20,16); 0xf0000 (33 joints) not read. Ported as
`anim::pose_mirrored` / `Body::mirror` (Olga's lean-outs; the Deck-2
defenders' mirrored actions 0xb / 0xc / 0x18 could use it now).

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

Each soldier's own check (`FUN_0021e7a8`, jump table w00a 0x3f0250 by the
level 3..11; the noise block is 0x3fe730: level +0x70, area bytes +0x78..
+0x7a, +0x74 a distance) returns the level he heard, or 0 (all of it off
under body +0xf2c bit 0x200):
- 3: `FUN_002541c8` (Snake covering him) and `FUN_00252e28`;
- 5: the same area (+0x138 / +0x13a / +0x13c against +0x78..+0x7a) and
  +0x74 within a quarter of his range (+0x24c, or 1000 under `-s` bit 0x40);
- 6, 9: the same area only;
- 8, 10: the straight distance to the noise (DAT_001ccd30, `FUN_0022de78`)
  within his range, doubled for 10; under body +0xf2c bit 0x200000 (not
  traced) also within 3000 mm up or down;
- 4, 7, 11: heard as is.
`FUN_00230830` turns 5 / 6 / 8 / 9 into stimulus 1, 10 into 0x800, 11 into
0x1000. Ported: level 10 (the USP; the port's 10 m was an assumption) and
level 8 (a soldier's "!", emitted at his place as the "!" begins: the
others within their range look into it as normal state 3, and say
0xc040020 "What's going on!?" when they hear one again while at it, the
port's rule for the second noise). Not ported: 5 / 6 / 9 by this check
(the port keeps the manager's nearest-soldier pick, `FUN_0021f958`).

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
| 0x2000 | the USP's light (`FUN_00230b40`, below) | 6 |
| 0x80 | `FUN_00230ad0`: `FUN_0021eaa8` -> `FUN_002224f0` sees an entry of the list at DAT_003fefa8 within his near range (+0x290 = its id; with +0x350 bit 0x8000000 also 0x2000000). Entries register through `FUN_00222410(actor + 0x2b0)` from `corps.c` (`FUN_002635f8`, `FUN_00263d90`): bodies (below, "Bodies"); ported | 4 |
| 0x2000000 | a comrade down or 0x80, under +0x350 bit 0x8000000 (never in the Tanker: `-s` is 34 / 10 / 2) | 0xe |
| 0x4000, 0x20000, 0x40000, 0x200000, 0x400000 | 0x230dd8, 0x2311f0, 0x2316b0, 0x230c20, 0x231418 | 6, 0xb, -, 7, 0xc |
| (0x400000) | `FUN_00231418` needs Snake's equipped item (`FUN_001541f8`) to be 6, the B.D.U (Raiden's disguise; item names at ELF 0x19209c), and a story flag (+0x10c bit 0x40): the Plant's, never in the Tanker | 0xc |

The USP's light (0x2000, `FUN_00230b40`): `FUN_0033e890` returns
DAT_001f857c, which the weapon manager (`wp_mng.c`, `FUN_0033ca68`) sets
while Snake aims (DAT_001ecd40 & 0x800) weapon 1 or 9 of its own numbering
(+0x258: 1 the USP, 2 the SOCOM, 5 the AKS, by the suppressors it pairs
with them: items 0x23 USP.SUPR, 0x1d SCM.SUPR, 0x1e AK.SUPR, item names at
ELF 0x19209c; the scripts' weapon numbers differ: 1 M9, 2 USP) when
DAT_00191a14 == 1, making its light (`FUN_00399d20`). DAT_00191a14 is
cleared at every stage start (`FUN_00154218`) and set only by resident
function `#f6ada8` (w00a 0x33e880), which `command #f6ada8` calls
(`command` = `#082bc9`, `FUN_0010a848`: a resident function by name);
only w03a / w03b's proc `#c4bdfe` (from main's map setup) does, so the
light exists on Deck 2 alone. Within 3 x his near range (+0x188 < 3 x +0x248), a
soldier notices it when, within 2 x, Snake faces him within 0x200
(`FUN_0022df10(bearing + 0x800, Snake +0x22)`), or else when Snake is
within his view (`FUN_0022df10(+0x240, +0x18c) < +0x244`), with a clear
line (`FUN_00252e28` -> `FUN_0014f230`). State 6's init (`FUN_0023c568`)
then targets Snake's place (`FUN_002571d8`) rather than the sight record.
Ported (the port's light is the USP's while aimed).

Level 8 is the soldier's own "!" call (`FUN_00237128`), so nearby soldiers
hear a discovery. Marker groups register through `FUN_00117798`.

Footprints (`foot_print.c`): leaving a type 0 / 2 named area (`0x839ecc` /
`0x97cb18`; w01a, w01b, w01e have `0x839ecc` at the doors in from the
rain) leaves Snake wet for 3000 frames. Blood (`blood_dp.c` -> `blood_sp.c`):
a hit that leaves him under a third of his life sets 0x1000000 and a drop
falls every 20 frames until life is a third again or a ration is used. A
soldier who sees either marker group goes to the newest marker in view and
marks the older ones (`FUN_00240858`); the port follows that way.

## Bodies (`corps.c`, stimulus 0x80, normal state 4)

What prevents most stimuli: `FUN_002316f8` (each frame, +0x268 = 0)
skips the hearing `FUN_00230830` and every check below it except
`FUN_002316b0` (stimulus 0x40000, the script's) while body flag
0x100000000000 is set (action 0x16's posts: ported as `Guard::still`);
in an alert (Bs16:0x112 == 1) it runs `FUN_00231380` instead; the
comrade (0x40) and body (0x80) checks also need body +0xf2c bit 0x1000
clear.

The corpse: a soldier's death step `FUN_0024e4f8` (motion 0xd2 / 0x80 /
0x72 by +0xf5c), from its frame 16, makes a `corps.c` actor
(`FUN_00263b00`, actor type 6) when more than 1 MB is free and he is not
parked far away (x < 1e6): it takes over his model (`FUN_003998b0`) and
registers a list entry at corpse +0x2b0 (`FUN_002223d0`: [0] its position
block, [1] +0xec, [2] the soldier's id, [3] a serial, [5] flags (4 =
found), [6] the corpse body at +0x1860; `FUN_00222410` appends it to
DAT_003fefa8). `**(soldier + 0xfa8)` (controller +0x350, the `-s`
flags: `FUN_0025ad08` sets +0xfa8 = controller + 0x350) goes to corpse
+0x1bb0 (`| 0x200`), not to the life. The life +0x2c0 is 18000: in
`FUN_00263d90` (the other constructor, `FUN_00263f88`) as a constant, and
in `FUN_002635f8` its 12th argument, on the stack, which the decomp does
not show: `FUN_00263b00` stores `addiu t4, zero, 18000` at 24(sp) before
the call (0x263b90 / 0x263be0; 0(sp) the soldier's model word, 8 / 16
its caller's stack words: the soldier, +0x1008 != 0). `FUN_00262c18` takes 0x80 a frame in an alert
(Bs16:0x112 == 1) or while `FUN_00262be0` is true, else 1.
`FUN_00262be0` is `FUN_001079b0(corpse + 0x40, 0x1f3d80, 1, 0)`: not a
view test but whether the object's name list (+0xf8, count +0xf4, filled
by `FUN_00107b10` -> `FUN_0010eda8`) holds the 24-bit word at 0x1f3d80,
0xccfd9f (w00a resident offset 0x2900): the trap name of boxes 1-2 m
below the deck in w00a and w00c (`routes <hzx> traps`), i.e. overboard.
So a body on deck lasts 18000 frames (5 min), one dropped over the side
or in an alert 141 frames. At 0 `FUN_00262688` starts its vanishing
(+0x2c4 bit 2), and bit 0x100 later deletes it and its entry
(`FUN_00262cc8`, `FUN_002224a8`). Ported (`Guard::corpse`,
`corpse_tick`; the body is hidden at the end of the fade's 61 frames,
whose colour change is not drawn).

The finder (`FUN_002224f0`): the first entry he sees at level 3
(`FUN_00255f30` with both area masks) within his near range +0x248 (or
any under +0x350 bit 0x8000000 unless entry flag 0x400) -> +0x290 = its
id, stimulus 0x80 (checked after 0x40 in `FUN_00227520`, so a comrade
down wins). State 4 (`FUN_002358b0`, init `FUN_00235a38`: actions table
`FUN_00234818`, target the corpse's position, entry flag 4, +0x294 =
`corpse body + 0x170`, pace 2, sub 1). Sub-states (+0x330, timer +0x338):

| Sub | Function | What |
|---|---|---|
| 1 | `FUN_00235418` | faces the body; frame 6 a "!" mark (`FUN_00253f50(.., 0xc, 0x8000)`) and voice 0x14c; frame 0x78: to sub 10 (`FUN_00234f88(cb 0x235388, 350 mm, else 0xb)`), or under +0x350 bit 0x20 with +0xf4c +0xc == 1 sub 0 |
| 0 | `FUN_002354e8` | action 6 until its motion ends, then as sub 1's end |
| 10 / 0xb | `FUN_00234fc0` / `FUN_00235120` | walk to the body (re-targeted each frame, `FUN_002357e0`) until within 350 mm, then the callback; a door in the way (`FUN_00119528`, flags 0x4100000000) -> 0xc..0xe (turn to it, action 8 up to 0x12d frames, action 9 and `FUN_001196f8(door, 100, 5, 0)`) |
| cb `FUN_00235388` | | another soldier on the same body within 2 m (`FUN_00235338`, `FUN_0021eb20(.., 0x80)`): if a soldier within his `-h` (`FUN_00252820`) look about (6), else report (line 0x1c00000a if `FUN_002527d0` else 0x1c000009, mask 0x80); else sub 2 |
| 2 | `FUN_00235558` | entry +0x10 = 0, action 7 (looks him over) to its end; in Evasion done (0xf), else as the callback (6 or the report) |
| 6 | `FUN_00235650` | looks about 240 frames (head to the area's look points every 60, `FUN_00253dd8` / `FUN_00253e30`), then done if a report is in hand (`FUN_00252708`), else as above |
| 4 / 3 / 5 / 7 / 8 / 9 | `FUN_002349c0` -> | the report as the comrade's (radio out, the line, `FUN_0021ee90(-h)` caution timer at its end; dead radio 5 -> run 7 -> in person 8 (line 0x18a00005); `FUN_00252708` already reported: 9, a "?" 0x79 frames) |
| 0xf | `FUN_002357d0` | done: `FUN_00227a50` returns him to his route once +0x26c bit 0x80 is clear |

Nothing in `FUN_002224f0` skips a body already found (entry flag 4 is
only read under +0x350 bit 0x8000000), but sub 2's first frame
(`FUN_00235558`) writes 0 to its entry's +0x10, which (the entry being
at corpse +0x2b0) is the corpse's life +0x2c0: the kick ends the body.
At life 0 `FUN_00262688` sets +0x1b88 = 4 and `FUN_002627e8` fades it
for 61 frames (+0x2a0 / +0x2a4 down 1, +0x2a8 down 2 a frame: its
colour), still listed, then removes the entry, frees the corpse and,
with over 1 MB free, calls `FUN_002644c8(+0x1c28, ..)` (something made
from the soldier's spawn data: not traced, not ported). Ported as `State::Body` / `corpse_found` (subs 1,
10, 2, 6, 4, 3, 5, 7, 8, 9, 0xf); a dead soldier is no longer a comrade
down (state 2's sub 0x17 is left unported). Not ported: sub 0 (action
6 under `-s` bit 0x20 with +0xf4c +0xc == 1), the walker 0xb and doors
(0xc-0xe), the area look points in 6. The report's follow-ups (HQ's
reply, then his 0xc00000e) play through `Phase::follow_up`. Tests:
`body_found`, `body_alert` (regress).

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
The hit box (`FUN_002662d0`): `FUN_00107da0(+0x160, 0x2000011, 1, 2,
(50, 100, 50), (0, -50, 0))`, the y offset (+0x1c4) -50 on the belt and
+50 in a hand (`FUN_00265fe0`). Damage 0x800000004801e includes the
darts' bit 2, so a dart breaks it too. Shot, the radio is no longer drawn
(+0x164 bit 0x80000) and `FUN_00265ea0` re-makes its collision as kind
0x11000000 (not traced). The flag 0x10000 is what every radio call checks
(see "A dead radio"). Ported: the box taken as half extents along the
world axes, judged against a 150 mm torso core for which is struck first
(the port's body cylinder is 300 mm and holds the radio); the effect
`FUN_002b9ca8` is not ported.
The hand flags come from the radio motions (body action functions):
`FUN_0025ebc0` (action 4, motion 0x23 radio out; 0x31 with the left arm
wounded, +0xf16 & 3 == 2) sets 0x2 (0x4 for the other motion) at frame 28
(7 with `DAT_0017f48c` bit 1) and signals +0xf58 = 3 at the motion's end;
`FUN_0025eda0` loops 0x1b (0x32) while he talks, ending after 600 frames
or when action 5 sets its counter +0xfb4 to 600; `FUN_0025f068` puts it
away (0x1c / 0x33, flag cleared at frame 62). Ported (left hand only).

## A comrade down (normal state 2, `FUN_00233120`)

Stimulus 0x40 (`FUN_00230a48`): `FUN_0021e998` finds a soldier in the
manager's list at +0x7c (count +0xdc) whom he sees at level 3 within his
near range. The list is filled each frame by `FUN_00220120` in code the
decomp leaves out (w00a 0x2201c0.., read with `re/tools/mips_res.py`):
for every squad member, +0xde counts those without body flag 0x40000000
(able), and while +0xdc < 24 a soldier joins when body +0xf98 meets
0x400080000401C or controller +0x348 has bit 0x100; one seeing Snake
(+0x18e == 3) sets bit 8 at 0x17f48c. Bit 0x100 is set wherever a report
takes the run to the report point (`FUN_00231c28`, `FUN_002349c0`, the
discovery's, the check-in's ...) and cleared at its end. The port lists
the soldiers lying down and the runners (the other body flags, 0x4 /
0x8 / 0x10 / 0x800000000, are not traced). `FUN_00233330` targets him (+0x2f4)
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
| 0 | `FUN_002321b8` | (a comrade not down, or running: `FUN_00233330` starts here) "!" (0x8000), faces him, voice 0x14c; frame 16: a runner still flagged hands over (his 0x348 bit 0x100 cleared, `FUN_00231c28(0x1c00000b, 0x40)` for him), else Snake within 1.5 x near range (+0x188 < +0x248 * 1.5) -> 2, else 1 |
| 1 / 3 | `FUN_002322c0` / `FUN_00232318` | action 1 (motion 6, binoculars out) toward Snake (+0x18c), then action 2 (motion 7) 90 frames -> 4 |
| 2 | `FUN_00232358` | faces Snake 90 frames -> 4 |
| 4 | `FUN_00232398` | "?" (0x8001); frame 0x5a: Evasion done; asleep (+0xf98 0x4000) or both arms hit: 9 if a comrade on the radio is within earshot (`FUN_00252820`), else the report; else 8 |
| 9 | `FUN_00232488` | looks about 240 frames (look points), then done with a report in hand, else 9 / the report |
| 0x18 | `FUN_00232ee0` | can't see him from beside him: looks about, "?" kind 5 at 200 and line 0x144, done at 0x141 |

The report's entry is `FUN_00231c28(line, mask)`; the line's length comes
from table 0x400ca8 (`FUN_0021c608`, filled at load), 0xf0 frames when the
line cannot play. Ported in `guard::comrade` (sub-states 0-9, 0xb-0x10,
0x12, 0x15); assumed: the shake wakes him; the lines now play (see "Radio
lines" below). So only a sleeper's kick leads to a call and Caution; a
knocked-out comrade does not, and the dead are bodies (state 4). The
runner takeover (sub 0 at frame 16) is ported but not yet seen in a
headless run: the Tanker's routes give no witness facing a run (w01d's
report point is beside route 28's deaf post; w02a's run passes behind
route 5). `MGS2_ROUTE` takes a list (`22:43,5:5:1`) for such staging.
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
and 0x20008003 -> an area line (`FUN_0021c940`). Read off the decomp's
branches, 0x14000, 0xc000011 and 0x18a00005 (the in-person report) also
fall through to the single reply. Ported as `Phase::follow_up` /
`queue_tick` (each queued line 0xf0 frames after the one before; the
0x1081400f limit is not traced: the reply is always taken).

Investigation (normal state 6, `FUN_0023c470`): sub 0 (`FUN_0023c028`,
line 0x140, a turn, action 3) goes to 5, to 6 in Evasion (action 4, then
5), or to 1 (flag 0x20 with a noise of kind 1); sub 5 (`FUN_0023c108`)
waits 61 frames, says 0xc04001d "Is there somebody there?" (0xc04001e
"Over there?" in Evasion; no click: bit 0x40000) and walks there (sub 8,
`FUN_0023bcf8(.., 0x23bfd8, 350)`).

Ported: the downed-comrade report (voice set `voice()`, set 0 only, line
0xb) and HQ's first reply, "it was nothing" (0xc000012) when a
comrade comes round, a let-go hold-up's report (0x1c000007), and the
investigation's line (its pause after the
port's assumed 30-frame turn) when he saw something (state 6); noises
(state 3) bring the same turn, pause and walk without a line (its first
pass: +0x286 is still 0); they play through the spoken-line player with their
subtitles (assumed shown, as the clips carry them), unplaced. Not ported:
the priority rule, the other lines' callers, the 0xe0 sets (what sets body
flag 0x4000 is not traced), the voice index (+0x284; the port's
`route % 4` is assumed).


Leads for the other radio lines (not ported):
- Normal state 3 (`FUN_00234410`, entered by `FUN_002345b0` on stimulus
  0x1, a heard level 5 / 6 / 8 / 9): sub 2 (`FUN_00233b20`) waits 61
  frames and, on his first hearing (+0x286 == 1) when his group has two or
  more soldiers (`FUN_0021d530`: count at `FUN_0021f330(+0x138, +0x13a)`
  +4), says 0xc040013 "What was that sound?" (0xc040014 in Evasion unless
  +0x348 & 0x80), or 0xc040020 "What's going on!?" when the noise was
  another soldier's "!" (level 8, +0x340). +0x286 is cleared at entry
  (`FUN_002345b0`) and counts noises heard again (`FUN_00234158` in subs
  5 / 9, `FUN_00234320` in 8, which go to 0xd, `FUN_00233c58`); 0xd goes
  to 6 (and then 2, the line) only when `FUN_001194e0` finds a nav cell
  for the noise and `FUN_00233888` > 0, else 7 / 4. So the line is said
  on a second noise. Ported in outline: a soldier still investigating
  (or searching) who hears another noise says it, given another able
  soldier in the stage (the group count and the 61-frame wait are not
  modelled).
- Discovery (table 0x3f0c20, `re/ghidra/ai_s8_table.c`): sub 0x14
  (`FUN_00237c90`) goes to 0x1e (`FUN_00237d48`) when `FUN_00256910`
  says so; 0x1e clears the radio-shot flag (+0xb4 0x10000), says
  0x1c000006 ("I'm under attack! Stay alert!" in set 0; HQ's reply
  follows), and at its end sets the caution timer and +0x348 |= 0x10000
  (not ported). The call for backup is ported (`guard::call`, see
  "Discovery call" below).
- The alert-phase brains (0x23a348, 0x23d420, 0x23f078, 0x244000,
  0x244e30, 0x3c5fe8) each play a +0x310 line, then 0x18a00005 (set
  line 5, "I saw him! Send someone over now!", with a click at its end:
  bit 0x800000).
- The hold-up let go (ported, `guard::holdup_call`): `FUN_002541c8` is
  "covered": Snake within 0x9c5 mm, aiming (DAT_001ecd40 & 0x800) and
  facing the guard within 0x200 (bearing vs facing over 0x5ff). Uncovered
  for 0x79 frames in sub 4 (`FUN_0023af20`, action 5; covered again ->
  sub 3 with effect 0x50) he goes to 0x11 (`FUN_0023a778`: "!", 9 frames
  facing Snake) and reports `FUN_0023a1e8(0x1c000007, 0x200)` ("The enemy
  is attacking!" in set 0; "We have an enemy onboard." in voice 3): sub 10
  ("?", 121 frames) when the caution timer is over 3/4 or in Evasion, else
  5 (radio out, action 6, until +0xf58 == 3) and 7 (`FUN_0023a348`: the
  line, then action 7 and the caution timer; HQ replies), then 0xe. Subs
  0xc, 0x10 / 0xb, 0x12 and 0x14 reach the same report. Seeing Snake at
  level 3 meanwhile sets +0x274 = 0x12 (the port: discovery). The port
  keeps turning him toward Snake's place through the call (assumed: the
  target heading +0xf54 is not cleared), so a Snake who stays is seen.
- The hold-up (normal state 5, `FUN_0023b580`): sub 3 (`FUN_0023ade8`,
  action 4, up to 100 frames) goes to 0x15 when +0x350 has bit
  0x20000000, `FUN_0023aad8` < 2 and body +0xf16 == 0, with +0x340 < 3
  (else 0x16, else 0xd). Sub 0x15 (`FUN_0023b300`, action 0xf) says
  0xc040034 "Are you going to shoot me?" on its first entry and
  0xc040035 "If you're gonna do it, then do it..." on its third
  (+0x340 counts entries). +0x350 bit 0x20000000 is only ever read: +0x350
  is the soldier's `-s` option (34 / 10 / 2 in w00a / w00c / w02a) and
  script messages set only bits 0x80000000, 0x100 and 1 (`FUN_00257df8`),
  so in the Tanker sub 3 always goes to 0xd and these lines are not said.
- A radio line cut short (`FUN_0021d0a8(soldier)`: called when the
  speaker is interrupted, e.g. from the hold-up's init `FUN_0023b8a8`):
  unless the line had bit 0x8000 or 0x40000 (no click), the manager opens
  a "no answer" slot (`FUN_0021d860(name, place, room, 900, 1)`; 0xe10 and
  kind 2 under +0x350 bit 0x2000000; not with manager `-s` bits 0x600 or
  during Alert). Eight slots at manager +0x17c (name), +0x19c (frames
  left), +0x250 (kind); `FUN_002211d0` counts them down: kind 1 at 600
  says 0x1081400a ("What's going on? Respond!", the 0x1c0 block of `-v`
  is HQ's: 0x1c6 "Why are you late with your status report?", 0x1c8
  "Report your status.", 0x1cd "Hey! You're sleeping?", 0x1ce "Carry out
  an investigation immediately."), at 300 0x914000 (`-y`'s -2 clip, w00a
  "We've lost communication with the aft deck.", followed by 0x1081400e)
  when `FUN_00221098` finds a soldier still able, else 0x914100; kind 2
  first checks at 900 / 600 whether the soldier answers (0x10814006).
  At 1 the slot's bit in +0x178 is set; `FUN_002316b0` then gives every
  other soldier stimulus 0x40000 (its consumer is not found in the
  transition functions). Kinds 4 / 8 / 0x20 come from state 0xe
  (`FUN_00245348` / `FUN_002454a8`, 600 frames). `FUN_0021d0a8` is also
  reached from the soldier's damage handling (`FUN_00252550` /
  `FUN_00252688`, which switch him to brain 4), so a soldier darted or
  knocked down mid-call opens a slot. Ported (kind 1, the port's two
  radio calls): the clip stops, HQ says 0x1ca at 600 and the lost-contact
  clip at 300 (w01d "We've lost contact with Deck-D."); the follow-up
  0x1081400e never comes in the Tanker: `FUN_00221098` looks for a soldier
  with `FUN_002527d0` (body +0xf28 & 0x2002200000000 == 0x200000000),
  never true here (see "Discovery call"), so HQ always takes 0x914100;
  not ported: the end click (bit 0x800000), the slot's
  end (stimulus 0x40000), kinds 2 / 4 / 8 / 0x20.
- State 10 (footprints and blood, `FUN_00240670`, init `FUN_00240858`)
  reports only from sub 0x12 (`FUN_0023fff8`, reached from sub 5
  `FUN_0023f970` when the marker's flags, copied to +0x310, have bit 8):
  `FUN_0023ef18(0x10000012, 0x10000)`, line 0x12 of his voice ("The
  situation is normal." in set 0; the 0x10000 is the mask, not a line
  bit), with the same radio / dead-radio / in-person subs (10, 0xc, 0xb,
  0xd, 0xe = `FUN_0023f340`: 0x18a00005). But the markers' flags never
  have bit 8 (or 0x20, its other branch): footprints are registered with
  0x100 (`foot_print.c` `FUN_0032ca60` -> `FUN_001179e0(.., 0x100)`), blood
  with 0x400 / 0x800 by Snake's state bits (`blood_sp.c` `FUN_00314dd0`).
  So in the Tanker that report cannot happen; not ported.

## Discovery call (`guard::call`)

The "!" does not raise the alert by itself: in the discovery table
(0x3f0c20) the meter goes to 0x400 (`FUN_00236f30`, which the manager
turns into Alert) only when a radio call ends, or straight after a line
0x20008003 ("Encountered the enemy, requesting backup.", no radio) on
the paths gated by controller +0x350 bit 0x10000 or body flag
0x200000000 (`FUN_002527d0`: body +0xf28 & 0x2002200000000 ==
0x200000000). Neither applies in the Tanker: +0x350 is the soldier's
`-s` option (`FUN_002233e8`: `FUN_00152a30('s')` -> guard+0x2820), which
is 34 / 10 / 2 in w00a / w00c / w02a and absent elsewhere; +0xf2c bit 2
(= +0xf28 bit 0x200000000) is never stored (a scan of the w00a resident
and the ELF for stores to +0xf28 / +0xf2c finds only `FUN_00247d10`,
called with 0x12000 and 1, and the troopers' / `FUN_0025b0f0`'s
constants). So (sub-states):

- 2 (`FUN_00237128`): the "!" (action 2), then 8 with effect 300 + voice;
  Snake within 1000 mm after 7 frames cuts it short.
- 8 (`FUN_002381e8`): aim (action 1); on frame 5 with Snake not seen
  (+0x18e != 3) 0x15; from frame 7 9 (0x15 once +0x2a0 > 30).
- 9 (`FUN_00238008`): every 4th frame action 9 and +0x2a0 += 1 (taken as
  a round: 31 of them lead to 0x12, `FUN_00238120`, action 10, which
  zeroes +0x2a0 and goes back to 8: a reload), or from frame 7 a 1-in-3
  chance (`FUN_0016f600() % 3`) of 0x15.
- 0x15 (`FUN_00237e00`): `FUN_00257508(FUN_00253c38(..))` targets the nav
  cell linked to his own in the direction away from Snake (bearing +
  0x800); `FUN_00256988` runs it and, on arrival or a failed way, 0xe
  with +0x310 = 0x2c000004 ("It's the enemy! I need help!" in set 0).
- 3 (`FUN_00237918`, a shot heard): mark and effect 0x53, 0x1f frames,
  then action 2 facing the noise; then 0xe with 0x2c000000 ("Possible
  hostile sighted. Send backup.", 0x2c000004 under DAT_001f39b0 bit 0)
  unless `FUN_0021eb20(.., 8, ..)` finds a comrade within +0x24c and
  `FUN_0021d230()` < 2 (-> 10). Before that, +0x348 bit 4 (radio known
  dead) or both arms hurt (+0xf16 & 3 == 3) -> 0x14, the run to report.
  `FUN_0021eb20(self, 8, &d)` walks the manager's groups (0x3fe730 ..)
  for the nearest other soldier with controller +0x344 bit 8 (set as 3
  and 0x15 go to 0xe: he has called this in), by straight distance
  (`FUN_0022de78`); `FUN_0021d230` is DAT_003fef7c, the on-air line's
  priority (its top nibble; `FUN_0021cdb8` sets it, -1 when the line's
  count DAT_003fef80 runs out). Sub 10 (`FUN_00237b10`): action 2 and
  effect 0x53 again, facing the noise; at the action's end the meter to
  0x400 and sub 0. Ported (assumed: bit 8 lasts the state; the port's
  on-air priority is the soldiers' lines only).
- 0xe (`FUN_00237528`): action 5 (radio out) until +0xf58 == 3, then 6
  (Snake seen) or 7; 0xf when the radio was shot (+0xb4 0x10000) or
  DAT_0017f48c bit 4.
- 6 / 7 (`FUN_002375d8` / `FUN_00237710`): say +0x310; 7 turns the head
  +0x200 on frames 0x3d-0x77 and -0x200 on 0x79-0xb3; cut short by the
  radio shot -> `FUN_0021d0a8` and 0xf. At the line's end action 6 and
  `FUN_00236f30` (meter 0x400); +0xf58 == 1 -> sub 0.

`FUN_0021cb40` follows 0x2c000000 / 2 / 4 and 0x20008003 with an area
line (`FUN_0021c940`): `FUN_0021c670` tests sixteen trap names (0x3a30ec
.. 0x3a30f5 for `-x` entries 0-9, 0x461dd7 / 8 for 10 / 11, 0x461dd9 ..
dc again for 8-11; each only when its entry has clips) with
`FUN_00148a98(0x1ece00, name)` (0x1ece00 taken as Snake's position) and
plays `0x11000000 | area * 4 + rand % count`: clip `area * 5 + 1 + n` of
0x4014b0, the manager's `-x` (`FUN_00221970` option 0x78: clips, -2
between areas, -1 at the end; w01a: two areas, "Understood. Search team,
head to port side!"...). No area: 0x10816004, line 0x1c4 of `-v` (w00a
"Acknowledged. Assault team, check it out.").

Sub 0x13 (`FUN_00237ef8`, from 8 when Snake is seen within 1000 mm
(+0x188) before frame 7, no leg wound (+0xf16 & 0xc) and DAT_001ecd40 &
0x620 clear): action 8 = `FUN_0025f810` (the alert's rifle butt, motion
0x55; 0xb with the right arm hurt), then 0x15 (0x14 with the radio dead).

Ported: subs 2, 8, 9, 0x12, 0x13, 0x15, 3, 0xe, 6, 7 and the area answer
(played at the line's end; the click as for other lines). Assumed: the
back-off runs and gives up after 300 frames; the nav link best aligned
with "away" stands for `FUN_00253c38`'s pick; the rounds use the alert's
bullet and sound; when the alert begins the caller switches to the chase
(the game finishes the radio-away action first). A caller darted or
knocked down mid-line raises no alert: the line is cut (`FUN_0021d0a8`)
and HQ calls for him. DAT_001ecd40 bits in 0x620 (the player's state flags, cleared each
frame by `FUN_001544a8` and set through `FUN_0028d420`): 0x20 prone
(`FUN_00269290`, stance +0xf40 == 2; 0x10 crouched), 0x400 thrown down
and lying (`FUN_00285690` sets 0x420, `FUN_00285b28` 0x400; also
`FUN_00286818`, not traced), 0x200 hit this frame (`FUN_002713e8` /
`FUN_00271af0`). Ported for 0x13 / 0x20 (prone, thrown down, lying; the
hit frame is not modelled).

Lead (not ported): the soldier's second sight record (controller +0x19c
.. +0x1b6, filled by `FUN_002559c0(c, guard + 0x266c)` in `FUN_00222978`
only under DAT_001f39b0 bit 4) watches a second target: the manager's
init sets that bit when `FUN_002d88c8()` (DAT_001f7750 + 0x70) is non-zero.
No store to DAT_001f7750 is in the w00a dump (it sits between
`foot_shadow.c` and `splash.c`; likely set by pointer from another
stage's actor), so in the Tanker +0x1b6 is taken as never 3: the
discovery's 0xf -> 0x18 (+0x348 bit 0x2000) and 0x18's 0x1c / 0x21
(`FUN_00238758`) do not occur. Sub 3's comrade check is ported (above). Not ported: the Marines (no radio motions: their "!" still raises the alert).

## Head look (`FUN_00254f..` draw, `FUN_00111f40` / `FUN_00112448`)

A state points the head by setting body +0xf66 (an absolute yaw) and
controller +0x354 bit 0x800 for that frame; the soldier's draw then calls
`FUN_00111f40(pos, yaw +0x22, pitch +0xf62, +0xf66, angles +0x132c, 0, 0
or 10 under bit 0x2000, limits)`, else `FUN_00111d80` looks at the point
+0xf70 (not traced). Limits: w00a 0x1f3c98 = yaw 0x400 / 0x400, pitch
0x300 / 0x300 (0x1f3c90, all 0x7ff, under bit 0x20000). The turn closes an
eighth of the way per frame (`FUN_001419c8`). `FUN_00112448(model, angles,
yaw, mask)` hands it to joint slots (16 bytes each at model +0x3c, flag
1 << joint at +0x30): mask 0x27 (bit 0x10000 clear; 0x21 with it, 0xf
under 0x400): 0x20 = joint 2 gets half the yaw (smoothed, angles[3]),
then joint 11 the rest, joint 12 (the head) pitch and yaw, joints 4 / 8
pitch and the half (bits 2 / 4). How the slots combine with the motion
is not traced. The sight's yaw (+0x240) is read back from the head's
matrix (`FUN_0022d8d0(model +0xf00)`), so the view follows the head.

Ported (`Guard::turn_head`): the yaw only, on joint 12 alone, and the
sight follows it; callers: discovery sub 7 (frames 0x3d-0x77 +0x200,
0x79-0xb3 -0x200, unless Snake is seen: then the body faces him).
Assumed: bit 0x800 lasts one frame (it is or-ed in every frame). Other
setters of bit 0x800 (not ported): `FUN_00224618`, `FUN_00224d20`,
`FUN_00225590`, `FUN_00225770`, `FUN_002258a0`, `FUN_00225dc8`,
`FUN_0022ab38` .. `FUN_0022adc0`, `FUN_002368c0`, `FUN_0023c890` /
`FUN_0023c970`, and `FUN_002a5380` .. `FUN_002aafa0`,
`FUN_003bd088` / `FUN_003bd2f0` (other actors, not traced).

## A dart that leaves him awake (normal state 7, `FUN_0023cb78`)

The dart handler (`FUN_0024f490`) sets body +0xf98 bit 0x800000000 (with
0x800, +0xf64 = 6 and effect 0x86) for a dart that does not put him
under; `FUN_00230c20` turns +0xf98 & 0x8800000000 into stimulus 0x200000
(and Snake in the bump box into 0x4000000, but a bumped soldier sees
Snake, level 3, and the discovery overrides). `FUN_00227520` sends both
to state 7 (`FUN_0023cc58`: target Snake's place via `FUN_002572a0`,
+0x26c |= 0x200000 so `FUN_00227888` keeps him there, pace 3; sub 1, or
3 / 4 when +0x270 is 1 / 2, which the hit reactions of `FUN_00251cc8`
set). Sub 1 (`FUN_0023c7f0`): mark kind 4 and `FUN_0021d1f8` (his radio
line cut, `FUN_0021da18(+0x140)`) on frame 6, the turn from 0x3d (action
3 = `FUN_0025e7b8`: motion 0xd / 0xe by the turn's sign, up to 0x440 by
the motion, then the heading), then sub 2 (`FUN_0023c890`): the head
look of the discovery's sub 7, mark kind 5 at 0xa0, sub 5 at 0xf0, which
clears the 0x26c bit: back to state 0 (`FUN_00227e00`; state 1 with
wounded limbs). Subs 3 / 4 (`FUN_0023c970` / `FUN_0023ca70`): actions 2
(motion 0xf) / 4 (motion 0x91) until they end, marks at 0x78, the head
look in 3. `FUN_0023cb18` (subs 2-4): the stimulus again -> sub 1.
Transitions `FUN_00226798` still run (noise, meter, books, prints...).

Ported (`guard::stung`): subs 1 and 2 from the patrol states (a line he
was saying is cut, as for any state change). Not ported:
the marks (kinds 4 / 5: `FUN_002c3590` -> `FUN_002c5ae8`, sprites not
identified), subs 3 / 4, the effect 0x86. Assumed:
idle motion while he waits and looks.

Lead (not ported): a round that hits him also sets +0xf98 bit
0x8000000000 (`FUN_0024a1e8`; also `FUN_002493e8`, `FUN_00250d68`), which
`FUN_00230c20` turns into the same stimulus 0x200000, and the hit
reactions (`FUN_00251cc8`) set +0x270 = 1 / 2 for state 7's subs 3 / 4.
So a soldier hit while unaware may look round rather than know Snake's
place at once (the port sets the meter full, assumed). Where the bit is
cleared is not traced (no plain store to +0xf98 found by grep).

Evasion (`FUN_0022C670`): its transitions (`FUN_0022c168`) also take
stimulus 0x4200000 to state 5, the same `FUN_0023cb78` (then
`FUN_0022c3a0`); ported (back to the search after it). Its stop-and-look
(state 4 sub 6 `FUN_0022ac58`, state 3 `FUN_0022ace8`, state 4 sub 10
`FUN_0022ab38`: head +0x200 on frames 1-0x3b, -0x200 on 0x3d-0x77, until
0xb5) is a head turn; the port had turned the body and now turns the head.

Lead (not ported): at a route point the walker (`FUN_00224d20`) sets the
body heading +0xf54 to the point's direction (runtime route +0xc04) and,
by the point's flags (+0xc08): none of 5 / 10 / 0x300 -> the head yaw
+0xf66 = +0xc04 (bit 0x800); 0x300 -> a look at the point +0x400 +
index*16 with pitch (bits 0x10200); 10 -> the same point with bit 0x400;
5 not read yet. Action 0xd (`FUN_00224618`, +0xc00) only sets bit 0x800.
How the hzx point (flags at +0x18, look point at +12..+20) becomes
+0xc00..+0xc08 is not traced (the copy near `FUN_002595e0`); the port turns
the body to the look point instead.

## A dead radio: chaff, and the report in person

`DAT_0017f48c` bit 0x10 is set by the chaff particles (`bomb_chaff.c`,
`FUN_003d6af8` creates one, counting them in `DAT_001fc3d8`; the first one
sets the bit in `FUN_003d62e0`, the last one's exit `FUN_003d6090` clears
it). Every radio call checks it together with the soldier's own shot
radio (+0xb4 bit 0x10000, set by `radio.c`'s hit box, `FUN_00265f50`):

| State | Radio out, jammed | Jammed mid-line | Dead radio | Run | In person |
|---|---|---|---|---|---|
| 2, a comrade (`FUN_00231d08` / `FUN_00231d88`) | 10 -> 0xb | 0xc -> `FUN_0021d0a8`, 0xb | 0xb `FUN_00231f20` (action 6) | 0xd `FUN_00231fb8` | 0xe `FUN_00232050`: 0x18a00005 |
| 5, a hold-up let go (`FUN_0023a2c8` / `FUN_0023a348`) | 5 -> 6 | 7 -> 6 | 6 `FUN_0023a4e0` (action 8) | 8 `FUN_0023a578` | 9 `FUN_0023a610`: 0x18a00005 |
| 8, discovery (`FUN_00237528` / `FUN_002375d8` / `FUN_00237710`) | 0xe -> 0xf | 6 / 7 -> `FUN_0021d0a8`, 0xf | 0xf `FUN_00237898` (action 7), then 8 | 0x14 `FUN_00237c90` | 0x1e `FUN_00237d48`: 0x1c000006 |

The dead-radio action (state 2's table `FUN_00231948` case 6 and the
discovery's `FUN_00236d60` case 7 are both `FUN_0025ef08`) plays gbs
motion 10 (120 frames; 0x30 with the left arm hurt) with the radio still
in hand, then `FUN_0025f068` puts it away; +0xf58 == 1 ends the sub-state.
On its frame 0x73 the sub-state sets +0x348 bit 4 (the radio known dead):
`FUN_00231c28` / `FUN_0023a1e8` then skip the radio, and the discovery's
8 / 9 go to 0x14 instead of the back-off 0x15 (8 from frame 7, without
firing). +0x348 is cleared when the state ends.

The run: `FUN_00257388` sets the walker's target from `FUN_0021eeb0`, the
manager's `-c` point (option 0x63: up to eight `x y z "#name"` entries at
0x3fea80 / 0x3feb00 via `FUN_0022e4b8`; the index 0x3fea74 is only set to
0 by init), the default the ELF's (0, 0, 0, 1) at 0x190a30; gait +0xf5a = 7
(route action 0x25, taken as the run). Tanker `-c` points: w00a (-8750, 0,
-2000), w00c (-10750, 12000, -13250), w01a (6000, 0, -13000), w01b (0, 0,
-13000), w01c (200, 9000, -14925), w01d (-3650, 6000, -13900), w01f (0, 0,
-7000), w02a (12500, 0, -15500), w03a (14000, -5000, -97000), w03b (14000,
-5000, -70000); the holds (w04a-c), w00b and w01e have no manager `-c`. On arrival
(`FUN_00256910` != 0) the line is said without the radio: 0x18a00005 "I saw
him! Send someone over now!" or 0x1c000006 "I'm under attack! Stay alert!"
(which HQ answers); at its end the caution timer (`FUN_0021ee90(-h)`) and
the state ends (state 8: +0x348 bit 0x10000 -> `FUN_00227b90`). So a
jammed discovery raises Caution, not Alert. The in-person subs also clear
the shot-radio flag and call `FUN_00255978`, which clears the body's limb
wounds (+0xf16 = 0, bytes +0xf1c..+0xf1f = 0; ported). It is also called
by `FUN_0025f598` on frame 0xa0 of motions 0xbe / 0xbf (the discovery's
action 0xc, motion 0xbf, used only by its sub 0x21 `FUN_00238bb0`, up
to 0x1f5 frames then 0x18; 0x21 comes from 0x18 with the second sight
record seeing (+0x1b6 == 3) and a wound (+0xf16 & 0xf), so not in the
Tanker, see the lead below; other states' action 0xc are their own tables') and by a dozen other state subs (`FUN_00234df8`,
`FUN_002360c0`, `FUN_0023d6e8`, `FUN_0023f340`, `FUN_002442c8`,
`FUN_00245158`, `FUN_00245c80`, `FUN_0024c7e0`, `FUN_0024cbb8`,
`FUN_0024edd8`, `FUN_003c62b0`; not checked).

Ported (`radio_fails`, `run_to_report`, `say_in_person`; chaff = the
port's `Jam` timer). Assumed: the run's reach (500 mm) and a 30 s cap,
the hold-up's action 8 as motion 10. The shot radio is ported too (see
"The radio"): `Guard::radio_shot`, cleared by the in-person subs, which
brings the radio back on his belt (as clearing 0x10000 does in
`FUN_00265fe0`). The run's close range is ported too: 0x14 goes to 0x20
(`FUN_00237e78`) when it has not arrived and Snake is seen (+0x18e == 3)
within 1000 mm (+0x188), the soldier's legs are whole (+0xf16 & 0xc) and
DAT_001ecd40 & 0x620 is clear; 0x20 is the rifle butt (+0xfdc = action 8,
0xb with +0xf16 & 1) until the action ends (+0xf58) or 0x79 frames, then
`FUN_00257388` and 0x14 again. Not ported: 0xf's 0x18 (+0x348 bit 0x2000), the
runner's +0x348 bit 0x100 (it puts him in the manager's list of soldiers
to see to, `FUN_0021e998`). Test: `MGS2_GRENADE_AT=<s>c` throws chaff.

## Script messages to soldiers (`FUN_00222810`, `FUN_00257df8`)

`mesg <soldier> <code> args..` (the soldier's name is his `chara #f7f777`
first argument; scripts also give it as a number or a variable):

- 1 `route [x y z map [point]]`: +0xc0f = route; with five or more
  arguments the post +0x2b0..+0x2b8 (as `-d`), its map +0x2c0
  (`FUN_0010e570(FUN_0010eb20(map))`), and the start point +0xc10 (0xff
  without a sixth). The route walker takes it up (`FUN_00224760` /
  `FUN_002247f0`: +0xc0f != +0xc0c -> `FUN_00252a20`; with +0xc10 < 0
  `FUN_00257210` starts at the nearest point, else `FUN_002570b0` at that
  point), also from `FUN_00227e00` (back to the route). The troopers'
  handler (`attacker.c` `FUN_00299ea8`) instead warps a trooper not yet in
  play (body +0xf98 bit 0x100000000000) to the new route (`FUN_00252ee8`).
- 2: +0x34c bit 8 (one frame; cleared by `FUN_00258028`); 3 / 4: +0xd8
  bit 2 on / off. Uses not traced.
- 0x100 / 0x101: +0x350 bit 0x80000000 on / off; 0x103 / 0x104: bit 0x100
  off / on; 0x105 n: down frames +0xf1a = n when asleep (+0xfb0 & 3).
- 0x106: out of play: +0x350 bit 1 (`FUN_00222978` and `FUN_00257f50`
  skip his senses and bookkeeping), `FUN_00257a68` hides the model and
  its attachments (+0x4c |= 0x3000), sets +0x70 |= 9 and clears his alert
  state; 0x107: bit 1 cleared, the model shown (`FUN_0022d320(.., 0x3000)`).

Uses: w01d sends `#2c441d` (route 11) to route 7 at the start on every
difficulty but Bs16:0x8 == 50, and moves `#2c441d` / `#2fa69f` between
routes as Snake enters and leaves traps; w02a keeps its door soldier
`#18e016` out of play (0x106) until an event brings him in (0x107, route
43); w00a, w00c, w01b do the same kind of route changes.

The maps named in these messages and in `-d` are `.hzx` areas: `chara
#35a2cf <map> -h "~w01d" n` names area n (w01d: `#284a23` 0, `#e494d1` 1,
`#0494ca` 3). Areas overlap and their node links stay within each area;
the game's crossing data is among the undecoded path tables. The port
joins nodes of different areas whose rectangles overlap or come within
100 mm at under 600 mm of height (assumed), so a soldier sent to a route
in another area walks there.

Ported (`guard::messages`): 1 (route, post, start point; taken up from
patrol, waiting or returning), 0x106 / 0x107 (hidden with his held
models, skipped by the brain, bullets, blows, grenades, the radar and
footsteps). Not ported: 2-4, 0x100-0x105, the post's map.

## Route points (normal state 0, `FUN_002261f0`)

The route loader `FUN_002593f0` copies each 40-byte `.hzx` point into the
soldier's route block: +0x000 position, +0x400 look point, +0x800 area bit
(0x40000000 >> u32 +0x24), +0x900 action (u16 +0x18), +0x980 wait (+0x1a),
+0xa00 direction (+0x1c), +0xa80 (+0x1e, use not traced), +0xb00 flags
(u32 +0x20). (The port's parser had the action and the flags swapped.)
`FUN_00224870` loads the current point into +0xc00 (action, through the
table at 0x1f39f0: 1 -> 4, 2 -> 5, 3 -> 6, 4 -> 0xa, 5 -> 7, 6 -> 8, 7 -> 9,
8 -> 0xd, 9 -> 0xe, 10..13 -> 0x10..0x13, 14 -> 0x15, 15 -> 0x16, 16 ->
0x17, 17 -> 0x18, 18 -> 2, 19 -> 0x14, 20 -> 0x1d, 21 -> 0x1e, 22 -> 0x25,
23 -> 0x19, 24 -> 0x1b, 25 -> 0x26; flags 0x40 / 0x80 instead keep the
index as a motion with bit 0x1000 / 0x2000), +0xc02 wait, +0xc04
direction, +0xc08 flags (in Caution flags 0x800 / 0x1000 and actions 4-6
become 2 / 0xd with a wait of -1 turned into 180 frames).

Sub-states (+0x330): 1 (`FUN_00224d20`) walks to the point (arrival
within 450 mm; the head looks at the point for the first 30 frames), 6
(`FUN_00225590`) turns to the direction (up to 60 frames), 10
(`FUN_00225770`, flags 0x20) plays action 0xf first; then `FUN_002244f0`
runs the manager's `-e` event for (route, point) (`FUN_0021dc60` ->
`FUN_0022e6d0(proc, soldier)`) and `FUN_00224a68` picks the wait: 7
(`FUN_00225dc8`) for -1 or flags 0x40: until the action ends (+0xf58 ==
1); 8 for -2 and 2 otherwise (`FUN_002258a0`): the wait counts down;
`FUN_002246c0` ends it at 0, -3 ends at the script's message 2 (+0x34c bit
8), -4 is set by `FUN_002246f8` to the length of the manager's `-f` line
for the point (`FUN_0021c9e0`: line 0x4060000 | entry). Each frame of the
wait `FUN_00224618` sets the body action to +0xc00 (0 from 0x1d on) and
the head by the flags: 0x5 / 0xa / 0x300 look at the look point (+0x354
0x300 / 0x400 / 0x10200), else the head yaw +0xf66 = the direction; action
0xd sets +0x354 bit 0x800 and its step `FUN_00223aa0` swings +0xf66 on a
270-frame cycle (front, +0x200 at 90, -0x200 at 180, done at 269). Moving
on, `FUN_00224c28` picks the gait from the next point's action (0x1d 2,
0x1e 3, 0x25 7, 0x26 9; flags 0xa 1; Caution 5; `-s` 0x40 10).

Actions and motions (`FUN_002240f8`): 4 15, 5 16, 6 17 (`FUN_00260320`),
9 231, 0xa 6 then 7 (`FUN_002237e0`, an effect at frame 18), 0xb 20, 0xc
232, 0xd idle + head swing, 0x15 5, 0x16 idle with body flag
0x100000000000 (`FUN_00260240`), 0x18 104, 0x19 radio out, 0x1b 228, 0x1c
229; 7 / 8 / 0xe / 0x10-0x14 / 0x17 use motions 274-282 (one-frame
placeholders in gbs.mar). The Tanker's points use actions 8 (45 points),
3 (27), 4 (21), 15 (21), 2 (9), 23 / 21 / 1 (5 each), 18 (4), 17 / 10 (2),
14 (1); waits -1 (63), -4 (6), -3 (2). `-e` events: w02a (routes 43 / 37 /
38 / 42) and w03a (2 / 16 / 17 / 18, for its `#3259e5` soldiers); `-f`
lines: w02a route 43.

Ported (`Wait` in `normal_brain`): the action's motion once, the head
swing of 0xd, the head turned to the look point (yaw only; the +0x354
modes 0x100 / 0x200 / 0x400 are not told apart), the body to the
direction, waits -1 / -2 / -3 / -4, `-e` events (the soldier's name as
$1) and `-f` lines; action 2 (the "!" motion 0x5c, route action 18);
the Caution changes (`point_plan`); the gaits (`pick_gait`, on arrival
at a point for the leg to the next, `FUN_00224d20` -> `FUN_00224c28`;
`gait_motion` from `FUN_0025dfd8`, speeds by the motions' root tracks:
the walk's 2110 mm / 95 frames and the run's 2160 / 32 match the port's
1330 / 4050 mm/s): 1 motion 0x12 (1150 / 53), 5 motion 99 (960 / 54), 2 /
3 the side steps 0x65 / 0x66 (740 / 90) with the body at the heading less
/ plus 0x400 (w03a route 5's points 1-4, route action 21), 7 the run;
action 0x19 (the check-in, below). Not ported: the turn sub-state 6 (the
body turns as he waits), 10 / action 0xf, gait 9 (motion 0x3c; no Tanker
point) and 10 (`-s` 0x40), the walking head look, actions 3 / 7 / 8 / 0xe / 0x10-0x14 / 0x16 / 0x17 and the
0x40 / 0x80 motion forms, 0xa's effect and second motion, 6's voices.
Test hook: `MGS2_CAUTION_AT=secs` sets the caution timer.

### Binoculars (action 0xa, `FUN_002237e0`)

Route action 4 -> 0xa (21 Tanker points): motion 6; on its frame 0x12 an
effect actor (`FUN_0022f358`, `FUN_0022f280`) holds model `sougan`
(binoculars; in every Tanker stage) on joint 10, the left hand, with
offset 0x1f3a60 and turn 0x1f3a58 (both zero); once motion 6 has played
(its flag 1), motion 7 for the rest of the wait; the model is released
when the action changes (`FUN_0022d610`). Body flag 1 only (he still
sees). Ported (`guard::binoculars`).

### Blind actions (body flag 0x1000)

The body runs its action step after clearing +0xf98 (`FUN_00247bb8`), and
the sight `FUN_002559c0` reads flag 0x1000 as nothing seen (level 0, the
bump box included; also `FUN_00252c70`, the joints test). Route actions
whose step sets it: 4, 5, 0xe, 0x12-0x14, 0x18 (`FUN_00223980`, every
frame until the motion ends: motions 15 (559 frames), 16 (440), 0x68) and
6 / 0xc (`FUN_00260320`, motion 17 / 0xe8: frames 0x3d-0x1e6, with
0x200000000000000; it also sets the eyes, +0xf4c +0x10: 5 shut at frames
30, 38, 60, 0 open at 34, 42, 1 at 0x1e7, not ported, and says 0x11c +
voice at frames 70, 226, 393, ported). In the Tanker: route actions 1 / 2
/ 3 (21 / 9 / 27 points) and 17. Action 0x16 (route action 15;
`FUN_00260240`, an idle) sets 0x100000000000, which the sight checks
first with the same result, while the action is unchanged from the
previous frame (+0xfdc, the action asked for, == +0xfe8, the one the
step runs; they differ only on the frame the brain asks for another,
when the step's +0xfd8 check switches over). All 21 Tanker points with it
are one-point posts with wait 0 / 1 / 30 (w00c, w01d routes 2 / 14 / 28,
w02a's waiting soldiers). The wait does not cycle back through the walk:
in `FUN_002258a0`, once the wait has counted down, a route of under 2
points (+0xc0e < 2) stays in the wait sub-state and sets +0xfdc = 0x16
again each frame (the `-e` event `FUN_002244f0` is called again too), so
such a soldier is blind from his second frame at the post until a script
message moves him (`FUN_00224760`, +0xc0f != +0xc0c). Ported (the wait's
`g.blind`); test `blind_post` (w01d guard 28). Other steps set it too (the hurt and
blow steps 0x24a938.., normal state 7 / 10 steps 0x23ce48 / 0x240a00,
0x243b00): not ported. Ported (`point_blind`, `Guard::blind`). Test:
`MGS2_ROUTE=from:to:point` places a guard at a point.

### The check-in (action 0x19, `FUN_00224530`) and normal state 0xf

Route action 23 maps to 0x19 (w02a route 43 point 1, the door soldier's
route; w04c's walkers have four, but no radio motions). Each frame of the
wait `FUN_00224618` calls `FUN_00224530`: +0x344 |= 0x1000; on the first
frame the wait becomes 600 and, with the radio shot (+0xb4 0x10000) or
jammed (DAT_0017f48c 0x10), stimulus 0x8000000; once the radio is out
(+0xf58 == 3) the wait becomes `FUN_0021c9e0(route, point, 0x4020000)`:
the manager's `-f` entry for the point (table 0x401e48; w02a `-f 43 1
536926251 2633`) said with the click, its length the wait (0xf0 without
a length, -1 without an entry); at 0 the action becomes 0x1a (radio
away), whose end closes the -1 wait. Stimulus 0x8000000 -> normal state
0xf (`FUN_00245db0`, sub-states `FUN_00245d18`): 1 radio out (action 2),
then 0 or, still dead, 2; 0 (`FUN_00245a00`) "The situation is normal."
(0xc000012), cut by a shot or jam -> 2, at its end -> 6; 2
(`FUN_00245b70`) the dead radio, then the run to `-c` (gait 7); 3
(`FUN_00245c08`) the run; 4 (`FUN_00245c80`) there: the shot flag and
wounds cleared, 0x79 frames, +0x348 = 0; 6 the end. Ported
(`State::Wait` for 0x19, `guard::check_in`); assumed: after 6 he returns
to the point and checks in again; FUN_002245f0's action 0x1c in the last
100 frames of a wait is not ported. Test: `MGS2_ROUTE=22:43` in w02a.

## Deck-2 defenders (w03a / w03b)

The enemy manager's `-a` proc `#a0402c` creates `#c7280a` "atcomm"
(`atcomm.c`, w03a resident `cmd_c7280a`; init `FUN_00272b48` runs `-e` via
`FUN_00272a78`, reads `-d` (up to 16 points x y z n), `-p`, `-t`, `-f`).
Its `-e` proc `#67a9d9` creates four `#3259e5` soldiers (`defender.c`) on
routes 0 / 8 / 8 / 4, or 1 / 3 / 2 / 13 when Au32:0xc4 is not "#cd9f50".
Their message handler `FUN_00273828`: 0 / 1 set +0x328 to 6 / 1; 2 warps
him (x y+1000 z, dir) when `FUN_002898e8` fails; 3 `route [x y z dir]`
sets the route byte (+0xc0f) and the post point +0x2b0; 4 / 5 flags.
Ported: creation with the stage (the manager's `-a` runs in its init),
message 3. Outside an alert they run the watcher's brain (`FUN_0038b4f0` / `FUN_00392af8`, the
port's own: assumed the same). Test: `MGS2_ALERT_AT=2` (meter full at
2 s) or `MGS2_ALERT_AT=2-30` (held full until 30 s) in w03a.

### The defence (alert)

atcomm (actor 0x530 bytes; its "squad" at +0x40, the defence block
`ctl` at +0x2f0): `-d` points at ctl+0x60 (16 bytes each, n at +0xc),
their count at ctl+0x1a0, occupants at ctl+4+i (soldier index +0x13c,
0xff free), `-f` front at ctl+0x1b0 and its nav node at +0x1c0. With
`-t` 0 (w03a) `FUN_00272308` stores the path length front -> point i at
ctl+0x160+4i (`FUN_0022a438`, cap 9 000 000) and each frame
`FUN_00272820` the length front -> Snake at ctl+0x1c4 (cap 120 000).
`FUN_0022a438` = first node, then the links' own lengths
(byte * 500 - 500), then the end: the port sums node centres instead.
A defender's unit is soldier +0x430 (+0x3ac points at it): +0 = ctl,
+0xa / +0xb his point, +0xf = `-t`, +0x12 flags (0x10 = no point / chase,
0x20, 0x01). His brain switch +0x31f8 (`FUN_002735b0`): 0 the watcher's
(`FUN_003971b8`), 1 the alert (`FUN_00283218`), 2 / 4 / 5 the others,
6 the defence (`FUN_0028db38`); phase changes run `FUN_00272ed0`.
On the alert (`FUN_00272ed0`, `-t` 0, no point): `FUN_0028dd28` takes
the first free point (in `-d` order) with length < Snake's - 4000, else
flag 0x10; then a point -> `FUN_0028dfc8` (state 6, sub-state 10, stance
0x13, actions by `FUN_0028bd90`), none -> `FUN_00283498` (state 1, the
alert's chase). Sub-state 10 (`FUN_0028db38`): Snake's path to the front
shorter than his -> flag 0x10, action 0x19 (the chase); else
`FUN_0028d970`: Snake's length - 3000 < his point's -> `FUN_0028ded8`
(give up the point, take the first free under Snake's - 3000, none: flag
0x10); stance 0x13 `FUN_0028c248` runs (action 0x11) and every 12 frames
`FUN_0028c8e8` gives up (flag 0x10, action 0x19) when his length > Snake's
- 2000, or when the squad's unseen count +0x214 > 300 and Snake is 10 m+
away; arriving (`FUN_00224ed8`) -> stance 0x14 `FUN_0028c498`: within
100 mm he is put on the point, sub-state 9 stance 4. Sub-state 9 stance 4
`FUN_0028cbb0`: after 60 frames, if his point's length + 4000 < Snake's
-> `FUN_0028ded8`; more than 250 mm off in x or z -> back to stance 0x13;
else action 2 (motion 0x45, `FUN_0028bca0`) facing +0x18c, and after 60
frames, if his order +0x3a8 +0x39 is 0 or 1, `FUN_0028c640` (the attack
from the point, below); the others keep holding.
Sub-state 10 stance 4 (`FUN_0028d8a8`, a re-entered defender) holds only
while atcomm's mode ctl+0 is 1: `FUN_00272820` sets it from the manager's
reinforcement counts (global 0x3e62b8 used / 0x3e62ba allowed, `-b`;
`FUN_00215930` hands them out) and to 2 after 3600 frames.

#### The attack from the point (stance 0x1b)

`FUN_0028c640` picks a pattern (unit +0x16): `-t` < 2: 4 when the squad's
unseen count +0x214 > 300, he is first in the order (+0x39 0) and Snake is
under 10 m (+0x188); that zeroes +0x214 and sets a target +0xf80 from
DAT_001ecd00 (Snake's place); else rand & 3. (`-t` 2+: 4 under
ctl+0x1f8 < 450 / +0x1fa 300 / order 2; 5 with ctl+3 and +0x1f0 0.) His
point's kind (unit +8) = the `-d` point's 4th value (as a float,
ctl+0x6c + 16i). Steps come from `PTR_DAT_001f50e0[kind * 6 + pattern]`
(w03a; w03b's resident has the table elsewhere: the port finds it by the
first list's bytes): pairs (action, frames) ending in action 0x1e.
w03a's points are kind 6 (the last, 7):
- kind 6: p0 `1/6 f/18 5/0 15/1 6/0 10/18 1/6`, p1 = p2 `1/6 f/36 1a/1
  10/36 1/6`, p3 `1/6 f/36 19/1 10/36 1/6`, p4 `1/6 f/36 12/0 e/36 10/36
  1/6`, p5 `2/6 19/1 2/6`
- kind 7: p0 = p2 = p3 `1/6 10/36 19/1 f/36 1/6`, p1 `1/6 10/36 1a/1 f/36
  1/6`, p4 `1/6 10/36 12/0 e/36 f/36 1/6`, p5 as kind 6
- kinds 0-3 use 3/4/7/8/0x16/0x1c/0x1d too; 4 and 5 only `2/6`.
Actions (`FUN_0028bd90` -> motion +0xfe4): 1 0, 2 0x45, 3 0x47, 4 0x46,
5 0x107, 6 0x108, 7 0x10a, 8 0x109, 9 0x10f, 10 0x110, 0xb/0xc 0x10a/0x109
mirrored (bits 0xc0000000 -> model flags 0x300; `FUN_0028bc60` then
fires from joint +0xca0 instead of +0x7e0), 0xd 99, 0xe 0x4b, 0xf 0x41,
0x10 0x40, 0x11 0x3c, 0x12 0x4e (`FUN_0028eab0`: makes an item, 6, in his
hand at frame 0x21 and throws it: the grenade), 0x13 0x15, 0x15 0x10c,
0x16 0x10b, 0x17 0x111, 0x18 0x10b mirrored, 0x19/0x1b 0x3d, 0x1a 0x154
(+0xf2c bit 0x4000) or 0x44, 0x1c 0x10d, 0x1d 0x10e. 3-0xc play once
(`FUN_0028bb68` sets +0xf58 at the end), the rest loop. Root travel:
0x41 -537 mm in x, 0x40 +557 (the side steps out from cover and back),
0x107 -45 / 0x108 +45.
`FUN_0028cdf8` each frame (stance 0x1b, `-t` != 3): `-t` < 2 first
`FUN_0028c8e8` (every 12 frames: his path length from the front, cap
5 000 000, > Snake's - 2000, or +0x214 > 300 with Snake 10 m+ off, order
0 and `-t` < 2 -> flag 0x10, action 0x19: the chase); faces Snake; the
step counter +0x1a counts unless the step's frames are 0 (then the step
ends on +0xf58); frames 1 = until +0x1a > +0x17 + +0x18 + +0x19 (12 + 48
+ 90, `FUN_0028dfc8`), else that many. Actions 0x15+ fire: +0x1a in
(+0x17, +0x17 + +0x18], every 6 frames of +0x338, cooldown +0x3a8 +0x20
0, `FUN_0028ba30` (`-t` < 2: Snake's length < his point's + his near range
+0x248 + 4000) -> aim (+0x14 spread, scaled by the window left and the
distance / 6000: never set in w03a, it seems) -> `FUN_0028a448` (the ray
from the muzzle joint, `FUN_0014f230` mode 0xf masks 0x10000 / 0x20:
distance to a hit, else 100000) >= distance - 2000 -> `FUN_0028c0b0` ->
`FUN_00223688` (the shot); else cooldown 12. Action 0x1e ends it: within
500 mm (x and z) of the point -> sub-state 9 stance 4 again, else 10 /
0x13. The order +0x39 (`FUN_00275af8`, after the squad's first 60
frames; before that the index): the squad sorted by +0x3a8 +0x28 plus
8000 / 20000 / 5000 for soldiers busy in some states.
Ported (`guard::defend`, `guard::attack`): the point picks, the run, the
hold facing Snake (motion 0x45), the attack from the point (patterns,
steps, root travel, fire windows, line of fire, the 12-frame give-ups),
the re-picks and giving up for the chase (checked each frame while
running). Assumed: the order = distance to Snake (without the extra
weights); the squad's unseen count zeroed whenever a soldier sees Snake.

#### The soldiers' grenade (`sig_grd.c`)

Action 0x12 (`FUN_0028eab0`, motion 0x4e): frame 0x21 makes the item
(`FUN_00232ce0(.., 6, .., 0x1aaae)`) in his hand, frame 0x60 (99 for
motion 0x152) throws (`FUN_0028e208(+0xf80, 3)`, 5 when the sighting
record's +0x14 has 0x48; 3 always with +0xf2c bit 0x20). Kind 3: from the
hand joint (+0x7e0) at the target (y = DAT_001ecd00 +0x90 + 10), arc
height = distance / 6; if `FUN_0014f230` (0xf, 0x10000, 0x20) finds the
line blocked, the target 1250 mm up with height 1000, else +-1000 mm to
the side (rotated by the throw's direction); then `FUN_00290398(height,
hand, target, 1, 0xb4, 0)` (kind 5: 500, fuse -1 = on landing, type 4).
`FUN_002900f0`: model 0x1aaae (type 3: 0xb932da); vertical speed
sqrt(18 h) mm/frame, flight T = 2 sqrt(18 h) / 9 frames, velocity
(target - hand) / T plus that; fuse +0x1f8, type +0x202. `FUN_0028ff08`
each frame: from frame T-1 on, +0x1ec = 1 (collision flags 0x3132);
`FUN_0028fd58` moves it (`FUN_00106a08`, the ELF's physics) and spins it
(+0x1e0 / +0x1e2 += 0x80 for types other than 3); gravity 9 mm/frame^2;
`FUN_0028fb78` on a floor contact: the first sets vy to a tenth of its
speed and halves x / z (sound 0x2b), the second stops it (0x2b); from
T-1 x / z * 0.9, and * 0.5 while +0x1ec; at the fuse `FUN_0028fe18`:
type 0 `FUN_0033cef0(+0x1b0 (the target), 1, 2000, 2000, 0x28 (0x50 under
the 0x48 flag), 10, 0x11, 0x31)`, type 2 a stun (`FUN_002ee658`, sound
0x42), 3 / 4 the same blast with 0x3c / 0x50.
`blast.c` `FUN_0033cef0(pos, ?, size, size2, damage, ?, group, flags)`:
two hit boxes of half the sizes (the damage halved unless flags & 4),
group 1 << 0x11; flags & 0x10 clear: their corners pulled in to clear
walls; & 2 clear: the bomb.c effect `FUN_00303750(pos, 2)`; & 1 clear:
noise 11 (`FUN_0011b630`); & 8 clear: sound 0x41 (0x29 under
DAT_001e6924 & 1). Its hits (`FUN_0033cd00`) need a clear line to the
target (0x4f, 0x40, 0x2000040); the actor lasts one frame.
Ported (`grenade::frags`, `guard::throw_grenade`): the throw, the flight,
bounces, fuse and blast on Snake (damage 20, sound 0x41, no noise).
Assumed: the hand 1400 mm up, 300 ahead; Snake hit when his middle (900
mm up) is within 1000 mm in each axis with a clear line. Not done: the
item in hand, the bomb.c effect, the blast on soldiers, kind 5.
Not ported: shooting on the move (`FUN_0028a280`), the spread, flag 4's skip of step 0 (+0x12 bit
4, distance under the near range - 250), mirrored motions. Port-only: a
defender kept 1.5 m short of his point by walls for a second holds where
he stands.

### w03b's fight (atcomm `-t` 1, defenders `-t` 2)

Story point As16:0xe 32 (after the demo `#7c273b` that the trap
`#8bc637` leads to: point 31, Abit:0xb4d.0 set, restart): the manager
(`-b` Au8:0xb61 = 4 / 5 / 8 by difficulty, `-r 14000 -5000 -70000`) makes
atcomm `-t 1 -d` 4 points (kinds 0 2 1 1) `-l` two lights `-c 1800 -p
#d77a1e`, whose `-e` makes three defenders `-t 2` (routes 3 3 4).
- `FUN_00272308` (`-t` 1): defender i gets point i (post +0x2b0, unit
  +0xa / +0xb, occupant); his near range +600 is 3000.
- `FUN_00272820` each frame: Bs16:0x112 = 1 and the meter 0x400 (the
  phase held at Alert); with the countdown +0x4e0 (`-c`) at 0, +0x4e8
  counts 0..900 round and +0x4ea up to 300; mode +0x2f0 from the
  manager's counts (record 0x3e61d0 +0xe8 = 0x3e62b8 used, 0 here, +0xea
  = 0x3e62ba left, `-b`): left - n < used (n = the squad, +0x8c) -> left
  != used: countdown 0, mode 1; else mode 2 (and +0x25c 2500 for all);
  also mode 2 when +0x4f4 >= n (+0x4f4 is zeroed each frame; its setter
  is not in the dump). `FUN_002727a8`: the gap +0x4e2 at 0 restarts at
  180 (mode 0) or 12, else counts down. The countdown runs down; at 60
  with lights on `FUN_00213260(-1, 0, 0x4060000, +0xbc)` (not ported).
  Beaten: left == used and left + n == +0x4f0 (deaths, `FUN_00273570`
  from `FUN_00273a30` when life +0xf0c drops to 0) + +0x4ec (soldiers
  down 120 frames this frame) -> the `-p` proc (`FUN_00221498`): a fade,
  then As16:0xe 33 and restart (the demo `#7ca73b`, then d10t).
- Messages (`FUN_00272208`): 1 n sets a 12-frame timer that clears light
  bit n (+0x2f3; the lights' `-b` procs send it when shot); 2 n sets the
  entry index +0x4e6.
- `FUN_00272ed0` (`-t` >= 2): mode != 2 -> `FUN_0028de58` (his own point
  again) -> `FUN_0028dfc8` (state 6 sub 10 stance 0x13, or 0x3f after a
  comeback: action 0x13, motion 0x15, until it ends). No path-length
  give-ups (`FUN_0028c248` / `FUN_0028cdf8` skip `FUN_0028c8e8`), no
  re-picks (`FUN_0028cbb0`), fire range always (`FUN_0028ba30`).
- Holding (`FUN_0028cbb0`): after 60 frames he attacks when the gap
  (ctl+0x1f2) is 0. `FUN_0028c640`: lights on (ctl+3), countdown 0 and
  `-t` 2 -> pattern 5; cycle < 450, warm == 300 and order 2 -> 4 (warm
  0, the gap 300, unseen 0, target Snake); else rand & 3; then (cycle >
  449, order 0, mode != 1) or mode 2 -> the chase.
- A defender down (`FUN_002733f0`, `-t` >= 2): life > 0 and down (+0xf1a)
  over 120 frames: +0xf98 |= 0x800; left == used -> +0x4ec += 1. left >
  used: with +0x3208 from 181 to 300 he blinks (odd frames hidden,
  `FUN_0028b638` / `FUN_0028b670`); before 301 `FUN_002898e8(1000, 2000,
  pos)` (a view test, it seems) >= 0 makes him wait; then left -= 1,
  `FUN_002732a0`: his point again, warped to the manager's place +0x4e6
  (`FUN_002158e8`: 0x3e6470, the `-r` places, and 0x3e64f0 a direction),
  `FUN_0028dfc8`; left - n < used and left != used -> sub 10 stance 4
  (`FUN_0028d8a8`: action 1 facing +0x18c while mode is 1; after 3600
  frames mode 2; mode != 1 -> the chase); left == used -> the chase.
Ported (`guard::atc_frames`, `guard::come_back`, `defend` under `fixed`):
all of the above but the 0x4f4 rule, the countdown's call, the 12-frame
light timer and +0x25c. Assumed: he comes back at 300 frames always (the
view test not ported), with full life and no darts (`FUN_00222c08` not
read); the dead never come back (as decoded), so a fight won by killing
leaves soldiers "left" and is never beaten (lead: check in the game).
Test: `MGS2_BANKS=a8:0xb4d=1,a16:0xe=32,a32:0x74=#272ebd` (w03b).
The demos at points 31 and 33 (`chara "demo" "#858bd0"`) play in the
stage (`demo::StageDemo`, see formats.md "DEMO.DAT"); their `-n` procs
set points 32 / 34 and restart (or load d10t). Test: `MGS2_BANKS=
a8:0xb4d=1,a16:0xe=31` reaches the fight with cutscenes skipped.
Lead: a defender's `-p` (up to 4 procs, `FUN_0021a750` into +0x322c) is
not yet traced to where it runs; w03a's fourth defender has
`-p #67363a` (300 frames later message 3 route 3 to the third, As16:0xb4a
+= 1). `-t` (+0x330f) 2 / 3 set flags 0x10000100 on +0x3220, 0..1 start
`FUN_003973f0`.
