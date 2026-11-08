# Worktrees

One worktree per concurrent change. A shared `target/` makes cargo serialize concurrent agents on its directory lock, which costs more than the isolation saves.

## Create

```sh
python3 tools/agent/worktree.py new <name>
```

This creates:

- `../<repo>-wt/<name>/` on branch `wt/<name>`, based on `origin/main`.
- `.cargo/config.toml` inside it, setting `build.build-dir` to `../.cargo-build/<name>`. That file is gitignored.
- `../notes/<repo>/<name>/INDEX.md`, the notes folder for the change. It sits outside the repo and cannot be committed.

The branch starts as `wt/<name>`. Before the first push, rename it to the AGENTS.md convention: `git branch -m wt/<name> fix/<slug>` (or `feat/<slug>`).

## Work

Run cargo from inside the worktree. Wrap heavy invocations with `tools/agent/buildslot.py run`.

## Finish

```sh
python3 tools/agent/worktree.py finish <name>
```

This refuses when the worktree has uncommitted changes. Use `--force` only after the changes are committed, pushed, or deliberately discarded. It removes the target and build-dir, then the worktree.

## List

```sh
python3 tools/agent/worktree.py list
```

## Rules

- Do not run `git worktree prune` or `git worktree remove` by hand. Use the tool, so the build-dir is cleaned too.
- Do not create worktrees inside the main checkout.
- Keep the notes folder current. It is the only memory a later session will have.
