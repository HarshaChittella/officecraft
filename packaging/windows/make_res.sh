#!/usr/bin/env bash
# Makes packaging/windows/craftworks.res: the .exe's icon (the same artwork as the Mac icon, from
# crates/craftworks-shell/assets/make_icons.py) and its version information. The .res is kept in
# the repository, so a Windows build needs no resource compiler; rerun this when the icon or the
# version changes. Needs Python with Pillow, and llvm-rc (Homebrew's llvm).
set -euo pipefail
cd "$(dirname "$0")/../.."
out=packaging/windows
python3 - "$out/craftworks.ico" <<'PY'
import importlib.util, sys
from pathlib import Path
spec = importlib.util.spec_from_file_location("make_icons", "crates/craftworks-shell/assets/make_icons.py")
art = importlib.util.module_from_spec(spec); spec.loader.exec_module(art)
art.app_master().save(sys.argv[1], sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
PY
version=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)"/\1/')
IFS=. read -r major minor patch <<<"$version"
cat > "$out/craftworks.rc" <<RC
1 ICON "craftworks.ico"
1 VERSIONINFO
FILEVERSION $major,$minor,$patch,0
PRODUCTVERSION $major,$minor,$patch,0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Craftworks"
      VALUE "FileDescription", "Craftworks"
      VALUE "FileVersion", "$version"
      VALUE "InternalName", "craftworks"
      VALUE "OriginalFilename", "Craftworks.exe"
      VALUE "ProductName", "Craftworks"
      VALUE "ProductVersion", "$version"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
RC
llvm=$( (brew --prefix llvm 2>/dev/null || echo /opt/homebrew/opt/llvm) )/bin
( cd "$out" && "$llvm/llvm-rc" /fo craftworks.res craftworks.rc )
rm "$out/craftworks.rc"
echo "$out/craftworks.res"
