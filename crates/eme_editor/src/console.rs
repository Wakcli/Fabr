//! The console's scroll of lines.
//!
//! Panels are rebuilt whenever the dock layout changes, so log lines cannot live
//! inside the panel tree. They sit in this resource and the console panel
//! mirrors them, exactly like the undo history mirrors [`crate::undo::Undo`].

use eme::prelude::*;

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
}

/// Host inside the console panel whose children mirror [`ConsoleLog`].
#[derive(Component, Clone, Copy, Default)]
pub struct ConsoleRows;

/// Rebuilds the console rows when a line is pushed.
pub fn sync(
    log: Res<ConsoleLog>,
    hosts: Query<Entity, With<ConsoleRows>>,
    mut commands: Commands,
    mut last: Local<Option<(Entity, u64)>>,
) {
    let Ok(host) = hosts.single() else {
        return;
    };
    if *last == Some((host, log.rev())) {
        return;
    }
    *last = Some((host, log.rev()));
    commands.entity(host).despawn_children();
    for line in log.lines() {
        let text = chrome::text_mono(line.clone(), FONT_SIZE_OUTPUT, tokens::TEXT);
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
        assert_eq!(log.lines(), ["imported thing.glb"]);
    }

    #[test]
    fn the_oldest_line_falls_off_the_end() {
        let mut log = ConsoleLog::default();
        for index in 0..MAX_LINES + 5 {
            log.push(format!("line {index}"));
        }
        assert_eq!(log.lines().len(), MAX_LINES);
        assert_eq!(log.lines()[0], "line 5");
    }
}
