#!/usr/bin/env python3
"""Writes OfficeCraft's app icon into an .iconset folder.

The artwork is drawn in code by crates/officecraft-shell/assets/make_icons.py (`app_master`):
a fan of four glass sheets on a blue glass tile, the same mark the app shows on its Home tab.

Usage: make_icon.py OUT.iconset
"""
import importlib.util
import sys
from pathlib import Path
from PIL import Image

ART = Path(__file__).resolve().parents[2] / "crates/officecraft-shell/assets/make_icons.py"


def main() -> None:
    spec = importlib.util.spec_from_file_location("make_icons", ART)
    art = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(art)
    img = art.app_master()
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    for base in (16, 32, 128, 256, 512):
        img.resize((base, base), Image.LANCZOS).save(out / f"icon_{base}x{base}.png")
        img.resize((base * 2, base * 2), Image.LANCZOS).save(out / f"icon_{base}x{base}@2x.png")


if __name__ == "__main__":
    main()
