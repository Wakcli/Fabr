# Workspace map

Find the owning crate before editing. Search for the symbol, then read that crate's `Cargo.toml` and `src/lib.rs`.

## Top level

| Path | Holds |
| --- | --- |
| `src/` | root `eme` facade crate: re-exports and the default plugin set |
| `crates/eme_*` | engine crates, one concern each |
| `crates/bevy/` | legacy umbrella crate; no new code |
| `tools/ci` | `cargo run -p ci -- <cmd>`, mirrors the CI jobs |
| `tools/agent` | Python helpers for worktrees, build slots, arch gate, error formatting |
| `tools/architecture/policy.toml` | crate layers and grandfathered edges |
| `examples/` | runnable and compile-only examples |
| `benches/` | benchmarks |
| `tests/`, `tests-integration/` | workspace-level tests |
| `docs/`, `docs-template/` | contributor docs; templates for generated pages |
| `errors/` | error-code catalog |
| `.cargo/` | build config templates (`config_aliases.toml`, `config_fast_builds.toml`) |

## Engine crates by concern

Derived from the crate names. Confirm the layer in `tools/architecture/policy.toml` before adding a dependency.

- **Foundation:** `eme_utils`, `eme_ptr`, `eme_macro_utils`, `eme_derive`, `eme_log`, `eme_math`, `eme_color`, `eme_time`, `eme_tasks`, `eme_diagnostic`
- **Data model:** `eme_ecs`, `eme_reflect`, `eme_app`, `eme_state`, `eme_scene`, `eme_transform`, `eme_world_serialization`, `eme_settings`
- **Assets and media:** `eme_asset`, `eme_image`, `eme_mesh`, `eme_gltf`, `eme_shader`, `eme_audio`, `eme_text`, `eme_curve`, `eme_animation`
- **Rendering:** `eme_render`, `eme_core_pipeline`, `eme_pbr`, `eme_material`, `eme_sprite`, `eme_sprite_render`, `eme_post_process`, `eme_anti_alias`, `eme_light`, `eme_shape`, `eme_gizmos`, `eme_gizmos_render`, `eme_solari`
- **UI:** `eme_ui`, `eme_ui_render`, `eme_ui_widgets`, `eme_feathers`, `eme_a11y`
- **Input and platform:** `eme_input`, `eme_input_focus`, `eme_window`, `eme_winit`, `eme_platform`, `eme_clipboard`, `eme_gilrs`, `eme_android`
- **Camera and picking:** `eme_camera`, `eme_camera_controller`, `eme_picking`
- **Developer tools:** `eme_dev_tools`, `eme_diagnostic`, `eme_remote`
- **Internal:** `eme_internal` (feature gate fan-in for the facade)

## Editor crates

| Crate | Holds | Depends on |
| --- | --- | --- |
| `eme_editor_core` | hub and project state, selection, undo/redo, panel layout tree, theme tokens | serde, ron only |
| `eme_editor` | the shell: hub screen, editor screen, rendering core state with `eme_feathers` | `eme_editor_core` plus engine crates |

Editor crates depend on engine crates. Engine crates never depend on editor crates. New editor crates take the `eme_editor_` prefix and are registered in `tools/architecture/policy.toml` at a layer above everything they use.

The split exists so the UI can be replaced. `eme_editor_core` must not carry a single rendering or styling type. If a change forces a UI type into core, the boundary is wrong.
