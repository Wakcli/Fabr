//! The Extensions dock: live panels contributed by editor plugins.

use eme::editor::{EditorExtensions, PanelRow, PanelTone};
use eme::feathers::constants::fonts;
use eme::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor, ThemeTextColor, ThemeToken};
use eme::prelude::*;
use eme::scene::bsn;
use eme::text::{FontSize, FontSourceTemplate, FontWeight, LineBreak, TextFont, TextLayout};
use eme::ui_widgets::{Activate, Button, ScrollArea};

use super::chrome::{self, text, text_bold, Part, FONT_SIZE};
use crate::tokens;

/// One plugin panel as it reads this frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderedPanel {
    /// Title above the rows.
    pub title: String,
    /// Live label/value lines.
    pub rows: Vec<PanelRow>,
    /// Button labels, in run order.
    pub buttons: Vec<String>,
}

/// The panels evaluated from the plugin registry.
#[derive(Resource, Default)]
pub struct RenderedExtensions {
    panels: Vec<RenderedPanel>,
    revision: u64,
}

/// Buttons clicked since the last evaluation.
#[derive(Resource, Default)]
pub struct PendingPanelActions(Vec<(usize, usize)>);

/// Whether an Extensions panel body is currently on screen.
#[derive(Resource, Default)]
pub struct ExtensionsVisible(bool);

/// Host inside the dock whose children mirror the rendered panels.
#[derive(Component, Clone, Copy, Default)]
pub struct ExtensionsBody;

/// One button in a plugin panel.
#[derive(Component, Clone, Copy, Default)]
pub struct PanelAction {
    /// Index of the panel that owns the button.
    pub panel: usize,
    /// Index of the button inside the panel.
    pub button: usize,
}

/// The value text of one panel row, repainted in place.
#[derive(Component, Clone, Copy, Default)]
pub struct PanelRowValue {
    /// Index of the panel that owns the row.
    pub panel: usize,
    /// Index of the row inside the panel.
    pub row: usize,
}

/// Installs the panel resources and their systems.
pub fn install(app: &mut App) {
    app.init_resource::<RenderedExtensions>()
        .init_resource::<PendingPanelActions>()
        .init_resource::<ExtensionsVisible>()
        .add_systems(Update, (evaluate, sync, sync_values).chain());
}

/// The dock body; its children are owned by [`sync`].
pub fn panel() -> Part {
    Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            overflow: Overflow::scroll_y(),
        }
        ScrollArea
        ThemeBackgroundColor(tokens::PANEL)
        Children [
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                row_gap: px(10),
                padding: px(10),
            }
            ExtensionsBody
        ]
    })
}

/// Evaluates every panel and runs the buttons clicked last frame.
///
/// Row providers and button actions take `&mut World`, so this is exclusive.
pub fn evaluate(world: &mut World) {
    // a hidden panel costs nothing per frame; the first pass still runs so the
    // rows are ready the moment the tab is shown
    let visible = world
        .get_resource::<ExtensionsVisible>()
        .is_some_and(|visible| visible.0);
    let populated = world
        .get_resource::<RenderedExtensions>()
        .is_some_and(|rendered| !rendered.panels.is_empty());
    if !visible && populated {
        return;
    }

    let queued: Vec<(usize, usize)> = world
        .get_resource_mut::<PendingPanelActions>()
        .map(|mut pending| core::mem::take(&mut pending.0))
        .unwrap_or_default();

    let panels = world
        .try_resource_scope::<EditorExtensions, _>(|world, extensions| {
            for (panel, button) in queued {
                let Some(spec) = extensions.panels().get(panel) else {
                    continue;
                };
                let Some(action) = spec.buttons().get(button) else {
                    continue;
                };
                action.run(world);
            }
            extensions
                .panels()
                .iter()
                .map(|spec| RenderedPanel {
                    title: spec.title().to_owned(),
                    rows: spec.rows(world),
                    buttons: spec
                        .buttons()
                        .iter()
                        .map(|button| button.label().to_owned())
                        .collect(),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let Some(mut rendered) = world.get_resource_mut::<RenderedExtensions>() else {
        return;
    };
    if rendered.panels != panels {
        rendered.panels = panels;
        rendered.revision = rendered.revision.wrapping_add(1);
    }
}

/// The part of a panel that decides whether its UI is rebuilt.
type PanelShape = Vec<(String, Vec<(String, PanelTone)>, Vec<String>)>;

/// Rebuilds the dock structure when panel titles, row labels, or buttons change.
pub fn sync(
    rendered: Res<RenderedExtensions>,
    mut commands: Commands,
    hosts: Query<Entity, With<ExtensionsBody>>,
    mut visible: ResMut<ExtensionsVisible>,
    mut shown: Local<Option<(Entity, PanelShape)>>,
) {
    let Ok(host) = hosts.single() else {
        visible.0 = false;
        return;
    };
    visible.0 = true;
    let shape: PanelShape = rendered
        .panels
        .iter()
        .map(|panel| {
            (
                panel.title.clone(),
                panel
                    .rows
                    .iter()
                    .map(|row| (row.label.clone(), row.tone))
                    .collect(),
                panel.buttons.clone(),
            )
        })
        .collect();
    let signature = (host, shape);
    if shown.as_ref() == Some(&signature) {
        return;
    }
    *shown = Some(signature);
    chrome::despawn_children(&mut commands, host);

    if rendered.panels.is_empty() {
        commands.spawn_scene(bsn! {
            Node { padding: px(2) }
            ChildOf(host)
            Children [
                {text("No editor extensions installed.", FONT_SIZE, tokens::TEXT_DIM)}
            ]
        });
        return;
    }

    for (index, panel) in rendered.panels.iter().enumerate() {
        spawn_panel(&mut commands, host, index, panel);
    }
}

/// Repaints row values in place.
///
/// A plugin may report a new value every frame; replacing the rows would take
/// the buttons out from under the pointer, so only the text is updated.
pub fn sync_values(
    rendered: Res<RenderedExtensions>,
    mut rows: Query<(&PanelRowValue, &mut Text)>,
) {
    for (seat, mut text) in &mut rows {
        let Some(row) = rendered
            .panels
            .get(seat.panel)
            .and_then(|panel| panel.rows.get(seat.row))
        else {
            continue;
        };
        if text.0 != row.value {
            text.0 = row.value.clone();
        }
    }
}

fn spawn_panel(commands: &mut Commands, host: Entity, panel_index: usize, panel: &RenderedPanel) {
    let group = commands
        .spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                row_gap: px(3),
                padding: UiRect { bottom: px(4) },
            }
            ChildOf(host)
        })
        .id();

    commands.spawn_scene(bsn! {
        Node {
            height: px(24),
            display: Display::Flex,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(8)),
        }
        ThemeBackgroundColor(tokens::CATEGORY)
        ChildOf(group)
        Children [ {text_bold(panel.title.clone(), FONT_SIZE, tokens::SECTION_TEXT)} ]
    });

    for (index, row) in panel.rows.iter().enumerate() {
        commands.spawn_scene(bsn! {
            Node {
                min_height: px(18),
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(8),
                padding: UiRect::horizontal(px(8)),
            }
            ChildOf(group)
            Children [ {row_part(panel_index, index, row)} ]
        });
    }

    for (button, label) in panel.buttons.iter().enumerate() {
        commands.spawn_scene(bsn! {
            Node {
                height: px(24),
                align_self: AlignSelf::FlexStart,
                display: Display::Flex,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(10)),
                margin: UiRect { left: px(8), top: px(2) },
                border: UiRect { bottom: px(2) },
                border_radius: px(3),
            }
            ThemeBackgroundColor(tokens::FIELD)
            ThemeBorderColor(tokens::FIELD_LINE)
            Button
            PanelAction { panel: {panel_index}, button: {button} }
            ChildOf(group)
            Children [ {text(label.clone(), FONT_SIZE, tokens::TEXT)} ]
            on(activate_panel_button)
        });
    }
}

fn row_part(panel: usize, row_index: usize, row: &PanelRow) -> Part {
    let label = Box::new(bsn! {
        Node {
            width: px(120),
            display: Display::Flex,
            align_items: AlignItems::Center,
        }
        Children [ {text(row.label.clone(), FONT_SIZE, tokens::TEXT_MEDIUM)} ]
    }) as Part;
    let tint = tone(row.tone);
    let value_text = row.value.clone();
    let value = Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            flex_basis: px(0),
            min_width: px(0),
            display: Display::Flex,
            justify_content: JustifyContent::FlexStart,
            overflow: Overflow::clip(),
        }
        Children [ {clipped_value(value_text, tint, panel, row_index)} ]
    }) as Part;
    Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(8),
        }
        Children [ {label} -- {value} ]
    })
}

fn clipped_value(value: String, tint: ThemeToken, panel: usize, row: usize) -> Part {
    Box::new(bsn! {
        Text(value)
        TextFont {
            font: FontSourceTemplate::Handle(fonts::REGULAR),
            font_size: FontSize::Px(FONT_SIZE),
            weight: FontWeight::NORMAL,
        }
        TextLayout { linebreak: LineBreak::NoWrap }
        ThemeTextColor(tint)
        PanelRowValue { panel, row }
    })
}

fn tone(kind: PanelTone) -> ThemeToken {
    match kind {
        PanelTone::Text => tokens::TEXT,
        PanelTone::Ok => tokens::OK,
        PanelTone::Warn => tokens::WARN,
    }
}

fn activate_panel_button(
    activate: On<Activate>,
    actions: Query<&PanelAction>,
    mut pending: ResMut<PendingPanelActions>,
) {
    let Ok(action) = actions.get(activate.entity) else {
        return;
    };
    pending.0.push((action.panel, action.button));
}

#[cfg(test)]
mod tests {
    use super::*;
    use eme::ecs::system::RunSystemOnce;

    #[derive(Resource, Default)]
    struct Value(u32);

    fn world() -> World {
        let mut world = World::new();
        world.insert_resource(Value::default());
        world.insert_resource(RenderedExtensions::default());
        world.insert_resource(PendingPanelActions::default());
        world.insert_resource(ExtensionsVisible(true));
        let mut extensions = EditorExtensions::default();
        extensions.add_panel(
            eme::editor::PanelSpec::new("test.panel", "Test Panel", |world| {
                let value = world.resource::<Value>().0;
                vec![PanelRow::text("Value", value.to_string())]
            })
            .with_button("Bump", |world| world.resource_mut::<Value>().0 += 1),
        );
        world.insert_resource(extensions);
        world
    }

    #[test]
    fn the_rendered_signature_tracks_row_values() {
        let mut world = world();
        world.run_system_once(evaluate).unwrap();
        let revision = world.resource::<RenderedExtensions>().revision;
        assert_eq!(revision, 1);
        assert_eq!(
            world.resource::<RenderedExtensions>().panels[0].rows[0].value,
            "0"
        );

        world.run_system_once(evaluate).unwrap();
        assert_eq!(
            world.resource::<RenderedExtensions>().revision,
            revision,
            "an unchanged panel must not bump the revision"
        );

        world.resource_mut::<Value>().0 = 7;
        world.run_system_once(evaluate).unwrap();
        let rendered = world.resource::<RenderedExtensions>();
        assert_eq!(rendered.revision, revision + 1);
        assert_eq!(rendered.panels[0].rows[0].value, "7");
    }

    #[test]
    fn a_hidden_panel_skips_evaluation() {
        let mut world = world();
        world.run_system_once(evaluate).unwrap();
        let revision = world.resource::<RenderedExtensions>().revision;
        world.resource_mut::<ExtensionsVisible>().0 = false;
        world.resource_mut::<Value>().0 = 9;
        world.run_system_once(evaluate).unwrap();
        let rendered = world.resource::<RenderedExtensions>();
        assert_eq!(rendered.revision, revision);
        assert_eq!(rendered.panels[0].rows[0].value, "0");
    }

    /// Times the exclusive panel pass while hidden and while on screen.
    ///
    /// Run with `cargo test -p eme_editor --bin eme_editor bench_hidden_extension_pass -- --ignored --nocapture`.
    #[test]
    #[ignore = "benchmark"]
    fn bench_hidden_extension_pass() {
        use std::io::Write;
        use std::time::Instant;
        let mut world = World::new();
        world.insert_resource(Value::default());
        world.insert_resource(RenderedExtensions::default());
        world.insert_resource(PendingPanelActions::default());
        world.insert_resource(ExtensionsVisible(false));
        let mut extensions = EditorExtensions::default();
        for panel in 0..4 {
            extensions.add_panel(
                eme::editor::PanelSpec::new(
                    format!("bench.panel.{panel}"),
                    format!("Panel {panel}"),
                    |_| {
                        (0..5)
                            .map(|row| PanelRow::text(format!("Row {row}"), format!("{row}")))
                            .collect()
                    },
                )
                .with_button("Go", |_| {}),
            );
        }
        world.insert_resource(extensions);
        evaluate(&mut world);
        let runs = 10_000;
        let start = Instant::now();
        for _ in 0..runs {
            evaluate(&mut world);
        }
        let hidden = start.elapsed().as_secs_f64() * 1_000_000.0 / runs as f64;
        world.resource_mut::<ExtensionsVisible>().0 = true;
        let start = Instant::now();
        for _ in 0..runs {
            evaluate(&mut world);
        }
        let visible = start.elapsed().as_secs_f64() * 1_000_000.0 / runs as f64;
        let _ = writeln!(
            std::io::stderr(),
            "extension pass: 4 panels x 5 rows -> {hidden:.3} us/pass hidden, {visible:.3} us/pass visible"
        );
    }

    #[test]
    fn a_queued_button_runs_before_the_rows_are_evaluated() {
        let mut world = world();
        world.resource_mut::<PendingPanelActions>().0.push((0, 0));
        world.run_system_once(evaluate).unwrap();
        assert_eq!(world.resource::<Value>().0, 1);
        assert_eq!(
            world.resource::<RenderedExtensions>().panels[0].rows[0].value,
            "1"
        );
    }
}
