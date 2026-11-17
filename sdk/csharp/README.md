# Fabr C# scripting

`Fabr.Bindings` is the managed side of the Fabr contract: the `FabrApi` callback
table, the `FabrOps` static facade, the `FabrScript` base class, and the
`ScriptHost` entry points the engine resolves. Game code derives from
`FabrScript`; the engine loads it into a collectible `AssemblyLoadContext`, so a
rebuilt assembly can be reloaded without restarting the game.

## Build a game assembly

`sdk/csharp/Game` is the sample project the demo scene attaches: `Game.Spin`
for the managed crate and `Game.Walker` for a character that reads input,
walks, jumps, swaps clips, and draws its own HUD. Build it straight into the
project's script folder:

```sh
dotnet build sdk/csharp/Game/Game.csproj -c Release -o assets/scripts/managed
```

That one command produces all three files the engine needs next to each other:

- `Game.dll`
- `Fabr.Bindings.dll`
- `Fabr.Bindings.runtimeconfig.json`

`assets/scripts/managed/` is build output and stays out of git. When an
assembly is missing, the engine logs the directory it expected to find it in.

The Inspector's Script section has a **Build** row that runs the same command
for the selected object's assembly and streams the output to the Console. The
assembly name picks the project: `Game.dll` builds
`sdk/csharp/Game/Game.csproj`. A successful build is picked up by the script
watcher and the live instance reloads.

## Attach it in a scene

A scene object's script names the assembly and the class:

```ron
script: Some((
    path: "scripts/managed/Game.dll",
    properties: {},
    language: csharp,
    class: Some("Game.Spin"),
))
```

The same scene can run Lua scripts on other objects. The Inspector's Script
section picks C#, filters the picker to `.dll` files, and edits the class name.

## The script surface

`FabrScript` projects the engine's operation vocabulary one to one. Every
operation is entity-addressed, so a script can drive the object it is attached
to or one it resolved with `Find`:

```csharp
public sealed class Lift : FabrScript
{
    private ulong? _door;

    public override void OnStart() => _door = Find("Door");

    public override void OnUpdate(float dt)
    {
        if (_door is ulong door && UiClicked(7))
        {
            Translate(door, new Vector3(0.0f, dt, 0.0f));
            PlaySound("sounds/door.wav");
        }
        UiPanel(0.1f, 0.1f, 0.2f, 0.06f, new Vector4(0.0f, 0.0f, 0.0f, 0.6f));
        UiButton(7, 0.1f, 0.1f, 0.2f, 0.06f, "Open");
    }
}
```

Writes queue against the current frame and land after the managed call
returns; reads answer from the frame the script was given, including its own
unapplied writes. `FindPath`, `AnimationClips`, `LoadSceneAdditive`, and
`Find` return `null` or an empty array when the engine has nothing to report.

## ABI

`FabrApi` is append-only. Adding a callback means appending a field and bumping
both `ABI_VERSION` in `crates/eme_script_csharp` and `FabrOps.AbiVersion` in
`Fabr.cs`. `ScriptHost.Load` refuses a table older than the SDK, so the engine
and the bindings must be built from the same commit. A test in
`eme_script_csharp` parses this struct and the Rust one and fails when a field
is missing, out of order, or has the wrong shape.

## Runtime rules

- One .NET runtime per process; the first script initializes it from the
  bindings' `runtimeconfig.json`.
- Class names are plain type names inside the assembly (`Game.Spin`), not
  assembly-qualified.
- Exports copy `assets/`, so the assemblies travel with the game; the target
  machine needs the .NET runtime installed.
- Unmanaged exceptions never cross the boundary: the host catches them and logs
  `csharp <phase>: <type>: <message>`.
