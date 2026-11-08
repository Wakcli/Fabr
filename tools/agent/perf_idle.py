#!/usr/bin/env python3
"""Measure idle CPU of the editor on a scene.

usage: perf_idle.py <scene> [seconds] [display]

Launches Xvfb + the built editor (EME_EDITOR_SCENE, no play), samples
/proc/<pid>/stat utime+stime for the requested window and prints CPU seconds
and percent of one core. Build the editor first with `cargo build -p eme_editor`.
"""

import os
import pathlib
import signal
import subprocess
import sys
import time

EDITOR = "/home/wak/.cargo-build/emberrly/debug/eme_editor"


def main() -> int:
    scene = os.path.abspath(sys.argv[1])
    seconds = int(sys.argv[2]) if len(sys.argv) > 2 else 30
    display = sys.argv[3] if len(sys.argv) > 3 else ":91"
    env = dict(os.environ)
    env.pop("WAYLAND_DISPLAY", None)
    env["DISPLAY"] = display
    env["EME_EDITOR_SCREEN"] = "editor"
    env["EME_EDITOR_SCENE"] = scene

    xvfb = subprocess.Popen(
        ["Xvfb", display, "-screen", "0", "1600x900x24", "-nolisten", "tcp"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    time.sleep(1.5)
    app = subprocess.Popen(
        [EDITOR], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
    )
    time.sleep(4)
    if app.poll() is not None:
        xvfb.kill()
        return 1

    def ticks() -> int:
        parts = pathlib.Path(f"/proc/{app.pid}/stat").read_text().split()
        return int(parts[13]) + int(parts[14])

    start = ticks()
    time.sleep(seconds)
    end = ticks()
    cpu = (end - start) / os.sysconf("SC_CLK_TCK")
    print(f"wall={seconds}s cpu={cpu:.2f}s pct={cpu / seconds * 100:.1f}%")

    app.send_signal(signal.SIGTERM)
    try:
        app.wait(timeout=8)
    except subprocess.TimeoutExpired:
        app.kill()
    xvfb.send_signal(signal.SIGTERM)
    try:
        xvfb.wait(timeout=5)
    except subprocess.TimeoutExpired:
        xvfb.kill()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
