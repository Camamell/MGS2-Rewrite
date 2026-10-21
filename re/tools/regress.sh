#!/usr/bin/env bash
# Headless regression runs of the Tanker port (Git Bash). Each check
# launches tanker.exe with --screenshot (second monitor, no pad, no save)
# and greps its log. Usage: re/tools/regress.sh [quick]
# Needs MGS2_ISO; builds the release binary first.
set -u
cd "$(dirname "$0")/../.."
: "${MGS2_ISO:?set MGS2_ISO to the disc image}"
export MGS2_NO_SAVE=1
OUT="${TMPDIR:-/tmp}/mgs2-regress"
mkdir -p "$OUT"
cargo build --release -p tanker || exit 1
EXE=./target/release/tanker.exe
pass=0
fail=0

check() { # name, pattern (grep -E), expected count op (ge N | eq N), args...
    local name=$1 pat=$2 op=$3 n=$4
    shift 4
    local log="$OUT/$name.log"
    timeout 400 "$EXE" "$@" --screenshot "$OUT/$name.png" >"$log" 2>&1
    local got
    got=$(grep -cE "$pat" "$log")
    if grep -q "panicked" "$log"; then
        echo "FAIL $name: panic"; fail=$((fail + 1)); return
    fi
    if { [ "$op" = ge ] && [ "$got" -ge "$n" ]; } || { [ "$op" = eq ] && [ "$got" -eq "$n" ]; }; then
        echo "ok   $name ($got)"; pass=$((pass + 1))
    else
        echo "FAIL $name: '$pat' x$got, wanted $op $n (log $log)"; fail=$((fail + 1))
    fi
}

# The holds finale: four photos, the terminal, Otacon, d12t, the end.
check finale "end of the Tanker chapter" ge 1 w04c --skip-cutscenes --codec-auto --press-at 7 --shot-at 120 \
    --shots "2,0,-17000,-3000,180,29.6;3,9446,-17000,-6804,225,34.3;4,-9446,-17000,-6804,135,34.3;5,11250,-17000,-22375,270,47.6;6,12000,-17000,-3750,180,0,0"
# The crowd: seen up close, not from 6.7 m; an approach point; a shot.
check crowd_seen "saw Snake" ge 1 w04c --skip-cutscenes --snake-at=-2010,-17000,-10630 --shot-at 4
check crowd_far "saw Snake" eq 0 w04c --skip-cutscenes --snake-at=-3512,-17000,-14500 --shot-at 4
check crowd_point "crowd point 1" ge 1 w04a --skip-cutscenes --snake-at=740,-15600,-3725 --snake-dir=-150 --shot-at 5
MGS2_USP=1 check crowd_shot "crowd heard a shot" ge 1 w04a --skip-cutscenes --fire-at 3 --shot-at 6
check holds_quiet "game over" eq 0 w04a --skip-cutscenes --auto-walk 8 --shot-at 9
# The commandant waits 1200 frames at route 0's first point, then paces
# 4 m to the next at 349 mm/s (past x -1000 by ~24 s).
check commandant "r0 \(-[1-9][0-9]{3},-14750" ge 1 w04c --skip-cutscenes --trace --snake-at=0,-17000,-2500 --shot-at 30
# Guards: the box check and the rifle butt.
MGS2_BOX_TEST=1 check box_check "checks the box" ge 1 w01a --skip-cutscenes --snake-at=6000,0,1500 --shot-at 26
check close_quarters "life=(8|7)" ge 1 w01a --skip-cutscenes --trace --snake-at=6000,0,3300 --snake-dir=0 --shot-at 6
# A soldier finds his comrade asleep, kicks him awake and radios it in.
MGS2_DOWN=7:s@1 check comrade "radios in a downed comrade" ge 1 w01d --skip-cutscenes --shot-at 22
# ... with his voice line from the manager's -v table, then HQ's reply.
MGS2_DOWN=7:s@1 check radio_line "radio line 0x20" ge 2 w01d --skip-cutscenes --shot-at 22
# Darted mid-report: the line stops and HQ calls for him.
MGS2_DOWN=7:s@1,18:s@air check radio_cut "HQ waits for an answer" ge 1 w01d --skip-cutscenes --shot-at 18
# Snake glimpsed (state 6): the guard turns, pauses and asks who is there;
# a dart's noise behind him (state 3) brings him over without a word.
check investigate_line "radio line 0x20" ge 1 w01a --skip-cutscenes --snake-at=6000,0,-2000 --fire-at 3 --shot-at 8
check noise_quiet "radio line 0x20" eq 0 w01a --skip-cutscenes --snake-at=9000,0,4100 --snake-dir=0 --fire-at 2 --shot-at 8
# A hold-up let go (empty M9): Snake walks off, the guard radios it in.
MGS2_WALK_DIR=2048 MGS2_GIVE=1/1:0 check holdup_call "radios in the hold-up" ge 1 w01a --skip-cutscenes --snake-at=6000,0,2500 --snake-dir=0 --fire-at 1 --aim-for 6 --walk-from 7.6 --auto-walk 3 --shot-at 16
# Discovery: the "!", a burst, the radio call for backup and HQ's area
# answer; darted mid-call, he raises no alert and HQ calls for him.
MGS2_GOD=1 check discovery_call "radios for backup|HQ answers" ge 2 w01a --skip-cutscenes --snake-at=6000,0,0 --snake-dir=0 --shot-at 18
MGS2_GOD=1 MGS2_DOWN=3:s@air check discovery_cut "radio cut off|radios for backup" eq 1 w01a --skip-cutscenes --snake-at=6000,0,0 --snake-dir=0 --shot-at 16
# Chaff jams the radio: the caller shakes it, runs to the manager's -c
# point and reports in person (Caution, no alert); the comrade report too.
MGS2_GOD=1 MGS2_GRENADE_AT=4c check discovery_jammed "reported Snake in person" ge 1 w01a --skip-cutscenes --snake-at=6000,0,0 --snake-dir=0 --shot-at 26
MGS2_DOWN=7:s@1 MGS2_JAM=1 check comrade_jammed "Comrade0xd/" ge 1 w01d --skip-cutscenes --trace --shot-at 32
# A dart into guard 3's radio from behind: his call fails the same way.
MGS2_FPV_TEST=1 MGS2_FPV_AT=5893,762,3886 check radio_shot "radio shot|reported Snake in person" ge 2 w01a --skip-cutscenes --snake-at=6000,0,2000 --fire-at 1 --aim-for 0.2 --shot-at 26
# Out of sight on the radio he looks about (sub 7: +-0x200 head turns).
MGS2_GOD=1 check radio_look "looks about on the radio" ge 1 w01a --skip-cutscenes --snake-at=6000,0,0 --snake-dir=0 --shot-at 12
# Darted from beyond earshot and still awake: he turns to look (state 7).
check dart_stung "feels a dart" ge 1 w01a --skip-cutscenes --snake-at=6000,0,-3000 --snake-dir=0 --fire-at 1 --shot-at 4
# A guard empties his magazine at an invulnerable Snake and reloads (the
# alert starts once his call for backup ends).
MGS2_GOD=1 check reload "reloads" ge 1 w01a --skip-cutscenes --snake-at=6000,0,0 --snake-dir=0 --shot-at 45
# A dart fired up from first person breaks a w03a lamp.
MGS2_FPV_TEST=1 MGS2_FPV_PITCH=41 check lamp "light 906464 shot" ge 1 w03a --skip-cutscenes --snake-at=-14800,-5000,-39250 --snake-dir=3072 --fire-at 2 --shot-at 4
# The deck's mirror reflects Snake once he is on it (mirror.c messages).
check mirror "reflects 28dce6" ge 1 w00b --skip-cutscenes --shot-at 4
# Items: a bandage stops the bleeding; cold medicine ends the sneezing.
MGS2_LIFE=25 MGS2_GIVE=0/5:1 MGS2_KEYS=N@2 check bandage "bandage used" ge 1 w01a --skip-cutscenes --shot-at 3
MGS2_COLD=120 MGS2_GIVE=0/4:1 MGS2_KEYS=N@4 check cold "cold medicine used" ge 1 w01a --skip-cutscenes --shot-at 6
MGS2_GOGGLES=1 check goggles "panicked" eq 0 w01a --skip-cutscenes --shot-at 3
# Deck 2: the manager's "atcomm" comes with the stage, and its four
# defenders (#3259e5) with it.
check defenders "script created .* 4 guards" ge 1 w03a --skip-cutscenes --shot-at 3
# In a held alert they run to atcomm's defence points and hold them.
MGS2_GOD=1 MGS2_ALERT_AT=2-30 check defence_hold "defender [0-9]+ holds point" ge 3 w03a --skip-cutscenes --shot-at 30
# Holding, the first two attack from their points (the pattern table in
# the resident image): out from cover, a burst, and back.
MGS2_GOD=1 MGS2_ALERT_AT=2-30 check defence_attack "defender [0-9]+ fires from point" ge 1 w03a --skip-cutscenes --snake-at=-19000,-5000,-127500 --shot-at 14
# Snake out of sight within 10 m of the first: pattern 4, the grenade.
# (The two who would chase Snake from where their routes start are taken
# down first.)
MGS2_DOWN=1:d@1,13:d@1 MGS2_GOD=1 MGS2_ALERT_AT=2-30 check defence_grenade "soldier's grenade goes off" ge 1 w03a --skip-cutscenes --snake-at=-21000,-5000,-125000 --shot-at 20
# With Snake at the front no point is nearer it than he is: they chase.
MGS2_GOD=1 MGS2_ALERT_AT=2-20 check defence_chase "takes point" eq 0 w03a --skip-cutscenes --snake-at=8000,-5000,-127000 --shot-at 12
# w03b's fight (story point 32): atcomm -t 1 holds the alert and gives
# each defender his own point; soldiers put to sleep come back in until
# the manager's -b count (5 on Normal) is used, then the -p proc runs.
W03B_FIGHT="a8:0xb4d=1,a16:0xe=32,a32:0x74=#272ebd"
MGS2_GOD=1 MGS2_BANKS=$W03B_FIGHT check w03b_hold "defender [0-9]+ holds point" ge 3 w03b --skip-cutscenes --shot-at 12
MGS2_DOWN=3:s@3,4:s@3,3:s@10,4:s@10,3:s@17,4:s@17 MGS2_GOD=1 MGS2_BANKS=$W03B_FIGHT check w03b_beaten "the defence is beaten" ge 1 w03b --skip-cutscenes --shot-at 26
# w03b's in-stage demos (chara "demo"): skipped, point 31's -n proc
# restarts into the fight; the fight won, point 33's demo leads to w04a.
MGS2_GOD=1 MGS2_BANKS="a8:0xb4d=1,a16:0xe=31" check w03b_demo "defender [0-9]+ holds point" ge 3 w03b --skip-cutscenes --shot-at 15
MGS2_DOWN=3:s@3,4:s@3,3:s@10,4:s@10,3:s@17,4:s@17 MGS2_GOD=1 MGS2_BANKS=$W03B_FIGHT check w03b_onward "loading w04a" ge 1 w03b --skip-cutscenes --shot-at 34

if [ "${1:-}" != quick ]; then
    for st in w00a w00b w00c w01a w01b w01c w01d w01e w01f w02a w03a w03b w04a w04b w04c; do
        MGS2_FUZZ=1 check "fuzz_$st" "panicked" eq 0 "$st" --skip-cutscenes --shot-at 25
    done
fi
echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]
