//! The console's scroll of lines.
//!
//! Panels are rebuilt whenever the dock layout changes, so log lines cannot live
//! inside the panel tree. They sit in this resource and the console panel
//! mirrors them, exactly like the undo history mirrors [`crate::undo::Undo`].

use eme::feathers::constants::fonts;
use eme::feathers::theme::ThemeTextColor;
use eme::prelude::*;
use eme::text::{FontSize, TextFont};

use crate::screens::editor::chrome::FONT_SIZE_OUTPUT;
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
    #[cfg_attr(not(test), expect(dead_code, reason = "phase 2 pushes a line after each import"))]
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
}

/// Host inside the console panel whose children mirror [`ConsoleLog`].
#[derive(Component, Clone, Copy, Default)]
pub struct ConsoleRows;

/// Rebuilds the console rows when a line is pushed.
pub fn sync(
    log: Res<ConsoleLog>,
    hosts: Query<Entity, With<ConsoleRows>>,
    mut commands: Commands,
    mut last: Local<Option<u64>>,
) {
    if *last == Some(log.rev()) {
        return;
    }
    *last = Some(log.rev());
    let Ok(host) = hosts.single() else {
        return;
    };
    commands.entity(host).despawn_children();
    for line in log.lines() {
        commands.spawn((
            Text(line.clone()),
            TextFont {
                font: fonts::MONO.into(),
                font_size: FontSize::Px(FONT_SIZE_OUTPUT),
                ..default()
            },
            ThemeTextColor(tokens::TEXT),
            ChildOf(host),
        ));
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

#[cfg(test)]
mod wiring {
    use super::*;
    use crate::import::{ImportSettings, import};

    #[test]
    fn an_import_summary_reaches_the_console_rows() {
        let mut app = App::new();
        app.init_resource::<ConsoleLog>()
            .add_systems(Update, sync);
        let host = app.world_mut().spawn(ConsoleRows).id();

        let path = std::env::temp_dir().join("fabr-console-summary.gltf");
        std::fs::write(
            &path,
            r#"{"asset":{"version":"2.0","generator":"console-test"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"name":"tri","mesh":0}],"meshes":[{"primitives":[{"attributes":{"POSITION":0},"indices":1}]}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]},{"bufferView":1,"componentType":5123,"count":3,"type":"SCALAR"}],"bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36},{"buffer":0,"byteOffset":36,"byteLength":6}],"buffers":[{"byteLength":42,"uri":"data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAgD8AAAAAAAABAAIA"}]}"#,
        )
        .unwrap();

        let report = import(&path, &ImportSettings::default()).expect("fixture imports");
        let summary = report.summary();
        app.world_mut()
            .resource_mut::<ConsoleLog>()
            .push(summary.clone());

        app.update();

        let rows = app.world().entity(host).get::<Children>().expect("rows attached");
        assert_eq!(rows.len(), 1, "one pushed line becomes one row");
        let line = app
            .world()
            .entity(rows[0])
            .get::<Text>()
            .expect("row is text");
        assert_eq!(line.0, summary);
    }
}
