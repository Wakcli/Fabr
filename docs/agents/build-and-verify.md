# Build and verify

Commands that work in this repo, in the order you usually need them.

## Tools

```sh
cargo install cargo-nextest --locked
cargo install cargo-deny --locked
cargo install cargo-machete --locked
cargo install typos-cli --locked
cargo install taplo-cli --locked
```

`sccache` is optional. Set `RUSTC_WRAPPER=sccache` only when incremental is off for that build; sccache cannot cache incrementally compiled crates.

## Inner loop

```sh
cargo check -p <crate> --tests
cargo test -p <crate>
cargo nextest run -p <crate>
```

Use `--message-format=json` piped through `python3 tools/agent/errfmt.py` when the output is long:

```sh
cargo clippy -p <crate> --all-targets --message-format=json | python3 tools/agent/errfmt.py
```

## Before a PR

```sh
cargo fmt --all -- --check
taplo fmt --check
typos
python3 tools/agent/archcheck.py
cargo clippy -p <crate> --all-targets -- -D warnings
```

Run workspace-wide checks only when the change touches shared crates or the workspace manifest:

```sh
cargo run -p ci -- lints
cargo run -p ci -- test
cargo run -p ci -- doc
```

`cargo deny check` is required when `Cargo.toml` dependencies change.

## Limiting parallel builds

Several agents share this machine. Acquire a slot before a heavy build and release it after:

```sh
python3 tools/agent/buildslot.py run --slots 2 -- cargo check -p eme_ecs --tests
```

Each cargo invocation is gated as a whole. Do not gate individual rustc processes.

## Rendered output

The editor captures its own frame, so no window is needed:

```sh
env -u WAYLAND_DISPLAY -u DISPLAY EME_EDITOR_SHOT=$HOME/.cache/emberrly/shot.png \
  EME_EDITOR_SHOT_FRAME=40 xvfb-run -a -s '-screen 0 1600x900x24' cargo run -p eme_editor
```

`EME_EDITOR_SCREEN=hub|editor` picks the start screen. The screenshot is written by the renderer and the app exits, so the process must be run under `xvfb-run` with both display variables unset. A PNG that is not 1600x900 came from the user's compositor and must be deleted, not used. Other examples can use `EME_TESTING_CONFIG`-style frame captures from `eme_dev_tools::ci_testing`.

## Reporting

State the exact command, its exit code, and the relevant tail of its output. Separate verified from assumed. If a check could not run, say why.
