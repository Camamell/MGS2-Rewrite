# mgs2-tanker

A reverse-engineering project to rebuild the Tanker chapter of
*Metal Gear Solid 2: Sons of Liberty* (PS2, NTSC-U, SLUS-201.44) in Rust on
the Bevy engine.

No game data lives in this repository. Every tool reads from your own disc
image at run time.

## Layout

| Path | Contents |
|---|---|
| `crates/mgs2-formats` | Library: ISO 9660 reader, STAGE.DAT cipher and archive reader |
| `crates/mgs2-extract` | CLI to list and extract stages |
| `crates/mgs2-bevy` | Shared Bevy code: stage loading, meshes/materials, skinning, fly camera |
| `crates/mgs2-viewer` | Stage and model viewer |
| `crates/tanker` | The game |
| `re/notes` | Format documentation with references to game functions |
| `re/scripts` | Ghidra scripts and Python prototypes |
| `re/disc`, `re/ghidra`, `extracted` | Local only (git-ignored): copied executable, Ghidra project, extracted data |

## Extracting

```bash
cargo run --release -p mgs2-extract -- "path/to/mgs2.iso" stages
cargo run --release -p mgs2-extract -- "path/to/mgs2.iso" list w00a
cargo run --release -p mgs2-extract -- "path/to/mgs2.iso" extract --tanker -o extracted
cargo run --release -p mgs2-extract -- extracted textures -o extracted-png
cargo run --release -p mgs2-extract -- extracted models --png-dir extracted-png -o extracted-obj
cargo run --release -p mgs2-extract -- extracted scripts -o extracted-gcl
cargo run --release -p mgs2-extract -- "path/to/mgs2.iso" codec
```

`cargo test` also parses every extracted file when `extracted/` exists.

Optional: put a [JoyDict](https://github.com/Joy-Division/JoyDict) checkout at
`re/third_party/JoyDict-master` (git-ignored) to resolve more hashes in
script output. Dictionary-only matches are shown as `~name`.

## Game (work in progress)

```bash
cargo run --release -p tanker -- --iso "path/to/mgs2.iso" w00a
```

`--title` opens a start menu: New game (with the difficulty), Continue
(the autosave) and Stage select. On Windows, `play.bat` asks for the .iso
once and opens it.

Progress is saved automatically each time a stage starts (stage, story
variables and inventory, in `%APPDATA%\mgs2-tanker\save.txt`); add
`--continue` to pick up from there.

The stage script runs through a GCL interpreter, which creates the level
geometry and the guards for the chosen difficulty (`--difficulty 10..50`,
default 30 = Normal). Snake arrives the way the previous stage's script sends
him (`--from d01t` by default for w00a: the deck after the opening). The
Tanker resident data (`r_tnk0`: Snake's model, motions, weapons) is loaded
too.

| Input | Action |
|---|---|
| W A S D | run (camera-relative) |
| Shift | walk |
| C | crouch; while running, a forward roll (knocks over a guard in the way) |
| Z | prone / crawl (lower to the guards' eyes and bullets; fits under low gaps) |
| Space | action: open doors (stage exits load the next stage); at a deck edge, hang over the railing (A/D shimmy, Space climb up, Z let go; the grip gauge runs down); some traps react to it too |
| F (hold, release) | aim (A/D turn) and fire the weapon in hand; aiming at an unaware guard from behind holds him up, and from his front he gives up his dog tags |
| 1 / 2 | M9 tranquilizer / USP (Snake takes Olga's USP when she goes down; a head shot kills, the body takes three rounds, limbs wound, and guards within ~10 m hear it) |
| V | punch (three in a row: punch, punch and a knock-down blow; two such combos knock a soldier out) |
| R | equip / unequip a ration (revives Snake at zero life) |
| N | use a bandage (stops the bleeding), else cold medicine (cures a cold: with one Snake sneezes when he keeps still, and guards hear it; no Tanker script gives him one) |
| T | thermal goggles on / off (once Snake has them; the look is assumed) |
| X (hold) | first-person view: A/D turn, W/S look up / down |
| (walk into a wall) | back to the wall: sidestep along it (keep going at its end to peek round the corner), Space knocks (draws guards within 4 m), step away to leave |
| G | throw a stun grenade (or chaff when out of stuns): stuns guards within 5 m of the blast; chaff jams the radar for 8 s |
| B | put on / take off the cardboard box (once Snake has one): guards ignore it while it keeps still, unless one walks right into it |
| K | call the Codec (141.12); Space next line, Esc hang up |
| L | call 140.96: Otacon saves the game when the call ends (`--continue` starts the stage over with the story and items as saved) |
| C (holds, w04c) | digital camera: first-person view; A/D W/S aim, Q/E zoom, F take a photo |
| P | pause |
| F11 / Alt+Enter | fullscreen on / off (`--fullscreen` starts that way) |
| Enter | continue after a game over |
| Q / E, right mouse drag | turn the camera |
| F1 | collision, plus patrol routes |
| F2 | guard vision cones |
| F3 | stage cameras on/off |
| F4 | debug text (stage, phase, position, controls) |

A gamepad works too, laid out like the original: left stick to move
(tilt gently to walk), right stick to turn the camera, Cross/A crouch,
Circle/B action, Square/X aim and fire, Triangle/Y punch, R1 first person,
L1 prone, R2 grenade, L2 ration, R3 bandage, D-pad left/right M9/USP, D-pad up box,
D-pad down Codec, Start pause/continue/skip, Select save (140.96) (`pad.rs`).

Testing hooks: `--auto-walk <seconds>` holds forward (`--walk-from`),
`--cam-yaw/--cam-dist/--cam-pitch`, `--follow-guard N`, `--trace` (guard
states every second, trap events), `--spawns` (every actor the script
creates), `--snake-at=x,y,z` and `--snake-dir` (4096 = full turn),
`--use-door N --door-at <seconds>`, `--fire-at <seconds>`, `--punch-at
t1,t2,..`, `--press-at t1,..` (action button), `--codec-at <seconds>`,
`--codec-auto` (page through calls by themselves), `--shots
"t,x,y,z,yaw,pitch;..."` (stand there and take a photo; a seventh value of
0 only moves Snake), `--screenshot out.png --shot-at <seconds>`, `--look=x,y,z,yaw,pitch` (fixed camera).

For example, the whole holds finale (four photos, the terminal, Otacon's
review, the chapter's end):

```bash
tanker w04c --shots "2,0,-17000,-3000,180,29.6;3,9446,-17000,-6804,225,34.3;4,-9446,-17000,-6804,135,34.3;5,11250,-17000,-22375,270,47.6;6,12000,-17000,-3750,180,0,0" --press-at 7 --codec-auto --skip-cutscenes
```

What works:

- Level geometry plus the script's scenery, plants and glass; item boxes
  you can pick up (inventory kept across stages).
- Guards see, hear the game's noises (the M9 nearby, a knock on a wall, a
  hard landing, darts and rounds striking surfaces, footsteps only on the floors the stage marks noisy;
  the nearest soldier within his `-h` range comes to look; an unsuppressed
  shot sends those in earshot straight to discovery, and a stun grenade's
  blast raises the Alert), investigate, raise the alert, strike with the rifle butt up close, chase along the
  navigation graph and shoot (aim, bursts, reloads as in the game's code).
  Damage follows the game's numbers: guards have life by difficulty and hit
  zones (a head shot kills, three body hits, limb wounds), darts drain their
  resistance until they drop (head at once, body about 8 s on Normal),
  blows wear down their stamina, they drop the items the stage gives them
  when they go down or are shaken after their tags (`-q` / `-a`, at the
  script's chances), and they sleep 5 minutes or stay knocked
  out 30 s before getting up. Guards who find a sleeping comrade wake him
  and raise caution. An alert brings in the stage's reinforcements (its
  own script proc), who leave again once the alert is over.
- Snake's life follows the difficulty (200/150/100/75/50) and carries over
  from stage to stage; he collapses with his own death motion at zero, the
  stage's game-over proc runs (Otacon calls out to him) and continuing
  restores life the way the stage's continue proc does.
- Doors and traps run the stage script's own handlers, so walking through a
  door or into an exit area loads the next stage with Snake where the
  script puts him (after his walk-in). Difficulty settings come from the
  title screen's script.
- Stage music from the game's own sound driver: a port of its sequencer
  plays the stage's songs from their sequences and samples, switching with
  the alert phase as `bgmanage.c` does (`re/notes/sound.md`; not yet checked
  by ear against the game), and its sound effects: the M9, guards' and Olga's
  shots, doors, damage and the alert sting, placed by the camera, and the
  footsteps the motions themselves call for (`.sar` events, Snake's and the
  soldiers', with the stage's variant for the floor underfoot).
- Stage lighting from the stage's `.lt2`, as the game computes it on VU0:
  ambient and a directional light plus point, spot and strip lights and
  shadow spheres, per vertex, on the map, props and (assumed the same way)
  the characters (`mgs2_formats::lt2`; `MGS2_NO_LT2=1` to compare unlit).
- The radar (`radar.c`, top right): the stage's walls around Snake's height,
  the guards with their vision cones, turned by the actor's `-d`; in Alert
  and Evasion it shows the phase instead (a redrawn stand-in for the game's
  textured radar).
- Marks over the guards' heads from the game's sprites: "!" on discovery,
  "?" on a noise, a rising "Z" while asleep (tints and timing assumed).
- A game-style HUD: LIFE gauge (and the boss's) top left, weapon box bottom
  right, item box bottom left, prompts at the bottom, the holds' timer top
  centre.
- Wet footprints for 50 s after Snake steps out of the stages' wet areas
  (`foot_print.c`; the doorways in from the rain in w01a, w01b, w01e; the
  prints' look is assumed), and blood drops (and a point of life every 3 s; keeping still instead wins back two points every 0.8 s) while Snake is under a third of
  his life after a hit (`blood_dp.c`); a soldier who sees either follows the
  trail.
- The USP's light: a pool of light where Snake aims it (look assumed).
- Character shadows: Snake's own shadow model (`sna_shadow`), others their
  body, posed with the body and flattened onto the floor (the game draws a
  shadow volume; here straight down, assumed).
- The sea around the ship from the stage script's `wave2.c` (its textures,
  height and lighting; the swell is approximated).
- Each stage's fog (`chara #a53aa7`: colour, near and far), the distant
  skyline exempt.
- Lightning on the open decks (`chara #89af5e`): lit clouds flicker in the
  sky, a branching bolt cracks (also where the cutscenes call for one) and the
  thunder follows, at the game's odds and timings (`MGS2_THUNDER=n` for a
  strike every ~n frames).
- Rain on the open decks where the stage script sets it up (`rain.c`:
  streaks around the camera, not where its boxes exclude it).
- The stage scripts' own cameras (follow-in-box and fixed, switched by traps)
  frame each area as in the game, with faces culled the way the PS2 does so
  ceilings drop away under overhead views; elsewhere (or with F3) a follow
  camera keeps walls and decks out of the way.

- Olga on the navigational deck (w00b): her original step tables are run
  from the disc (aim, burst timings, reloads, moving between her positions);
  only the choice of the next sequence is simplified (`re/notes/guard-ai.md`).
- Codec calls (K) with the original dialogue and voices (VOX.DAT) and the
  speakers' 3D faces from FACE.DAT, moving with their idle motions and the voice clips' own face animation (lips, brows, posture); hanging from railings (the deck's ledges, and in the holds the railings the stage scripts mark for climbing over).
- Cutscenes play from DEMO.DAT: the camera track, the characters posed by
  the demo's actor stream, the cutscene stage's scenery, and subtitles
  (`--skip-cutscenes` to skip; Space next line, Esc skip), with the
  characters' skinned cutscene meshes (`.evm`) and the demos' own audio
  (voices, music and effects), their screen fades and flashes, their own fog and the characters' lighting (other
  visual effects not yet).
  Exits through a cutscene continue to the stage it leads to.
- The holds' finale (w04c): photograph Metal Gear RAY with the digital
  camera. Photos are graded by a port of the game's own routine against the
  script's targets, saved to the six-photo album, and sent from the
  terminal; Otacon's review is the original Codec record, and the stage
  script's own callbacks restart the stage on the success path and lead to
  the chapter's last cutscene (d12t) and an end screen. The Marines stand
  in formation where the script places them (`re/notes/formats.md`), and
  the Marines on watch in the holds use the guard AI with their own motions.
  A Marine in formation who sees Snake (within 6 m and 22.5 degrees of his
  facing), a shot or explosion, or Snake walking up to the formation's
  front (the script's approach points) starts the script's own "caught"
  sequence, which ends the game. The commandant's speech and other spoken
  lines play with their voices and subtitles (VOX.DAT / MOVIE.DAT), and the
  holds' time limit (`#9b65f0`, 7 minutes from the new-game variables)
  counts down on screen, ticking, and runs the script's proc at zero.

Snake starts with the darts the title script gives a new Tanker game (106 /
46 / 16 / 16 / 16 by difficulty), carries at most the script's maximums, and
Olga's USP comes empty (its rounds lie in the later stages; `MGS2_USP=1`
starts with it and 15 rounds, for tests).

Assumptions (not taken from the game yet): the USP is heard within 10 m,
the reload pause and where on the body a round or dart lands (hit zones by height). In the holds, the
camera is always available, a photo goes to the first empty album slot (else
over the worst one).

## Viewer

```bash
cargo run --release -p mgs2-viewer -- --iso "path/to/mgs2.iso" w00a
```

Set `MGS2_ISO` to skip `--iso`. Level geometry is placed the way the game
does it, from the stage script's map-piece actors.

| Input | Action |
|---|---|
| Right mouse + move | look |
| W A S D / Q E | fly / down, up (Shift = fast) |
| `[` `]` | previous / next Tanker stage |
| C / V | collision overlay (walls red, floors green) / second collision set |
| L | toggle lighting (default unlit, closest to the PS2 look) |
| B, then N / P | model browser: every model in the stage |
| F / G | overview / jump to the main floor |

Other options: `--collision`, `--lit`, `--browse <index>`,
`--cam=x,y,z,tx,ty,tz` (mm), `--screenshot out.png` (render, save, exit) and
`--check` (load every Tanker stage and print a summary).

## Ghidra setup

- Ghidra 12.1.4 with the
  [ghidra-emotionengine-reloaded](https://github.com/chaoticgd/ghidra-emotionengine-reloaded)
  extension (language `r5900:LE:32:default`), running on JDK 21.
- `re/ghidra/mgs2` contains `SLUS_201.44`, with the `w00a` resident gameplay
  code mapped at `0x1F1480` (`re/scripts/LoadResident.java`).
- `re/scripts/DumpAll.java` writes every decompiled function to
  `re/ghidra/decomp/all.c`. `re/scripts/fn.py` prints functions from it by
  name or address.

## Status

1. Tooling: done
2. STAGE.DAT unpacking: done
3. Asset formats: textures, models, collision and scripts done; motions
   (`mar`) and minor formats open (see `re/notes/formats.md`)
4. Bevy viewer: done (static levels, collision overlay, model browser)
5. Gameplay: in progress
   - done: motion decoding (joint rotations, root height), Snake spawned
     from script entry points, run/walk/crouch with collision, stairs, GCL
     interpreter for stage setup, guards with routes, guard AI ported from
     the game (perception, suspicion meter, discovery, alert, evasion,
     caution, shooting), Snake's life and game over, door transitions
     between stages, an obstacle-aware camera
   - since: items and weapons, hold-ups, traps, stage cameras, Codec,
     hanging, Olga, cutscene subtitles, the holds' photo finale: the Tanker
     can be played from the deck to its end
   - then: cutscenes from DEMO.DAT with voices, music and lighting; sound
     (the IOP driver ported); the Marines' formation (variants, idles,
     drills, head turns, its sight and events); footprints and blood;
     close quarters; texture blend modes
   - next: radio reports, the troopers' own brain, Olga's sequence selector
     (needs her cover objects), the commandant's walk
