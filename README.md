<p align="center">
  <img src="https://i.imgur.com/XmLFET2.png" width="543" alt="Fabr">
</p>

<h3 align="center">A game engine with an editor, built on Bevy.</h3>

<p align="center">
  <a href="#license"><img src="https://img.shields.io/badge/license-MIT%2FApache-blue.svg" alt="License"></a>
  <a href="https://discord.gg/EWb5w2TcnF"><img src="https://img.shields.io/badge/discord-join-5865F2?logo=discord&logoColor=white" alt="Discord"></a>
  <img src="https://img.shields.io/badge/status-pre--1.0-black.svg" alt="Status">
</p>

![The Fabr editor: hierarchy, viewport, inspector and file browser in one window](docs/images/editor.png)

Fabr is a desktop game engine with a dockable editor, gameplay scripting in Lua, C# and Rust, and a
build panel that packages a game into a native binary. Scenes are plain text, so they diff and
merge like any other source file, and scripts reload while the game is running.

The engine underneath is [Bevy](https://bevy.org). Fabr is a fork: the ECS, renderer, asset
pipeline, audio and reflection come from upstream, and the editor, scripting hosts, player and
export pipeline are Fabr's. Rendering and ECS behaviour will be familiar to anyone who has used it.

## Editor

Dockable panels for hierarchy, inspector, viewport, file system, console and stats. Panels split,
drag into tabs, and the layout is stored per project.

- Move, rotate and scale gizmos
- Undo and redo across the scene
- glTF import with per-instance material overrides
- Live frame time and draw counts in the stats panel

## Scripting

Pick a language per object. All three sit behind the same interface, and script properties appear
in the inspector.

```lua
return {
    properties = { speed = 1.0 },
    on_start = function(self)
        fabr.log("spin started on " .. self.name)
    end,
    on_update = function(self, dt)
        self:rotate_y(self.properties.speed * dt)
    end,
}
```

That is the whole script. Lua and C# reload the moment the file is saved, with no restart and no
lost play state. C# is real .NET, loaded through a collectible context so reloads do not leak.
Rust gameplay is compiled into the host with the `fabr` crate, so it needs a rebuild — it is not
hot-reloaded.

## Export

The build panel stages the player, the scenes and the assets for a target platform.

- Linux and Windows targets
- Release player is about 5 MB before assets
- The player reads the same `.ember` scenes the editor writes, so there is no editor-only format
- macOS appears in the panel but requires a macOS host to build

## Engine

The engine is Bevy: ECS, renderer, asset pipeline, UI, audio and reflection.

- 2D sprites and 3D PBR rendering
- Physics with broadphase collision
- Navigation meshes
- Animation graphs and transitions
- Client and server networking over TCP

## Extensibility

- The `fabr` crate for gameplay code that needs to run inside the engine
- Plugin API for adding systems to a running game
- Editor extensions for adding panels and tools without forking the editor
- Packages install from folders

## Getting started

```sh
git clone https://github.com/Wakcli/Fabr
cd Fabr
cargo run -p eme_editor
```

Requires Rust 1.96 or newer and a Vulkan or DX12 capable GPU. Linux is the supported host; the
editor builds on Windows but has not been used there for long enough to call it supported.

To run the player without the editor:

```sh
cargo run -p eme_player -- scenes/main.ember
```

## Status

Fabr is pre-1.0 and Linux-first. This is the current state, not a roadmap.

| Works today | Not yet |
|:--|:--|
| Editor, Lua and C# on Linux | macOS and mobile targets |
| Windows export builds | untested on Windows hardware |
| Text scenes, glTF import, undo and redo | asset registries are folders only, no HTTP client |
| Lua and C# hot reload | Rust gameplay needs a rebuild |

The demo scene references a managed assembly that is not checked in, so its C# object logs a load
error until you build one. If something on this list looks wrong, open an issue.

## Contributing

The most useful things right now, roughly in order:

- **Run it and report what breaks.** Windows and the export path are the thinnest parts. A
  paragraph and a screenshot is enough.
- **Make a game with it and complain.** Jam-sized is fine. The friction you hit is the roadmap.
- **Rendering, editor and viewport work.** Known rough edges are filed; ask for one.
- **Docs and examples.** Plenty of the tree still describes a different project.

Open an issue before a large change so nobody builds the same thing twice. Small fixes can go
straight to a pull request. See [CONTRIBUTING.md](CONTRIBUTING.md).

Work in progress is shown first on Discord: **https://discord.gg/EWb5w2TcnF**.

## License

MIT OR Apache-2.0, the same terms as the Bevy it is built on. Fabr is a modified fork; the
changes are described in [NOTICE](NOTICE), and upstream, its contributors and its assets are
credited in [CREDITS.md](CREDITS.md) and [THIRD-PARTY-LICENSES.md](THIRD-PARTY-LICENSES.md).
