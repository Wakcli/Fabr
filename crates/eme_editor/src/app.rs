use eme::prelude::*;

use crate::{
    screens::{editor_screen, hub_screen},
    HubRes, LayoutRes,
};

/// Which shell screen is visible.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Screen {
    /// Project hub.
    #[default]
    Hub,
    /// Editor frame.
    Editor,
}

/// Root entity of the currently shown screen.
#[derive(Component, Clone, Default)]
pub struct ScreenRoot;

#[derive(Resource, Default)]
struct Showing(Option<Screen>);

/// Installs the screen state and swaps the active screen.
pub struct EditorShellPlugin;

impl Plugin for EditorShellPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Screen>()
            .init_resource::<Showing>()
            .add_systems(Update, swap_screen);
    }
}

fn swap_screen(
    mut commands: Commands,
    screen: Res<Screen>,
    mut showing: ResMut<Showing>,
    hub: Res<HubRes>,
    layout: Res<LayoutRes>,
    roots: Query<Entity, With<ScreenRoot>>,
) {
    if showing.0 == Some(*screen) {
        return;
    }
    showing.0 = Some(*screen);
    for ent in &roots {
        commands.entity(ent).despawn();
    }
    match *screen {
        Screen::Hub => {
            commands.spawn_scene(bsn! { @hub_screen((*hub).clone()) ScreenRoot });
        }
        Screen::Editor => {
            commands.spawn_scene(bsn! { @editor_screen((*layout).clone()) ScreenRoot });
        }
    }
}
