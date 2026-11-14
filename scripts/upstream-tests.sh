#!/usr/bin/env bash
# Run each vendored Craft repo's own test suite on Craftworks' shared pins (docs/PLAN.md task 0.6).
#
# Each repo is tested inside its own checkout, with `--config` overrides that move it onto the
# pins our workspace uses (egui 725a39f, krilla 6e03f65, craft-i18n 1e51542). Nothing in the
# vendored repo is edited: the Cargo.lock that cargo rewrites is restored afterwards.
#
# Usage: scripts/upstream-tests.sh [repo...]        (default: pdfcraft wordcraft gridcraft deckcraft)
# Env:   CW_TEST_ARGS   extra args for `cargo test` (default: --workspace)
#        CW_TARGET_ROOT where per-repo target dirs go (default: target/upstream)
set -uo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
logs="$root/plan/logs"
target_root="${CW_TARGET_ROOT:-$root/target/upstream}"
mkdir -p "$logs" "$target_root"

egui_rev="725a39fca940f77149ffbb9c58eef405a16997bc"
krilla_rev="6e03f65bf45181db77ba6229716e6624db9eb25b"
egui_crates=(ecolor emath epaint epaint_default_fonts egui egui-winit egui-wgpu egui_glow egui_extras
  egui_kittest egui_system_font_provider eframe egui_demo_lib egui_inspection)

overrides=()
for c in "${egui_crates[@]}"; do
  overrides+=(--config "patch.crates-io.$c.git=\"https://github.com/storytold/egui.git\"")
  overrides+=(--config "patch.crates-io.$c.rev=\"$egui_rev\"")
done
overrides+=(--config "patch.crates-io.krilla.git=\"https://github.com/storytold/craft-libs\"")
overrides+=(--config "patch.crates-io.krilla.rev=\"$krilla_rev\"")

# CW_ALIGN=1: also put every craft-libs crate on our single checkout (vendor/craft-libs), the
# way the workspace's own patch block does. Reads the crate list from the root Cargo.toml.
if [ "${CW_ALIGN:-0}" = 1 ]; then
  while IFS= read -r line; do
    name="${line%% =*}"
    path="$(sed -E 's/.*path = "([^"]+)".*/\1/' <<<"$line")"
    overrides+=(--config "patch.\"https://github.com/storytold/craft-libs\".$name.path=\"$root/$path\"")
  done < <(sed -n '/^\[patch\."https:\/\/github.com\/storytold\/craft-libs"\]/,/^\[/p' "$root/Cargo.toml" | grep -E '^craft-[a-z-]+ = \{ path')
fi

repos=("$@")
[ ${#repos[@]} -eq 0 ] && repos=(pdfcraft wordcraft gridcraft deckcraft)

status=0
for repo in "${repos[@]}"; do
  dir="$root/vendor/$repo"
  log="$logs/upstream-test-$repo${CW_ALIGN:+-aligned}-$(date +%Y%m%d-%H%M%S).log"
  echo "== $repo → $log"
  (
    cd "$dir" || exit 1
    export CRAFT_FONTS_DIR="$root/vendor/craft-fonts"
    export CRAFT_I18N_DIR="${CW_I18N_DIR:-$root/vendor/craft-i18n}"
    export CARGO_TARGET_DIR="$target_root/$repo${CW_ALIGN:+-aligned}"
    # shellcheck disable=SC2086
    cargo test "${overrides[@]}" --no-fail-fast ${CW_TEST_ARGS:---workspace}
  ) >"$log" 2>&1
  rc=$?
  git -C "$dir" checkout -q -- Cargo.lock 2>/dev/null
  passed=$(grep -Eo '[0-9]+ passed' "$log" | awk '{s+=$1} END {print s+0}')
  failed=$(grep -Eo '[0-9]+ failed' "$log" | awk '{s+=$1} END {print s+0}')
  echo "   exit=$rc passed=$passed failed=$failed"
  [ $rc -ne 0 ] && status=1
done
exit $status
