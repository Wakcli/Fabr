use crate::{DocError, DocState};

impl DocState {
    /// Snapshots the world as RON. Both entering and exiting play cost one full world
    /// serialize/deserialize, and entity ids are reassigned on exit.
    pub fn enter_play(&mut self) -> Result<(), DocError> {
        if self.play.is_some() {
            return Ok(());
        }
        self.play = Some(self.encode()?);
        Ok(())
    }

    /// Throws away everything done since [`Self::enter_play`].
    pub fn exit_play(&mut self) -> Result<(), DocError> {
        let Some(src) = &self.play else {
            return Ok(());
        };
        let world = self.decode(src)?;
        self.world = world;
        self.play = None;
        Ok(())
    }

    /// Whether the document is in play mode.
    pub fn is_playing(&self) -> bool {
        self.play.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eme_ecs::name::Name;
    use eme_editor_core::{History, Op};

    use crate::Rename;

    #[test]
    fn edits_during_play_are_dropped() {
        let mut doc = DocState::new_empty();
        doc.world_mut().spawn(Name::new("Kept"));
        doc.enter_play().unwrap();
        assert!(doc.is_playing());

        let spawned = doc.world_mut().spawn(Name::new("Temp")).id();
        let kept = doc.hierarchy()[0].entity;
        let mut history = History::default();
        history.push(
            &mut doc,
            Box::new(Rename {
                entity: kept,
                before: "Kept".into(),
                after: "Changed".into(),
            }),
        );
        assert_eq!(doc.hierarchy().len(), 2);

        doc.exit_play().unwrap();
        assert!(!doc.is_playing());
        let rows = doc.hierarchy();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Kept");
        assert!(doc.world().get_entity(spawned).is_err());
    }

    #[test]
    fn enter_play_twice_keeps_the_first_snapshot() {
        let mut doc = DocState::new_empty();
        doc.world_mut().spawn(Name::new("Kept"));
        doc.enter_play().unwrap();
        doc.world_mut().spawn(Name::new("Temp"));
        doc.enter_play().unwrap();
        doc.exit_play().unwrap();
        assert_eq!(doc.hierarchy().len(), 1);
    }

    #[test]
    fn exit_play_without_entering_is_a_no_op() {
        let mut doc = DocState::new_empty();
        doc.world_mut().spawn(Name::new("Kept"));
        doc.exit_play().unwrap();
        assert_eq!(doc.hierarchy().len(), 1);
    }
}
