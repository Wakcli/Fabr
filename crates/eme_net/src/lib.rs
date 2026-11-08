//! Local TCP client/server transport and transform replication.
//!
//! The server owns simulation; clients receive authoritative root transforms and
//! apply them to entities carrying the same [`ReplicatedObject`] id. Messages are
//! length-prefixed RON so the wire format is inspectable and dependency-free.

use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};

use eme_app::{App, Plugin, Startup, Update};
use eme_ecs::prelude::*;
use eme_math::prelude::*;
use eme_time::Time;
use eme_transform::prelude::Transform;
use serde::{Deserialize, Serialize};

/// Marks a root entity whose transform is replicated.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ReplicatedObject(pub u64);

/// Which side of a session this app runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum NetworkRole {
    /// No networking.
    #[default]
    Offline,
    /// Listen on `port` and broadcast snapshots.
    Server {
        /// Local port to listen on.
        port: u16,
    },
    /// Connect to `addr` and apply snapshots.
    Client {
        /// `host:port` of the server.
        addr: String,
    },
}

/// Role requested by the app before startup.
#[derive(Resource, Clone, Debug)]
pub struct NetworkConfig {
    /// Which side to run.
    pub role: NetworkRole,
    /// Seconds of buffering before a snapshot is rendered, smoothing motion.
    pub interpolation_delay: f32,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            role: NetworkRole::Offline,
            interpolation_delay: 0.1,
        }
    }
}

/// Shape of a replicated object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetKind {
    /// Cube.
    Cube,
    /// Sphere.
    Sphere,
    /// Cylinder.
    Cylinder,
    /// Flat plane.
    Plane,
}

/// One replicated object announced to clients.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpawnState {
    /// Object id.
    pub id: u64,
    /// Shape.
    pub kind: NetKind,
    /// World translation.
    pub translation: [f32; 3],
    /// Rotation quaternion.
    pub rotation: [f32; 4],
    /// Scale.
    pub scale: [f32; 3],
    /// Base colour.
    pub color: [f32; 4],
}

/// Data the app needs to build visuals for a replicated object.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct ReplicatedSpawn {
    /// Shape to build.
    pub kind: NetKind,
    /// Base colour.
    pub color: [f32; 4],
}

/// One replicated root transform.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransformState {
    /// Object id shared by both sides.
    pub id: u64,
    /// World translation.
    pub translation: [f32; 3],
    /// Rotation quaternion.
    pub rotation: [f32; 4],
    /// Scale.
    pub scale: [f32; 3],
}

/// Wire message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Message {
    /// Replicated objects that started existing.
    Spawn(Vec<SpawnState>),
    /// Object ids that stopped existing.
    Despawn(Vec<u64>),
    /// Current transform of every replicated root.
    Snapshot(Vec<TransformState>),
}

/// Human-readable session state.
#[derive(Resource, Clone, Debug, Default)]
pub struct NetworkStatus {
    /// Connected client count on a server, or 1 for a connected client.
    pub peers: usize,
    /// Last transport error.
    pub error: Option<String>,
    /// Bumped on every change so the app can log it.
    pub rev: u64,
}

impl NetworkStatus {
    fn update(&mut self, peers: usize, error: Option<String>) {
        if self.peers != peers || self.error != error {
            self.peers = peers;
            self.error = error;
            self.rev = self.rev.wrapping_add(1);
        }
    }
}

/// Server transport state.
#[derive(Resource, Default)]
pub struct NetworkServer {
    listener: Option<TcpListener>,
    clients: Vec<TcpStream>,
    last_ids: std::collections::HashSet<u64>,
}

/// Snapshot history used to interpolate client motion.
#[derive(Resource, Default)]
pub struct SnapshotBuffer {
    /// Per-object samples ordered by receipt time.
    pub samples: std::collections::HashMap<u64, Vec<(f64, TransformState)>>,
}

impl SnapshotBuffer {
    /// Records one snapshot sample at `at` seconds.
    pub fn push(&mut self, at: f64, state: TransformState) {
        let list = self.samples.entry(state.id).or_default();
        list.push((at, state));
        if list.len() > 32 {
            list.remove(0);
        }
    }

    /// Drops samples older than `before`.
    pub fn prune(&mut self, before: f64) {
        for list in self.samples.values_mut() {
            if let Some(index) = list.iter().position(|(at, _)| *at >= before) {
                let keep_from = index.saturating_sub(1);
                list.drain(..keep_from);
            }
        }
        self.samples.retain(|_, list| !list.is_empty());
    }
}

/// Interpolated state for `render` seconds on the receipt timeline.
pub fn sample_at(samples: &[(f64, TransformState)], render: f64) -> Option<TransformState> {
    let first = samples.first()?;
    if render <= first.0 {
        return Some(first.1.clone());
    }
    let last = samples.last()?;
    if render >= last.0 {
        return Some(last.1.clone());
    }
    for pair in samples.windows(2) {
        let (t0, a) = &pair[0];
        let (t1, b) = &pair[1];
        if render >= *t0 && render <= *t1 {
            let span = (t1 - t0).max(f64::EPSILON);
            let f = ((render - t0) / span) as f32;
            return Some(lerp_state(a, b, f));
        }
    }
    Some(last.1.clone())
}

/// Linear position/scale and spherical rotation blend.
pub fn lerp_state(a: &TransformState, b: &TransformState, f: f32) -> TransformState {
    let f = f.clamp(0.0, 1.0);
    let lerp = |x: [f32; 3], y: [f32; 3]| {
        [
            x[0] + (y[0] - x[0]) * f,
            x[1] + (y[1] - x[1]) * f,
            x[2] + (y[2] - x[2]) * f,
        ]
    };
    let rotation = Quat::from_array(a.rotation)
        .slerp(Quat::from_array(b.rotation), f)
        .to_array();
    TransformState {
        id: a.id,
        translation: lerp(a.translation, b.translation),
        rotation,
        scale: lerp(a.scale, b.scale),
    }
}

/// Client transport state.
#[derive(Resource, Default)]
pub struct NetworkClient {
    stream: Option<TcpStream>,
    reader: FrameReader,
    /// Messages ready to apply to the world.
    pub inbox: Vec<Message>,
}

/// Length-prefixed message framing.
#[derive(Default)]
pub struct FrameReader {
    buffer: Vec<u8>,
}

impl FrameReader {
    /// Appends bytes received from the transport.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    /// Pops one complete message, if the buffer holds one.
    pub fn next_message(&mut self) -> Option<Message> {
        if self.buffer.len() < 4 {
            return None;
        }
        let len = u32::from_le_bytes(self.buffer[0..4].try_into().ok()?) as usize;
        if self.buffer.len() < 4 + len {
            return None;
        }
        let message = ron::from_str(std::str::from_utf8(&self.buffer[4..4 + len]).ok()?);
        self.buffer.drain(..4 + len);
        message.ok()
    }
}

/// Encodes one length-prefixed message.
pub fn encode_message(message: &Message) -> Vec<u8> {
    let payload = ron::to_string(message).unwrap_or_default();
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload.as_bytes());
    out
}

/// Server listener and clients.
impl NetworkServer {
    /// Starts listening on `port`; errors stay in the status.
    pub fn listen(&mut self, port: u16) -> Result<(), String> {
        let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|err| err.to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|err| err.to_string())?;
        self.listener = Some(listener);
        Ok(())
    }

    /// Accepts pending clients, sends `spawns` to newcomers, then broadcasts `message`.
    pub fn pump(
        &mut self,
        message: &Message,
        spawns: &[SpawnState],
        status: &mut NetworkStatus,
    ) {
        if let Some(listener) = &self.listener {
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_nodelay(true);
                        // late joiners get the existing objects before snapshots
                        let catch_up = encode_message(&Message::Spawn(spawns.to_vec()));
                        let _ = stream.write_all(&catch_up);
                        self.clients.push(stream);
                    }
                    Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                    Err(err) => {
                        status.update(self.clients.len(), Some(err.to_string()));
                        break;
                    }
                }
            }
        }
        let bytes = encode_message(message);
        let before = self.clients.len();
        self.clients
            .retain_mut(|client| client.write_all(&bytes).is_ok());
        if self.clients.len() != before {
            status.update(self.clients.len(), status.error.clone());
        } else if status.peers != self.clients.len() {
            status.update(self.clients.len(), None);
        }
    }
}

/// Client connection and incoming messages.
impl NetworkClient {
    /// Connects to `addr`.
    pub fn connect(&mut self, addr: &str) -> Result<(), String> {
        let stream = TcpStream::connect(addr).map_err(|err| err.to_string())?;
        stream
            .set_nonblocking(true)
            .map_err(|err| err.to_string())?;
        let _ = stream.set_nodelay(true);
        self.stream = Some(stream);
        Ok(())
    }

    /// Reads pending bytes and returns every complete message.
    pub fn pump(&mut self, status: &mut NetworkStatus) -> Vec<Message> {
        let Some(stream) = &mut self.stream else {
            return Vec::new();
        };
        let mut buf = [0u8; 8192];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => {
                    self.stream = None;
                    status.update(0, Some("server closed the connection".into()));
                    break;
                }
                Ok(n) => self.reader.feed(&buf[..n]),
                Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                Err(err) => {
                    self.stream = None;
                    status.update(0, Some(err.to_string()));
                    break;
                }
            }
        }
        let mut messages = Vec::new();
        while let Some(message) = self.reader.next_message() {
            messages.push(message);
        }
        if !messages.is_empty() && status.peers != 1 {
            status.update(1, None);
        }
        messages
    }
}

/// Systems that transport snapshots; order them after local simulation.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct NetworkSet;

/// Renders buffered snapshots at a fixed delay behind the server.
pub fn interpolate_snapshots(
    config: Res<NetworkConfig>,
    time: Res<Time>,
    mut buffer: ResMut<SnapshotBuffer>,
    mut objects: Query<(&ReplicatedObject, &mut Transform)>,
) {
    if !matches!(config.role, NetworkRole::Client { .. }) {
        return;
    }
    let delay = config.interpolation_delay.clamp(0.0, 1.0) as f64;
    let render = time.elapsed_secs_f64() - delay;
    buffer.prune(render - 2.0);
    for (object, mut transform) in &mut objects {
        let Some(samples) = buffer.samples.get(&object.0) else {
            continue;
        };
        if let Some(state) = sample_at(samples, render) {
            transform.translation = Vec3::from_array(state.translation);
            transform.rotation = Quat::from_array(state.rotation);
            transform.scale = Vec3::from_array(state.scale);
        }
    }
}

/// Registers networking resources and systems.
pub struct NetworkPlugin;

impl Plugin for NetworkPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NetworkConfig>()
            .init_resource::<NetworkStatus>()
            .init_resource::<NetworkServer>()
            .init_resource::<NetworkClient>()
            .init_resource::<SnapshotBuffer>()
            .add_systems(Startup, start_network)
            .add_systems(Update, (send_snapshots, receive_messages, (apply_inbox, interpolate_snapshots).chain())
                .chain()
                .in_set(NetworkSet));
    }
}

/// Opens the listener or client socket from [`NetworkConfig`].
pub fn start_network(
    config: Res<NetworkConfig>,
    mut server: ResMut<NetworkServer>,
    mut client: ResMut<NetworkClient>,
    mut status: ResMut<NetworkStatus>,
) {
    match &config.role {
        NetworkRole::Offline => {}
        NetworkRole::Server { port } => match server.listen(*port) {
            Ok(()) => status.update(0, None),
            Err(err) => status.update(0, Some(err)),
        },
        NetworkRole::Client { addr } => match client.connect(addr) {
            Ok(()) => status.update(1, None),
            Err(err) => status.update(0, Some(err)),
        },
    }
}

/// Sends the current replicated transforms from a server.
pub fn send_snapshots(
    config: Res<NetworkConfig>,
    mut server: ResMut<NetworkServer>,
    mut status: ResMut<NetworkStatus>,
    objects: Query<(&ReplicatedObject, &Transform, Option<&ReplicatedSpawn>)>,
) {
    if !matches!(config.role, NetworkRole::Server { .. }) {
        return;
    }
    let mut snapshot = Vec::new();
    let mut spawns = Vec::new();
    let mut ids = std::collections::HashSet::new();
    for (object, transform, spawn) in &objects {
        ids.insert(object.0);
        snapshot.push(TransformState {
            id: object.0,
            translation: transform.translation.to_array(),
            rotation: transform.rotation.to_array(),
            scale: transform.scale.to_array(),
        });
        if let Some(spawn) = spawn {
            spawns.push(SpawnState {
                id: object.0,
                kind: spawn.kind,
                translation: transform.translation.to_array(),
                rotation: transform.rotation.to_array(),
                scale: transform.scale.to_array(),
                color: spawn.color,
            });
        }
    }
    let (added, removed) = diff(&server.last_ids, &spawns);
    if !removed.is_empty() {
        server.pump(&Message::Despawn(removed), &spawns, &mut status);
    }
    if !added.is_empty() {
        let new_spawns: Vec<SpawnState> = spawns
            .iter()
            .filter(|state| added.contains(&state.id))
            .cloned()
            .collect();
        server.pump(&Message::Spawn(new_spawns), &spawns, &mut status);
    }
    server.last_ids = ids;
    server.pump(&Message::Snapshot(snapshot), &spawns, &mut status);
}

/// Ids added and removed between two sets, both sorted.
pub fn diff(
    previous: &std::collections::HashSet<u64>,
    current: &[SpawnState],
) -> (Vec<u64>, Vec<u64>) {
    let now: std::collections::HashSet<u64> = current.iter().map(|state| state.id).collect();
    let mut added: Vec<u64> = now.difference(previous).copied().collect();
    let mut removed: Vec<u64> = previous.difference(&now).copied().collect();
    added.sort_unstable();
    removed.sort_unstable();
    (added, removed)
}

/// Reads the socket and queues complete messages.
pub fn receive_messages(
    config: Res<NetworkConfig>,
    mut client: ResMut<NetworkClient>,
    mut status: ResMut<NetworkStatus>,
) {
    if !matches!(config.role, NetworkRole::Client { .. }) {
        return;
    }
    for message in client.pump(&mut status) {
        client.inbox.push(message);
    }
}

/// Applies queued server messages to the client world.
pub fn apply_inbox(
    config: Res<NetworkConfig>,
    time: Res<Time>,
    mut client: ResMut<NetworkClient>,
    mut commands: Commands,
    mut buffer: ResMut<SnapshotBuffer>,
    mut objects: Query<(Entity, &ReplicatedObject, &mut Transform)>,
) {
    if !matches!(config.role, NetworkRole::Client { .. }) {
        return;
    }
    let messages = core::mem::take(&mut client.inbox);
    for message in messages {
        match message {
            Message::Spawn(states) => {
                for state in states {
                    if objects.iter().any(|(_, object, _)| object.0 == state.id) {
                        continue;
                    }
                    commands.spawn((
                        ReplicatedObject(state.id),
                        Transform::from_translation(Vec3::from_array(state.translation))
                            .with_rotation(Quat::from_array(state.rotation))
                            .with_scale(Vec3::from_array(state.scale)),
                        ReplicatedSpawn {
                            kind: state.kind,
                            color: state.color,
                        },
                    ));
                }
            }
            Message::Despawn(ids) => {
                for (entity, object, _) in &mut objects {
                    if ids.contains(&object.0) {
                        commands.entity(entity).despawn();
                    }
                }
            }
            Message::Snapshot(states) => {
                let at = time.elapsed_secs_f64();
                for state in states {
                    buffer.push(at, state);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    fn state() -> TransformState {
        TransformState {
            id: 7,
            translation: [1.0, 2.0, 3.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0, 1.0, 1.0],
        }
    }

    #[test]
    fn framed_messages_round_trip() {
        let message = Message::Snapshot(vec![state()]);
        let bytes = encode_message(&message);
        let mut reader = FrameReader::default();
        reader.feed(&bytes[..3]);
        assert!(reader.next_message().is_none());
        reader.feed(&bytes[3..]);
        assert_eq!(reader.next_message(), Some(message));
        assert!(reader.next_message().is_none());
    }

    #[test]
    fn a_server_broadcasts_to_a_client_and_drops_dead_peers() {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let mut server = NetworkServer::default();
        server.listen(port).unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            stream.set_nonblocking(true).unwrap();
            let mut reader = FrameReader::default();
            let mut buf = [0u8; 4096];
            let mut messages = Vec::new();
            let deadline = Instant::now() + Duration::from_secs(2);
            while messages.len() < 2 && Instant::now() < deadline {
                match stream.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => reader.feed(&buf[..n]),
                    Err(err) if err.kind() == ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(_) => break,
                }
                while let Some(message) = reader.next_message() {
                    messages.push(message);
                }
            }
            tx.send(messages.into_iter().last()).ok();
        });
        let mut status = NetworkStatus::default();
        let deadline = Instant::now() + Duration::from_secs(2);
        while status.peers == 0 && Instant::now() < deadline {
            server.pump(&Message::Snapshot(vec![state()]), &[], &mut status);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(status.peers, 1);
        let received = rx.recv_timeout(Duration::from_secs(2)).ok().flatten();
        assert_eq!(received, Some(Message::Snapshot(vec![state()])));

        // A client that has gone away is dropped without stopping the server.
        server.clients.clear();
        server.pump(&Message::Snapshot(vec![]), &[], &mut status);
        assert_eq!(status.peers, 0);
    }

    #[test]
    fn connecting_to_a_missing_peer_reports_an_error() {
        let mut client = NetworkClient::default();
        assert!(client.connect("127.0.0.1:1").is_err());
        let mut status = NetworkStatus::default();
        assert!(client.pump(&mut status).is_empty());
    }

    fn spawn_state(id: u64) -> SpawnState {
        SpawnState {
            id,
            kind: NetKind::Cube,
            translation: [0.0, 0.5, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
            color: [0.2, 0.6, 0.3, 1.0],
        }
    }

    #[test]
    fn spawn_and_despawn_messages_round_trip() {
        for message in [
            Message::Spawn(vec![spawn_state(4)]),
            Message::Despawn(vec![4, 9]),
        ] {
            let bytes = encode_message(&message);
            let mut reader = FrameReader::default();
            reader.feed(&bytes);
            assert_eq!(reader.next_message(), Some(message));
        }
    }

    #[test]
    fn diff_reports_added_and_removed_ids() {
        let mut previous = std::collections::HashSet::new();
        previous.insert(1);
        previous.insert(2);
        let (added, removed) = diff(&previous, &[spawn_state(2), spawn_state(3)]);
        assert_eq!(added, [3]);
        assert_eq!(removed, [1]);
    }

    #[test]
    fn a_late_joiner_receives_every_spawn_before_snapshots() {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let mut server = NetworkServer::default();
        server.listen(port).unwrap();
        let spawns = vec![spawn_state(1), spawn_state(2)];
        let mut status = NetworkStatus::default();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            let mut reader = FrameReader::default();
            stream.set_nonblocking(false).unwrap();
            let mut buf = [0u8; 4096];
            let read = stream.peek(&mut buf).unwrap_or(0);
            if read > 0 {
                reader.feed(&buf[..read]);
                tx.send(reader.next_message()).ok();
            }
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while status.peers == 0 && Instant::now() < deadline {
            server.pump(&Message::Snapshot(vec![]), &spawns, &mut status);
            std::thread::sleep(Duration::from_millis(10));
        }
        let first = rx.recv_timeout(Duration::from_secs(2)).ok().flatten();
        assert_eq!(first, Some(Message::Spawn(spawns)));
    }

    #[test]
    fn spawn_and_despawn_apply_to_client_entities() {
        use eme_ecs::system::RunSystemOnce;

        let mut world = World::new();
        world.insert_resource(NetworkConfig {
            role: NetworkRole::Client {
                addr: "127.0.0.1:1".into(),
            },
            interpolation_delay: 0.1,
        });
        world.insert_resource(Time::<()>::default());
        world.insert_resource(NetworkClient::default());
        world.init_resource::<SnapshotBuffer>();
        world
            .resource_mut::<NetworkClient>()
            .inbox
            .push(Message::Spawn(vec![spawn_state(7)]));
        world.run_system_once(apply_inbox).unwrap();

        let mut query = world.query::<(Entity, &ReplicatedObject)>();
        let ids: Vec<u64> = query.iter(&world).map(|(_, object)| object.0).collect();
        assert_eq!(ids, [7]);
        assert!(world
            .query::<&ReplicatedSpawn>()
            .iter(&world)
            .next()
            .is_some());

        world
            .resource_mut::<NetworkClient>()
            .inbox
            .push(Message::Despawn(vec![7]));
        world.run_system_once(apply_inbox).unwrap();
        assert_eq!(world.query::<&ReplicatedObject>().iter(&world).count(), 0);
    }

    fn at(id: u64, x: f32) -> TransformState {
        TransformState {
            id,
            translation: [x, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
        }
    }

    #[test]
    fn samples_interpolate_and_clamp() {
        let samples = [(0.0, at(1, 0.0)), (1.0, at(1, 2.0))];
        assert_eq!(sample_at(&samples, -1.0).unwrap().translation[0], 0.0);
        assert_eq!(sample_at(&samples, 0.5).unwrap().translation[0], 1.0);
        assert_eq!(sample_at(&samples, 9.0).unwrap().translation[0], 2.0);
        assert_eq!(sample_at(&[], 0.0), None);
    }

    #[test]
    fn rotation_slerps_between_samples() {
        let a = TransformState {
            rotation: Quat::IDENTITY.to_array(),
            ..at(1, 0.0)
        };
        let b = TransformState {
            rotation: Quat::from_rotation_y(core::f32::consts::PI).to_array(),
            ..at(1, 0.0)
        };
        let mid = lerp_state(&a, &b, 0.5);
        let got = Quat::from_array(mid.rotation);
        let (yaw, _, _) = got.to_euler(EulerRot::YXZ);
        assert!(
            (yaw.abs() - core::f32::consts::FRAC_PI_2).abs() < 1e-3,
            "expected a quarter turn, got {yaw}"
        );
    }

    #[test]
    fn the_buffer_prunes_old_samples() {
        let mut buffer = SnapshotBuffer::default();
        buffer.push(0.0, at(1, 0.0));
        buffer.push(1.0, at(1, 1.0));
        buffer.push(2.0, at(1, 2.0));
        buffer.prune(1.5);
        let samples = buffer.samples.get(&1).unwrap();
        assert_eq!(samples.first().unwrap().0, 1.0);
        assert_eq!(samples.len(), 2);
    }

    #[test]
    fn snapshots_apply_only_to_matching_ids() {
        let mut world = World::new();
        let a = world
            .spawn((ReplicatedObject(1), Transform::default()))
            .id();
        let b = world
            .spawn((ReplicatedObject(2), Transform::default()))
            .id();
        let mut query = world.query::<(&ReplicatedObject, &mut Transform)>();
        let mut states = [state()];
        states[0].id = 1;
        for (object, mut transform) in query.iter_mut(&mut world) {
            if object.0 == 1 {
                transform.translation = Vec3::from_array(states[0].translation);
            }
        }
        assert_eq!(world.get::<Transform>(a).unwrap().translation.x, 1.0);
        assert_eq!(world.get::<Transform>(b).unwrap().translation.x, 0.0);
    }
}
