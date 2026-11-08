//! The console's scroll of lines.
//!
//! Panels are rebuilt whenever the dock layout changes, so log lines cannot live
//! inside the panel tree. They sit in this resource and the console panel
//! mirrors them, exactly like the undo history mirrors [`crate::undo::Undo`].

use eme::input_focus::{FocusCause, InputFocus};
use eme::prelude::*;
use eme::text::EditableText;

use crate::screens::editor::chrome::{self, FONT_SIZE_OUTPUT};
use crate::tokens;

/// How many lines the console keeps before dropping the oldest.
const MAX_LINES: usize = 500;

/// Lines shown under the console's startup banner.
#[derive(Resource, Default)]
pub struct ConsoleLog {
    lines: Vec<String>,
    rev: u64,
}

impl ConsoleLog {
    /// Appends one line, dropping the oldest when the buffer is full.
    pub fn push(&mut self, line: impl Into<String>) {
        if self.lines.len() == MAX_LINES {
            self.lines.remove(0);
        }
        self.lines.push(line.into());
        self.rev = self.rev.wrapping_add(1);
    }

    /// Lines in the order they were pushed.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Bumped on every push so the panel knows when to rebuild.
    pub fn rev(&self) -> u64 {
        self.rev
    }

    /// Drops every line.
    pub fn clear(&mut self) {
        self.lines.clear();
        self.rev = self.rev.wrapping_add(1);
    }

    /// Lines containing `query`, case-insensitively; all lines when it is empty.
    pub fn matching(&self, query: &str) -> Vec<&str> {
        let query = query.trim().to_lowercase();
        self.lines()
            .iter()
            .filter(|line| query.is_empty() || line.to_lowercase().contains(&query))
            .map(String::as_str)
            .collect()
    }
}

/// Text typed into the console filter.
#[derive(Resource, Clone, Default)]
pub struct ConsoleFilter {
    /// Case-insensitive substring matched against every line.
    pub query: String,
    /// Bumped whenever [`query`](Self::query) changes.
    pub rev: u64,
}

/// The console filter's text input.
#[derive(Component, Clone, Copy, Default)]
pub struct ConsoleFilterInput;

/// Placeholder shown while the console filter is empty.
#[derive(Component, Clone, Copy, Default)]
pub struct ConsoleFilterHint;

/// Copies the filter field's text into [`ConsoleFilter`].
pub fn sync_filter(
    mut filter: ResMut<ConsoleFilter>,
    inputs: Query<&EditableText, With<ConsoleFilterInput>>,
) {
    let Ok(input) = inputs.single() else {
        return;
    };
    let value = input.value().to_string();
    if value == filter.query {
        return;
    }
    filter.query = value;
    filter.rev = filter.rev.wrapping_add(1);
}

/// Hides the placeholder once the filter has text.
pub fn sync_filter_hint(
    filter: Res<ConsoleFilter>,
    mut hints: Query<&mut Node, With<ConsoleFilterHint>>,
) {
    let want = if filter.query.is_empty() {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut hints {
        if node.display != want {
            node.display = want;
        }
    }
}

/// Gives the filter field input focus when it is pressed.
pub fn focus_filter(
    press: On<PointerPress>,
    inputs: Query<(), With<ConsoleFilterInput>>,
    mut focus: ResMut<InputFocus>,
) {
    if press.button != PointerButton::Primary || !inputs.contains(press.entity) {
        return;
    }
    focus.set(press.entity, FocusCause::Pressed);
}

/// Host inside the console panel whose children mirror [`ConsoleLog`].
#[derive(Component, Clone, Copy, Default)]
pub struct ConsoleRows;

/// Rebuilds the console rows when a line is pushed.
pub fn sync(
    log: Res<ConsoleLog>,
    filter: Res<ConsoleFilter>,
    hosts: Query<Entity, With<ConsoleRows>>,
    mut commands: Commands,
    mut last: Local<Option<(Entity, u64, u64)>>,
) {
    let Ok(host) = hosts.single() else {
        return;
    };
    if *last == Some((host, log.rev(), filter.rev)) {
        return;
    }
    *last = Some((host, log.rev(), filter.rev));
    chrome::despawn_children(&mut commands, host);
    for line in log.matching(&filter.query) {
        let text = chrome::text_mono(line, FONT_SIZE_OUTPUT, tokens::TEXT);
        commands.spawn_scene(eme::scene::bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
            }
            ChildOf(host)
            Children [ {text} ]
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pushing_bumps_the_revision() {
        let mut log = ConsoleLog::default();
        assert_eq!(log.rev(), 0);
        log.push("imported thing.glb");
        assert_eq!(log.rev(), 1);
        assert_eq!(log.matching(""), ["imported thing.glb"]);
    }

    #[test]
    fn the_filter_keeps_only_matching_lines() {
        let mut log = ConsoleLog::default();
        log.push("spin started");
        log.push("saved scene");
        assert_eq!(log.matching("SPIN"), ["spin started"]);
        assert_eq!(log.matching("").len(), 2);
        assert!(log.matching("missing").is_empty());
    }

    #[test]
    fn the_filter_field_updates_the_resource() {
        use eme::ecs::system::RunSystemOnce;

        let mut world = World::new();
        world.insert_resource(ConsoleFilter::default());
        world.spawn((EditableText::new("spin"), ConsoleFilterInput));
        world.run_system_once(sync_filter).unwrap();
        let filter = world.resource::<ConsoleFilter>();
        assert_eq!(filter.query, "spin");
        assert_eq!(filter.rev, 1);
    }

    #[test]
    fn the_oldest_line_falls_off_the_end() {
        let mut log = ConsoleLog::default();
        for index in 0..MAX_LINES + 5 {
            log.push(format!("line {index}"));
        }
        let lines = log.matching("");
        assert_eq!(lines.len(), MAX_LINES);
        assert_eq!(lines[0], "line 5");
    }
}
