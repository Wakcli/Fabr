#!/usr/bin/env python3
"""Build-slot limiter for agents sharing a machine."""
import argparse
import errno
import fcntl
import os
import subprocess
import sys
import time
from pathlib import Path


def log(msg):
    print(f"buildslot: {msg}", file=sys.stderr, flush=True)


def slots_default():
    raw = os.environ.get("EMBER_SLOTS", "2")
    try:
        n = int(raw)
    except ValueError:
        sys.exit(f"invalid EMBER_SLOTS: {raw!r}")
    if n < 1:
        sys.exit("EMBER_SLOTS must be >= 1")
    return n


def slot_dir():
    base = os.environ.get("XDG_RUNTIME_DIR")
    d = Path(base) / "emberrly-slots" if base else Path("/tmp/opencode/emberrly-slots")
    d.mkdir(parents=True, exist_ok=True)
    return d


def slot_path(i):
    return slot_dir() / f"slot-{i}.lock"


def parse_slot(raw):
    s = raw.strip()
    if s.startswith("slot-"):
        s = s[5:]
    if not s.isdigit() or int(s) < 1:
        sys.exit(f"bad slot id: {raw!r}")
    return int(s)


def try_lock(path):
    fd = os.open(path, os.O_CREAT | os.O_RDWR, 0o644)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError as e:
        os.close(fd)
        if e.errno in (errno.EAGAIN, errno.EACCES):
            return None
        raise
    return fd


def acquire(slots, timeout):
    deadline = time.monotonic() + timeout
    announced = False
    while True:
        busy = 0
        for i in range(1, slots + 1):
            fd = try_lock(slot_path(i))
            if fd is None:
                busy += 1
                continue
            os.ftruncate(fd, 0)
            os.write(fd, f"{os.getpid()}\n".encode())
            log(f"acquired slot-{i}" + (f", {busy} busy" if busy else ""))
            return i, fd
        if time.monotonic() >= deadline:
            log(f"timed out after {timeout:g}s waiting for one of {slots} slots")
            return None, None
        if not announced:
            log(f"waiting for a slot ({busy}/{slots} busy)")
            announced = True
        time.sleep(0.1)


def release_fd(fd):
    try:
        fcntl.flock(fd, fcntl.LOCK_UN)
    finally:
        os.close(fd)


def cmd_acquire(a):
    i, fd = acquire(a.slots, a.timeout)
    if fd is None:
        return 1
    print(f"slot-{i}", flush=True)
    return 0


def cmd_release(a):
    i = parse_slot(a.slot)
    p = slot_path(i)
    if not p.exists():
        log(f"slot-{i} has no lock file")
        return 1
    fd = try_lock(p)
    if fd is None:
        log(f"slot-{i} is held by another process")
        return 1
    release_fd(fd)
    log(f"slot-{i} was already free")
    return 0


def cmd_status(a):
    for i in range(1, a.slots + 1):
        fd = try_lock(slot_path(i))
        if fd is None:
            print(f"slot-{i} busy")
        else:
            release_fd(fd)
            print(f"slot-{i} free")
    return 0


def cmd_run(a):
    cmd = list(a.cmd)
    if cmd and cmd[0] == "--":
        cmd = cmd[1:]
    if not cmd:
        sys.exit("run needs a command after --")
    i, fd = acquire(a.slots, a.timeout)
    if fd is None:
        return 1
    try:
        if "CARGO_BUILD_JOBS" not in os.environ:
            jobs = max(1, (os.cpu_count() or 1) // a.slots)
            os.environ["CARGO_BUILD_JOBS"] = str(jobs)
            log(f"CARGO_BUILD_JOBS={jobs}")
        try:
            rc = subprocess.run(cmd).returncode
        except OSError as e:
            log(f"exec {cmd[0]}: {e}")
            return 127
        if rc < 0:
            log(f"command killed by signal {-rc}")
            return 128 - rc
        return rc
    finally:
        release_fd(fd)
        log(f"released slot-{i}")


def main():
    ap = argparse.ArgumentParser(prog="buildslot")
    sub = ap.add_subparsers(dest="sub", required=True)

    def common(p):
        p.add_argument("--slots", type=int)
        p.add_argument("--timeout", type=float, default=1800)

    p = sub.add_parser("acquire")
    common(p)
    p.set_defaults(fn=cmd_acquire)

    p = sub.add_parser("release")
    p.add_argument("slot")
    p.set_defaults(fn=cmd_release)

    p = sub.add_parser("status")
    common(p)
    p.set_defaults(fn=cmd_status)

    p = sub.add_parser("run")
    common(p)
    p.add_argument("cmd", nargs=argparse.REMAINDER)
    p.set_defaults(fn=cmd_run)

    a = ap.parse_args()
    if getattr(a, "slots", None) is None:
        a.slots = slots_default()
    if a.slots < 1:
        sys.exit("--slots must be >= 1")
    return a.fn(a)


if __name__ == "__main__":
    sys.exit(main())
