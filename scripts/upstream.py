#!/usr/bin/env python3
"""Follows the Craft apps' releases: WordCraft, GridCraft, DeckCraft and PdfCraft (docs/updates.md).

    upstream.py status [--json] [--installed FILE|-]
        Each app's newest published release, against the versions Craftworks is built from
        (vendor/versions.json, or FILE, or the JSON on stdin with `-`: what the app embeds).

    upstream.py sync [--app ID]... [--to REF] [--no-install] [--skip-check] [--dry-run]
        Moves our patches onto each newer release, aligns craft-libs and egui, builds, tests,
        runs --check in light and dark, bundles, installs to /Applications and commits. Any
        failure puts everything back as it was: nothing is committed or installed.
        --to REF syncs to an upstream ref instead of the release (a rehearsal, e.g.
        origin/release); it needs --no-install and is never committed to main.

    upstream.py init
        Writes vendor/versions.json for what is checked out now.

Lines starting with STAGE, LOG, INSTALLED and RESULT are for Craftworks, which runs this from its
"Update available" pill (the local rebuild mode). Everything is also written to the log.
Nothing here touches a remote: forks are not pushed.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VERSIONS = ROOT / "vendor/versions.json"
APPS = {
    "wordcraft": "WordCraft",
    "gridcraft": "GridCraft",
    "deckcraft": "DeckCraft",
    "pdfcraft": "PdfCraft",
}
BRANCH = "craftworks/hosted"
CACHE = ROOT / "target/upstream"
EGUI_URL = "https://github.com/storytold/egui.git"
INSTALL_TO = Path("/Applications/Craftworks.app")
LOG_DIR = Path.home() / "Library/Logs/Craftworks"

# Launched from the app, PATH is the system's bare one: find cargo and the developer tools.
os.environ["PATH"] = os.pathsep.join([str(Path.home() / ".cargo/bin"), "/opt/homebrew/bin", "/usr/local/bin", os.environ.get("PATH", "/usr/bin:/bin")])

log_file = None


class Failed(Exception):
    def __init__(self, stage: str, message: str):
        super().__init__(f"{stage}: {message}")
        self.stage, self.message = stage, message


def say(line: str) -> None:
    print(line, flush=True)
    if log_file:
        log_file.write(line + "\n")
        log_file.flush()


def stage(name: str, text: str) -> None:
    say(f"STAGE {name}: {text}")


def run(cmd: list[str], cwd: Path = ROOT, check: bool = True, quiet: bool = False) -> str:
    """Runs `cmd`, its output going to the log. Returns stdout."""
    if log_file:
        log_file.write(f"$ {' '.join(cmd)}  (in {cwd})\n")
        log_file.flush()
    p = subprocess.run(cmd, cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    if log_file and not quiet:
        log_file.write(p.stdout)
        log_file.flush()
    if check and p.returncode != 0:
        tail = "\n".join(p.stdout.strip().splitlines()[-12:])
        raise subprocess.CalledProcessError(p.returncode, cmd, output=tail)
    return p.stdout


def git(repo: Path, *args: str, check: bool = True) -> str:
    return run(["git", *args], cwd=repo, check=check, quiet=True).strip()


def is_ancestor(repo: Path, a: str, b: str) -> bool:
    return subprocess.run(["git", "merge-base", "--is-ancestor", a, b], cwd=repo, capture_output=True).returncode == 0


def version_tuple(v: str) -> tuple:
    m = re.match(r"v?(\d+)\.(\d+)\.(\d+)", v or "")
    return tuple(int(x) for x in m.groups()) if m else (0, 0, 0)


# ---- releases ---------------------------------------------------------------------------------


def latest_release(app: str) -> dict:
    """The app's newest published, stable release, as craft-update chooses it."""
    # curl, not urllib: it uses the system's certificates, which some Pythons on macOS don't.
    cmd = ["/usr/bin/curl", "-sS", "--fail-with-body", "--max-time", "20", "-H", "Accept: application/vnd.github+json", "-A", "craftworks-upstream"]
    token = os.environ.get("GITHUB_TOKEN")
    if token:
        cmd += ["-H", f"Authorization: Bearer {token}"]
    p = subprocess.run([*cmd, f"https://api.github.com/repos/storytold/{app}/releases?per_page=30"], capture_output=True, text=True)
    if p.returncode != 0:
        limited = "rate limit" in p.stdout.lower()
        raise ConnectionError("GitHub's request limit was reached; try again in an hour" if limited else f"Couldn't reach GitHub ({(p.stderr or p.stdout).strip()[:160]})")
    releases = json.loads(p.stdout)
    stable = [r for r in releases if not r.get("draft") and not r.get("prerelease") and re.fullmatch(r"v\d+\.\d+\.\d+", r.get("tag_name", ""))]
    if not stable:
        raise ValueError(f"{app} has no published release")
    best = max(stable, key=lambda r: version_tuple(r["tag_name"]))
    return {"version": best["tag_name"].lstrip("v"), "tag": best["tag_name"], "url": best["html_url"], "published": (best.get("published_at") or "")[:10]}


def load_versions(path: str | None = None) -> dict:
    if path == "-":
        return json.load(sys.stdin)
    return json.loads(Path(path or VERSIONS).read_text())


def cmd_status(args) -> int:
    try:
        have = load_versions(args.installed)
    except (OSError, ValueError) as e:
        return report_status({"ok": False, "error": f"Couldn't read the versions Craftworks is built from: {e}"}, args.json)
    apps = []
    try:
        for app, name in APPS.items():
            rel = latest_release(app)
            mine = have["apps"][app]["release"]
            apps.append({"id": app, "name": name, "have": mine, "latest": rel["version"], "newer": version_tuple(rel["version"]) > version_tuple(mine), "url": rel["url"], "published": rel["published"]})
    except (ConnectionError, ValueError) as e:
        return report_status({"ok": False, "error": str(e)}, args.json)
    return report_status({"ok": True, "checked": dt.datetime.now().isoformat(timespec="seconds"), "apps": apps, "any": any(a["newer"] for a in apps)}, args.json)


def report_status(result: dict, as_json: bool) -> int:
    if as_json:
        print(json.dumps(result))
    elif not result["ok"]:
        print(result["error"])
    else:
        for a in result["apps"]:
            mark = "update" if a["newer"] else "current"
            print(f"{a['name']:<10} built from {a['have']:<8} latest {a['latest']:<8} {mark}")
    return 0 if result["ok"] else 1


# ---- init -------------------------------------------------------------------------------------


def egui_rev() -> str:
    m = re.search(r'egui = \{ git = "https://github.com/storytold/egui.git", rev = "([0-9a-f]+)"', (ROOT / "Cargo.toml").read_text())
    if not m:
        raise Failed("align", "Cargo.toml has no egui patch")
    return m.group(1)


def contained_release(repo: Path, base: str) -> str:
    """The newest release whose tested main is already in `base`."""
    tags = sorted(git(repo, "tag", "--list", "v*").split(), key=version_tuple, reverse=True)
    for tag in tags:
        if not re.fullmatch(r"v\d+\.\d+\.\d+", tag):
            continue
        tested = release_source(repo, tag)
        if is_ancestor(repo, tested, base):
            return tag.lstrip("v")
    return "0.0.0"


def release_source(repo: Path, tag: str) -> str:
    """What a release was built from: its tag, or the tested main it merges when the release
    branch adds nothing of its own (upstream's "Merge tested main for vX")."""
    second = git(repo, "rev-parse", "--verify", "--quiet", f"{tag}^2", check=False)
    if second and subprocess.run(["git", "diff", "--quiet", second, tag], cwd=repo).returncode == 0:
        return second
    return git(repo, "rev-parse", f"{tag}^{{commit}}")


def cmd_init(_args) -> int:
    data = {"note": "What Craftworks is built from. Written by scripts/upstream.py; see docs/updates.md.", "apps": {}, "craft_libs": git(ROOT / "vendor/craft-libs", "rev-parse", "HEAD"), "egui": egui_rev()}
    for app in APPS:
        repo = ROOT / "vendor" / app
        git(repo, "fetch", "-q", "--tags", "origin", "main", "release")
        base = git(repo, "merge-base", "HEAD", "origin/main")
        data["apps"][app] = {"release": contained_release(repo, base), "base": base, "head": git(repo, "rev-parse", "HEAD")}
    VERSIONS.write_text(json.dumps(data, indent=2) + "\n")
    print(json.dumps(data, indent=2))
    return 0


# ---- sync -------------------------------------------------------------------------------------


def newest(repo: Path, revs: list[str], what: str) -> str:
    """The one of `revs` every other is an ancestor of."""
    revs = list(dict.fromkeys(revs))
    for r in revs:
        if all(is_ancestor(repo, o, r) for o in revs):
            return r
    raise Failed("align", f"the apps need {what} revisions that don't build on one another: {', '.join(r[:9] for r in revs)}")


def pins(text: str, url: str) -> list[str]:
    return re.findall(rf'git = "{re.escape(url)}"[^}}\n]*?rev = "([0-9a-f]+)"', text) + re.findall(rf'rev = "([0-9a-f]+)"[^}}\n]*?git = "{re.escape(url)}"', text)


def egui_cache() -> Path:
    repo = CACHE / "egui.git"
    if not repo.exists():
        CACHE.mkdir(parents=True, exist_ok=True)
        run(["git", "clone", "-q", "--bare", "--filter=blob:none", EGUI_URL, str(repo)], cwd=CACHE)
    else:
        git(repo, "fetch", "-q", "origin", "+refs/heads/*:refs/heads/*")
    return repo


def cmd_sync(args) -> int:
    global log_file
    LOG_DIR.mkdir(parents=True, exist_ok=True)
    log_path = LOG_DIR / f"update-{dt.datetime.now():%Y%m%d-%H%M%S}.log"
    log_file = log_path.open("w")
    say(f"LOG {log_path}")
    if args.to and not args.no_install:
        say("RESULT failed setup: --to is a rehearsal: add --no-install")
        return 2
    CACHE.mkdir(parents=True, exist_ok=True)
    lock = CACHE / "sync.lock"
    try:
        fd = os.open(lock, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
        os.write(fd, str(os.getpid()).encode())
        os.close(fd)
    except FileExistsError:
        say(f"RESULT failed setup: another update is running (remove {lock} if not)")
        return 2
    state: dict = {}
    try:
        summary = sync(args, state)
        say(f"RESULT ok {summary}")
        return 0
    except Failed as e:
        rollback(state)
        say(f"RESULT failed {e.stage}: {e.message}")
        return 1
    except subprocess.CalledProcessError as e:
        rollback(state)
        say(f"RESULT failed {state.get('stage', 'build')}: {' '.join(e.cmd[:3])} failed\n{e.output or ''}")
        return 1
    except Exception as e:  # noqa: BLE001 - anything unexpected must still put things back
        rollback(state)
        say(f"RESULT failed {state.get('stage', 'setup')}: {e}")
        return 1
    finally:
        lock.unlink(missing_ok=True)


def sync(args, state: dict) -> str:
    def at(name: str, text: str) -> None:
        state["stage"] = name
        stage(name, text)

    at("check", "Looking for new releases")
    if git(ROOT, "status", "--porcelain", "--untracked-files=no"):
        raise Failed("check", "the Craftworks checkout has uncommitted changes; commit or stash them first")
    if git(ROOT, "branch", "--show-current") != "main":
        raise Failed("check", "the Craftworks checkout isn't on main")
    have = load_versions()
    wanted = args.app or list(APPS)
    targets = {}
    for app in wanted:
        repo = ROOT / "vendor" / app
        if git(repo, "status", "--porcelain", "--untracked-files=no"):
            raise Failed("check", f"vendor/{app} has uncommitted changes")
        if git(repo, "branch", "--show-current") != BRANCH:
            raise Failed("check", f"vendor/{app} isn't on {BRANCH}")
        git(repo, "fetch", "-q", "--tags", "origin", "main", "release")
        if args.to:
            target, label = git(repo, "rev-parse", f"{args.to}^{{commit}}"), f"{args.to} ({git(repo, 'describe', '--tags', args.to, check=False)})"
            version = have["apps"][app]["release"]
        else:
            rel = latest_release(app)
            if version_tuple(rel["version"]) <= version_tuple(have["apps"][app]["release"]):
                continue
            target, label, version = release_source(repo, rel["tag"]), rel["tag"], rel["version"]
        targets[app] = {"target": target, "label": label, "version": version}
    if not targets:
        return "Craftworks already has the newest releases"
    say("Releases to take: " + ", ".join(f"{APPS[a]} {t['label']}" for a, t in targets.items()))
    if args.dry_run:
        return "dry run: " + ", ".join(f"{APPS[a]} {t['label']}" for a, t in targets.items())

    # From here on, everything changed is recorded in `state` so a failure can put it back.
    state["root_head"] = git(ROOT, "rev-parse", "HEAD")
    state["subs"] = {}
    new = json.loads(json.dumps(have))
    moved = []
    for app, t in targets.items():
        repo = ROOT / "vendor" / app
        at("rebase", f"Moving our changes onto {APPS[app]} {t['label']}")
        head, base = git(repo, "rev-parse", "HEAD"), have["apps"][app]["base"]
        state["subs"][app] = head
        if is_ancestor(repo, t["target"], base):
            say(f"{APPS[app]} {t['label']} is already in what we build")
            new["apps"][app]["release"] = t["version"]
            continue
        patches = git(repo, "rev-list", "--reverse", "--no-merges", f"{base}..{head}").split()
        git(repo, "branch", "-f", f"{BRANCH}-before-sync", head)
        git(repo, "switch", "-q", "-C", BRANCH, t["target"])
        for p in patches:
            r = subprocess.run(["git", "cherry-pick", "--allow-empty", p], cwd=repo, capture_output=True, text=True)
            if r.returncode != 0:
                files = git(repo, "diff", "--name-only", "--diff-filter=U", check=False)
                subprocess.run(["git", "cherry-pick", "--abort"], cwd=repo, capture_output=True)
                raise Failed("rebase", f"our change \"{git(repo, 'log', '-1', '--format=%s', p)}\" no longer applies to {APPS[app]} {t['label']} ({files.replace(chr(10), ', ') or 'see log'})")
        new["apps"][app].update({"release": t["version"], "base": t["target"], "head": git(repo, "rev-parse", "HEAD")})
        moved.append(app)
        say(f"{APPS[app]}: {len(patches)} change(s) moved onto {t['label']}")

    if not moved:
        # Every new release was already in what we build: only the record changes.
        VERSIONS.write_text(json.dumps(new, indent=2) + "\n")
        if not args.to:
            git(ROOT, "add", "vendor/versions.json")
            git(ROOT, "commit", "-q", "-m", "Sync upstream: record releases already included")
        return "the new releases were already included; nothing to rebuild"

    at("align", "Choosing one craft-libs and one egui for all four apps")
    libs = ROOT / "vendor/craft-libs"
    state["craft_libs"] = git(libs, "rev-parse", "HEAD")
    git(libs, "fetch", "-q", "origin")
    texts = [(ROOT / "vendor" / a / "Cargo.toml").read_text() for a in APPS]
    lib_revs = [state["craft_libs"]] + [r for t in texts for r in pins(t, "https://github.com/storytold/craft-libs")]
    for r in lib_revs:
        if not git(libs, "cat-file", "-t", r, check=False):
            raise Failed("align", f"craft-libs revision {r[:9]} isn't on GitHub's craft-libs")
    chosen_lib = newest(libs, lib_revs, "craft-libs")
    if chosen_lib != state["craft_libs"]:
        git(libs, "checkout", "-q", chosen_lib)
        say(f"craft-libs {state['craft_libs'][:9]} -> {chosen_lib[:9]}")
    egui = egui_cache()
    old_egui = egui_rev()
    chosen_egui = newest(egui, [old_egui] + [r for t in texts for r in pins(t, EGUI_URL)], "egui")
    if chosen_egui != old_egui:
        cargo = ROOT / "Cargo.toml"
        cargo.write_text(cargo.read_text().replace(old_egui, chosen_egui))
        say(f"egui {old_egui[:9]} -> {chosen_egui[:9]}")
    new.update({"craft_libs": chosen_lib, "egui": chosen_egui})
    VERSIONS.write_text(json.dumps(new, indent=2) + "\n")

    at("build", "Building Craftworks (several minutes)")
    run(["cargo", "build", "--profile", "measure", "-p", "craftworks"])
    at("test", "Running the tests")
    run(["cargo", "test", "-p", "craftworks-shell", "-p", "craftworks-core", "-p", "craftworks"])
    if not args.skip_check:
        for mode in ("light", "dark"):
            at("check", f"Running the acceptance check ({mode}): a test window opens for a few minutes")
            out = CACHE / f"check-{dt.datetime.now():%Y%m%d-%H%M%S}-{mode}"
            out.mkdir(parents=True)
            run([str(ROOT / "target/measure/craftworks"), "--check", str(out), "--appearance", mode], check=False)
            report = next(out.glob("report*.txt"), None)
            text = report.read_text() if report else ""
            if "ALL PASS" not in text:
                failed = [line for line in text.splitlines() if line.startswith("FAIL")][:5]
                raise Failed("check", f"the {mode} acceptance check failed: {'; '.join(failed) or 'no report'} (screenshots in {out})")

    summary = ", ".join(f"{APPS[a]} {t['version'] if not args.to else t['label']}" for a, t in targets.items())
    if args.no_install:
        if args.to:
            # A rehearsal: everything passed; put the checkout back as it was.
            rollback(state)
            return f"rehearsal passed ({summary}); the checkout is back as it was"
    else:
        at("install", "Packaging and installing Craftworks")
        app_dir = ROOT / "build/next/Craftworks.app"
        env = dict(os.environ, CRAFTWORKS_APP=str(app_dir.relative_to(ROOT)))
        subprocess.run(["packaging/macos/bundle.sh", "release"], cwd=ROOT, env=env, check=True, stdout=log_file, stderr=subprocess.STDOUT)
        run(["codesign", "--verify", "--deep", str(app_dir)])
        # Fine while Craftworks runs: rsync replaces each file by renaming a new one over it.
        run(["rsync", "-a", "--delete", f"{app_dir}/", f"{INSTALL_TO}/"])
        run(["touch", str(INSTALL_TO)])
        run(["/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister", "-f", str(INSTALL_TO)], check=False)
        say(f"INSTALLED {INSTALL_TO}")

    at("commit", "Recording what Craftworks is built from")
    git(ROOT, "add", "Cargo.toml", "Cargo.lock", "vendor/versions.json", "vendor/craft-libs", *[f"vendor/{a}" for a in targets])
    git(ROOT, "commit", "-q", "-m", f"Sync upstream: {summary}\n\nWritten by scripts/upstream.py sync (docs/updates.md).")
    for app in targets:
        git(ROOT / "vendor" / app, "branch", "-D", f"{BRANCH}-before-sync", check=False)
    return f"updated to {summary}"


def rollback(state: dict) -> None:
    """Puts the checkout back as it was before the sync. Nothing was committed or installed."""
    for app, head in state.get("subs", {}).items():
        repo = ROOT / "vendor" / app
        subprocess.run(["git", "cherry-pick", "--abort"], cwd=repo, capture_output=True)
        subprocess.run(["git", "switch", "-q", "-C", BRANCH, head], cwd=repo, capture_output=True)
        subprocess.run(["git", "branch", "-D", f"{BRANCH}-before-sync"], cwd=repo, capture_output=True)
    if "craft_libs" in state:
        subprocess.run(["git", "checkout", "-q", state["craft_libs"]], cwd=ROOT / "vendor/craft-libs", capture_output=True)
    if "root_head" in state:
        subprocess.run(["git", "checkout", "-q", "--", "Cargo.toml", "Cargo.lock", "vendor/versions.json"], cwd=ROOT, capture_output=True)
    state.clear()


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("status")
    s.add_argument("--json", action="store_true")
    s.add_argument("--installed")
    y = sub.add_parser("sync")
    y.add_argument("--app", action="append", choices=list(APPS))
    y.add_argument("--to")
    y.add_argument("--no-install", action="store_true")
    y.add_argument("--skip-check", action="store_true")
    y.add_argument("--dry-run", action="store_true")
    sub.add_parser("init")
    args = ap.parse_args()
    return {"status": cmd_status, "sync": cmd_sync, "init": cmd_init}[args.cmd](args)


if __name__ == "__main__":
    sys.exit(main())
