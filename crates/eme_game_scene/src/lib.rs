//! Runtime-safe scene document: the objects an editor authors and a player loads.
//!
//! This crate is pure data. It depends on serde and RON only, so an exported game
//! can read a scene without pulling in the editor, rendering, or windowing.

use core::fmt;
extern crate alloc;

use alloc::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Version written into every document; older documents are rejected.
///
/// [`SceneObject::behaviour`] was added without a version bump: the field is
/// defaulted, so a document written before it existed still decodes to `None`.
pub const FORMAT_VERSION: u32 = 1;

/// What a scene object is, for spawning, icon choice, and inspector behavior.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default, Serialize, Deserialize)]
pub enum ObjectKind {
    /// Empty transform parent; nothing renders.
    #[default]
    Empty,
    /// Unit cube.
    Cube,
    /// Unit-diameter sphere.
    Sphere,
    /// Unit-height cylinder.
    Cylinder,
    /// Flat plane.
    Plane,
    /// Point light.
    PointLight,
    /// Directional or sun light.
    Sun,
    /// Camera.
    Camera,
    /// A glTF/GLB model instanced from an imported asset.
    Model,
}

impl ObjectKind {
    /// Every kind the create menu offers, in menu order.
    pub const ALL: [ObjectKind; 9] = [
        ObjectKind::Empty,
        ObjectKind::Cube,
        ObjectKind::Sphere,
        ObjectKind::Cylinder,
        ObjectKind::Plane,
        ObjectKind::PointLight,
        ObjectKind::Sun,
        ObjectKind::Camera,
        ObjectKind::Model,
    ];

    /// Stable index for per-kind tables; must stay in `ALL` order.
    pub fn index(self) -> usize {
        self as usize
    }

    /// Human label used for object names and menus.
    pub fn label(self) -> &'static str {
        match self {
            ObjectKind::Empty => "Empty",
            ObjectKind::Cube => "Cube",
            ObjectKind::Sphere => "Sphere",
            ObjectKind::Cylinder => "Cylinder",
            ObjectKind::Plane => "Plane",
            ObjectKind::PointLight => "Point Light",
            ObjectKind::Sun => "Sun",
            ObjectKind::Camera => "Camera",
            ObjectKind::Model => "Model",
        }
    }
}

/// A TRS transform in plain arrays so the document never needs a math crate.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct ObjectTransform {
    /// Local position.
    pub translation: [f32; 3],
    /// Local rotation as an `[x, y, z, w]` quaternion.
    pub rotation: [f32; 4],
    /// Local scale.
    pub scale: [f32; 3],
}

impl Default for ObjectTransform {
    fn default() -> Self {
        Self {
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
        }
    }
}

impl ObjectTransform {
    /// Identity transform with the given position.
    pub fn from_translation(translation: [f32; 3]) -> Self {
        Self {
            translation,
            ..Self::default()
        }
    }

    /// Replaces the rotation.
    pub fn with_rotation(mut self, rotation: [f32; 4]) -> Self {
        self.rotation = rotation;
        self
    }

    /// Replaces the scale.
    pub fn with_scale(mut self, scale: [f32; 3]) -> Self {
        self.scale = scale;
        self
    }
}

/// What a behaviour does to an object every frame.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default, Serialize, Deserialize)]
pub enum BehaviourKind {
    /// Nothing moves.
    #[default]
    None,
    /// Rotates around the world Y axis.
    Spin,
    /// Moves up and down along the world Y axis.
    Bob,
}

impl BehaviourKind {
    /// Every kind the inspector cycles through, in order.
    pub const ALL: [BehaviourKind; 3] =
        [BehaviourKind::None, BehaviourKind::Spin, BehaviourKind::Bob];

    /// The next kind in the inspector cycle, wrapping back to [`Self::None`].
    pub fn next(self) -> Self {
        match self {
            BehaviourKind::None => BehaviourKind::Spin,
            BehaviourKind::Spin => BehaviourKind::Bob,
            BehaviourKind::Bob => BehaviourKind::None,
        }
    }

    /// Human label shown in the inspector and the behaviour row.
    pub fn label(self) -> &'static str {
        match self {
            BehaviourKind::None => "None",
            BehaviourKind::Spin => "Spin",
            BehaviourKind::Bob => "Bob",
        }
    }
}

/// A running behaviour authored on an object.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Behaviour {
    /// What the object does every frame.
    pub kind: BehaviourKind,
    /// Radians per second for spin, cycles per second for bob.
    pub speed: f32,
    /// Vertical travel in world units for bob.
    pub amount: f32,
}

impl Default for Behaviour {
    fn default() -> Self {
        Self {
            kind: BehaviourKind::None,
            speed: 1.0,
            amount: 0.25,
        }
    }
}

/// Stable identity of one node inside a model instance.
///
/// The path is the child ordinal from the instance root; the name makes a
/// reorder or rename detectable instead of silently applying to the wrong node.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeKey {
    /// Child ordinals from the root to this node.
    pub path: Vec<u32>,
    /// Node name at the time the key was recorded.
    pub name: String,
}

impl NodeKey {
    /// The key for a model's root node.
    pub fn root(name: &str) -> Self {
        Self {
            path: Vec::new(),
            name: name.to_owned(),
        }
    }

    /// The key for a child at `ordinal` under this key.
    pub fn child(&self, ordinal: usize, name: &str) -> Self {
        let mut path = self.path.clone();
        path.push(ordinal as u32);
        Self {
            path,
            name: name.to_owned(),
        }
    }

    /// True when this key sits at or below `ancestor`'s path.
    pub fn is_within(&self, ancestor: &NodeKey) -> bool {
        self.path.starts_with(&ancestor.path)
    }
}

/// What an author changed on one model node.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum OverrideOp {
    /// Absolute local transform written by scenes before deltas existed.
    ///
    /// Loading such a scene in the editor migrates these to
    /// [`OverrideOp::SetTransformDelta`]; the player still applies them as-is.
    SetTransform(ObjectTransform),
    /// Offset applied on top of the imported node transform.
    SetTransformDelta(TransformDelta),
    /// Show or hide the node.
    SetVisible {
        /// Whether the node renders.
        visible: bool,
    },
    /// Leave the node and its subtree out of the instance.
    SetExcluded {
        /// Whether the node is excluded.
        excluded: bool,
    },
}

/// Which field an [`OverrideOp`] writes, used to replace an existing edit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OverrideKind {
    /// [`OverrideOp::SetTransform`].
    Transform,
    /// [`OverrideOp::SetVisible`].
    Visible,
    /// [`OverrideOp::SetExcluded`].
    Excluded,
}

impl OverrideOp {
    /// The field this op writes.
    pub fn kind(&self) -> OverrideKind {
        match self {
            OverrideOp::SetTransform(_) | OverrideOp::SetTransformDelta(_) => {
                OverrideKind::Transform
            }
            OverrideOp::SetVisible { .. } => OverrideKind::Visible,
            OverrideOp::SetExcluded { .. } => OverrideKind::Excluded,
        }
    }
}

/// One author edit keyed by the node it applies to.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct NodeOverride {
    /// Node identity the edit was authored against.
    pub key: NodeKey,
    /// The edit itself.
    pub op: OverrideOp,
}

/// Value a script exposes as an editable property.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ScriptValue {
    /// Boolean property.
    Bool(bool),
    /// Numeric property.
    Number(f64),
    /// Text property.
    Text(String),
}

impl Default for ScriptValue {
    fn default() -> Self {
        Self::Text(String::new())
    }
}

impl ScriptValue {
    /// Short type name used by editor readers that cannot show the value.
    pub fn type_name(&self) -> &'static str {
        match self {
            ScriptValue::Bool(_) => "bool",
            ScriptValue::Number(_) => "number",
            ScriptValue::Text(_) => "text",
        }
    }
}

/// Script attached to a scene object.
///
/// The path is relative to the project's `assets/` folder. Properties are
/// author overrides for the defaults declared in the script module.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct ScriptRef {
    /// Path below `assets/`, for example `scripts/spin.lua`.
    pub path: String,
    /// Author overrides of the script's declared properties.
    #[serde(default)]
    pub properties: BTreeMap<String, ScriptValue>,
}

/// Authoring data for a simple physics body.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhysicsBodySpec {
    /// Whether gravity and collisions move this body.
    pub dynamic: bool,
    /// Half extents of the axis-aligned collider.
    #[serde(default = "default_half_extents")]
    pub half_extents: [f32; 3],
}

fn default_half_extents() -> [f32; 3] {
    [0.5; 3]
}

impl Default for PhysicsBodySpec {
    fn default() -> Self {
        Self {
            dynamic: true,
            half_extents: default_half_extents(),
        }
    }
}

/// Animation selection authored on a model object.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct AnimationRef {
    /// Clip selected for playback.
    #[serde(default)]
    pub clip: Option<String>,
    /// Whether the clip starts with the scene.
    #[serde(default)]
    pub autoplay: bool,
}

/// Pointer from a scene object to an imported model asset.
///
/// The GUID is the durable identity: moving or renaming the file keeps it, and a
/// scene resolves by GUID first. The path is a readable fallback for when the
/// sidecar cannot be found.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct AssetRef {
    /// GUID stored in `<asset>.import.ron`.
    pub guid: String,
    /// Project-relative path, used when the GUID lookup fails.
    pub path: String,
}

/// One authored object. `id` is stable for the life of the document.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct SceneObject {
    /// Stable object id, unique inside the document.
    pub id: u64,
    /// Editor-facing and player-facing name.
    pub name: String,
    /// What the object is.
    pub kind: ObjectKind,
    /// Parent object id, if the object is nested.
    pub parent: Option<u64>,
    /// Local position.
    pub translation: [f32; 3],
    /// Local rotation as an `[x, y, z, w]` quaternion.
    pub rotation: [f32; 4],
    /// Local scale.
    pub scale: [f32; 3],
    /// Per-object base color as linear RGBA; `None` means the kind's default.
    pub color: Option<[f32; 4]>,
    /// Per-frame animation applied at runtime; the documented transform stays the
    /// authored base.
    #[serde(default)]
    pub behaviour: Behaviour,
    /// Imported model this object instances, for [`ObjectKind::Model`].
    #[serde(default)]
    pub asset: Option<AssetRef>,
    /// Author edits to the instanced model's nodes, kept beside the source asset.
    #[serde(default)]
    pub overrides: Vec<NodeOverride>,
    /// Optional gameplay script attached to this object.
    #[serde(default)]
    pub script: Option<ScriptRef>,
    /// Optional simple rigid body.
    #[serde(default)]
    pub physics: Option<PhysicsBodySpec>,
    /// Optional animation selection for imported models.
    #[serde(default)]
    pub animation: Option<AnimationRef>,
}

/// Offset applied on top of an imported node's transform.
///
/// A new edit never stores the absolute transform: translation is an offset,
/// rotation is composed onto the imported rotation, and scale is a multiplier,
/// so changing the source node still shows through.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TransformDelta {
    /// Added to the imported translation.
    pub translation: [f32; 3],
    /// Composed after the imported rotation.
    pub rotation: [f32; 4],
    /// Multiplied into the imported scale.
    pub scale: [f32; 3],
}

impl Default for TransformDelta {
    fn default() -> Self {
        Self {
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
        }
    }
}

impl TransformDelta {
    /// The delta that turns `base` into `absolute`.
    pub fn from_absolute(absolute: ObjectTransform, base: ObjectTransform) -> Self {
        let inverse = [
            -base.rotation[0],
            -base.rotation[1],
            -base.rotation[2],
            base.rotation[3],
        ];
        Self {
            translation: [
                absolute.translation[0] - base.translation[0],
                absolute.translation[1] - base.translation[1],
                absolute.translation[2] - base.translation[2],
            ],
            rotation: mul_quat(inverse, absolute.rotation),
            scale: [
                div_or_one(absolute.scale[0], base.scale[0]),
                div_or_one(absolute.scale[1], base.scale[1]),
                div_or_one(absolute.scale[2], base.scale[2]),
            ],
        }
    }

    /// Applies this delta on top of the imported transform.
    pub fn apply(&self, base: ObjectTransform) -> ObjectTransform {
        ObjectTransform {
            translation: [
                base.translation[0] + self.translation[0],
                base.translation[1] + self.translation[1],
                base.translation[2] + self.translation[2],
            ],
            rotation: mul_quat(base.rotation, self.rotation),
            scale: [
                base.scale[0] * self.scale[0],
                base.scale[1] * self.scale[1],
                base.scale[2] * self.scale[2],
            ],
        }
    }
}

fn div_or_one(value: f32, divisor: f32) -> f32 {
    if divisor.abs() < f32::EPSILON {
        1.0
    } else {
        value / divisor
    }
}

fn mul_quat(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let (ax, ay, az, aw) = (a[0], a[1], a[2], a[3]);
    let (bx, by, bz, bw) = (b[0], b[1], b[2], b[3]);
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

impl SceneObject {
    /// The object's TRS transform.
    pub fn transform(&self) -> ObjectTransform {
        ObjectTransform {
            translation: self.translation,
            rotation: self.rotation,
            scale: self.scale,
        }
    }
}

/// The serializable scene: the only thing an exported game needs from authoring.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct SceneDoc {
    /// Document format version; see [`FORMAT_VERSION`].
    pub format_version: u32,
    /// Root-level and nested objects, in authoring order.
    pub objects: Vec<SceneObject>,
}

impl Default for SceneDoc {
    fn default() -> Self {
        Self {
            format_version: FORMAT_VERSION,
            objects: Vec::new(),
        }
    }
}

impl SceneDoc {
    /// An empty document at the current format version.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of objects in the document.
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// Whether the document has no objects.
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    /// Every object in authoring order.
    pub fn iter(&self) -> impl Iterator<Item = &SceneObject> {
        self.objects.iter()
    }

    /// The object with `id`, if present.
    pub fn get(&self, id: u64) -> Option<&SceneObject> {
        self.objects.iter().find(|obj| obj.id == id)
    }

    /// Mutable access to the object with `id`, if present.
    pub fn get_mut(&mut self, id: u64) -> Option<&mut SceneObject> {
        self.objects.iter_mut().find(|obj| obj.id == id)
    }

    /// Smallest id that is not in use; ids only ever move forward.
    pub fn next_id(&self) -> u64 {
        self.objects
            .iter()
            .map(|obj| obj.id)
            .max()
            .map_or(1, |max| max + 1)
    }

    /// Appends an object and returns the id it was assigned.
    pub fn add(
        &mut self,
        kind: ObjectKind,
        name: impl Into<String>,
        transform: ObjectTransform,
    ) -> u64 {
        let id = self.next_id();
        self.objects.push(SceneObject {
            id,
            name: name.into(),
            kind,
            parent: None,
            translation: transform.translation,
            rotation: transform.rotation,
            scale: transform.scale,
            color: None,
            behaviour: Behaviour::default(),
            asset: None,
            overrides: Vec::new(),
            script: None,
            physics: None,
            animation: None,
        });
        id
    }

    /// Removes the object and detaches any children so no dangling parent ids remain.
    pub fn remove(&mut self, id: u64) -> Option<SceneObject> {
        let idx = self.objects.iter().position(|obj| obj.id == id)?;
        let removed = self.objects.remove(idx);
        for obj in &mut self.objects {
            if obj.parent == Some(id) {
                obj.parent = None;
            }
        }
        Some(removed)
    }

    /// Copies the object right after itself, offset one unit along X.
    pub fn duplicate(&mut self, id: u64) -> Option<u64> {
        let idx = self.objects.iter().position(|obj| obj.id == id)?;
        let mut copy = self.objects[idx].clone();
        copy.id = self.next_id();
        copy.translation[0] += 1.0;
        let new_id = copy.id;
        self.objects.insert(idx + 1, copy);
        Some(new_id)
    }

    /// Renames an object; false when the id is unknown.
    pub fn rename(&mut self, id: u64, name: impl Into<String>) -> bool {
        match self.get_mut(id) {
            Some(obj) => {
                obj.name = name.into();
                true
            }
            None => false,
        }
    }

    /// Moves an object; false when the id is unknown.
    pub fn set_translation(&mut self, id: u64, translation: [f32; 3]) -> bool {
        match self.get_mut(id) {
            Some(obj) => {
                obj.translation = translation;
                true
            }
            None => false,
        }
    }

    /// Replaces an object's rotation; false when the id is unknown.
    pub fn set_rotation(&mut self, id: u64, rotation: [f32; 4]) -> bool {
        match self.get_mut(id) {
            Some(obj) => {
                obj.rotation = rotation;
                true
            }
            None => false,
        }
    }

    /// Replaces an object's scale; false when the id is unknown.
    ///
    /// The caller owns validity: this stores the array as given, so scale
    /// clamping stays with the editor that authored the value.
    pub fn set_scale(&mut self, id: u64, scale: [f32; 3]) -> bool {
        match self.get_mut(id) {
            Some(obj) => {
                obj.scale = scale;
                true
            }
            None => false,
        }
    }

    /// Replaces an object's authored material color; false when the id is unknown.
    pub fn set_color(&mut self, id: u64, color: Option<[f32; 4]>) -> bool {
        match self.get_mut(id) {
            Some(obj) => {
                obj.color = color;
                true
            }
            None => false,
        }
    }

    /// Replaces an object's runtime behaviour; false when the id is unknown.
    pub fn set_behaviour(&mut self, id: u64, behaviour: Behaviour) -> bool {
        match self.get_mut(id) {
            Some(obj) => {
                obj.behaviour = behaviour;
                true
            }
            None => false,
        }
    }

    /// Points an object at an imported asset, or clears it.
    pub fn set_asset(&mut self, id: u64, asset: Option<AssetRef>) -> bool {
        match self.get_mut(id) {
            Some(obj) => {
                obj.asset = asset;
                true
            }
            None => false,
        }
    }

    /// The animation selection authored on `id`, if any.
    pub fn animation(&self, id: u64) -> Option<&AnimationRef> {
        self.get(id).and_then(|obj| obj.animation.as_ref())
    }

    /// Sets or clears the animation selection on `id`.
    pub fn set_animation(&mut self, id: u64, animation: Option<AnimationRef>) -> bool {
        let Some(obj) = self.get_mut(id) else {
            return false;
        };
        if obj.animation == animation {
            return false;
        }
        obj.animation = animation;
        true
    }

    /// The physics body authored on `id`, if any.
    pub fn physics(&self, id: u64) -> Option<&PhysicsBodySpec> {
        self.get(id).and_then(|obj| obj.physics.as_ref())
    }

    /// Attaches or clears the physics body on `id`.
    pub fn set_physics(&mut self, id: u64, physics: Option<PhysicsBodySpec>) -> bool {
        let Some(obj) = self.get_mut(id) else {
            return false;
        };
        if obj.physics == physics {
            return false;
        }
        obj.physics = physics;
        true
    }

    /// The script attached to `id`, if any.
    pub fn script(&self, id: u64) -> Option<&ScriptRef> {
        self.get(id).and_then(|obj| obj.script.as_ref())
    }

    /// Attaches or clears the script on `id`.
    pub fn set_script(&mut self, id: u64, script: Option<ScriptRef>) -> bool {
        let Some(obj) = self.get_mut(id) else {
            return false;
        };
        if obj.script == script {
            return false;
        }
        obj.script = script;
        true
    }

    /// Sets one author override for the script on `id`.
    pub fn set_script_property(
        &mut self,
        id: u64,
        key: impl Into<String>,
        value: ScriptValue,
    ) -> bool {
        let Some(obj) = self.get_mut(id) else {
            return false;
        };
        let Some(script) = obj.script.as_mut() else {
            return false;
        };
        let key = key.into();
        if script.properties.get(&key) == Some(&value) {
            return false;
        }
        script.properties.insert(key, value);
        true
    }

    /// Drops one script property override, falling back to the script default.
    pub fn remove_script_property(&mut self, id: u64, key: &str) -> bool {
        let Some(obj) = self.get_mut(id) else {
            return false;
        };
        let Some(script) = obj.script.as_mut() else {
            return false;
        };
        script.properties.remove(key).is_some()
    }

    /// The authored overrides for `id`, or an empty slice.
    pub fn overrides(&self, id: u64) -> &[NodeOverride] {
        self.get(id).map_or(&[], |obj| obj.overrides.as_slice())
    }

    /// Records one edit, replacing an earlier edit of the same kind on the same node.
    pub fn set_override(&mut self, id: u64, key: NodeKey, op: OverrideOp) -> bool {
        let kind = op.kind();
        let Some(obj) = self.get_mut(id) else {
            return false;
        };
        if let Some(existing) = obj
            .overrides
            .iter_mut()
            .find(|entry| entry.key == key && entry.op.kind() == kind)
        {
            if existing.op == op {
                return false;
            }
            existing.op = op;
            return true;
        }
        obj.overrides.push(NodeOverride { key, op });
        true
    }

    /// Removes one edit of `kind` for `key`.
    pub fn remove_override(&mut self, id: u64, key: &NodeKey, kind: OverrideKind) -> bool {
        let Some(obj) = self.get_mut(id) else {
            return false;
        };
        let before = obj.overrides.len();
        obj.overrides
            .retain(|entry| !(entry.key == *key && entry.op.kind() == kind));
        obj.overrides.len() != before
    }

    /// Removes every edit attached to `key`.
    pub fn remove_node_overrides(&mut self, id: u64, key: &NodeKey) -> bool {
        let Some(obj) = self.get_mut(id) else {
            return false;
        };
        let before = obj.overrides.len();
        obj.overrides.retain(|entry| entry.key != *key);
        obj.overrides.len() != before
    }

    /// Points every edit for `from` at `to`, used to reattach an orphan.
    pub fn reattach_override(&mut self, id: u64, from: &NodeKey, to: NodeKey) -> bool {
        let Some(obj) = self.get_mut(id) else {
            return false;
        };
        let mut moved = false;
        for entry in &mut obj.overrides {
            if entry.key == *from {
                entry.key = to.clone();
                moved = true;
            }
        }
        moved
    }

    /// Reparents an object, refusing self-parents, unknown parents and cycles.
    pub fn set_parent(&mut self, id: u64, parent: Option<u64>) -> bool {
        if parent == Some(id) {
            return false;
        }
        if let Some(target) = parent
            && (self.get(target).is_none() || self.is_ancestor(id, target))
        {
            return false;
        }
        match self.get_mut(id) {
            Some(obj) => {
                obj.parent = parent;
                true
            }
            None => false,
        }
    }

    /// Direct children of `id`, in document order.
    pub fn children(&self, id: u64) -> impl Iterator<Item = &SceneObject> {
        self.objects
            .iter()
            .filter(move |obj| obj.parent == Some(id))
    }

    /// Whether `ancestor` sits anywhere above `of` in the tree.
    pub fn is_ancestor(&self, ancestor: u64, of: u64) -> bool {
        let mut current = self.get(of).and_then(|obj| obj.parent);
        let mut guard = 0;
        while let Some(id) = current {
            if id == ancestor {
                return true;
            }
            guard += 1;
            if guard > self.objects.len() {
                return false;
            }
            current = self.get(id).and_then(|obj| obj.parent);
        }
        false
    }

    /// Every id below `id`, depth first.
    pub fn descendants(&self, id: u64) -> Vec<u64> {
        let mut out = Vec::new();
        let mut stack: Vec<u64> = self.children(id).map(|obj| obj.id).collect();
        while let Some(next) = stack.pop() {
            if out.contains(&next) {
                continue;
            }
            out.push(next);
            stack.extend(self.children(next).map(|obj| obj.id));
        }
        out
    }

    /// Depth of `id` in the tree; roots are 0.
    pub fn depth(&self, id: u64) -> usize {
        let mut depth = 0;
        let mut current = self.get(id).and_then(|obj| obj.parent);
        while let Some(parent) = current {
            depth += 1;
            if depth > self.objects.len() {
                break;
            }
            current = self.get(parent).and_then(|obj| obj.parent);
        }
        depth
    }

    /// Objects that hang off the root: no parent, or a parent that no longer exists.
    pub fn roots(&self) -> impl Iterator<Item = &SceneObject> {
        self.objects.iter().filter(|obj| match obj.parent {
            None => true,
            Some(id) => self.get(id).is_none(),
        })
    }

    /// Encodes the document as pretty RON.
    pub fn to_ron(&self) -> Result<String, SceneError> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|err| SceneError::Encode(err.to_string()))
    }

    /// Decodes a document, rejecting unknown versions and duplicate ids.
    pub fn from_ron(src: &str) -> Result<Self, SceneError> {
        let doc: SceneDoc =
            ron::from_str(src).map_err(|err| SceneError::Decode(err.to_string()))?;
        if doc.format_version != FORMAT_VERSION {
            return Err(SceneError::UnsupportedVersion(doc.format_version));
        }
        let mut seen = std::collections::HashSet::new();
        for obj in &doc.objects {
            if !seen.insert(obj.id) {
                return Err(SceneError::DuplicateId(obj.id));
            }
        }
        Ok(doc)
    }
}

/// Why a scene document could not be encoded, decoded, or trusted.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SceneError {
    /// RON encoding failed.
    Encode(String),
    /// RON decoding failed.
    Decode(String),
    /// The document was written by an incompatible format version.
    UnsupportedVersion(u32),
    /// Two objects share an id.
    DuplicateId(u64),
}

impl fmt::Display for SceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SceneError::Encode(err) => write!(f, "could not encode scene: {err}"),
            SceneError::Decode(err) => write!(f, "could not decode scene: {err}"),
            SceneError::UnsupportedVersion(version) => write!(
                f,
                "scene format version {version} is not supported (expected {FORMAT_VERSION})"
            ),
            SceneError::DuplicateId(id) => write!(f, "scene has more than one object with id {id}"),
        }
    }
}

impl std::error::Error for SceneError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn authored() -> SceneDoc {
        let mut doc = SceneDoc::new();
        doc.add(
            ObjectKind::Cube,
            "Cube 1",
            ObjectTransform::from_translation([-1.0, 0.5, -1.0]),
        );
        doc.add(
            ObjectKind::Sphere,
            "Sphere 1",
            ObjectTransform::from_translation([1.0, 0.5, -1.0]),
        );
        doc.add(
            ObjectKind::PointLight,
            "Point Light 1",
            ObjectTransform::from_translation([-1.0, 3.0, 1.0]),
        );
        doc.add(
            ObjectKind::Sun,
            "Sun 1",
            ObjectTransform::from_translation([1.0, 4.0, 1.0])
                .with_rotation([-0.383, 0.0, 0.0, 0.924]),
        );
        doc.add(
            ObjectKind::Camera,
            "Camera 1",
            ObjectTransform::from_translation([0.0, 1.8, 3.0]),
        );
        doc
    }

    #[test]
    fn round_trip_keeps_every_field() {
        let doc = authored();
        let text = doc.to_ron().unwrap();
        let back = SceneDoc::from_ron(&text).unwrap();
        assert_eq!(back, doc);
        assert_eq!(back.format_version, FORMAT_VERSION);
        assert_eq!(back.objects.len(), 5);
        assert!(back.get(4).is_some_and(|obj| obj.kind == ObjectKind::Sun));
        assert!(back
            .get(5)
            .is_some_and(|obj| obj.kind == ObjectKind::Camera));
        assert!(back.get(1).is_some_and(|obj| obj.kind == ObjectKind::Cube));
    }

    #[test]
    fn ron_holds_object_names() {
        let text = authored().to_ron().unwrap();
        for name in ["Cube 1", "Sphere 1", "Point Light 1", "Sun 1", "Camera 1"] {
            assert!(text.contains(name), "{name} missing from {text}");
        }
    }

    #[test]
    fn behaviour_round_trips_through_ron() {
        let mut doc = authored();
        let behaviour = Behaviour {
            kind: BehaviourKind::Spin,
            speed: 2.5,
            amount: 0.5,
        };
        assert!(doc.set_behaviour(1, behaviour));
        let back = SceneDoc::from_ron(&doc.to_ron().unwrap()).unwrap();
        assert_eq!(back, doc);
        assert_eq!(back.get(1).unwrap().behaviour, behaviour);
        assert!(!doc.set_behaviour(99, behaviour));
    }

    #[test]
    fn documents_without_behaviour_load_as_none() {
        let old = r#"(
    format_version: 1,
    objects: [(
        id: 1,
        name: "Cube 1",
        kind: Cube,
        parent: None,
        translation: (0.0, 0.5, -1.0),
        rotation: (0.0, 0.0, 0.0, 1.0),
        scale: (1.0, 1.0, 1.0),
        color: None,
    )],
)"#;
        let back = SceneDoc::from_ron(old).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back.get(1).unwrap().behaviour, Behaviour::default());
        assert_eq!(back.get(1).unwrap().behaviour.kind, BehaviourKind::None);
    }

    #[test]
    fn behaviour_kinds_cycle_and_all_are_listed() {
        let mut kind = BehaviourKind::None;
        for want in [BehaviourKind::Spin, BehaviourKind::Bob, BehaviourKind::None] {
            kind = kind.next();
            assert_eq!(kind, want);
        }
        let mut labels: Vec<&str> = BehaviourKind::ALL.iter().map(|kind| kind.label()).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), BehaviourKind::ALL.len());
    }

    #[test]
    fn duplicate_ids_are_unique_and_offset() {
        let mut doc = authored();
        let new_id = doc.duplicate(1).unwrap();
        assert_eq!(doc.objects.len(), 6);
        assert_eq!(doc.get(1).unwrap().translation[0], -1.0);
        assert_eq!(doc.get(new_id).unwrap().translation[0], 0.0);
        assert_eq!(doc.get(new_id).unwrap().name, "Cube 1");
        let ids: Vec<u64> = doc.objects.iter().map(|obj| obj.id).collect();
        let mut unique = ids.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(ids.len(), unique.len());
        assert!(doc.duplicate(99).is_none());
    }

    #[test]
    fn remove_drops_the_object_and_detaches_children() {
        let mut doc = authored();
        let child = doc.add(ObjectKind::Sphere, "Child", ObjectTransform::default());
        assert!(doc.set_parent(child, Some(1)));
        let removed = doc.remove(1).unwrap();
        assert_eq!(removed.id, 1);
        assert_eq!(doc.len(), 5);
        assert!(doc.get(1).is_none());
        assert_eq!(doc.get(child).unwrap().parent, None);
        assert!(doc.remove(1).is_none());
    }

    #[test]
    fn rename_and_translate_only_touch_existing_objects() {
        let mut doc = authored();
        assert!(doc.rename(2, "Widget"));
        assert_eq!(doc.get(2).unwrap().name, "Widget");
        assert!(doc.set_translation(2, [4.0, 5.0, 6.0]));
        assert_eq!(doc.get(2).unwrap().translation, [4.0, 5.0, 6.0]);
        assert!(!doc.rename(99, "Nope"));
        assert!(!doc.set_translation(99, [0.0; 3]));
    }

    #[test]
    fn rotation_and_scale_setters_only_touch_existing_objects() {
        let mut doc = authored();
        let cube = doc.get(1).unwrap().clone();
        assert!(doc.set_rotation(1, [0.0, 0.0, 0.0, 0.5]));
        assert_eq!(doc.get(1).unwrap().rotation, [0.0, 0.0, 0.0, 0.5]);
        assert_eq!(doc.get(1).unwrap().translation, cube.translation);
        assert_eq!(doc.get(1).unwrap().scale, cube.scale);
        assert!(doc.set_scale(1, [2.0, 3.0, 4.0]));
        assert_eq!(doc.get(1).unwrap().scale, [2.0, 3.0, 4.0]);
        assert!(!doc.set_rotation(99, [0.0, 0.0, 0.0, 1.0]));
        assert!(!doc.set_scale(99, [1.0; 3]));
    }

    #[test]
    fn every_kind_has_a_distinct_label() {
        let mut labels: Vec<&str> = ObjectKind::ALL.iter().map(|kind| kind.label()).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), ObjectKind::ALL.len());
        for (i, kind) in ObjectKind::ALL.iter().enumerate() {
            assert_eq!(kind.index(), i);
        }
    }

    #[test]
    fn rejects_unknown_versions_and_duplicate_ids() {
        let text = authored().to_ron().unwrap();
        let future = text.replace("format_version: 1", "format_version: 9");
        assert_eq!(
            SceneDoc::from_ron(&future),
            Err(SceneError::UnsupportedVersion(9))
        );

        let mut doc = authored();
        doc.objects[1].id = 1;
        assert_eq!(
            SceneDoc::from_ron(&doc.to_ron().unwrap()),
            Err(SceneError::DuplicateId(1))
        );
    }

    #[test]
    fn a_model_object_round_trips_with_its_asset() {
        let mut doc = SceneDoc::new();
        let id = doc.add(
            ObjectKind::Model,
            "chair",
            ObjectTransform::from_translation([1.0, 0.0, 2.0]),
        );
        doc.set_asset(
            id,
            Some(AssetRef {
                guid: "0123456789abcdef".into(),
                path: "models/chair.glb".into(),
            }),
        );

        let text = doc.to_ron().unwrap();
        let back = SceneDoc::from_ron(&text).unwrap();
        let object = back.get(id).unwrap();
        assert_eq!(object.kind, ObjectKind::Model);
        assert_eq!(
            object.asset,
            Some(AssetRef {
                guid: "0123456789abcdef".into(),
                path: "models/chair.glb".into(),
            })
        );
    }

    #[test]
    fn objects_without_an_asset_still_load() {
        let text = r#"(
            format_version: 1,
            objects: [(
                id: 1,
                name: "old",
                kind: Cube,
                parent: None,
                translation: (0.0, 0.0, 0.0),
                rotation: (0.0, 0.0, 0.0, 1.0),
                scale: (1.0, 1.0, 1.0),
                color: None,
            )],
        )"#;
        let doc = SceneDoc::from_ron(text).unwrap();
        assert_eq!(doc.objects[0].asset, None);
        assert_eq!(doc.objects[0].kind, ObjectKind::Cube);
    }

    fn node(path: &[u32], name: &str) -> NodeKey {
        NodeKey {
            path: path.to_vec(),
            name: name.to_owned(),
        }
    }

    #[test]
    fn overrides_round_trip_with_the_object() {
        let mut doc = SceneDoc::new();
        let id = doc.add(
            ObjectKind::Model,
            "chair",
            ObjectTransform::from_translation([0.0, 0.0, 0.0]),
        );
        assert!(doc.set_override(
            id,
            node(&[0, 1], "Leg"),
            OverrideOp::SetTransform(ObjectTransform::from_translation([1.0, 2.0, 3.0])),
        ));
        assert!(doc.set_override(
            id,
            node(&[0, 1], "Leg"),
            OverrideOp::SetVisible { visible: false },
        ));
        assert!(doc.set_override(
            id,
            node(&[0], "Chair"),
            OverrideOp::SetExcluded { excluded: true },
        ));

        let text = doc.to_ron().unwrap();
        let back = SceneDoc::from_ron(&text).unwrap();
        let overrides = back.overrides(id);
        assert_eq!(overrides.len(), 3);
        assert!(overrides.iter().any(|entry| entry.key == node(&[0, 1], "Leg")
            && entry.op == OverrideOp::SetVisible { visible: false }));
        assert!(overrides
            .iter()
            .any(|entry| entry.op == OverrideOp::SetExcluded { excluded: true }));
    }

    #[test]
    fn a_transform_delta_round_trips_and_composes() {
        let base = ObjectTransform {
            translation: [1.0, 2.0, 3.0],
            rotation: [0.0, 0.707_106_77, 0.0, 0.707_106_77],
            scale: [2.0, 2.0, 2.0],
        };
        let absolute = ObjectTransform {
            translation: [4.0, 2.0, 3.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [4.0, 4.0, 4.0],
        };
        let delta = TransformDelta::from_absolute(absolute, base);
        let back = delta.apply(base);
        for axis in 0..3 {
            assert!((back.translation[axis] - absolute.translation[axis]).abs() < 1e-5);
            assert!((back.scale[axis] - absolute.scale[axis]).abs() < 1e-5);
        }
        assert!(back.rotation[3].abs() > 0.99);

        let text = ron::ser::to_string(&delta).unwrap();
        let decoded: TransformDelta = ron::from_str(&text).unwrap();
        assert_eq!(decoded, delta);

        let empty: TransformDelta = ron::from_str("()").unwrap();
        assert_eq!(empty, TransformDelta::default());
    }

    #[test]
    fn a_script_round_trips_with_properties() {
        let mut doc = SceneDoc::new();
        let id = doc.add(
            ObjectKind::Cube,
            "Spinner",
            ObjectTransform::from_translation([0.0; 3]),
        );
        let mut properties = BTreeMap::new();
        properties.insert("speed".to_string(), ScriptValue::Number(2.5));
        properties.insert("label".to_string(), ScriptValue::Text("spin".into()));
        assert!(doc.set_script(
            id,
            Some(ScriptRef {
                path: "scripts/spin.lua".into(),
                properties,
            })
        ));

        let text = doc.to_ron().unwrap();
        let back = SceneDoc::from_ron(&text).unwrap();
        let script = back.script(id).unwrap();
        assert_eq!(script.path, "scripts/spin.lua");
        assert_eq!(
            script.properties.get("speed"),
            Some(&ScriptValue::Number(2.5))
        );
        assert_eq!(
            script.properties.get("label"),
            Some(&ScriptValue::Text("spin".into()))
        );
        assert!(text.contains("script: Some"));
    }

    #[test]
    fn an_authored_material_color_round_trips() {
        let mut doc = SceneDoc::new();
        let id = doc.add(
            ObjectKind::Cube,
            "Red",
            ObjectTransform::from_translation([0.0; 3]),
        );
        assert!(doc.set_color(id, Some([0.8, 0.1, 0.2, 1.0])));
        let back = SceneDoc::from_ron(&doc.to_ron().unwrap()).unwrap();
        assert_eq!(back.get(id).unwrap().color, Some([0.8, 0.1, 0.2, 1.0]));
    }

    #[test]
    fn an_animation_selection_round_trips() {
        let mut doc = SceneDoc::new();
        let id = doc.add(
            ObjectKind::Model,
            "Animated",
            ObjectTransform::default(),
        );
        assert!(doc.set_animation(
            id,
            Some(AnimationRef {
                clip: Some("Spin".into()),
                autoplay: true,
            })
        ));
        let back = SceneDoc::from_ron(&doc.to_ron().unwrap()).unwrap();
        assert_eq!(
            back.animation(id).and_then(|animation| animation.clip.as_deref()),
            Some("Spin")
        );
        assert!(back.animation(id).is_some_and(|animation| animation.autoplay));
    }

    #[test]
    fn a_physics_body_round_trips_and_old_scenes_load() {
        let mut doc = SceneDoc::new();
        let id = doc.add(
            ObjectKind::Cube,
            "Falling",
            ObjectTransform::from_translation([0.0, 3.0, 0.0]),
        );
        assert!(doc.set_physics(id, Some(PhysicsBodySpec::default())));
        let back = SceneDoc::from_ron(&doc.to_ron().unwrap()).unwrap();
        assert_eq!(back.physics(id), Some(&PhysicsBodySpec::default()));

        let old = r#"(
            format_version: 1,
            objects: [(id: 1, name: "old", kind: Cube, parent: None,
                translation: (0.0, 0.0, 0.0), rotation: (0.0, 0.0, 0.0, 1.0),
                scale: (1.0, 1.0, 1.0), color: None)],
        )"#;
        assert!(SceneDoc::from_ron(old).unwrap().physics(1).is_none());
    }

    #[test]
    fn an_old_scene_without_a_script_still_loads() {
        let text = r#"(
            format_version: 1,
            objects: [(
                id: 1,
                name: "old",
                kind: Cube,
                parent: None,
                translation: (0.0, 0.0, 0.0),
                rotation: (0.0, 0.0, 0.0, 1.0),
                scale: (1.0, 1.0, 1.0),
                color: None,
            )],
        )"#;
        let doc = SceneDoc::from_ron(text).unwrap();
        assert!(doc.script(1).is_none());
    }

    #[test]
    fn script_properties_can_be_set_and_removed() {
        let mut doc = SceneDoc::new();
        let id = doc.add(
            ObjectKind::Cube,
            "Spinner",
            ObjectTransform::from_translation([0.0; 3]),
        );
        doc.set_script(
            id,
            Some(ScriptRef {
                path: "scripts/spin.lua".into(),
                properties: BTreeMap::new(),
            }),
        );
        assert!(doc.set_script_property(id, "speed", ScriptValue::Bool(true)));
        assert_eq!(
            doc.script(id).unwrap().properties.get("speed"),
            Some(&ScriptValue::Bool(true))
        );
        assert!(doc.remove_script_property(id, "speed"));
        assert!(doc.script(id).unwrap().properties.is_empty());
    }

    #[test]
    fn an_old_scene_without_overrides_still_loads() {
        let text = r#"(
            format_version: 1,
            objects: [(
                id: 1,
                name: "old",
                kind: Cube,
                parent: None,
                translation: (0.0, 0.0, 0.0),
                rotation: (0.0, 0.0, 0.0, 1.0),
                scale: (1.0, 1.0, 1.0),
                color: None,
            )],
        )"#;
        let doc = SceneDoc::from_ron(text).unwrap();
        assert!(doc.overrides(1).is_empty());
    }

    #[test]
    fn setting_the_same_kind_replaces_and_reattach_moves_every_op() {
        let mut doc = SceneDoc::new();
        let id = doc.add(
            ObjectKind::Model,
            "chair",
            ObjectTransform::from_translation([0.0; 3]),
        );
        let key = node(&[0], "Seat");
        assert!(doc.set_override(
            id,
            key.clone(),
            OverrideOp::SetVisible { visible: false }
        ));
        assert!(doc.set_override(
            id,
            key.clone(),
            OverrideOp::SetVisible { visible: true }
        ));
        assert_eq!(doc.overrides(id).len(), 1);
        assert!(doc.set_override(
            id,
            key.clone(),
            OverrideOp::SetTransform(ObjectTransform::from_translation([1.0; 3])),
        ));

        let to = node(&[0], "Moved");
        assert!(doc.reattach_override(id, &key, to.clone()));
        assert!(doc.overrides(id).iter().all(|entry| entry.key == to));
        assert!(doc.remove_override(id, &to, OverrideKind::Visible));
        assert_eq!(doc.overrides(id).len(), 1);
    }

    #[test]
    fn parenting_refuses_cycles_and_self() {
        let mut doc = authored();
        let cube = doc.objects[0].id;
        let sphere = doc.objects[1].id;
        let light = doc.objects[2].id;

        assert!(doc.set_parent(sphere, Some(cube)));
        assert!(doc.set_parent(light, Some(sphere)));
        assert!(
            !doc.set_parent(cube, Some(light)),
            "grandparent cycle slipped through"
        );
        assert!(
            !doc.set_parent(cube, Some(cube)),
            "self-parent slipped through"
        );
        assert!(
            !doc.set_parent(cube, Some(999)),
            "unknown parent slipped through"
        );
        assert_eq!(doc.get(cube).unwrap().parent, None);
    }

    #[test]
    fn tree_walks_report_children_descendants_and_depth() {
        let mut doc = authored();
        let cube = doc.objects[0].id;
        let sphere = doc.objects[1].id;
        let light = doc.objects[2].id;
        let sun = doc.objects[3].id;

        doc.set_parent(sphere, Some(cube));
        doc.set_parent(light, Some(sphere));
        doc.set_parent(sun, Some(cube));

        let mut kids: Vec<u64> = doc.children(cube).map(|obj| obj.id).collect();
        kids.sort();
        let mut want = vec![sphere, sun];
        want.sort();
        assert_eq!(kids, want);

        let mut below = doc.descendants(cube);
        below.sort();
        let mut want_below = vec![sphere, light, sun];
        want_below.sort();
        assert_eq!(below, want_below);

        assert_eq!(doc.depth(cube), 0);
        assert_eq!(doc.depth(sphere), 1);
        assert_eq!(doc.depth(light), 2);
        assert!(doc.is_ancestor(cube, light));
        assert!(doc.is_ancestor(sphere, light));
        assert!(!doc.is_ancestor(light, cube));
    }

    #[test]
    fn removing_a_parent_promotes_its_children_to_roots() {
        let mut doc = authored();
        let cube = doc.objects[0].id;
        let sphere = doc.objects[1].id;
        doc.set_parent(sphere, Some(cube));

        doc.remove(cube);

        assert_eq!(doc.get(sphere).unwrap().parent, None);
        let roots: Vec<u64> = doc.roots().map(|obj| obj.id).collect();
        assert!(roots.contains(&sphere));
    }
}
