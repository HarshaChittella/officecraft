#!/usr/bin/env bash
# Opens files in OfficeCraft.app the way Finder does (an open-documents Apple event via `open`),
# then checks that each became a tab, and quits it with the Quit Apple event (as the Dock does).
# Uses its own settings folder, so your recent files and tabs are untouched.
#
# Usage: scripts/finder-open-test.sh   (after packaging/macos/bundle.sh)
set -uo pipefail
cd "$(dirname "$0")/.."
app="$PWD/build/OfficeCraft.app"
work=$(mktemp -d)
settings="$work/settings"
mkdir -p "$settings"
[ -d "$app" ] || { echo "no $app: run packaging/macos/bundle.sh first"; exit 2; }

# Files to open: the Phase 2 check's saved ones (made by the apps themselves).
src=plan/p4-light
for f in "Q3 report.docx" "Budget.xlsx" "Pitch deck.pptx" "Contract.pdf"; do
  cp "$src/$f" "$work/$f" || { echo "missing $src/$f: run officecraft --check $src first"; exit 2; }
done

open -n --env OFFICECRAFT_SETTINGS_DIR="$settings" -a "$app" "$work/Q3 report.docx" "$work/Budget.xlsx" "$work/Pitch deck.pptx" "$work/Contract.pdf"
ok=0
for _ in $(seq 1 60); do
  sleep 1
  n=$(python3 -c "import json,sys; print(len(json.load(open('$settings/session.json'))['tabs']))" 2>/dev/null || echo 0)
  if [ "$n" -ge 4 ]; then ok=1; break; fi
done
echo "tabs after open: $(cat "$settings/session.json" 2>/dev/null)"
pid=$(pgrep -nf "$app/Contents/MacOS/officecraft")
osascript -e "tell application \"$app\" to quit" >/dev/null 2>&1
for _ in $(seq 1 15); do kill -0 "$pid" 2>/dev/null || break; sleep 1; done
if kill -0 "$pid" 2>/dev/null; then quit=0; kill "$pid"; else quit=1; fi
[ $ok = 1 ] && echo "PASS four files opened from an open-documents event are four tabs" || echo "FAIL the files didn't all become tabs"
[ $quit = 1 ] && echo "PASS the Quit Apple event quits the app" || echo "FAIL the Quit Apple event didn't quit it"

# Relaunch with no files: the last session comes back, minus a file deleted meanwhile.
rm "$work/Contract.pdf"
open -n --env OFFICECRAFT_SETTINGS_DIR="$settings" -a "$app"
restored=0
for _ in $(seq 1 60); do
  sleep 1
  n=$(python3 -c "import json; print(len(json.load(open('$settings/session.json'))['tabs']))" 2>/dev/null || echo 0)
  if [ "$n" = 3 ]; then restored=1; break; fi
done
echo "tabs after relaunch: $(cat "$settings/session.json" 2>/dev/null)"
pid=$(pgrep -nf "$app/Contents/MacOS/officecraft")
osascript -e "tell application \"$app\" to quit" >/dev/null 2>&1
for _ in $(seq 1 15); do kill -0 "$pid" 2>/dev/null || break; sleep 1; done
kill -0 "$pid" 2>/dev/null && kill "$pid"
[ $restored = 1 ] && echo "PASS relaunching restores the three tabs whose files still exist" || echo "FAIL the session wasn't restored"
[ $ok = 1 ] && [ $quit = 1 ] && [ $restored = 1 ]
