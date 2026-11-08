# The Fabr gameplay contract

One value-based surface, implemented by the Rust SDK, the C# bindings and the Lua
runtime. Engine handles, numbers, strings, booleans and enums cross the boundary;
lifetimes, generics and borrowed ECS queries never do.

## Lifecycle

Every language exposes the same three hooks. They run on the main thread, in
scene order, once per frame for `update`.

| Phase | Rust (`FabrGame`) | C# (`FabrScript`) | Lua (module table) |
| --- | --- | --- | --- |
| start | `on_start(&mut self, ctx)` | `OnStart()` | `on_start(self)` |
| update | `on_update(&mut self, ctx, dt)` | `OnUpdate(float dt)` | `on_update(self, dt)` |
| stop | `on_stop(&mut self, ctx)` | `OnStop()` | `on_stop(self)` |

A script or game attached to a scene object receives that object's handle. In C#
the handle is `FabrScript.Self`; in Lua it is `self.entity`; in Rust it is the
`Entity` the game looks up. Rust gameplay is compiled and loaded as a plugin, so
a rebuild needs a host restart; C# assemblies reload through a collectible
`AssemblyLoadContext`, Lua reloads on request.

## Calls

| Call | Rust | C# | Lua |
| --- | --- | --- | --- |
| log | `ctx.log(text)` | `Log(text)` | `fabr.log(value)` |
| read position | `ctx.translation(handle)` | `Position`, `TryGetPosition(out p)` | `self:get_translation()` |
| write position | `ctx.set_translation(handle, [x, y, z])` | `SetPosition(p)` | `self:set_translation(x, y, z)` |
| offset position | compose with read/write | `Translate(offset)` | `self:translate(x, y, z)` |

Positions are three `f32` in world space. A write is applied after the current
language call returns (C# and Lua queue it; Rust writes through the world), and a
read sees the frame's starting state plus the script's own writes from the same
frame.

Lua additionally exposes input actions, physics bodies and controllers, animation,
navigation, audio, HUD elements, scene loading and joints. Those are engine
extras on top of the contract, not a second contract; the Rust and C# surfaces
grow the same calls over time.

## Scene data

`ScriptRef` names the artifact and its lifecycle owner:

- `language: lua` — `path` is a `.lua` module below `assets/scripts/`.
- `language: csharp` — `path` is a managed assembly below `assets/scripts/` and
  `class` names the type deriving from `FabrScript`, for example `Game.Spin`.

Older scenes without `language` load as Lua. Script property overrides are a Lua
feature today; C# classes own their own fields.

## Hosting

- Rust: the game crate builds a `cdylib` with `fabr::game_plugin!(Type)` and is
  listed in `fabr.plugins.ron`. Linux exports stage the engine dylib and the
  game library next to the player.
- C#: `Fabr.Bindings.dll` plus its `runtimeconfig.json` sit beside the game
  assemblies. The engine hosts one .NET runtime per process.
- Lua: embedded in the player and editor; no extra artifacts.
