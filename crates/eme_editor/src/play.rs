//! Play mode: runs the shipped runtime against a snapshot of the open scene.
//!
//! The editor never re-implements gameplay. Pressing play writes the current
//! document to a scratch file and launches `eme_player --scene <file>` as a child
//! process, so the thing being tested is exactly the runtime a build ships, with
//! no compile step and no in-editor copy of the game loop.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use eme::prelude::*;
use eme::ui_widgets::{Activate, Button};

use crate::build;
use crate::scene::SceneState;
use crate::screens::editor::chrome::Part;
use crate::screens::editor::icons;

/// Snapshot the running game reads, relative to the cache root.
const PLAY_SCENE: &str = "play/scene.ember";

/// Marks the icon slot that shows while a game is running.
#[derive(Component, Clone, Copy, Default)]
pub struct PlayGlyph(pub bool);

/// The player process started by the play button.
#[derive(Resource, Default)]
pub struct PlayState {
    child: Option<Child>,
}

impl PlayState {
    /// Whether a game process started by the editor is still alive.
    pub fn running(&self) -> bool {
        self.child.is_some()
    }

    /// Kills the game process if one is running.
    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Starts the runtime player against `state`'s document.
    pub fn start(&mut self, state: &SceneState) {
        if self.child.is_some() {
            return;
        }
        match launch(state) {
            Ok(child) => {
                info!("playing in pid {}", child.id());
                self.child = Some(child);
            }
            Err(err) => error!("could not start the game: {err}"),
        }
    }
}

impl Drop for PlayState {
    fn drop(&mut self) {
        self.stop();
    }
}

fn launch(state: &SceneState) -> Result<Child, String> {
    let player = build::player_binary()
        .ok_or_else(|| "no eme_player binary; run Build once to compile it".to_string())?;
    let scene = snapshot_path().ok_or_else(|| "no cache directory for the snapshot".to_string())?;
    if let Some(dir) = scene.parent() {
        std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    }
    let text = state
        .doc
        .to_ron()
        .map_err(|err| format!("could not encode the scene: {err}"))?;
    std::fs::write(&scene, text).map_err(|err| format!("{}: {err}", scene.display()))?;
    player_command(&player, &scene, &build::workspace_dir())
        .spawn()
        .map_err(|err| format!("{}: {err}", player.display()))
}

fn player_command(player: &Path, scene: &Path, cwd: &Path) -> Command {
    let mut command = Command::new(player);
    command.arg("--scene").arg(scene).current_dir(cwd);
    command
}

fn snapshot_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    Some(base.join("emberrly").join(PLAY_SCENE))
}

/// Starts the game, or stops the one already running.
pub fn toggle(_: On<Activate>, mut play: ResMut<PlayState>, scene: Res<SceneState>) {
    if play.running() {
        play.stop();
        return;
    }
    play.start(&scene);
}

/// Drops the handle once the player exits on its own.
pub fn poll(mut play: ResMut<PlayState>) {
    let finished = match play.child.as_mut() {
        Some(child) => matches!(child.try_wait(), Ok(Some(_))),
        None => false,
    };
    if finished {
        play.child = None;
    }
}

/// Shows the stop glyph while a game runs, the play glyph otherwise.
pub fn sync_button(play: Res<PlayState>, mut glyphs: Query<(&PlayGlyph, &mut Node)>) {
    for (glyph, mut node) in &mut glyphs {
        let want = if glyph.0 {
            play.running()
        } else {
            !play.running()
        };
        let display = if want { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
}

/// The top bar play/stop control.
pub fn button() -> Part {
    Box::new(bsn! {
        Node {
            width: px(24),
            height: px(24),
            display: Display::Flex,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border_radius: px(3),
        }
        Button
        Children [
            Node {
                display: Display::Flex,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
            }
            PlayGlyph(false)
            Children [ {icons::play()} ]
            --
            Node {
                display: Display::None,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
            }
            PlayGlyph(true)
            Children [ {icons::close()} ]
        ]
        on(toggle)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_player_is_launched_with_the_snapshot() {
        let command = player_command(
            Path::new("/bin/eme_player"),
            Path::new("/cache/play/scene.ember"),
            Path::new("/workspace"),
        );
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, ["--scene", "/cache/play/scene.ember"]);
        assert_eq!(command.get_current_dir(), Some(Path::new("/workspace")));
    }

    #[test]
    fn stopping_without_a_child_is_a_no_op() {
        let mut play = PlayState::default();
        play.stop();
        assert!(!play.running());
    }
}
