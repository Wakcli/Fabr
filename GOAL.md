# GOAL — Fabr

I'm developing a game engine called **Fabr**. It was previously called Emberrly.

Repository: https://git.endxel.com/WakCLI/Fabr

I've attached the current repository ZIP so you can inspect the actual source code.

I want you to help me plan a major overnight development sprint using an autonomous coding agent.

## My vision

I want Fabr to become a proper standalone game engine and development ecosystem, with an experience comparable in breadth to Unity.

I do NOT want it to just be a Bevy frontend.

Bevy can be used internally, but Fabr needs its own identity, architecture, public SDK, APIs, tooling and development workflows.

I want someone to be able to create an entire game using Fabr without needing to understand Bevy or work directly with its internals.

## Current problems

The editor is still early-stage and has substantial missing functionality.

For example:

- The top menus Scene, Project, Debug, Editor and Help are mostly non-functional.
- Project Settings do not exist as a proper settings system.
- There are no proper graphics, physics, audio, input, player or quality settings.
- There is no scripting system.
- There is no proper Fabr SDK comparable to what game developers expect from Unity.
- Outline currently does nothing.
- Stats currently does nothing.
- The Console is very basic.
- There are many missing editor panels and development workflows.
- The runtime needs substantial expansion.
- The asset pipeline needs improvements.
- The build and export experience needs expansion.
- Performance and memory optimizations are needed.
- The overall editor UI needs improvements to speed up development and increase productivity.

There are existing systems, including scene editing, hierarchy, inspector, asset importing, docking, transform gizmos and a Linux build workflow. Do not assume everything is missing. Inspect the repository first.

## What I want

I want a MASSIVE expansion of Fabr.

I want you to think like a team building a serious game engine.

Consider the entire ecosystem:

**Editor**
- Fully functional menus.
- Project Settings.
- Player Settings.
- Graphics Settings.
- Physics Settings.
- Audio Settings.
- Input Settings.
- Quality Settings.
- Build Profiles.
- Package Manager.
- Animation Editor.
- Material Editor.
- Lighting Editor.
- Audio Mixer.
- Profiler.
- Console.
- Game View.
- Scene View.
- Useful Outline and Stats panels.
- Better docking and saved layouts.
- Command palette.
- Keyboard shortcuts.
- Searchable Inspector and Hierarchy.
- Better asset browser.

**SDK and scripting**
- A public Fabr SDK.
- A proper scripting language/runtime.
- Script assets.
- Attaching scripts to objects.
- Exposed script properties.
- Lifecycle callbacks.
- Hot reloading.
- Input APIs.
- Transform APIs.
- Scene APIs.
- Physics APIs.
- Audio APIs.
- Rendering APIs.
- Animation APIs.
- UI APIs.
- Plugin and extension APIs.

Investigate Lua or other appropriate scripting technologies. I want actual executable gameplay scripts, not a code editor that does nothing.

**Engine and runtime**
- Proper gameplay architecture.
- Input actions.
- Physics and collision systems.
- Audio.
- Animation.
- Camera controllers.
- Navigation and pathfinding.
- UI system.
- Networking.
- Scene transitions.
- Runtime lifecycle.
- Asset management.
- Serialization.

**Performance**
- CPU and GPU profiling.
- Memory profiling.
- ECS optimization.
- Render optimization.
- GPU upload reduction.
- Asset caching.
- Incremental importing.
- Avoiding unnecessary allocations.
- Resource management.
- Meshlet optimization.
- Render graph optimization.
- Efficient editor UI updates.

**Build system**
- Build profiles.
- Standalone exports.
- Asset staging.
- Runtime packaging.
- Development and release configurations.
- Windows, Linux and macOS support.

**Extensibility**
- Engine plugins.
- Editor plugins.
- Custom panels.
- Custom Inspector controls.
- Asset importer extensions.
- Package dependencies.
- Package versioning.

## The important part

I am going to run an agent overnight for approximately six hours.

The agent uses:

- OpenCode.
- DeepSeek V4.1 Flash Thinking, maximum reasoning effort.
- Oh My OpenCode Lite.

I want a highly specific development assignment.

Do not give me vague instructions like "improve the editor" or "add scripting".

Specify:
- Exactly which features to implement.
- What files and systems to investigate.
- What functionality each feature must provide.
- How features should integrate.
- What the user should be able to do after implementation.
- What tests must exist.
- What constitutes completion.
- What should be prioritised within six hours.

Do not ask the agent to implement everything in the list in six hours. Choose a realistic but ambitious combination of high-impact, complete features.

## Critical requirements

1. Inspect the attached source code before making recommendations.
2. Identify what already exists and what genuinely does not.
3. Do not duplicate existing functionality.
4. Do not create placeholder interfaces.
5. Do not count a button, struct or empty panel as a completed feature.
6. Require actual integration with the engine and editor.
7. Include performance work with measurable results.
8. Require automated tests.
9. Require meaningful Git commits.
10. Work on `feat/editor-selection`, never main.
11. Do not push without permission.
12. Preserve existing functionality.

I want your honest technical opinion. Challenge my ideas if there are better approaches.

Your final output should be a detailed, copy-paste-ready overnight development prompt for my coding agent.

The goal is to make Fabr noticeably more capable as a standalone game engine after six hours of autonomous development.
