#!/usr/bin/env python3
"""Architecture gate: crate layering and file-size limits."""
import argparse
import json
import subprocess
import sys
import tomllib
from pathlib import Path

SKIP_PARTS = ("/tests/", "/benches/", "/examples/")
DEFAULT_MAX = 1000
DEFAULT_POLICY = """[layers]

[same_layer_ok]
pairs = []

[limits]
max_file_lines = 1000
"""


def cargo_meta(root):
    r = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        cwd=root, capture_output=True, text=True,
    )
    if r.returncode:
        sys.exit(f"cargo metadata failed:\n{r.stderr.strip()}")
    return json.loads(r.stdout)


def build_graph(meta):
    names = {p["name"] for p in meta["packages"]}
    edges = {}
    for p in meta["packages"]:
        deps = set()
        for d in p["dependencies"]:
            if d.get("kind") is None and d.get("path") and d["name"] in names:
                deps.add(d["name"])
        edges[p["name"]] = deps
    return names, edges


def assign_layers(names, edges):
    layers = {}

    def visit(n, stack):
        if n in layers:
            return layers[n]
        if n in stack:
            sys.exit(f"dependency cycle: {' -> '.join([*stack, n])}")
        stack.append(n)
        layers[n] = 1 + max((visit(d, stack) for d in edges.get(n, ())), default=0)
        stack.pop()
        return layers[n]

    for n in sorted(names):
        visit(n, [])
    return layers


def same_pairs(policy):
    raw = policy.get("same_layer_ok", {}).get("pairs", [])
    return {frozenset(p) for p in raw if isinstance(p, list) and len(p) == 2}


def violated_edges(edges, layers, allowed):
    out = []
    for a in sorted(edges):
        la = layers.get(a)
        if la is None:
            continue
        for b in sorted(edges[a]):
            lb = layers.get(b)
            if lb is None:
                continue
            if lb < la or (lb == la and frozenset((a, b)) in allowed):
                continue
            out.append(f"{a} -> {b}")
    return out


def oversized(root, limit):
    out = []
    for top in ("crates", "src"):
        base = root / top
        if not base.is_dir():
            continue
        for p in base.rglob("*.rs"):
            rel = "/" + p.relative_to(root).as_posix()
            if p.name == "tests.rs" or any(s in rel for s in SKIP_PARTS):
                continue
            try:
                n = sum(1 for _ in p.open("r", encoding="utf-8", errors="replace"))
            except OSError:
                continue
            if n > limit:
                out.append((rel.lstrip("/"), n))
    return sorted(out, key=lambda x: (-x[1], x[0]))


def split_sections(text):
    pre, secs, cur = [], [], None
    for line in text.splitlines():
        s = line.strip()
        if s.startswith("[") and s.endswith("]") and not s.startswith("[["):
            cur = [s[1:-1].strip(), []]
            secs.append(cur)
        elif cur is None:
            pre.append(line)
        else:
            cur[1].append(line)
    return pre, secs


def render_policy(text, layers, grand):
    pre, secs = split_sections(text)
    out = "\n".join(pre).strip("\n")
    if out:
        out += "\n\n"
    out += "[layers]\n"
    out += "".join(f"{json.dumps(k)} = {layers[k]}\n" for k in sorted(layers))
    out += "\n[grandfathered]\nedges = [\n"
    out += "".join(f"  {json.dumps(e)},\n" for e in grand)
    out += "]\n"
    for name, body in secs:
        if name in ("layers", "grandfathered"):
            continue
        out += f"\n[{name}]\n"
        chunk = "\n".join(body).strip("\n")
        if chunk:
            out += chunk + "\n"
    return out


def read_policy(path):
    return tomllib.loads(path.read_text())


def write_policy(path, names, edges):
    layers = assign_layers(names, edges)
    text = path.read_text() if path.exists() else DEFAULT_POLICY
    allowed = same_pairs(read_policy(path)) if path.exists() else set()
    grand = violated_edges(edges, layers, allowed)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(render_policy(text, layers, grand))
    print(f"wrote {path}: {len(layers)} layers, {len(grand)} grandfathered edges")


def main():
    ap = argparse.ArgumentParser(prog="archcheck")
    ap.add_argument("--root", default=".")
    ap.add_argument("--policy", default="tools/architecture/policy.toml")
    ap.add_argument("--write-policy", action="store_true")
    ap.add_argument("--json", action="store_true")
    a = ap.parse_args()

    root = Path(a.root).resolve()
    ppath = Path(a.policy)
    if not ppath.is_absolute():
        ppath = root / ppath
    meta = cargo_meta(root)
    names, edges = build_graph(meta)

    if a.write_policy:
        write_policy(ppath, names, edges)
        return 0

    policy = read_policy(ppath) if ppath.exists() else tomllib.loads(DEFAULT_POLICY)
    declared = {k: v for k, v in policy.get("layers", {}).items() if isinstance(v, int)}
    missing = sorted(names - set(declared))
    grand = set(policy.get("grandfathered", {}).get("edges", []))
    raw_bad = violated_edges(edges, declared, same_pairs(policy))
    bad = [e for e in raw_bad if e not in grand]
    limit = policy.get("limits", {}).get("max_file_lines", DEFAULT_MAX)
    big = oversized(root, limit)
    n_edges = sum(len(v) for v in edges.values())

    ok = not missing and not bad and not big
    result = {
        "ok": ok,
        "members": len(names),
        "edges": n_edges,
        "missing_layers": missing,
        "layer_violations": bad,
        "max_file_lines": limit,
        "oversized": [{"path": p, "lines": n} for p, n in big],
    }
    if a.json:
        print(json.dumps(result, indent=2))
    else:
        print(f"members: {len(names)}")
        print(f"edges:   {n_edges} normal workspace deps")
        print(f"layers:  {len(declared)} declared")
        if missing:
            print(f"missing from [layers] ({len(missing)}): {' '.join(missing)}")
        if bad:
            print(f"layer violations ({len(bad)}):")
            for e in bad[:20]:
                print(f"  {e}")
            if len(bad) > 20:
                print(f"  ... {len(bad) - 20} more")
        else:
            print("layer violations: 0")
        if big:
            print(f"files over {limit} lines ({len(big)}):")
            for p, n in big[:20]:
                print(f"  {n:6d}  {p}")
            if len(big) > 20:
                print(f"  ... {len(big) - 20} more")
        else:
            print(f"files over {limit} lines: 0")
        print("OK" if ok else "FAILED")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
