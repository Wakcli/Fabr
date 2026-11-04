//! Runtime-safe scene document: the objects an editor authors and a player loads.
//!
//! This crate is pure data. It depends on serde and RON only, so an exported game
//! can read a scene without pulling in the editor, rendering, or windowing.

use core::fmt;

use serde::{Deserialize, Serialize};

/// Version written into every document; older documents are rejected.
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
}

impl ObjectKind {
    /// Every kind the create menu offers, in menu order.
    pub const ALL: [ObjectKind; 8] = [
        ObjectKind::Empty,
        ObjectKind::Cube,
        ObjectKind::Sphere,
        ObjectKind::Cylinder,
        ObjectKind::Plane,
        ObjectKind::PointLight,
        ObjectKind::Sun,
        ObjectKind::Camera,
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
        self.objects.iter().filter(move |obj| obj.parent == Some(id))
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
    fn parenting_refuses_cycles_and_self() {
        let mut doc = authored();
        let cube = doc.objects[0].id;
        let sphere = doc.objects[1].id;
        let light = doc.objects[2].id;

        assert!(doc.set_parent(sphere, Some(cube)));
        assert!(doc.set_parent(light, Some(sphere)));
        assert!(!doc.set_parent(cube, Some(light)), "grandparent cycle slipped through");
        assert!(!doc.set_parent(cube, Some(cube)), "self-parent slipped through");
        assert!(!doc.set_parent(cube, Some(999)), "unknown parent slipped through");
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
