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
use eme_input::prelude::*;
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
    /// Shared session token; a server requires it and a client presents it.
    pub auth_token: Option<String>,
    /// Object a client asks to own on connect.
    pub claim: Option<u64>,
    /// Walk speed the server announces for client-side prediction.
    pub walk_speed: f32,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            role: NetworkRole::Offline,
            interpolation_delay: 0.1,
            auth_token: None,
            claim: None,
            walk_speed: 4.0,
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
    /// Client greeting; carries the shared token, empty when none is configured.
    Hello(String),
    /// Server reply: the token was accepted; carries the client id and walk speed.
    Welcome {
        /// Id assigned to this client, used in ownership messages.
        client: u64,
        /// Walk speed the server simulates controllers with.
        walk_speed: f32,
    },
    /// Server reply: the handshake failed; the payload is the reason.
    Rejected(String),
    /// Client asks to own an object.
    Claim(u64),
    /// Ownership granted; broadcast to every client.
    Owner {
        /// Object id.
        id: u64,
        /// Client that now owns it.
        owner: u64,
    },
    /// A claim was refused.
    Denied(u64),
    /// Client input for the object it owns.
    Input {
        /// Monotonic sequence number.
        seq: u32,
        /// Walk direction on XZ, already normalised by the client.
        walk: [f32; 2],
        /// Whether a jump was requested.
        jump: bool,
    },
    /// Server acknowledges that input `seq` was applied for `client`.
    Ack {
        /// Client the acknowledgement belongs to.
        client: u64,
        /// Last applied input sequence.
        seq: u32,
    },
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
    clients: Vec<ClientConn>,
    pending: Vec<PendingClient>,
    last_ids: std::collections::HashSet<u64>,
    next_client: u64,
    owners: std::collections::HashMap<u64, u64>,
    claims: std::collections::HashMap<u64, u64>,
    inputs: std::collections::HashMap<u64, InputState>,
}

/// One authenticated client connection.
struct ClientConn {
    id: u64,
    stream: TcpStream,
    reader: FrameReader,
}

/// Latest input received from a client.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InputState {
    /// Input sequence number.
    pub seq: u32,
    /// Walk direction on XZ.
    pub walk: [f32; 2],
    /// Jump requested.
    pub jump: bool,
}

/// A connection waiting for its [`Message::Hello`].
struct PendingClient {
    id: u64,
    stream: TcpStream,
    reader: FrameReader,
}

/// Unauthenticated connections waiting for a handshake.
const MAX_PENDING: usize = 32;

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
    /// Whether the server accepted the token and replication may be applied.
    pub authenticated: bool,
    /// Id the server assigned to this client.
    pub client_id: u64,
    /// Object-to-owner map announced by the server.
    pub owners: std::collections::HashMap<u64, u64>,
    /// Objects this client owns.
    pub owned: std::collections::HashSet<u64>,
    /// Object requested on connect, sent once the welcome arrives.
    pub pending_claim: Option<u64>,
    /// Last input sequence the server acknowledged.
    pub last_ack: u32,
    /// Walk speed the server reported.
    pub walk_speed: f32,
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

    /// Accepts pending clients, completes their handshake, handles their
    /// claims and inputs, then broadcasts `message`.
    ///
    /// A client only joins after sending [`Message::Hello`] with the token the
    /// server expects; every newcomer first receives [`Message::Welcome`] and a
    /// spawn catch-up, then the regular snapshots.
    pub fn pump(
        &mut self,
        message: &Message,
        spawns: &[SpawnState],
        token: Option<&str>,
        walk_speed: f32,
        status: &mut NetworkStatus,
    ) {
        if let Some(listener) = &self.listener {
            loop {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nodelay(true);
                        let _ = stream.set_nonblocking(true);
                        if self.pending.len() < MAX_PENDING {
                            let id = self.next_client;
                            self.next_client = self.next_client.wrapping_add(1);
                            self.pending.push(PendingClient {
                                id,
                                stream,
                                reader: FrameReader::default(),
                            });
                        }
                    }
                    Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                    Err(err) => {
                        status.update(self.clients.len(), Some(err.to_string()));
                        break;
                    }
                }
            }
        }

        let mut index = 0;
        while index < self.pending.len() {
            let mut accepted = false;
            let mut drop_client = false;
            {
                let pending = &mut self.pending[index];
                let mut buf = [0u8; 4096];
                loop {
                    match pending.stream.read(&mut buf) {
                        Ok(0) => {
                            drop_client = true;
                            break;
                        }
                        Ok(n) => pending.reader.feed(&buf[..n]),
                        Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                        Err(_) => {
                            drop_client = true;
                            break;
                        }
                    }
                }
                if !drop_client
                    && let Some(reply) = pending.reader.next_message()
                {
                    match reply {
                        Message::Hello(got) => {
                            let allowed =
                                token.is_none_or(|expected| expected == got.as_str());
                            if allowed {
                                let welcome = Message::Welcome {
                                    client: pending.id,
                                    walk_speed,
                                };
                                let _ = pending.stream.write_all(&encode_message(&welcome));
                                let _ = pending
                                    .stream
                                    .write_all(&encode_message(&Message::Spawn(spawns.to_vec())));
                                accepted = true;
                            } else {
                                let _ = pending.stream.write_all(&encode_message(
                                    &Message::Rejected("invalid token".into()),
                                ));
                                drop_client = true;
                            }
                        }
                        _ => {
                            let _ = pending.stream.write_all(&encode_message(
                                &Message::Rejected("expected hello".into()),
                            ));
                            drop_client = true;
                        }
                    }
                }
            }
            if accepted {
                let pending = self.pending.remove(index);
                self.clients.push(ClientConn {
                    id: pending.id,
                    stream: pending.stream,
                    reader: pending.reader,
                });
            } else if drop_client {
                self.pending.remove(index);
            } else {
                index += 1;
            }
        }

        let mut index = 0;
        while index < self.clients.len() {
            let mut closed = false;
            let mut inbound = Vec::new();
            {
                let client = &mut self.clients[index];
                let mut buf = [0u8; 4096];
                loop {
                    match client.stream.read(&mut buf) {
                        Ok(0) => {
                            closed = true;
                            break;
                        }
                        Ok(n) => client.reader.feed(&buf[..n]),
                        Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                        Err(_) => {
                            closed = true;
                            break;
                        }
                    }
                }
                if !closed {
                    while let Some(message) = client.reader.next_message() {
                        inbound.push(message);
                    }
                }
            }
            if closed {
                let client = self.clients.remove(index);
                self.owners.retain(|_, owner| *owner != client.id);
                self.claims.remove(&client.id);
                self.inputs.remove(&client.id);
                continue;
            }
            let id = self.clients[index].id;
            let mut outbound = Vec::new();
            for message in inbound {
                match message {
                    Message::Claim(object) => {
                        if self.claims.get(&id).is_some_and(|owned| *owned != object) {
                            outbound.push(Message::Denied(object));
                            continue;
                        }
                        match self.owners.get(&object).copied() {
                            None => {
                                self.owners.insert(object, id);
                                self.claims.insert(id, object);
                                self.inputs.remove(&id);
                                let notice = Message::Owner { id: object, owner: id };
                                for client in &mut self.clients {
                                    let _ = client.stream.write_all(&encode_message(&notice));
                                }
                            }
                            Some(owner) if owner == id => {}
                            Some(_) => outbound.push(Message::Denied(object)),
                        }
                    }
                    Message::Input { seq, walk, jump } if self.claims.contains_key(&id) => {
                        self.inputs.insert(
                            id,
                            InputState {
                                seq,
                                walk,
                                jump,
                            },
                        );
                    }
                    _ => {}
                }
            }
            for message in &outbound {
                let _ = self.clients[index]
                    .stream
                    .write_all(&encode_message(message));
            }
            index += 1;
        }

        let bytes = encode_message(message);
        let mut dead = Vec::new();
        self.clients.retain_mut(|client| {
            if client.stream.write_all(&bytes).is_ok() {
                true
            } else {
                dead.push(client.id);
                false
            }
        });
        for id in dead {
            self.owners.retain(|_, owner| *owner != id);
            self.claims.remove(&id);
            self.inputs.remove(&id);
        }
        if status.peers != self.clients.len() {
            status.update(self.clients.len(), status.error.clone());
        }
    }
}

/// Client connection and incoming messages.
impl NetworkClient {
    /// Connects to `addr`, presents `token` and asks for `claim` once accepted.
    pub fn connect(
        &mut self,
        addr: &str,
        token: Option<&str>,
        claim: Option<u64>,
    ) -> Result<(), String> {
        let mut stream = TcpStream::connect(addr).map_err(|err| err.to_string())?;
        stream
            .set_nonblocking(true)
            .map_err(|err| err.to_string())?;
        let _ = stream.set_nodelay(true);
        let hello = encode_message(&Message::Hello(token.unwrap_or_default().to_owned()));
        stream
            .write_all(&hello)
            .map_err(|err| format!("handshake: {err}"))?;
        self.stream = Some(stream);
        self.authenticated = false;
        self.client_id = 0;
        self.owners.clear();
        self.owned.clear();
        self.pending_claim = claim;
        self.last_ack = 0;
        self.walk_speed = 4.0;
        Ok(())
    }

    /// Writes one message to the server.
    pub fn send(&mut self, message: &Message) -> bool {
        let Some(stream) = &mut self.stream else {
            return false;
        };
        stream.write_all(&encode_message(message)).is_ok()
    }

    /// Reads pending bytes and handles the session messages it understands.
    pub fn pump(&mut self, status: &mut NetworkStatus) -> Vec<Message> {
        if self.stream.is_none() {
            return Vec::new();
        }
        let mut closed = false;
        let mut read_error = None;
        {
            let stream = self.stream.as_mut().expect("checked above");
            let mut buf = [0u8; 8192];
            loop {
                match stream.read(&mut buf) {
                    Ok(0) => {
                        closed = true;
                        break;
                    }
                    Ok(n) => self.reader.feed(&buf[..n]),
                    Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                    Err(err) => {
                        read_error = Some(err.to_string());
                        break;
                    }
                }
            }
        }
        let mut messages = Vec::new();
        let mut rejected = false;
        while let Some(message) = self.reader.next_message() {
            match message {
                Message::Welcome { client, walk_speed } => {
                    self.client_id = client;
                    self.walk_speed = walk_speed;
                    self.authenticated = true;
                    status.update(1, None);
                    if let Some(object) = self.pending_claim.take() {
                        self.send(&Message::Claim(object));
                    }
                }
                Message::Owner { id, owner } => {
                    self.owners.insert(id, owner);
                    if owner == self.client_id {
                        self.owned.insert(id);
                    } else {
                        self.owned.remove(&id);
                    }
                }
                Message::Denied(id) => {
                    self.pending_claim = None;
                    let peers = status.peers;
                    status.update(peers, Some(format!("claim denied for object {id}")));
                }
                Message::Ack { client, seq } => {
                    if client == self.client_id {
                        self.last_ack = self.last_ack.max(seq);
                    }
                }
                Message::Rejected(reason) => {
                    rejected = true;
                    self.stream = None;
                    self.authenticated = false;
                    status.update(0, Some(format!("rejected: {reason}")));
                }
                other => messages.push(other),
            }
        }
        // The server may reject and close in the same read; report the peer
        // message rather than "closed" when one arrived first.
        if !rejected && (closed || read_error.is_some()) {
            self.stream = None;
            self.authenticated = false;
            let reason = read_error.unwrap_or_else(|| "server closed the connection".into());
            status.update(0, Some(reason));
        }
        messages
    }
}

impl NetworkServer {
    /// Drains the latest input per client with the object that client owns.
    pub fn take_inputs(&mut self) -> Vec<(u64, u64, InputState)> {
        let mut out = Vec::new();
        for (client, input) in self.inputs.drain() {
            if let Some(object) = self.claims.get(&client).copied() {
                out.push((client, object, input));
            }
        }
        out
    }

    /// Sends one message to a client, when it is still connected.
    pub fn send_to(&mut self, client: u64, message: &Message) {
        if let Some(conn) = self.clients.iter_mut().find(|conn| conn.id == client) {
            let _ = conn.stream.write_all(&encode_message(message));
        }
    }
}

/// One input the client sent and may need to replay.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PendingInput {
    /// Sequence number sent to the server.
    pub seq: u32,
    /// Walk direction on XZ.
    pub walk: [f32; 2],
    /// Frame time used when the input was predicted.
    pub dt: f32,
}

/// Client-side prediction state for the owned object.
#[derive(Resource, Default, Debug)]
pub struct Prediction {
    /// Object this client owns and predicts.
    pub object: Option<u64>,
    /// Predicted world position.
    pub position: Vec3,
    /// Last input sequence sent.
    pub seq: u32,
    /// Last acknowledged sequence.
    pub last_ack: u32,
    /// Inputs the server has not acknowledged yet.
    pub history: Vec<PendingInput>,
    /// How many times a correction was applied.
    pub corrections: u32,
    /// Whether [`position`](Self::position) holds a real value.
    pub initialized: bool,
}

impl Prediction {
    /// Applies one walk input to `position`.
    pub fn step(position: Vec3, walk: [f32; 2], dt: f32, speed: f32) -> Vec3 {
        Vec3::new(
            position.x + walk[0] * speed * dt,
            position.y,
            position.z + walk[1] * speed * dt,
        )
    }

    /// Drops inputs the server acknowledged.
    pub fn prune(&mut self, ack: u32) {
        self.history.retain(|input| input.seq > ack);
        self.last_ack = ack;
    }

    /// Replays every unacknowledged input starting from `authoritative`.
    pub fn replay(&self, authoritative: Vec3, speed: f32) -> Vec3 {
        self.history.iter().fold(authoritative, |position, input| {
            Self::step(position, input.walk, input.dt, speed)
        })
    }
}

/// Sends local movement for the owned object and predicts its position.
pub fn predict_owned(
    config: Res<NetworkConfig>,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut client: ResMut<NetworkClient>,
    mut prediction: ResMut<Prediction>,
    mut objects: Query<(&ReplicatedObject, &mut Transform)>,
) {
    if !matches!(config.role, NetworkRole::Client { .. }) || !client.authenticated {
        return;
    }
    let object = prediction
        .object
        .or_else(|| client.owned.iter().next().copied());
    let Some(object) = object else {
        return;
    };
    prediction.object = Some(object);
    let Some((_, mut transform)) = objects
        .iter_mut()
        .find(|(replicated, _)| replicated.0 == object)
    else {
        return;
    };
    if !prediction.initialized {
        prediction.position = transform.translation;
        prediction.initialized = true;
        prediction.last_ack = client.last_ack;
    }
    prediction.prune(client.last_ack);

    let mut walk = [0.0f32; 2];
    if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
        walk[1] -= 1.0;
    }
    if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
        walk[1] += 1.0;
    }
    if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
        walk[0] -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
        walk[0] += 1.0;
    }
    let length = (walk[0] * walk[0] + walk[1] * walk[1]).sqrt();
    if length > 0.0 {
        walk[0] /= length;
        walk[1] /= length;
    }
    let dt = time.delta_secs().clamp(0.0, 0.05);
    if dt <= 0.0 {
        return;
    }
    let jump = keys.just_pressed(KeyCode::Space);
    let speed = client.walk_speed.max(0.1);
    prediction.seq = prediction.seq.wrapping_add(1);
    let seq = prediction.seq;
    prediction.history.push(PendingInput { seq, walk, dt });
    if prediction.history.len() > 256 {
        prediction.history.remove(0);
    }
    prediction.position = Prediction::step(prediction.position, walk, dt, speed);
    transform.translation.x = prediction.position.x;
    transform.translation.z = prediction.position.z;
    client.send(&Message::Input {
        seq: prediction.seq,
        walk,
        jump,
    });
}

/// Compares the predicted position with the authoritative snapshot and replays
/// the inputs the server has not acknowledged yet.
pub fn reconcile_owned(
    config: Res<NetworkConfig>,
    client: Res<NetworkClient>,
    mut prediction: ResMut<Prediction>,
    buffer: Res<SnapshotBuffer>,
    mut objects: Query<(&ReplicatedObject, &mut Transform)>,
) {
    if !matches!(config.role, NetworkRole::Client { .. }) {
        return;
    }
    let Some(object) = prediction.object else {
        return;
    };
    let Some(authoritative) = buffer
        .samples
        .get(&object)
        .and_then(|samples| samples.last())
    else {
        return;
    };
    let auth = Vec3::from_array(authoritative.1.translation);
    prediction.prune(client.last_ack);
    let speed = client.walk_speed.max(0.1);
    let deviation = Vec3::new(
        prediction.position.x - auth.x,
        0.0,
        prediction.position.z - auth.z,
    )
    .length();
    if deviation > 0.25 {
        prediction.corrections = prediction.corrections.wrapping_add(1);
        prediction.position = prediction.replay(auth, speed);
        if let Some((_, mut transform)) = objects
            .iter_mut()
            .find(|(replicated, _)| replicated.0 == object)
        {
            transform.translation.x = prediction.position.x;
            transform.translation.z = prediction.position.z;
            transform.translation.y = auth.y;
        }
    } else if let Some((_, mut transform)) = objects
        .iter_mut()
        .find(|(replicated, _)| replicated.0 == object)
    {
        transform.translation.y = auth.y;
    }
    prediction.position.y = auth.y;
}

/// Systems that transport snapshots; order them after local simulation.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct NetworkSet;

/// Renders buffered snapshots at a fixed delay behind the server.
pub fn interpolate_snapshots(
    config: Res<NetworkConfig>,
    time: Res<Time>,
    client: Res<NetworkClient>,
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
        if client.owned.contains(&object.0) {
            continue;
        }
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
            .init_resource::<Prediction>()
            .add_systems(Startup, start_network)
            .add_systems(
                Update,
                (
                    send_snapshots,
                    receive_messages,
                    (apply_inbox, interpolate_snapshots).chain(),
                    predict_owned.after(receive_messages),
                    reconcile_owned.after(apply_inbox),
                )
                    .in_set(NetworkSet),
            );
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
        NetworkRole::Client { addr } => match client.connect(addr, config.auth_token.as_deref(), config.claim) {
            Ok(()) => status.update(0, None),
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
        server.pump(
            &Message::Despawn(removed),
            &spawns,
            config.auth_token.as_deref(),
            config.walk_speed,
            &mut status,
        );
    }
    if !added.is_empty() {
        let new_spawns: Vec<SpawnState> = spawns
            .iter()
            .filter(|state| added.contains(&state.id))
            .cloned()
            .collect();
        server.pump(
            &Message::Spawn(new_spawns),
            &spawns,
            config.auth_token.as_deref(),
            config.walk_speed,
            &mut status,
        );
    }
    server.last_ids = ids;
    server.pump(
        &Message::Snapshot(snapshot),
        &spawns,
        config.auth_token.as_deref(),
        config.walk_speed,
        &mut status,
    );
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
            Message::Hello(_)
            | Message::Welcome { .. }
            | Message::Rejected(_)
            | Message::Claim(_)
            | Message::Owner { .. }
            | Message::Denied(_)
            | Message::Input { .. }
            | Message::Ack { .. } => {}
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
            let _ = stream.write_all(&encode_message(&Message::Hello(String::new())));
            let mut reader = FrameReader::default();
            let mut buf = [0u8; 4096];
            let mut messages = Vec::new();
            let deadline = Instant::now() + Duration::from_secs(2);
            while !messages
                .iter()
                .any(|message| matches!(message, Message::Snapshot(_)))
                && Instant::now() < deadline
            {
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
            server.pump(
                &Message::Snapshot(vec![state()]),
                &[],
                None,
                4.0,
                &mut status,
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(status.peers, 1);
        let received = rx.recv_timeout(Duration::from_secs(2)).ok().flatten();
        assert_eq!(received, Some(Message::Snapshot(vec![state()])));

        // A client that has gone away is dropped without stopping the server.
        server.clients.clear();
        server.pump(&Message::Snapshot(vec![]), &[], None, 4.0, &mut status);
        assert_eq!(status.peers, 0);
    }

    #[test]
    fn connecting_to_a_missing_peer_reports_an_error() {
        let mut client = NetworkClient::default();
        assert!(client.connect("127.0.0.1:1", None, None).is_err());
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
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            stream.set_nonblocking(true).unwrap();
            let _ = stream.write_all(&encode_message(&Message::Hello(String::new())));
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
            tx.send(messages).ok();
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while status.peers == 0 && Instant::now() < deadline {
            server.pump(&Message::Snapshot(vec![]), &spawns, None, 4.0, &mut status);
            std::thread::sleep(Duration::from_millis(10));
        }
        let messages = rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
        assert_eq!(
            messages.first(),
            Some(&Message::Welcome { client: 0, walk_speed: 4.0 }),
            "welcome must arrive first: {messages:?}"
        );
        assert_eq!(
            messages.get(1),
            Some(&Message::Spawn(spawns)),
            "spawn catch-up must follow the welcome: {messages:?}"
        );
    }

    fn handshake(
        server: &mut NetworkServer,
        token: Option<&str>,
        client: &mut NetworkClient,
        status: &mut NetworkStatus,
    ) -> NetworkStatus {
        let mut server_status = NetworkStatus::default();
        let deadline = Instant::now() + Duration::from_secs(2);
        while status.peers == 0 && status.error.is_none() && Instant::now() < deadline {
            server.pump(&Message::Snapshot(vec![]), &[], token, 4.0, &mut server_status);
            client.pump(status);
            std::thread::sleep(Duration::from_millis(5));
        }
        server_status
    }

    fn connect_pair(
        token: Option<&str>,
        client_token: Option<&str>,
    ) -> (NetworkServer, NetworkClient, NetworkStatus, NetworkStatus) {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let mut server = NetworkServer::default();
        server.listen(port).unwrap();
        let mut client = NetworkClient::default();
        client
            .connect(&format!("127.0.0.1:{port}"), client_token, None)
            .unwrap();
        let mut status = NetworkStatus::default();
        let server_status = handshake(&mut server, token, &mut client, &mut status);
        (server, client, status, server_status)
    }

    #[test]
    fn a_client_with_the_matching_token_is_accepted() {
        let (mut server, mut client, mut status, mut server_status) =
            connect_pair(Some("secret"), Some("secret"));
        assert_eq!(status.error, None);
        assert_eq!(status.peers, 1);
        assert_eq!(server_status.peers, 1);
        assert!(client.authenticated);
        server.pump(
            &Message::Snapshot(vec![state()]),
            &[],
            Some("secret"),
            4.0,
            &mut server_status,
        );
        let messages = client.pump(&mut status);
        assert!(
            messages
                .iter()
                .any(|message| matches!(message, Message::Snapshot(_))),
            "{messages:?}"
        );
    }

    #[test]
    fn a_wrong_token_is_rejected() {
        let (_server, client, status, _) = connect_pair(Some("secret"), Some("guess"));
        assert!(
            status.error.as_deref().is_some_and(|err| err.contains("rejected")),
            "error={:?} peers={} authenticated={}",
            status.error,
            status.peers,
            client.authenticated
        );
        assert_eq!(status.peers, 0);
        assert!(!client.authenticated);
    }

    #[test]
    fn a_missing_token_is_rejected_when_the_server_requires_one() {
        let (_server, _client, status, _) = connect_pair(Some("secret"), None);
        assert!(
            status.error.as_deref().is_some_and(|err| err.contains("rejected")),
            "error={:?} peers={}",
            status.error,
            status.peers
        );
        assert_eq!(status.peers, 0);
    }

    #[test]
    fn a_server_without_a_token_accepts_any_client() {
        let (_server, client, status, _) = connect_pair(None, None);
        assert_eq!(status.error, None);
        assert_eq!(status.peers, 1);
        assert!(client.authenticated);
    }

    fn listening_server() -> (NetworkServer, String) {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let mut server = NetworkServer::default();
        server.listen(port).unwrap();
        (server, format!("127.0.0.1:{port}"))
    }

    #[test]
    fn ownership_is_granted_to_one_client_and_denied_to_another() {
        let (mut server, addr) = listening_server();
        let mut first = NetworkClient::default();
        first.connect(&addr, None, Some(1)).unwrap();
        let mut srv = NetworkStatus::default();
        let mut s1 = NetworkStatus::default();
        let deadline = Instant::now() + Duration::from_secs(2);
        while first.owned.is_empty() && Instant::now() < deadline {
            server.pump(&Message::Snapshot(vec![]), &[], None, 4.0, &mut srv);
            first.pump(&mut s1);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(first.owned.contains(&1), "first claimant owns the object");

        let mut second = NetworkClient::default();
        second.connect(&addr, None, Some(1)).unwrap();
        let mut s2 = NetworkStatus::default();
        let deadline = Instant::now() + Duration::from_secs(2);
        while s2.error.is_none() && Instant::now() < deadline {
            server.pump(&Message::Snapshot(vec![]), &[], None, 4.0, &mut srv);
            first.pump(&mut s1);
            second.pump(&mut s2);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(second.owned.is_empty());
        assert!(
            s2.error.as_deref().is_some_and(|err| err.contains("claim denied")),
            "{:?}",
            s2.error
        );
        assert_eq!(srv.peers, 2);
    }

    #[test]
    fn inputs_are_stored_for_the_owned_object() {
        let (mut server, addr) = listening_server();
        let mut client = NetworkClient::default();
        client.connect(&addr, None, Some(7)).unwrap();
        let mut srv = NetworkStatus::default();
        let mut status = NetworkStatus::default();
        let deadline = Instant::now() + Duration::from_secs(2);
        while client.owned.is_empty() && Instant::now() < deadline {
            server.pump(&Message::Snapshot(vec![]), &[], None, 4.0, &mut srv);
            client.pump(&mut status);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(client.owned.contains(&7));
        client.send(&Message::Input {
            seq: 5,
            walk: [1.0, 0.0],
            jump: true,
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut received = Vec::new();
        while received.is_empty() && Instant::now() < deadline {
            server.pump(&Message::Snapshot(vec![]), &[], None, 4.0, &mut srv);
            client.pump(&mut status);
            received = server.take_inputs();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(received.len(), 1, "{received:?}");
        let (owner, object, input) = received[0];
        assert_eq!(owner, client.client_id);
        assert_eq!(object, 7);
        assert_eq!(input.seq, 5);
        assert_eq!(input.walk, [1.0, 0.0]);
        assert!(input.jump);
    }

    #[test]
    fn prediction_replays_only_unacknowledged_inputs() {
        let mut prediction = Prediction {
            history: vec![
                PendingInput {
                    seq: 1,
                    walk: [1.0, 0.0],
                    dt: 0.1,
                },
                PendingInput {
                    seq: 2,
                    walk: [1.0, 0.0],
                    dt: 0.1,
                },
                PendingInput {
                    seq: 3,
                    walk: [0.0, 1.0],
                    dt: 0.1,
                },
            ],
            ..Default::default()
        };
        prediction.prune(1);
        assert_eq!(prediction.history.len(), 2);
        let corrected = prediction.replay(Vec3::new(1.0, 0.0, 0.0), 4.0);
        assert!((corrected.x - 1.4).abs() < 1e-5, "{corrected:?}");
        assert!((corrected.z - 0.4).abs() < 1e-5, "{corrected:?}");
    }

    #[test]
    fn handshake_messages_round_trip() {
        for message in [
            Message::Hello("secret".into()),
            Message::Welcome {
                client: 3,
                walk_speed: 4.0,
            },
            Message::Rejected("invalid token".into()),
            Message::Claim(4),
            Message::Owner { id: 4, owner: 2 },
            Message::Denied(4),
            Message::Input {
                seq: 9,
                walk: [0.5, -0.5],
                jump: true,
            },
            Message::Ack { client: 2, seq: 9 },
        ] {
            let bytes = encode_message(&message);
            let mut reader = FrameReader::default();
            reader.feed(&bytes);
            assert_eq!(reader.next_message(), Some(message));
        }
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
            auth_token: None,
            claim: None,
            walk_speed: 4.0,
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
