//! Per-phase CPU timings for the Stats panel.

use std::time::Instant;

use eme::prelude::*;

/// Editor frame phases that are timed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Script updates.
    Scripts,
    /// Physics step.
    Physics,
    /// Navigation agents.
    Nav,
    /// Dock and panel sync.
    Ui,
}

impl Phase {
    /// Every phase, in display order.
    pub const ALL: [Phase; 4] = [Phase::Scripts, Phase::Physics, Phase::Nav, Phase::Ui];

    /// Index used by [`FrameProfile::ms`].
    pub fn index(self) -> usize {
        self as usize
    }

    /// Short label shown in the Stats panel.
    pub fn label(self) -> &'static str {
        match self {
            Phase::Scripts => "Scripts",
            Phase::Physics => "Physics",
            Phase::Nav => "Nav",
            Phase::Ui => "UI sync",
        }
    }
}

/// Timing state for the phases in the current frame.
#[derive(Resource, Default)]
pub struct FrameProfile {
    starts: [Option<Instant>; 4],
    /// Last completed duration per phase, in milliseconds.
    pub ms: [f32; 4],
}

impl FrameProfile {
    /// Marks the start of `phase`.
    pub fn begin(&mut self, phase: Phase) {
        self.starts[phase.index()] = Some(Instant::now());
    }

    /// Marks the end of `phase` and stores its duration.
    pub fn end(&mut self, phase: Phase) {
        let Some(start) = self.starts[phase.index()].take() else {
            return;
        };
        self.ms[phase.index()] = start.elapsed().as_secs_f32() * 1000.0;
    }
}

/// System set covering the panel sync work measured as UI.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct UiProfileSet;

fn begin(profile: &mut FrameProfile, phase: Phase) {
    profile.begin(phase);
}

fn end(profile: &mut FrameProfile, phase: Phase) {
    profile.end(phase);
}

pub fn begin_scripts(mut profile: ResMut<FrameProfile>) {
    begin(&mut profile, Phase::Scripts);
}

pub fn end_scripts(mut profile: ResMut<FrameProfile>) {
    end(&mut profile, Phase::Scripts);
}

pub fn begin_physics(mut profile: ResMut<FrameProfile>) {
    begin(&mut profile, Phase::Physics);
}

pub fn end_physics(mut profile: ResMut<FrameProfile>) {
    end(&mut profile, Phase::Physics);
}

pub fn begin_nav(mut profile: ResMut<FrameProfile>) {
    begin(&mut profile, Phase::Nav);
}

pub fn end_nav(mut profile: ResMut<FrameProfile>) {
    end(&mut profile, Phase::Nav);
}

pub fn begin_ui(mut profile: ResMut<FrameProfile>) {
    begin(&mut profile, Phase::Ui);
}

pub fn end_ui(mut profile: ResMut<FrameProfile>) {
    end(&mut profile, Phase::Ui);
}

/// Registers the timing resource.
pub fn install(app: &mut App) {
    app.init_resource::<FrameProfile>();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_begin_and_end_independently() {
        let mut profile = FrameProfile::default();
        profile.begin(Phase::Scripts);
        std::thread::sleep(std::time::Duration::from_millis(2));
        profile.end(Phase::Scripts);
        assert!(profile.ms[Phase::Scripts.index()] >= 1.0);
        assert_eq!(profile.ms[Phase::Physics.index()], 0.0);
    }

    #[test]
    fn ending_without_beginning_is_ignored() {
        let mut profile = FrameProfile::default();
        profile.end(Phase::Nav);
        assert_eq!(profile.ms[Phase::Nav.index()], 0.0);
    }
}
