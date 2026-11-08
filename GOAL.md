# GOAL — Fabr (v2)

Fabr (formerly Emberrly) must become a proper standalone game engine and development
ecosystem with the breadth of Unity and its own identity: architecture, SDK, tooling and
workflows. Bevy is an internal dependency, never the developer-facing API. A developer must be
able to build a whole game without knowing Bevy exists.

The original brief is preserved in `GOAL.archive.md`; this version keeps its intent and sets
the current focus.

## The three-language stack — primary focus

One engine, three first-class ways to build a game, all over **one `fabr` API contract**:

- **Rust — engine and performance layer.** Rust gameplay compiles into the game (or loads as a
  dylib where reload matters). Direct calls, no marshalling, full type safety. This is the
  reference implementation of the SDK.
- **C# — the full gameplay SDK.** Compiled scripts hosted through CoreCLR (`hostfxr`), the
  same `fabr` surface, lifecycle callbacks, hot reload via collectable `AssemblyLoadContext`,
  and an export step that ships the compiled scripts and runtime.
- **Lua — lightweight iteration.** Triggers, HUD, prototypes and small objects. Already
  implemented; it must be brought to the same contract rather than drifting into its own API.

Rules for the stack:

- The shared API is value-based: handles, numbers, strings, enums, callbacks. No lifetimes,
  generics or borrowed ECS queries leak into the cross-language surface.
- Lua, C# and Rust gameplay can run in the same scene.
- Every binding is real, tested and live-verified. No placeholder APIs, no empty panels.
- "Rust scripting" means compiled Rust gameplay, and its no-hot-reload trade-off is stated
  plainly wherever it matters.

## Performance mandate

"Native, optimized, speedy" is measured, never asserted:

- Benchmark before/after for: script update cost with N scripts per language, editor idle CPU,
  physics step with many bodies, and exported-game startup.
- Remove the per-script, per-frame clones in the scripting bridge (transforms, names, input
  maps, anim sets) in favour of a shared borrowed snapshot that every language reads once.
- Continue engine hot paths: ECS query/iteration cost, render graph and upload cost, asset
  caching, editor UI update churn.
- Numbers go into the sprint notes and the reports.

## Current state — do not rebuild

Already implemented and pushed on `feat/editor-selection`:

- **Editor**: working menus, player/graphics/audio/quality/physics/input/project settings,
  build profiles, local package manager with semver + `fabr.lock`, animation editor, material
  and lighting editors, audio mixer, CPU/GPU/memory profiler, console with a command line and
  Tab completion, scene and game views, Outline/Stats, docking with saved layouts and named
  presets, command palette, rebindable shortcuts, searchable Hierarchy/Inspector, asset
  browser and importer.
- **Scripting (Lua)**: Lua 5.4 runtime, per-object scripts and properties, lifecycle, manual
  reload, and APIs for transforms, input, physics/controllers, animation, navigation, audio,
  UI HUD, scene load/reload/additive/unload, joints and logging.
- **Runtime**: input actions, AABB physics with a step-up character controller, angular
  velocity and distance joints, animation, navigation, audio, UI, local TCP networking
  (snapshots, spawn/despawn, interpolation, token auth, ownership + client prediction),
  whole-scene and additive transitions, serialization, package staging.
- **Build/export**: Linux and Windows (MinGW cross-compile) exports with static players.
- **Extensibility**: native plugin ABI with a versioned entry point; local packages with
  dependency resolution.
- **Profiling**: frame phases, GPU pass timings, process memory and asset counts.

Known gaps: C# scene integration, Rust gameplay SDK, Lua contract parity, scripting-bridge
optimization, remote registry packages, editor plugin panels/custom Inspector controls/importer
extensions, physics broadphase and dynamic-dynamic collision, macOS export.

## Priority order

1. **SDK contract + Rust reference SDK.** Write the shared surface down; implement the typed
   Rust gameplay entry (`on_start`/`on_update`/`on_stop`, world access, events) and the export
   path that links user gameplay into the player.
2. **C# scene integration.** `ScriptLanguage` on `ScriptRef`, a managed `Fabr` facade over the
   existing callback table, lifecycle and spawn/update, hot reload, export staging.
3. **Lua parity.** Normalize the existing Lua API to the contract; fill the missing calls.
4. **Scripting-bridge optimization with benchmarks**, then engine hot paths.
5. **Remaining breadth**: physics broadphase and dynamic-dynamic collision; registry-backed
   packages; editor plugin panels, custom Inspector controls and importer extensions.
6. **macOS export** only when a macOS host with Apple's linker is available. Do not fake it.

## Engineering rules

- Inspect the existing systems before changing anything; preserve architecture and behaviour.
- Complete, integrated features only. No placeholders, no "button exists" as done.
- Automated tests for every feature; verify real behaviour live (Xvfb where needed).
- Measure performance before and after optimisation and report the numbers.
- Document blockers and move to the next achievable item; never claim unfinished work is done.
- Work only on `feat/editor-selection`; never `main`; commit logically; push frequently; never
  force-push or change remotes.
- Report substantive progress through the configured Discord webhook using Components V2,
  with `@everyone` and allowed mentions; include screenshots and numbers. Never expose secrets.
- Keep `notes` current outside the repo: commands, evidence, decisions, dead ends.

## Definition of done for this phase

- A scene runs Lua and C# side by side over the same `fabr` API, and a Rust gameplay build runs
  through the same contract.
- The scripting bridge shows a measured before/after improvement, and the benchmark commands
  are recorded.
- Full tests, clippy and archcheck are green; everything is committed and pushed.
- The final report honestly lists what is complete, what is not, and what is blocked.
