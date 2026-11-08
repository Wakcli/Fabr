//! Grid navigation: a walkable grid built from static physics bodies, A* paths,
//! and agents that follow them. The grid lives on the ground plane; agents are
//! expected to be simple movers rather than physics bodies.

use std::cmp::Ordering;
use alloc::collections::BinaryHeap;
use std::hash::{Hash, Hasher};

use eme_app::{App, Plugin, PreUpdate, Update};
use eme_ecs::prelude::*;
use eme_math::prelude::*;
use eme_time::Time;
use eme_transform::prelude::Transform;

use crate::PhysicsBody;

/// Default half extent of the navigation area, in world units.
pub const NAV_EXTENT: f32 = 25.0;
/// Default size of one navigation cell.
pub const NAV_CELL: f32 = 1.0;
/// Extra clearance blocked around static bodies so agents do not clip them.
pub const NAV_AGENT_RADIUS: f32 = 0.45;

/// Size and resolution of the navigation grid.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct NavGridSettings {
    /// Half extent of the square grid centred on the origin.
    pub extent: f32,
    /// Side length of one cell.
    pub cell: f32,
}

impl Default for NavGridSettings {
    fn default() -> Self {
        Self {
            extent: NAV_EXTENT,
            cell: NAV_CELL,
        }
    }
}

/// A square walkable grid on the XZ plane.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct NavGrid {
    origin: Vec2,
    cell: f32,
    cols: usize,
    rows: usize,
    blocked: Vec<bool>,
}

impl Default for NavGrid {
    fn default() -> Self {
        Self::from_settings(NavGridSettings::default())
    }
}

impl NavGrid {
    /// Empty grid covering the same area as `settings`.
    pub fn from_settings(settings: NavGridSettings) -> Self {
        let cell = settings.cell.max(0.1);
        let cols = ((settings.extent * 2.0) / cell).ceil().max(1.0) as usize;
        Self {
            origin: Vec2::splat(-settings.extent),
            cell,
            cols,
            rows: cols,
            blocked: vec![false; cols * cols],
        }
    }

    /// Empty grid with explicit bounds; used by tests.
    pub fn new(origin: Vec2, cell: f32, cols: usize, rows: usize) -> Self {
        Self {
            origin,
            cell: cell.max(0.1),
            cols: cols.max(1),
            rows: rows.max(1),
            blocked: vec![false; cols.max(1) * rows.max(1)],
        }
    }

    /// Number of columns.
    pub fn cols(&self) -> usize {
        self.cols
    }

    /// Number of rows.
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Side length of one cell.
    pub fn cell(&self) -> f32 {
        self.cell
    }

    /// Grid origin (minimum corner) on the XZ plane.
    pub fn origin(&self) -> Vec2 {
        self.origin
    }

    /// Cell containing `point`, if it lies inside the grid.
    pub fn cell_at(&self, point: Vec3) -> Option<(usize, usize)> {
        let local = Vec2::new(point.x, point.z) - self.origin;
        if local.x < 0.0 || local.y < 0.0 {
            return None;
        }
        let col = (local.x / self.cell).floor() as usize;
        let row = (local.y / self.cell).floor() as usize;
        (col < self.cols && row < self.rows).then_some((col, row))
    }

    /// World centre of a cell, at `y`.
    pub fn cell_center(&self, col: usize, row: usize, y: f32) -> Vec3 {
        Vec3::new(
            self.origin.x + (col as f32 + 0.5) * self.cell,
            y,
            self.origin.y + (row as f32 + 0.5) * self.cell,
        )
    }

    /// Whether a cell is outside the grid or occupied by an obstacle.
    pub fn is_blocked_cell(&self, col: usize, row: usize) -> bool {
        col >= self.cols || row >= self.rows || self.blocked[row * self.cols + col]
    }

    /// Whether the cell containing `point` is blocked or out of bounds.
    pub fn is_blocked(&self, point: Vec3) -> bool {
        self.cell_at(point)
            .is_none_or(|(col, row)| self.is_blocked_cell(col, row))
    }

    /// Blocks every cell overlapped by a world-space box footprint.
    pub fn block_footprint(&mut self, center: Vec3, half: Vec3) {
        let half = half + Vec3::splat(NAV_AGENT_RADIUS);
        let min = Vec2::new(center.x - half.x, center.z - half.z);
        let max = Vec2::new(center.x + half.x, center.z + half.z);
        let Some((c0, r0)) = self.cell_at(Vec3::new(min.x, 0.0, min.y)) else {
            return;
        };
        let Some((c1, r1)) = self.cell_at(Vec3::new(max.x, 0.0, max.y)) else {
            return;
        };
        for row in r0..=r1 {
            for col in c0..=c1 {
                self.blocked[row * self.cols + col] = true;
            }
        }
    }

    /// Grid with every static body's footprint blocked.
    pub fn with_statics<'a>(
        settings: NavGridSettings,
        statics: impl Iterator<Item = (Vec3, Vec3)> + 'a,
    ) -> Self {
        let mut grid = Self::from_settings(settings);
        for (center, half) in statics {
            grid.block_footprint(center, half);
        }
        grid
    }

    /// A* path from `from` to `to`, as world points.
    ///
    /// Eight-way movement with no corner cutting. Endpoints inside obstacles
    /// are nudged to the nearest free cell within two cells.
    pub fn find_path(&self, from: Vec3, to: Vec3) -> Option<Vec<Vec3>> {
        let start = self.free_cell(from)?;
        let goal = self.free_cell(to)?;
        let exact = self.cell_at(to) == Some(goal);
        let end_point = if exact {
            to
        } else {
            self.cell_center(goal.0, goal.1, to.y)
        };
        if start == goal {
            return Some(vec![end_point]);
        }
        let mut open = BinaryHeap::new();
        let mut cost = vec![u32::MAX; self.cols * self.rows];
        let mut came = vec![usize::MAX; self.cols * self.rows];
        let start_index = start.1 * self.cols + start.0;
        cost[start_index] = 0;
        open.push(Open {
            estimate: 0,
            index: start_index,
        });
        while let Some(Open { index, .. }) = open.pop() {
            let (col, row) = (index % self.cols, index / self.cols);
            if (col, row) == goal {
                return Some(self.rebuild(&came, index, from.y, end_point));
            }
            for (dc, dr, step) in NEIGHBOURS {
                let nc = col as isize + dc;
                let nr = row as isize + dr;
                if nc < 0 || nr < 0 || nc as usize >= self.cols || nr as usize >= self.rows {
                    continue;
                }
                let (nc, nr) = (nc as usize, nr as usize);
                if self.is_blocked_cell(nc, nr) {
                    continue;
                }
                if dc != 0
                    && dr != 0
                    && (self.is_blocked_cell(col, nr) || self.is_blocked_cell(nc, row))
                {
                    continue;
                }
                let next = nr * self.cols + nc;
                let tentative = cost[index].saturating_add(step);
                if tentative >= cost[next] {
                    continue;
                }
                cost[next] = tentative;
                came[next] = index;
                open.push(Open {
                    estimate: tentative + self.heuristic(nc, nr, goal),
                    index: next,
                });
            }
        }
        None
    }

    fn free_cell(&self, point: Vec3) -> Option<(usize, usize)> {
        let (col, row) = self.cell_at(point)?;
        if !self.is_blocked_cell(col, row) {
            return Some((col, row));
        }
        for radius in 1..=2isize {
            for dr in -radius..=radius {
                for dc in -radius..=radius {
                    let nc = col as isize + dc;
                    let nr = row as isize + dr;
                    if nc < 0 || nr < 0 || nc as usize >= self.cols || nr as usize >= self.rows {
                        continue;
                    }
                    if !self.is_blocked_cell(nc as usize, nr as usize) {
                        return Some((nc as usize, nr as usize));
                    }
                }
            }
        }
        None
    }

    fn heuristic(&self, col: usize, row: usize, goal: (usize, usize)) -> u32 {
        let dx = col.abs_diff(goal.0) as u32;
        let dy = row.abs_diff(goal.1) as u32;
        let diagonal = dx.min(dy);
        (dx.max(dy) - diagonal) * 10 + diagonal * 14
    }

    fn rebuild(&self, came: &[usize], end: usize, y: f32, end_point: Vec3) -> Vec<Vec3> {
        let mut points = Vec::new();
        let mut current = end;
        while current != usize::MAX {
            let (col, row) = (current % self.cols, current / self.cols);
            points.push(self.cell_center(col, row, y));
            current = came[current];
        }
        points.reverse();
        if let Some(last) = points.last_mut() {
            *last = end_point;
        }
        points
    }
}

const NEIGHBOURS: [(isize, isize, u32); 8] = [
    (1, 0, 10),
    (-1, 0, 10),
    (0, 1, 10),
    (0, -1, 10),
    (1, 1, 14),
    (1, -1, 14),
    (-1, 1, 14),
    (-1, -1, 14),
];

struct Open {
    estimate: u32,
    index: usize,
}

impl PartialEq for Open {
    fn eq(&self, other: &Self) -> bool {
        self.estimate == other.estimate
    }
}

impl Eq for Open {}

impl Ord for Open {
    fn cmp(&self, other: &Self) -> Ordering {
        other.estimate.cmp(&self.estimate)
    }
}

impl PartialOrd for Open {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A transform mover that follows a [`NavGrid`] path.
#[derive(Component, Clone, Debug)]
pub struct NavAgent {
    /// Movement speed in world units per second.
    pub speed: f32,
    /// Destination requested by a script.
    pub goal: Option<Vec3>,
    /// Whether the last request found a path.
    pub failed: bool,
    /// Whether the agent is standing on its goal.
    pub arrived: bool,
    path: Vec<Vec3>,
    next: usize,
}

impl Default for NavAgent {
    fn default() -> Self {
        Self {
            speed: 3.0,
            goal: None,
            failed: false,
            arrived: false,
            path: Vec::new(),
            next: 0,
        }
    }
}

impl NavAgent {
    /// Requests a new destination; the path is planned on the next step.
    pub fn go_to(&mut self, goal: Vec3) {
        self.goal = Some(goal);
        self.path.clear();
        self.next = 0;
        self.failed = false;
        self.arrived = false;
    }

    /// Stops and forgets the destination.
    pub fn stop(&mut self) {
        self.goal = None;
        self.path.clear();
        self.next = 0;
    }

    /// Planned points, when a path has been computed.
    pub fn path(&self) -> &[Vec3] {
        &self.path
    }
}

/// Rebuilds the grid when static bodies or settings change.
pub fn refresh_nav_grid(
    settings: Res<NavGridSettings>,
    mut grid: ResMut<NavGrid>,
    statics: Query<(&Transform, &PhysicsBody)>,
    mut last: Local<Option<u64>>,
) {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    settings.extent.to_bits().hash(&mut hasher);
    settings.cell.to_bits().hash(&mut hasher);
    for (transform, body) in &statics {
        if body.dynamic {
            continue;
        }
        for value in transform.translation.to_array() {
            value.to_bits().hash(&mut hasher);
        }
        for value in (body.half_extents * transform.scale.abs()).to_array() {
            value.to_bits().hash(&mut hasher);
        }
    }
    let signature = hasher.finish();
    if *last == Some(signature) {
        return;
    }
    *last = Some(signature);
    *grid = NavGrid::with_statics(
        *settings,
        statics
            .iter()
            .filter(|(_, body)| !body.dynamic)
            .map(|(transform, body)| {
                (
                    transform.translation,
                    body.half_extents * transform.scale.abs(),
                )
            }),
    );
}

/// Moves agents along their planned paths.
pub fn step_nav_agents(
    time: Res<Time>,
    grid: Res<NavGrid>,
    mut agents: Query<(&mut Transform, &mut NavAgent), Without<PhysicsBody>>,
) {
    let dt = time.delta_secs().clamp(0.0, 0.1);
    if dt <= 0.0 {
        return;
    }
    for (mut transform, mut agent) in &mut agents {
        let Some(goal) = agent.goal else {
            continue;
        };
        if agent.path.is_empty() && !agent.failed && !agent.arrived {
            match grid.find_path(transform.translation, goal) {
                Some(path) => {
                    agent.path = path;
                    agent.next = 0;
                }
                None => {
                    agent.failed = true;
                    continue;
                }
            }
        }
        if agent.next >= agent.path.len() {
            agent.arrived = true;
            agent.goal = None;
            continue;
        }
        let target = agent.path[agent.next];
        let delta = target - transform.translation;
        let distance = delta.length();
        let step = agent.speed * dt;
        if distance <= step {
            transform.translation = target;
            agent.next += 1;
            if agent.next >= agent.path.len() {
                agent.arrived = true;
                agent.goal = None;
            }
        } else {
            transform.translation += delta / distance * step;
        }
    }
}

/// System set for agent movement; order it after script updates.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct NavSet;

/// Registers grid refresh and agent movement.
pub struct NavPlugin;

impl Plugin for NavPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NavGridSettings>()
            .init_resource::<NavGrid>()
            .add_systems(PreUpdate, refresh_nav_grid)
            .add_systems(Update, step_nav_agents.in_set(NavSet));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walled_grid() -> NavGrid {
        let mut grid = NavGrid::new(Vec2::splat(-10.0), 1.0, 20, 20);
        for row in 0..15 {
            grid.blocked[row * grid.cols + 10] = true;
        }
        grid
    }

    #[test]
    fn cells_map_back_to_their_centre() {
        let grid = NavGrid::new(Vec2::splat(-10.0), 1.0, 20, 20);
        let (col, row) = grid.cell_at(Vec3::new(-9.5, 0.0, 5.5)).unwrap();
        assert_eq!((col, row), (0, 15));
        assert_eq!(grid.cell_center(col, row, 0.0), Vec3::new(-9.5, 0.0, 5.5));
        assert!(grid.cell_at(Vec3::new(40.0, 0.0, 0.0)).is_none());
    }

    #[test]
    fn a_path_goes_around_a_wall() {
        let grid = walled_grid();
        let path = grid
            .find_path(Vec3::new(-5.0, 0.0, 0.0), Vec3::new(5.0, 0.0, 0.0))
            .expect("a path around the wall");
        assert!(path.len() > 12, "path should detour, got {path:?}");
        for point in &path {
            assert!(!grid.is_blocked(*point), "{point:?} is blocked");
        }
        assert_eq!(*path.last().unwrap(), Vec3::new(5.0, 0.0, 0.0));
    }

    #[test]
    fn a_sealed_goal_has_no_path() {
        let mut grid = NavGrid::new(Vec2::splat(-10.0), 1.0, 20, 20);
        for dc in -1..=1 {
            for dr in -1..=1 {
                if dc == 0 && dr == 0 {
                    continue;
                }
                let col = (15.0 + dc as f32) as usize;
                let row = (15.0 + dr as f32) as usize;
                grid.blocked[row * grid.cols + col] = true;
            }
        }
        assert!(
            grid.find_path(Vec3::new(0.0, 0.0, 0.0), Vec3::new(5.5, 0.0, 5.5))
                .is_none()
        );
    }

    #[test]
    fn static_bodies_block_their_footprint() {
        let settings = NavGridSettings {
            extent: 10.0,
            cell: 1.0,
        };
        let grid = NavGrid::with_statics(
            settings,
            [(Vec3::new(0.0, 0.5, 0.0), Vec3::splat(0.5))].into_iter(),
        );
        assert!(grid.is_blocked(Vec3::new(0.0, 0.0, 0.0)));
        assert!(!grid.is_blocked(Vec3::new(4.0, 0.0, 0.0)));
    }

    #[test]
    fn a_goal_in_an_obstacle_is_nudged_to_a_free_cell() {
        let grid = walled_grid();
        let path = grid.find_path(
            Vec3::new(-5.0, 0.0, 0.0),
            Vec3::new(0.5, 0.0, 0.5),
        );
        assert!(path.is_some());
        let end = *path.unwrap().last().unwrap();
        assert!(!grid.is_blocked(end));
    }

    #[test]
    fn an_agent_walks_to_its_goal() {
        let mut world = World::new();
        world.insert_resource(NavGrid::new(Vec2::splat(-10.0), 1.0, 20, 20));
        world.insert_resource(Time::<()>::default());
        let agent = world
            .spawn((
                Transform::from_xyz(0.0, 0.0, 0.0),
                NavAgent {
                    speed: 5.0,
                    ..Default::default()
                },
            ))
            .id();
        world
            .get_mut::<NavAgent>(agent)
            .unwrap()
            .go_to(Vec3::new(5.0, 0.0, 0.0));
        let mut system = IntoSystem::into_system(step_nav_agents);
        system.initialize(&mut world);
        for _ in 0..120 {
            world
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(1.0 / 30.0));
            let _ = system.run((), &mut world);
        }
        let transform = world.get::<Transform>(agent).unwrap();
        assert!(
            (transform.translation.x - 5.0).abs() < 0.5,
            "agent stopped at {transform:?}"
        );
        assert!(world.get::<NavAgent>(agent).unwrap().arrived);
    }
}
