use eme::feathers::{
    constants::fonts,
    containers::*,
    controls::*,
    display::{caption, label_dim},
    theme::{ThemeBackgroundColor, ThemeTextColor},
    tokens,
};
use eme::prelude::*;
use eme::text::{EditableText, FontSize, FontSourceTemplate, FontWeight, TextFont};
use eme::ui::{InteractionDisabled, Selected};
use eme::ui_widgets::{listbox_update_selection, Activate, ValueChange};
use eme_editor_core::{HubState, ProjectId, EDITOR_VERSION};

use crate::{app::Screen, HubRes};

/// Project hub scene.
pub fn screen(hub: HubState) -> impl Scene {
    let rows = rows(&hub);
    bsn! {
        Node {
            width: percent(100),
            height: percent(100),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
        }
        ThemeBackgroundColor(tokens::WINDOW_BG)
        Children [
            @pane_header()
            Children [
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(8),
                }
                Children [
                    Node { width: px(23), height: px(22) }
                    ImageNode { image: "editor/logo.png" }
                    --
                    Text({ crate::PRODUCT })
                    TextFont {
                        font: FontSourceTemplate::Handle(fonts::BOLD),
                        font_size: FontSize::Px(17.0),
                        weight: FontWeight::BOLD,
                    }
                    ThemeTextColor(tokens::TEXT_MAIN)
                ]
                --
                @flex_spacer()
                --
                @label_dim(EDITOR_VERSION)
            ]
            --
            @pane_body()
            Node { flex_grow: 1.0 }
            Children [
                @subpane()
                Node { flex_grow: 1.0 }
                Children [
                    @subpane_header()
                    Children [
                        @caption("Recent projects")
                    ]
                    --
                    @subpane_body()
                    Node { flex_grow: 1.0 }
                    Children [
                        Node { width: percent(100) }
                        Children [
                            @FeathersTextInputContainer
                            Children [
                                @FeathersTextInput
                                HubFilter
                            ]
                        ]
                        --
                        @FeathersListView {
                            @rows: {rows},
                        }
                        Node { flex_grow: 1.0 }
                        on(listbox_update_selection)
                        on(on_row_selected)
                        --
                        Node {
                            width: percent(100),
                            display: Display::Flex,
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Center,
                        }
                        Children [
                            @FeathersButton {
                                @caption: bsn! { @caption("New Project") },
                            }
                            on(|_: On<Activate>, mut screen: ResMut<Screen>| {
                                *screen = Screen::Editor;
                            })
                            --
                            @flex_spacer()
                            --
                            @FeathersButton {
                                @caption: bsn! { @caption("Open") },
                                @variant: ButtonVariant::Primary,
                            }
                            HubOpen
                            InteractionDisabled
                            on(|_: On<Activate>, mut screen: ResMut<Screen>| {
                                *screen = Screen::Editor;
                            })
                        ]
                    ]
                ]
            ]
        ]
    }
}

fn rows(hub: &HubState) -> Box<dyn SceneList> {
    if hub.recent.is_empty() {
        return Box::new(bsn! {
            @label_dim("No recent projects")
        });
    }

    let mut list = Vec::with_capacity(hub.recent.len());
    for project in &hub.recent {
        let id = project.id;
        let body: Box<dyn SceneList> = Box::new(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
            }
            Children [
                @caption(project.name.clone())
                --
                @label_dim(project.root.display().to_string())
            ]
            --
            @label_dim(opened(project.last_opened))
        });
        list.push(Box::new(bsn! {
            @FeathersListRow
            HubRow(id)
            Children [
                {body}
            ]
        }) as Box<dyn SceneList>);
    }
    Box::new(list)
}

fn opened(last: Option<u64>) -> String {
    match last {
        Some(secs) => format!("opened {secs}"),
        None => "never opened".into(),
    }
}

#[derive(Component, Clone, Copy)]
struct HubRow(ProjectId);

impl Default for HubRow {
    fn default() -> Self {
        Self(ProjectId(u64::MAX))
    }
}

#[derive(Component, Clone, Default)]
struct HubFilter;

#[derive(Component, Clone, Default)]
struct HubOpen;

/// Hub screen systems.
pub struct HubPlugin;

impl Plugin for HubPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (read_filter, sync_hub));
    }
}

fn read_filter(input: Single<&EditableText, With<HubFilter>>, mut hub: ResMut<HubRes>) {
    let text = input.value().to_string();
    if text != hub.filter {
        hub.set_filter(&text);
    }
}

fn on_row_selected(change: On<ValueChange<Entity>>, rows: Query<&HubRow>, mut hub: ResMut<HubRes>) {
    let Ok(row) = rows.get(change.value) else {
        return;
    };
    hub.selected = Some(row.0);
}

fn sync_hub(
    hub: Res<HubRes>,
    mut rows: Query<(
        Entity,
        &HubRow,
        &mut Node,
        Has<Selected>,
        Has<InteractionDisabled>,
    )>,
    mut open: Query<(Entity, Has<InteractionDisabled>), With<HubOpen>>,
    mut commands: Commands,
) {
    let visible: Vec<ProjectId> = hub.filtered().iter().map(|p| p.id).collect();

    for (ent, row, mut node, selected, disabled) in &mut rows {
        let shown = visible.contains(&row.0);
        let display = if shown { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }

        let want_selected = hub.selected == Some(row.0);
        if want_selected != selected {
            if want_selected {
                commands.entity(ent).insert(Selected);
            } else {
                commands.entity(ent).remove::<Selected>();
            }
        }

        let want_disabled = !shown;
        if disabled != want_disabled {
            if want_disabled {
                commands.entity(ent).insert(InteractionDisabled);
            } else {
                commands.entity(ent).remove::<InteractionDisabled>();
            }
        }
    }

    let want_disabled = hub.selected.is_none();
    for (ent, disabled) in &mut open {
        if disabled == want_disabled {
            continue;
        }
        if want_disabled {
            commands.entity(ent).insert(InteractionDisabled);
        } else {
            commands.entity(ent).remove::<InteractionDisabled>();
        }
    }
}
