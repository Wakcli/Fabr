//! Emberrly runtime player: loads an exported scene document and renders it.

mod scene;

use std::io::Write;
use std::path::{Path, PathBuf};

use eme::asset::AssetPlugin;
use eme::prelude::*;
use eme::render::view::screenshot::{save_to_disk, Screenshot, ScreenshotCaptured};
use eme::window::{PresentMode, WindowResolution};

const DEMO_PATH: &str = "scenes/player-demo.ember";
const SHOT_FRAME: u32 = 40;

/// Assets next to the executable when shipped, otherwise the development folder.
pub(crate) fn asset_root() -> PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent().map(|dir| dir.join("assets"))
        && dir.is_dir()
    {
        return dir;
    }
    std::env::current_dir()
        .map(|dir| dir.join("assets"))
        .unwrap_or_else(|_| PathBuf::from("assets"))
}

#[derive(Default)]
struct Args {
    scene: Option<PathBuf>,
    host: Option<u16>,
    connect: Option<String>,
    write_demo: bool,
    help: bool,
}

impl Args {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut out = Self::default();
        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--scene" | "-s" => {
                    let path = args.next().ok_or("--scene needs a path")?;
                    out.scene = Some(path.into());
                }
                "--host" => {
                    let port = args.next().ok_or("--host needs a port")?;
                    out.host = Some(port.parse().map_err(|_| "invalid host port")?);
                }
                "--connect" => {
                    out.connect = Some(args.next().ok_or("--connect needs host:port")?);
                }
                "--write-demo" => out.write_demo = true,
                "--help" | "-h" => out.help = true,
                other => return Err(format!("unknown argument `{other}`; try --help")),
            }
        }
        Ok(out)
    }
}

fn main() {
    let args = Args::parse(std::env::args().skip(1)).unwrap_or_else(|err| die(err));

    if args.help {
        say(HELP);
        return;
    }
    if args.write_demo {
        write_demo();
        return;
    }

    let loaded = scene::load(args.scene.as_deref()).unwrap_or_else(|err| die(err));
    let config = fabr::FabrConfig::load(Path::new(".")).unwrap_or_else(|err| {
        let _ = writeln!(std::io::stderr(), "fabr settings: {err}; using defaults");
        fabr::FabrConfig::default()
    });
    let settings = &config.settings;
    let actions = config.input.clone();
    let title = if loaded.doc.objects.is_empty() {
        format!("{} (empty scene)", settings.player.title)
    } else {
        settings.player.title.clone()
    };
    let present_mode = if settings.player.vsync {
        PresentMode::AutoVsync
    } else {
        PresentMode::AutoNoVsync
    };

    let assets = asset_root();
    App::new()
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            file_path: assets.to_string_lossy().into_owned(),
            ..default()
        }).set(WindowPlugin {
            primary_window: Some(Window {
                title: title.clone(),
                resolution: WindowResolution::new(settings.player.width, settings.player.height),
                present_mode,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(fabr::ScriptPlugin)
        .add_plugins(eme_anim::AnimationRuntimePlugin)
        .add_plugins(eme_net::NetworkPlugin)
        .insert_resource(network_config(&args))
        .insert_resource(fabr::ScriptsActive(!matches!(
            network_config(&args).role,
            eme_net::NetworkRole::Client { .. }
        )))
        .insert_resource(fabr::ScriptRoot(assets))
        .insert_resource(actions)
        .insert_resource(scene::PlayerMsaa(settings.graphics.msaa))
        .insert_resource(scene::PlayerShadows(settings.quality.shadows))
        .insert_resource(fabr::AudioMix {
            sfx: settings.audio.sfx_volume,
            music: settings.audio.music_volume,
        })
        .insert_resource(eme_game_runtime::PhysicsSettings {
            gravity: settings.physics.gravity,
            bounce: settings.physics.bounce,
            enabled: settings.physics.enabled
                && !matches!(network_config(&args).role, eme_net::NetworkRole::Client { .. }),
        })
        .insert_resource(GlobalVolume::new(eme::audio::Volume::Linear(
            settings.audio.master_volume,
        )))
        .insert_resource(loaded)
        .add_systems(Startup, (scene::announce, scene::spawn_scene))
        .add_systems(
            Update,
            (
                exit_on_esc,
                capture_on_frame,
                eme_game_runtime::run_behaviours.run_if(is_not_client),
                eme_game_overrides::sync_model_nodes,
                log_script_lines,
                eme_game_runtime::step_physics.after(fabr::update_scripts),
                log_network,
            ),
        )
            .configure_sets(
            Update,
            eme_net::NetworkSet
                .after(eme_game_runtime::step_physics)
                .after(fabr::update_scripts),
        )
        .run();
}

fn is_not_client(config: Res<eme_net::NetworkConfig>) -> bool {
    !matches!(config.role, eme_net::NetworkRole::Client { .. })
}

fn network_config(args: &Args) -> eme_net::NetworkConfig {
    let role = if let Some(port) = args.host {
        eme_net::NetworkRole::Server { port }
    } else if let Some(addr) = &args.connect {
        eme_net::NetworkRole::Client { addr: addr.clone() }
    } else {
        eme_net::NetworkRole::Offline
    };
    eme_net::NetworkConfig { role }
}

fn log_network(status: Res<eme_net::NetworkStatus>, mut last: Local<u64>) {
    if status.rev == *last {
        return;
    }
    *last = status.rev;
    if let Some(error) = &status.error {
        info!("net: {error}");
    } else {
        info!("net: {} peer(s)", status.peers);
    }
}

const HELP: &str = "\
Emberrly runtime player

usage: eme_player [--scene <path>]
       eme_player --write-demo

  --scene <path>  render the given scene document; without it the player tries
                  scene.ember next to the executable, then scenes/main.ember
  --host <port>   host a local replication server
  --connect <a>   connect to host:port and apply replicated transforms
  --write-demo    write a demo scene to scenes/player-demo.ember and exit
  -h, --help      show this text

env:
  EME_PLAYER_SHOT        save a screenshot of the game window here and exit
  EME_PLAYER_SHOT_FRAME  frame to capture (default 40)";

fn write_demo() {
    let text = scene::demo_doc()
        .to_ron()
        .unwrap_or_else(|err| die(format!("could not encode the demo scene: {err}")));
    if let Some(dir) = PathBuf::from(DEMO_PATH).parent()
        && let Err(err) = std::fs::create_dir_all(dir)
    {
        die(format!("could not create {}: {err}", dir.display()));
    }
    if let Err(err) = std::fs::write(DEMO_PATH, text) {
        die(format!("could not write {DEMO_PATH}: {err}"));
    }
    say(&format!("wrote {DEMO_PATH}"));
}

fn log_script_lines(mut log: ResMut<fabr::ScriptLog>, mut seen: Local<u64>) {
    if log.rev() == *seen {
        return;
    }
    *seen = log.rev();
    for line in log.drain() {
        info!("{line}");
    }
}

fn exit_on_esc(keys: Res<ButtonInput<KeyCode>>, mut exit: MessageWriter<AppExit>) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
}

fn capture_on_frame(mut commands: Commands, mut frame: Local<u32>) {
    let Ok(path) = std::env::var("EME_PLAYER_SHOT") else {
        return;
    };
    let target: u32 = std::env::var("EME_PLAYER_SHOT_FRAME")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(SHOT_FRAME);
    *frame += 1;
    if *frame != target {
        return;
    }
    commands.spawn(Screenshot::primary_window()).observe(
        move |captured: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
            save_to_disk(path.clone())(captured);
            exit.write(AppExit::Success);
        },
    );
    info!("capture requested at frame {target}");
}

fn say(msg: &str) {
    let _ = writeln!(std::io::stdout(), "{msg}");
}

fn die(msg: impl core::fmt::Display) -> ! {
    let _ = writeln!(std::io::stderr(), "eme_player: {msg}");
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Result<Args, String> {
        Args::parse(list.iter().map(|s| (*s).to_owned()))
    }

    #[test]
    fn scene_flag_takes_a_path() {
        let a = args(&["--scene", "build/game.ember"]).unwrap();
        assert_eq!(
            a.scene.as_deref(),
            Some(Path::new("build/game.ember"))
        );
        assert!(!a.write_demo);
        assert!(!a.help);
    }

    #[test]
    fn scene_flag_without_a_path_is_an_error() {
        assert!(args(&["--scene"]).is_err());
    }

    #[test]
    fn unknown_arguments_fail_instead_of_being_ignored() {
        assert!(args(&["--fullscreen"]).is_err());
    }

    #[test]
    fn help_and_demo_are_independent_flags() {
        let a = args(&["--write-demo", "--help"]).unwrap();
        assert!(a.write_demo && a.help && a.scene.is_none());
    }
}
