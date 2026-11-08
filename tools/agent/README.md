# Agent tools

Python 3.11+, stdlib only. Run from the repository root.

## Build slots

Serializes heavy builds across agents. Locks live in `$XDG_RUNTIME_DIR/emberrly-slots`
(fallback `/tmp/opencode/emberrly-slots`). `EMBER_SLOTS` sets the default slot count (2).

```sh
python3 tools/agent/buildslot.py status
python3 tools/agent/buildslot.py acquire --slots 2 --timeout 1800
python3 tools/agent/buildslot.py release slot-1
python3 tools/agent/buildslot.py run --slots 2 -- cargo check -p eme_ecs --tests
```

`run` sets `CARGO_BUILD_JOBS` to `max(1, cpu_count // slots)` unless it is already set,
and forwards the command's exit code.

## Worktrees

`new` creates `../<repo>-wt/<name>` on branch `wt/<name>`, writes a gitignored
`.cargo/config.toml` pointing at `../.cargo-build/<name>`, and creates
`../notes/<repo>/<name>/INDEX.md`.

```sh
python3 tools/agent/worktree.py new my-change
python3 tools/agent/worktree.py new my-change --base origin/main
python3 tools/agent/worktree.py list
python3 tools/agent/worktree.py finish my-change
python3 tools/agent/worktree.py finish my-change --force
```

`finish` refuses a dirty worktree without `--force`; it removes the worktree's `target`
and build-dir before `git worktree remove`. Notes are left in place.

## Architecture gate

Reads `cargo metadata` and `tools/architecture/policy.toml`: layer ordering, missing
layers, and production file length. `--write-policy` regenerates `[layers]` and
`[grandfathered]` while preserving `[same_layer_ok]` and `[limits]`.

```sh
python3 tools/agent/archcheck.py
python3 tools/agent/archcheck.py --json
python3 tools/agent/archcheck.py --write-policy
```

## Error formatter

Reads cargo `--message-format=json` from stdin or a file. Prints at most 30 unique
diagnostics grouped by package, then a count summary.

```sh
cargo clippy -p eme_ecs --all-targets --message-format=json | python3 tools/agent/errfmt.py
python3 tools/agent/errfmt.py build.json
```
