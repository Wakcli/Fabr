# Fabr C# scripting

`Fabr.Bindings` is the managed side of the Fabr contract: the `FabrApi` callback
table, the `Fabr` static facade, the `FabrScript` base class, and the
`ScriptHost` entry points the engine resolves. Game code derives from
`FabrScript`; the engine loads it into a collectible `AssemblyLoadContext`, so a
rebuilt assembly can be reloaded without restarting the game.

## Build a game assembly

```sh
dotnet build sdk/csharp/Example/Example.csproj -c Release -o /tmp/fabr-csharp
```

Copy these into the project's `assets/scripts/managed/`:

- `Fabr.Bindings.dll`
- `Fabr.Bindings.runtimeconfig.json`
- `Example.dll`

The bindings build produces the first two; the example build produces the third.

## Attach it in a scene

A scene object's script names the assembly and the class:

```ron
script: Some((
    path: "scripts/managed/Example.dll",
    properties: {},
    language: csharp,
    class: Some("Example.Spin"),
))
```

The same scene can run Lua scripts on other objects. The Inspector's Script
section picks C#, filters the picker to `.dll` files, and edits the class name.

## Runtime rules

- One .NET runtime per process; the first script initializes it from the
  bindings' `runtimeconfig.json`.
- Class names are plain type names inside the assembly (`Example.Spin`), not
  assembly-qualified.
- Exports copy `assets/`, so the assemblies travel with the game; the target
  machine needs the .NET runtime installed.
- Unmanaged exceptions never cross the boundary: the host catches them and logs
  `csharp <phase>: <type>: <message>`.
