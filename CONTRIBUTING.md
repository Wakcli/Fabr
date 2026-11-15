# Contributing to Fabr

Fabr is pre-1.0 and deliberately unfinished. The useful contributions right now are the ones
that tell us where it hurts.

## Before you write code

Open an issue describing the change. For small fixes — a typo, a broken link, an obviously
wrong default — skip that and send the pull request.

## What is most useful

- **Running it and reporting what breaks.** Windows and the export path are the thinnest
  parts, and neither has been exercised much outside the machine it was written on. A bug
  report with your OS, GPU, the exact command, and a screenshot is worth more than a large
  patch.
- **Making a game with it.** Jam-sized is fine. The friction you hit is the roadmap; write it
  down even if you work around it.
- **Rendering, editor and viewport work.** Known rough edges are filed; ask for one.
- **Docs and examples.** Plenty of the tree still describes a different project. If a
  sentence is wrong, deleting it counts as a contribution.

## Pull requests

- Branch from `feat/editor-selection`. No force-pushes to shared branches.
- `cargo test --workspace` has to pass. If you add behaviour, add the test that fails
  without it.
- Keep the change to the thing you said you would change. A fix is the fix, not the fix plus
  the refactor you noticed on the way past.
- Rust formatting follows `rustfmt.toml` in the root. Clippy is run with `--no-deps`.

## Reporting a bug

Include what you ran, what you expected, what happened instead, and the log output. If it is
visual, a screenshot. If it is a crash, the backtrace.

## Where to talk

Discord: **https://discord.gg/EWb5w2TcnF**

## License

By contributing you agree that your work is licensed under MIT OR Apache-2.0, matching the
rest of the project.
