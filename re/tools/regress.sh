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
# Guards: the box check and the rifle butt.
MGS2_BOX_TEST=1 check box_check "checks the box" ge 1 w01a --skip-cutscenes --snake-at=6000,0,1500 --shot-at 26
check close_quarters "life=(8|7)" ge 1 w01a --skip-cutscenes --trace --snake-at=6000,0,3300 --snake-dir=0 --shot-at 6
# A soldier finds his comrade asleep, kicks him awake and radios it in.
MGS2_DOWN=11:s@3 check comrade "radios in a downed comrade" ge 1 w01d --skip-cutscenes --shot-at 22
# ... with his voice line from the manager's -v table, then HQ's reply.
MGS2_DOWN=11:s@3 check radio_line "radio line 0x20" ge 2 w01d --skip-cutscenes --shot-at 22
# A guard empties his magazine at an invulnerable Snake and reloads.
MGS2_GOD=1 check reload "reloads" ge 1 w01a --skip-cutscenes --snake-at=6000,0,0 --snake-dir=0 --shot-at 14
# Items: a bandage stops the bleeding; cold medicine ends the sneezing.
MGS2_LIFE=25 MGS2_GIVE=0/5:1 MGS2_KEYS=N@2 check bandage "bandage used" ge 1 w01a --skip-cutscenes --shot-at 3
MGS2_COLD=120 MGS2_GIVE=0/4:1 MGS2_KEYS=N@4 check cold "cold medicine used" ge 1 w01a --skip-cutscenes --shot-at 6
MGS2_GOGGLES=1 check goggles "panicked" eq 0 w01a --skip-cutscenes --shot-at 3

if [ "${1:-}" != quick ]; then
    for st in w00a w00b w00c w01a w01b w01c w01d w01e w01f w02a w03a w03b w04a w04b w04c; do
        MGS2_FUZZ=1 check "fuzz_$st" "panicked" eq 0 "$st" --skip-cutscenes --shot-at 25
    done
fi
echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]
