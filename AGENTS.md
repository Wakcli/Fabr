# Repository agent instructions

Fabr is a Rust game engine and editor. It is a modified fork of Bevy: the engine crates are
derived from Bevy and renamed to `eme` / `eme_*`, and the editor, export path and scripting
layer are ours. Upstream attribution lives in `NOTICE` and `CREDITS.md`.

Read the code before changing it. Do not guess at Bevy internals from memory; the crates in this
repository are the source of truth. Keep this file current: when a rule here turns out to be
wrong for the code, fix the rule in the same change.

## Contents

- [Product north star](#product-north-star)
- [Start here](#start-here)
- [Operating rules](#operating-rules)
- [Repository layout](#repository-layout)
- [Architecture rules](#architecture-rules)
- [Scripting](#scripting)
- [Shared values](#shared-values)
- [Code style](#code-style)
- [Comments and docs](#comments-and-docs)
- [Tests](#tests)
- [Build and verify](#build-and-verify)
- [Branches and publishing](#branches-and-publishing)
- [Worktrees and captures](#worktrees-and-captures)
- [Notes folder](#notes-folder)
- [Licensing and attribution](#licensing-and-attribution)
- [Asset and private-content boundaries](#asset-and-private-content-boundaries)
- [When you are unsure](#when-you-are-unsure)

## Product north star

The editor's primary goal is **shipping a game from the editor**: a Build tab where the user
picks a target platform and gets a native executable of the game they authored. Every editor
feature is judged by whether it keeps that path open.

Concretely:

- Scenes and objects authored in the editor must be serializable and loadable by a runtime
  player. Do not build authoring features on state that can only live in the editor process.
- Keep a strict split between editor UI and runtime/player code. Editor crates must not be
  required to run an exported game.
- The first export target is a template player (engine runtime + scene data + assets), packaged
  per platform; compiling user Rust gameplay is a later tier.
- The Build tab, scene format, asset staging, and platform templates are the roadmap spine. UI
  polish and authoring tools come after the scene document, not before it.

## Start here

| Read | When |
| --- | --- |
| [Workspace map](docs/agents/workspace-map.md) | Finding the right crate before editing |
| [Build and verify](docs/agents/build-and-verify.md) | Running checks, limits, and gates |
| [Worktrees](docs/agents/worktrees.md) | Starting any change that is not a one-file edit |
| [Agent tools](tools/agent/README.md) | Python helpers for worktrees, build slots, gates, and linting |
| [C# SDK](sdk/csharp/README.md) | Writing or changing managed gameplay code |

If a linked doc does not exist yet, create it in the change that depends on it.

## Operating rules

- Plan before touching more than one crate. Write the plan in the notes folder, not in the repo.
- Make the smallest change that fixes the stated problem. Do not refactor adjacent code, rename
  things you were not asked to rename, or reformat files you did not otherwise change.
- Prove a fix with a test that fails on the old code and passes on the new code. Confirm the
  failure before applying the fix.
- Stop and ask when a change would alter a public API, a crate boundary in
  `tools/architecture/policy.toml`, a file format, or the license of anything vendored.
- Never run `git push`, `git push --force`, `gh pr merge`, or a release command without an
  explicit instruction in the current session.
- Never use pattern-based process kills (`pkill`, `killall`). Stop only PIDs you started.
- Never open a window on the user's display. The live session is `DISPLAY=:1` and
  `WAYLAND_DISPLAY=wayland-1`; both are exported in shells, and `xvfb-run` alone does not stop
  winit from choosing Wayland. The only safe capture command is:
  ```sh
  env -u WAYLAND_DISPLAY -u DISPLAY EME_EDITOR_SHOT=/home/wak/.cache/emberrly/shot.png EME_EDITOR_SHOT_FRAME=40 \
    xvfb-run -a -s '-screen 0 1600x900x24' cargo run -p eme_editor
  ```
  Any capture whose size is not 1600x900 came from the user's compositor; treat it as a failed
  run and delete it.
- Keep build output off `/tmp`. It is a 16 GiB tmpfs, and a workspace target dir fills it.
  `.cargo/config.toml` sets `build-dir` to `~/.cargo-build/emberrly`; put captures and scratch
  files under `~/.cache/emberrly/`.
- Keep secrets, tokens, real account names, and server addresses out of commits, logs,
  screenshots, and PR text.

## Repository layout

- `crates/` — the engine. Every crate is a workspace member. Package names are `eme_*`; the
  facade is the `eme` package at the root (`src/`).
- `crates/bevy/` — the legacy `bevy` umbrella crate still in the workspace. Do not add new code
  there.
- `sdk/` — gameplay SDKs. `sdk/csharp/` holds the managed bindings (`Fabr.Bindings`), the
  `Game` sample project and `README.md`; `sdk/CONTRACT.md` defines the surface every language
  binding must expose.
- `tools/` — workspace tools run with `cargo run -p <name> --`: `ci`, `build-templated-pages`,
  `build-wasm-example`, `build-easefunction-graphs`, `export-content`, `example-showcase`,
  `compile_fail_utils`.
- `examples/` — runnable examples and compile-only targets (mobile, no_std, large scenes).
  Examples are checked by CI.
- `tests/` and `tests-integration/` — workspace-level tests.
- `benches/` — benchmarks. Built by `ci bench-check`.
- `assets/` — engine test assets. Do not add large binaries without asking.
- `docs/` — contributor docs. `docs-template/` holds templates for generated pages; edit the
  template, not the output.
- `errors/` — the error-code catalog.
- `.cargo/` — `config_aliases.toml` and `config_fast_builds.toml` are templates. Copy them to
  `config.toml` locally; `.cargo/config.toml` is gitignored.

Find the owning crate before editing. Search by symbol, not by filename, and read the crate's
`Cargo.toml` and `src/lib.rs` first.

## Architecture rules

- Follow `tools/architecture/policy.toml` for crate boundaries, owned types, and allowed
  dependency directions. If that file does not exist yet, creating it is the first task in any
  architecture work.
- Register every new crate and every new dependency edge in the policy file. A dependency the
  policy does not list fails the architecture check.
- Put logic in the lowest crate that owns its data. Domain crates (math, ECS, asset data, editor
  model) do not depend on rendering or windowing crates.
- Editor code goes in editor crates that depend on engine crates, never the reverse. An engine
  crate must build and test with no editor crate present.
- The editor is split so its UI can be replaced: `eme_editor_core` holds state and behavior
  (hub, selection, undo/redo, panel layout, theme tokens) with no `eme_*` and no rendering
  dependency; `eme_editor` renders that state and contains no business logic. A visual redesign
  must not need changes in `eme_editor_core`.
- All editor styling comes from theme tokens in `assets/editor/theme.ron`. All panel arrangement
  comes from `assets/editor/layout.ron`. Do not hardcode a color, size scale, or panel tree in
  editor code.
- Do not grow a production file past 1,000 lines. Existing files already past that are tracked
  for splitting by cohesive owner; `archcheck.py` currently enforces `max_file_lines = 6000` as a
  ceiling while those splits happen. Lower the ceiling as files are split.
- Do not add a trait with one implementation. Add the abstraction when the second implementation
  arrives.
- Do not add a re-export shim or temporary facade during a refactor. Move the code and update
  the callers in the same change.
- Public API changes need a short note in the PR body. Create `CHANGELOG.md` when the first
  release needs one; it does not exist yet.

## Scripting

C# is the primary gameplay language. Lua remains supported and is frozen: fix bugs that block
users, add no new features to it, and do not delete it.

- `sdk/CONTRACT.md` defines the operation surface. The Rust SDK in `crates/eme_script` is the
  naming authority; the C# facade is projected from it, one operation per method.
- Scripts act on entities they resolve themselves (`Find`, `FindAll`), so the operation layer is
  entity-addressed. Do not add operations that assume an implicit "current object".
- The `FabrApi` table in `crates/eme_script_csharp` is append-only. Never renumber or remove a
  field; older tables must keep working against newer engines, and
  `the_managed_table_matches_the_engine_table` enforces the mirror.
- Managed assemblies are loaded from bytes (`AssemblyLoadContext.LoadFromStream`), never by
  path. Path loads are cached for the process lifetime and return stale images after a rebuild.
- Managed bootstrap entry points are resolved by name, so adding one does not change the ABI
  table. `Fabr.Bindings.dll` is cached per process; a rebuild of it needs an editor restart.
  Game assemblies hot-reload.
- A C# instance is constructed, then scene property overrides are applied, then `OnStart` runs.
  Edit-mode inspection must not start the script; use the `Inspect` path instead.

## Shared values

- Versions, crate names, product names, paths, file-format magic numbers, and feature names come
  from one place: a constant, a manifest, or a generated file.
- Consumers and tests read that source. Do not copy a literal into a second file.
- Any hand-maintained list or table that can drift silently must either be derived from its
  source or guarded by a compile-time assertion, a test, or an architecture rule. If a missed
  update would not fail CI, the list is wrong.
- Engine constants that must be deterministic across platforms go through `eme_math::ops`
  (libm-backed float functions), not `f32::sin` and friends. `clippy.toml` already enforces this
  with `disallowed-methods`. Do not add `#[allow]` to bypass it.

## Code style

- Rust edition 2024, MSRV 1.96.0 (`rust-version` in the root `Cargo.toml`). Do not use features
  newer than the MSRV without bumping it in the same change.
- `cargo fmt` with `rustfmt.toml`. `taplo fmt` for TOML.
- Clippy must pass with `-D warnings` on all targets. Do not silence a lint to make a warning go
  away. Fix it or justify a narrow `#[allow]` on the item with a reason.
- Names are short and plain. Do not write enterprise names (`ConfigurationManagerFactory`) or
  single letters outside tight math.
- Avoid unsafe code unless the crate already uses it for the same purpose. Every `unsafe` block
  gets a `// SAFETY:` comment naming the invariant.
- Prefer early returns and guard clauses. Keep `else` chains short.
- Do not add dependencies casually. Check `deny.toml` licenses and run `cargo deny check` when a
  dependency changes.

## Comments and docs

- Doc comments on public items explain the contract: what the item guarantees, what the caller
  must do, and what it returns on failure. Do not restate the signature.
- Inline comments record a *why* the code cannot show: an invariant, a workaround for a specific
  bug, a platform quirk, a reference to a spec or upstream issue. One or two lines, lowercase.
- No PR numbers, phase names, or "added for X" in comments. That history belongs in git and in
  the changelog.
- Keep `docs/` pages accurate when behavior changes. Generated pages come from `docs-template/`
  through `build-templated-pages`.

## Tests

- Reproduce a bug in the smallest crate that owns the behavior. Add a regression test there.
- Assert behavior and outputs. Do not assert on source text, system schedule names, exact layout
  trees, or internal ordering unless the ordering is the contract.
- Domain code takes `now` or a timer from its caller. Only edge code reads the clock. Tests
  never call `sleep`. Wait on a condition with a bounded helper, not a fixed delay.
- Tests needing a GPU, a display, or a large asset skip with a named message when the fixture is
  missing. Do not use `#[ignore]`; `ignore` hides them from every run.
- Compile-fail tests (`compile_fail` crates) pin diagnostic output. Update their expected output
  only when the diagnostic change is intended, and say so in the change.
- Test per crate: `cargo test -p <crate>`. `cargo test --workspace` currently fails to resolve in
  the root `eme` crate (`std` shows up twice); do not treat that as a regression in your change.

## Build and verify

Full procedure is in [docs/agents/build-and-verify.md](docs/agents/build-and-verify.md). The
short version:

- Inner loop: `cargo check -p <crate> --tests` and `cargo test -p <crate>`.
- Before a PR: `cargo fmt --all -- --check`, `cargo clippy -p <crate> --all-targets -- -D
  warnings`, `cargo run -p ci -- lints` if the change touches many crates.
- Architecture check: `python3 tools/agent/archcheck.py` (until the policy gate lands in `ci`).
- When a change alters rendered output, run the affected example headless and capture a frame
  (see [Worktrees and captures](#worktrees-and-captures)).
- Report exactly what ran and what passed. Separate "verified" from "assumed" in the PR body.
- `cargo fmt --check` is currently dirty in several crates from before the current work; format
  only the lines you changed instead of reformatting whole files.

Limit parallel builds to the machine. Use `python3 tools/agent/buildslot.py acquire` before a
heavy build when more than one agent is active; release it after. Without the helper, set
`CARGO_BUILD_JOBS` to fit available CPU and memory. This machine has 12 cores and 31 GiB RAM;
leave headroom for the editor.

## Branches and publishing

- Default branch: `main`. Work on a topic branch named `<type>/<short-slug>`, for example
  `fix/ui-focus-loop` or `feat/editor-undo`.
- `origin` is the private Gitea remote. `gh` is the public GitHub mirror (`Wakcli/Fabr`). Publish
  with `git push gh <branch>`; never push to `gh` without an explicit instruction.
- Open a PR into `main` on `origin`. Direct pushes to `main` are for docs, comments, and
  formatting only, unless the session says otherwise.
- PR titles are the changelog. Use an imperative summary without a `type:` prefix or issue
  numbers.
- Apply one label: `new`, `fix`, `performance`, or `internal`. Check the label the automation
  assigns.
- Squash-merge with a final title and body. Keep branch commits descriptive.
- Do not add `Co-authored-by` trailers for any AI tool, in commits or squash messages.
- A change to visible output includes before and after screenshots and a short "How to see it"
  note: where in the editor and which setting or action shows the change.
- Report pushed, committed, tested-but-uncommitted, and in-progress work accurately. Never
  describe unfinished work as done.

## Worktrees and captures

- One worktree per concurrent change. Each gets its own `build-dir` outside the checkout, set in
  `.cargo/config.toml` for that worktree, and its own `target` for final artifacts.
- Remove a worktree's target directory after its PR merges. `python3 tools/agent/worktree.py
  finish <name>` does this and checks for uncommitted changes first.
- Captures, recordings, traces, and screenshots are not committed except the ones a PR attaches.
  Delete raw captures after the PR is open.
- Do not commit game assets that are not already tracked, build outputs, credentials, `.local/`
  carriers, or personal project files.

## Notes folder

Every checkout has a notes folder as a sibling directory, outside the repo, for example
`/home/wak/notes/emberrly/`. Nothing in it can be committed by accident. Put plans, findings,
reproduction steps, command lines that worked, run output, dead ends, and open questions there.

Each notes folder starts with `INDEX.md`, a one-line-per-file index. Write notes for a reader
with no memory of the session: absolute paths, exact commands, what was verified, what was
assumed, and what remains open. Update notes during the work and again at the end. A stale note
is worse than none.

## Licensing and attribution

- The engine is dual-licensed MIT OR Apache-2.0. Keep `LICENSE-MIT` and `LICENSE-APACHE` at the
  root and in any crate that ships them.
- `NOTICE` states that this is a modified fork of Bevy and what changed. `CREDITS.md` and
  `THIRD-PARTY-LICENSES.md` list upstream sources and third-party assets.
- `THIRD-PARTY-LICENSES.md` is generated from `Cargo.lock` and the local cargo registry. After a
  dependency change, regenerate it and commit the diff rather than editing it by hand.
- When you adopt code or an asset from another project, add it to the right file with its
  license. Do not remove existing copyright notices, author lines, or attribution.
- New third-party dependencies must have a license accepted in `deny.toml`.
- Engine-author code added under this repo's license needs no extra notice; say so if the code
  was derived from upstream.

## Asset and private-content boundaries

- Editor project templates and default content live in-repo only when they are generic and
  small.
- Proprietary game content, paid asset packs, and anything under a non-redistributable license
  stay out of this repo. Reference them through the project's asset root at runtime.
- Test fixtures must be small. A fixture over 1 MB needs a reason and a path under `assets/` that
  already accepts large files.

## When you are unsure

- A missing requirement is a question for the user, not a guess. Use the `question` tool when the
  answer unblocks work.
- Uncertainty about Bevy behavior is answered by reading the vendored crate, not by memory.
- A failing check is investigated before it is retried. Repeat a check only when its inputs
  changed or the earlier run was incomplete.
