#!/usr/bin/env python3
"""Git worktree lifecycle helper for agent workspaces."""
import argparse
import json
import shutil
import subprocess
import sys
from pathlib import Path


def git(*args, cwd=None, check=True):
    r = subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True)
    if check and r.returncode:
        sys.exit(r.stderr.strip() or f"git {args[0]} failed")
    return r


def repo_root():
    r = git("rev-parse", "--show-toplevel")
    return Path(r.stdout.strip()).resolve()


def wt_parent(root):
    return root.parent / f"{root.name}-wt"


def build_dir(root, name):
    return root.parent / ".cargo-build" / name


def notes_dir(root, name):
    return root.parent / "notes" / root.name / name


def worktrees(root):
    out = git("worktree", "list", "--porcelain", cwd=root).stdout
    rows, cur = [], None
    for line in out.splitlines():
        if not line:
            if cur:
                rows.append(cur)
            cur = None
            continue
        key, _, val = line.partition(" ")
        if key == "worktree":
            cur = {"path": Path(val), "branch": None, "bare": False}
        elif cur is None:
            continue
        elif key == "branch":
            cur["branch"] = val.removeprefix("refs/heads/")
        elif key in ("detached", "bare"):
            cur["bare"] = key == "bare"
    if cur:
        rows.append(cur)
    return rows


def dirty(path):
    if not path.exists():
        return True
    return bool(git("-C", str(path), "status", "--porcelain", check=False).stdout.strip())


def cmd_new(a):
    root = repo_root()
    path = wt_parent(root) / a.name
    branch = f"wt/{a.name}"
    if path.exists():
        sys.exit(f"path already exists: {path}")
    if git("show-ref", "--verify", "--quiet", f"refs/heads/{branch}", cwd=root, check=False).returncode == 0:
        sys.exit(f"branch already exists: {branch}")
    path.parent.mkdir(parents=True, exist_ok=True)
    git("worktree", "add", "-b", branch, str(path), a.base, cwd=root)

    cfg = path / ".cargo" / "config.toml"
    cfg.parent.mkdir(parents=True, exist_ok=True)
    cfg.write_text(f"[build]\nbuild-dir = {json.dumps(str(build_dir(root, a.name)))}\n")

    nd = notes_dir(root, a.name)
    nd.mkdir(parents=True, exist_ok=True)
    index = nd / "INDEX.md"
    if not index.exists():
        index.write_text(
            f"# {a.name}\n\n"
            f"- worktree: `{path}`\n"
            f"- branch: `{branch}`\n"
            f"- base: `{a.base}`\n"
            f"- build-dir: `{build_dir(root, a.name)}`\n\n"
            "## Notes\n"
        )
    print(path)


def cmd_list(a):
    for wt in worktrees(repo_root()):
        state = "dirty" if dirty(wt["path"]) else "clean"
        branch = wt["branch"] or "(detached)"
        print(f"{state:5}  {branch:28}  {wt['path']}")


def find_wt(root, name):
    for wt in worktrees(root):
        if wt["path"].name == name or wt["branch"] == f"wt/{name}":
            return wt
    sys.exit(f"no worktree named {name}")


def cmd_finish(a):
    root = repo_root()
    path = find_wt(root, a.name)["path"]
    if dirty(path) and not a.force:
        sys.exit(f"{path} is dirty; commit, stash, or pass --force")
    for d in (path / "target", build_dir(root, a.name)):
        if not d.exists():
            continue
        try:
            shutil.rmtree(d)
        except OSError as e:
            sys.exit(f"remove {d}: {e}")
    args = ["worktree", "remove"]
    if a.force:
        args.append("--force")
    git(*args, str(path), cwd=root)
    print(f"removed {path}")


def main():
    ap = argparse.ArgumentParser(prog="worktree")
    sub = ap.add_subparsers(dest="sub", required=True)

    p = sub.add_parser("new")
    p.add_argument("name")
    p.add_argument("--base", default="origin/main")
    p.set_defaults(fn=cmd_new)

    p = sub.add_parser("list")
    p.set_defaults(fn=cmd_list)

    p = sub.add_parser("finish")
    p.add_argument("name")
    p.add_argument("--force", action="store_true")
    p.set_defaults(fn=cmd_finish)

    a = ap.parse_args()
    return a.fn(a)


if __name__ == "__main__":
    sys.exit(main())
