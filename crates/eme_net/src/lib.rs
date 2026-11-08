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
#[derive(Resource, Clone, Debug, Default)]
pub struct NetworkConfig {
    /// Which side to run.
    pub role: NetworkRole,
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
}

/// Client transport state.
#[derive(Resource, Default)]
pub struct NetworkClient {
    stream: Option<TcpStream>,
    reader: FrameReader,
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

    /// Accepts any pending clients and broadcasts `message`.
    pub fn pump(&mut self, message: &Message, status: &mut NetworkStatus) {
        if let Some(listener) = &self.listener {
            loop {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nodelay(true);
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

/// Registers networking resources and systems.
pub struct NetworkPlugin;

impl Plugin for NetworkPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NetworkConfig>()
            .init_resource::<NetworkStatus>()
            .init_resource::<NetworkServer>()
            .init_resource::<NetworkClient>()
            .add_systems(Startup, start_network)
            .add_systems(Update, (send_snapshots, receive_snapshots).in_set(NetworkSet));
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
    objects: Query<(&ReplicatedObject, &Transform)>,
) {
    if !matches!(config.role, NetworkRole::Server { .. }) {
        return;
    }
    let snapshot = objects
        .iter()
        .map(|(object, transform)| TransformState {
            id: object.0,
            translation: transform.translation.to_array(),
            rotation: transform.rotation.to_array(),
            scale: transform.scale.to_array(),
        })
        .collect();
    server.pump(&Message::Snapshot(snapshot), &mut status);
}

/// Applies snapshots received by a client.
pub fn receive_snapshots(
    config: Res<NetworkConfig>,
    mut client: ResMut<NetworkClient>,
    mut status: ResMut<NetworkStatus>,
    mut objects: Query<(&ReplicatedObject, &mut Transform)>,
) {
    if !matches!(config.role, NetworkRole::Client { .. }) {
        return;
    }
    for message in client.pump(&mut status) {
        let Message::Snapshot(states) = message;
        for state in states {
            for (object, mut transform) in &mut objects {
                if object.0 != state.id {
                    continue;
                }
                transform.translation = Vec3::from_array(state.translation);
                transform.rotation = Quat::from_array(state.rotation);
                transform.scale = Vec3::from_array(state.scale);
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
        let mut status = NetworkStatus::default();
        let deadline = Instant::now() + Duration::from_secs(2);
        while status.peers == 0 && Instant::now() < deadline {
            server.pump(&Message::Snapshot(vec![state()]), &mut status);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(status.peers, 1);
        let received = rx.recv_timeout(Duration::from_secs(2)).ok().flatten();
        assert_eq!(received, Some(Message::Snapshot(vec![state()])));

        // A client that has gone away is dropped without stopping the server.
        server.clients.clear();
        server.pump(&Message::Snapshot(vec![]), &mut status);
        assert_eq!(status.peers, 0);
    }

    #[test]
    fn connecting_to_a_missing_peer_reports_an_error() {
        let mut client = NetworkClient::default();
        assert!(client.connect("127.0.0.1:1").is_err());
        let mut status = NetworkStatus::default();
        assert!(client.pump(&mut status).is_empty());
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
