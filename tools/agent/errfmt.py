#!/usr/bin/env python3
"""Compact cargo JSON diagnostics, grouped and deduplicated."""
import argparse
import json
import sys
from collections import Counter


def first_line(msg):
    for line in msg.splitlines():
        if line.strip():
            return line.strip()
    return "(empty message)"


def collect(stream):
    groups = {}
    total = 0
    for line in stream:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        if msg.get("reason") != "compiler-message":
            continue
        d = msg.get("diagnostic") or {}
        if not d:
            continue
        pkg = (msg.get("package_id") or "?").split()[0]
        spans = d.get("spans") or []
        sp = next((s for s in spans if s.get("is_primary")), None)
        if sp is None:
            sp = next((s for s in spans if s.get("file_name")), None)
        if sp:
            loc = f"{sp.get('file_name', '?')}:{sp.get('line_start', 0)}:{sp.get('column_start', 0)}"
        else:
            loc = "-:0:0"
        level = d.get("level", "?")
        code = (d.get("code") or {}).get("code")
        text = first_line(d.get("message", ""))
        key = (loc, level, code, text)
        total += 1
        bucket = groups.setdefault(pkg, {})
        if key in bucket:
            bucket[key]["n"] += 1
        else:
            bucket[key] = {"loc": loc, "level": level, "code": code, "text": text, "n": 1}
    return groups, total


def main():
    ap = argparse.ArgumentParser(prog="errfmt")
    ap.add_argument("path", nargs="?")
    a = ap.parse_args()
    try:
        stream = open(a.path, encoding="utf-8", errors="replace") if a.path else sys.stdin
    except OSError as e:
        print(f"errfmt: {e}", file=sys.stderr)
        return 0
    try:
        groups, total = collect(stream)
    finally:
        if a.path:
            stream.close()

    rows = [r for bucket in groups.values() for r in bucket.values()]
    shown = 0
    for pkg, bucket in groups.items():
        if shown >= 30:
            break
        print(f"{pkg}:")
        for r in bucket.values():
            if shown >= 30:
                break
            code = f"[{r['code']}]" if r["code"] else ""
            suffix = f" (x{r['n']})" if r["n"] > 1 else ""
            print(f"  {r['loc']}: {r['level']}{code}: {r['text']}{suffix}")
            shown += 1
    if len(rows) > shown:
        print(f"  ... {len(rows) - shown} more")

    counts = Counter(r["level"] for r in rows)
    summary = ", ".join(f"{n} {lvl}" for lvl, n in sorted(counts.items())) or "none"
    print(f"summary: {len(rows)} unique of {total} messages ({summary})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
