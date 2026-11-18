//! The display list of a file (`Contents/DisplayLists`): the tessellation SolidWorks saves for
//! drawing the model without rebuilding it.
//!
//! The stream is an archive of objects. Each object starts with a tag: a new class (`FF FF`, a
//! version, the length of the name and the name) or `0x8000 | n` for the class that was number
//! `n`; classes and objects are numbered in the order they appear, from 1, in one sequence. A
//! pointer to an object is a tag (the object follows), the number of an object written before,
//! or 0 for none. Text is `FF FE FF`, a length and UTF-16.
//!
//! What is read of it:
//!
//! - **faces** (`uoTempFaceTessData_c`): the number of triangles and of strips, then arrays,
//!   each headed by element size, a type, 2 and the count: the length of each strip, the
//!   positions and the normals of the strip vertices (3 floats each, metres), and three arrays
//!   about edges. After more arrays (texture coordinates) follows a pointer to the appearance
//!   of the face;
//! - **appearances** (`moVisualProperties_c`): they start with their colour, red green blue;
//! - **bodies** (`uoTempBodyTessData_c`): a count and that many faces; after them
//!   `uoBodyPropInfo_c` points at the appearance of the body;
//! - **parts** (`uoTempPartTessData_c`): a count and that many bodies. A part file has just
//!   bodies;
//! - **placed parts** (`uoTempAssemblySHDData_c`, and `uoTempSubAssemblySHDData_c` for a placed
//!   assembly), in an assembly: the path of the component through the assemblies, where it is
//!   placed (a rotation, a translation and a scale), its configuration, name and file, and a
//!   pointer to the tessellation of its part. Parts placed more than once point at the same
//!   one.
//!
//! The rest of the stream (lights, line fonts, bounding boxes, flags) is not understood and is
//! stepped over: faces are found by the shape of their arrays, and the bytes between them are
//! searched for the tags above, each checked against what must follow it. The numbering has to
//! be kept exactly for the pointers to appearances and parts to land; where it is lost, a
//! pointer finds nothing and the face takes the colour of its body.

use std::collections::HashMap;

/// The tessellation of a file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DisplayList {
    /// The tessellated parts: one in a part file, one per different part in an assembly.
    pub parts: Vec<Part>,
    /// The placed parts and assemblies of an assembly, in the order of the file: an assembly
    /// before what is placed in it.
    pub components: Vec<Component>,
    /// Faces whose appearance points at something that is no appearance.
    pub unresolved_colors: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Part {
    pub bodies: Vec<Body>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Body {
    pub faces: Vec<Face>,
    /// The colour of the body, for its faces without one.
    pub color: Option<[u8; 3]>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Face {
    /// The vertices of the strips, one strip after the other.
    pub positions: Vec<[f32; 3]>,
    /// A normal per vertex; empty when the file has none that fit.
    pub normals: Vec<[f32; 3]>,
    /// The number of vertices of each triangle strip.
    pub strips: Vec<u32>,
    pub color: Option<[u8; 3]>,
}

/// Where a component is placed: a point `p` of the part lies at `p · rotation · scale +
/// translation` in the assembly, `p` a row vector.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub rotation: [[f64; 3]; 3],
    pub translation: [f64; 3],
    pub scale: f64,
}

impl Default for Transform {
    fn default() -> Self {
        Transform { rotation: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], translation: [0.0; 3], scale: 1.0 }
    }
}

impl Transform {
    /// As a 4 x 4 matrix, row by row, for column vectors.
    pub fn matrix(&self) -> [f64; 16] {
        let (r, t, s) = (&self.rotation, &self.translation, self.scale);
        [r[0][0] * s, r[1][0] * s, r[2][0] * s, t[0], r[0][1] * s, r[1][1] * s, r[2][1] * s, t[1], r[0][2] * s, r[1][2] * s, r[2][2] * s, t[2], 0.0, 0.0, 0.0, 1.0]
    }
}

/// A placed part or assembly.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Component {
    /// The way to it through the assemblies: `Arm-1@Robot/Bracket-2@Arm`.
    pub path: String,
    /// A placed assembly: it has no tessellation of its own, its parts follow.
    pub assembly: bool,
    /// Its place in the coordinates of the file (not of the assembly it is placed in).
    pub transform: Transform,
    pub configuration: String,
    /// The name of its document.
    pub name: String,
    /// The path of its file, as the computer that saved the assembly knew it.
    pub file: String,
    /// Its tessellation: an index into [`DisplayList::parts`]. None when the assembly was saved
    /// without it.
    pub part: Option<usize>,
}

impl Component {
    /// The name of the component in its assembly: `Bracket-2`.
    pub fn instance_name(&self) -> &str {
        let last = self.path.rsplit('/').next().unwrap_or(&self.path);
        last.split('@').next().unwrap_or(last)
    }

    /// The file name without its folders (which are those of another computer).
    pub fn file_name(&self) -> &str {
        self.file.rsplit(['\\', '/']).next().unwrap_or(&self.file)
    }
}

pub(crate) const APPEARANCE: &str = "moVisualProperties_c";
pub(crate) const FACE: &str = "uoTempFaceTessData_c";
pub(crate) const BODY: &str = "uoTempBodyTessData_c";
pub(crate) const BODY_PROPERTIES: &str = "uoBodyPropInfo_c";
pub(crate) const PART: &str = "uoTempPartTessData_c";
pub(crate) const COMPONENT: &str = "uoTempAssemblySHDData_c";
pub(crate) const SUB_ASSEMBLY: &str = "uoTempSubAssemblySHDData_c";

/// The number the appearance class has in the files seen so far; tried first.
const USUAL_APPEARANCE_INDEX: u32 = 42;
/// Faces have at most this many vertices: a bound against counts that are no counts.
const MAX_ARRAY: usize = 1 << 26;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Class {
    Appearance,
    Face,
    Body,
    BodyProperties,
    Part,
    Component,
    SubAssembly,
    Other,
}

impl Class {
    fn of(name: &[u8]) -> Class {
        match std::str::from_utf8(name).unwrap_or("") {
            APPEARANCE => Class::Appearance,
            FACE => Class::Face,
            BODY => Class::Body,
            BODY_PROPERTIES => Class::BodyProperties,
            PART => Class::Part,
            COMPONENT => Class::Component,
            SUB_ASSEMBLY => Class::SubAssembly,
            _ => Class::Other,
        }
    }
}

/// A pointer to an appearance, before the numbers are followed.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
enum Color {
    #[default]
    None,
    Is([u8; 3]),
    Object(u32),
}

/// A pointer to the tessellation of a part, likewise.
#[derive(Debug, Clone, Copy, PartialEq)]
enum PartPointer {
    None,
    /// Its tag follows the component.
    Follows,
    Own(usize),
    Object(u32),
}

#[derive(Default)]
struct RawBody {
    faces: Vec<(Face, Color)>,
    color: Color,
}

struct Walk<'a> {
    d: &'a [u8],
    /// The number the next class or object gets.
    count: u32,
    classes: HashMap<u32, Class>,
    index: HashMap<Class, u32>,
    appearances: HashMap<u32, [u8; 3]>,
    part_objects: HashMap<u32, usize>,
    parts: Vec<Vec<RawBody>>,
    components: Vec<(Component, PartPointer)>,
    /// The next appearance met is that of the last body.
    body_color_follows: bool,
    /// Where the next face starts: a face tag is one only right before it.
    next_face: usize,
}

fn u16_at(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(at..at.checked_add(2)?)?.try_into().ok()?))
}

fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

fn f64_at(d: &[u8], at: usize) -> Option<f64> {
    Some(f64::from_le_bytes(d.get(at..at.checked_add(8)?)?.try_into().ok()?))
}

/// The text at `at` and where it ends.
fn text_at(d: &[u8], at: usize) -> Option<(String, usize)> {
    if d.get(at..at + 3)? != [0xFF, 0xFE, 0xFF] {
        return None;
    }
    let (length, start) = match *d.get(at + 3)? {
        0xFF => match u16_at(d, at + 4)? {
            0xFFFF => (u32_at(d, at + 6)? as usize, at + 10),
            n => (usize::from(n), at + 6),
        },
        n => (usize::from(n), at + 4),
    };
    let bytes = d.get(start..start.checked_add(length.checked_mul(2)?)?)?;
    let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    Some((String::from_utf16_lossy(&units), start + bytes.len()))
}

/// The name of the class defined at `at` and where the definition ends.
fn class_at(d: &[u8], at: usize) -> Option<(&[u8], usize)> {
    if d.get(at..at + 2)? != [0xFF, 0xFF] || u16_at(d, at + 2)? > 0xFF {
        return None;
    }
    let length = usize::from(u16_at(d, at + 4)?);
    let name = d.get(at + 6..at + 6 + length)?;
    ((3..=64).contains(&length) && name.ends_with(b"_c") && name.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_')).then_some((name, at + 6 + length))
}

/// The array at `at`: element size, count, and where it ends.
fn array_at(d: &[u8], at: usize) -> Option<(usize, usize, usize)> {
    let (size, kind, two, count) = (u32_at(d, at)? as usize, u32_at(d, at + 4)?, u32_at(d, at + 8)?, u32_at(d, at + 12)? as usize);
    if two != 2 || !matches!(size, 1 | 2 | 4 | 8 | 12 | 16 | 24) || !matches!(kind, 8 | 100) || count > MAX_ARRAY {
        return None;
    }
    let end = at + 16 + size * count;
    (end <= d.len()).then_some((size, count, end))
}

/// The face whose counts start at `at`, and where its six arrays end.
fn face_at(d: &[u8], at: usize) -> Option<(Face, usize)> {
    let (triangles, strip_count) = (u32_at(d, at)? as usize, u32_at(d, at + 4)? as usize);
    let (size, count, mut end) = array_at(d, at + 8)?;
    if size != 4 || count != strip_count || u32_at(d, at + 12)? != 8 {
        return None;
    }
    let strips: Vec<u32> = d[at + 24..end].chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    let vertices = strips.iter().fold(0usize, |sum, &n| sum.saturating_add(n as usize));
    if strips.iter().any(|&n| n < 3) || triangles != vertices - 2 * strip_count {
        return None;
    }
    let mut vectors = [Vec::new(), Vec::new()];
    for vector in &mut vectors {
        let (size, count, after) = array_at(d, end)?;
        if size != 12 || u32_at(d, end + 4)? != 100 {
            return None;
        }
        *vector = d[end + 16..after].chunks_exact(12).map(|c| [0, 4, 8].map(|k| f32::from_le_bytes([c[k], c[k + 1], c[k + 2], c[k + 3]]))).collect();
        let _ = count;
        end = after;
    }
    let [positions, mut normals] = vectors;
    if positions.len() != vertices {
        return None;
    }
    if normals.len() != vertices {
        normals.clear();
    }
    for _ in 0..3 {
        end = array_at(d, end)?.2;
    }
    Some((Face { positions, normals, strips, color: None }, end))
}

/// Where the faces of the stream start (at their counts) and where their arrays end.
fn find_faces(d: &[u8]) -> Vec<(usize, usize)> {
    const HEAD: [u8; 12] = [4, 0, 0, 0, 8, 0, 0, 0, 2, 0, 0, 0];
    let (mut out, mut at) = (Vec::new(), 8);
    while at + 12 <= d.len() {
        if d[at] == 4 && d[at..at + 12] == HEAD {
            if let Some((_, end)) = face_at(d, at - 8) {
                out.push((at - 8, end));
                at = end.max(at + 1);
                continue;
            }
        }
        at += 1;
    }
    out
}

/// Whether an appearance starts at `at`: a colour, eight bytes that say little, and a text.
fn appearance_at(d: &[u8], at: usize) -> Option<[u8; 3]> {
    let b = d.get(at..at + 15)?;
    (b[3] == 0 && b[4..8] == [0; 4] && b[11] == 0 && b[12..15] == [0xFF, 0xFE, 0xFF]).then_some([b[0], b[1], b[2]])
}

impl<'a> Walk<'a> {
    fn new(d: &'a [u8], first: u32) -> Walk<'a> {
        Walk {
            d,
            count: first,
            classes: HashMap::new(),
            index: HashMap::new(),
            appearances: HashMap::new(),
            part_objects: HashMap::new(),
            parts: Vec::new(),
            components: Vec::new(),
            body_color_follows: false,
            next_face: usize::MAX,
        }
    }

    fn tag_of(&self, class: Class) -> Option<u16> {
        self.index.get(&class).map(|&n| 0x8000 | n as u16)
    }

    /// Whether at `at` an object of this class starts: its tag, or the definition of the class.
    fn starts(&self, class: Class, at: usize) -> bool {
        let tag = u16_at(self.d, at);
        tag.is_some() && tag == self.tag_of(class) || class_at(self.d, at).is_some_and(|(name, _)| Class::of(name) == class)
    }

    /// Whether what follows a tag of this class at `at` is what an object of it starts with.
    fn fits(&self, class: Class, at: usize) -> bool {
        let d = self.d;
        match class {
            Class::Appearance => appearance_at(d, at).is_some(),
            Class::Face => at == self.next_face,
            Class::Body => u16_at(d, at).is_some_and(|faces| faces > 0) && (at + 4 == self.next_face || class_at(d, at + 2).is_some()),
            Class::Part => self.starts(Class::Body, at + 2),
            Class::BodyProperties => {
                u16_at(d, at).is_some_and(|pointer| pointer < 0x8000 && u32_at(d, at + 2).is_some_and(|n| n <= 16) && u32_at(d, at + 6) == Some(6)) || self.starts(Class::Appearance, at)
            }
            Class::Component | Class::SubAssembly => text_at(d, at).is_some_and(|(_, end)| d.get(end).is_some_and(|&flag| flag <= 1)),
            Class::Other => true,
        }
    }

    fn last_body(&mut self) -> &mut RawBody {
        if self.parts.is_empty() {
            self.parts.push(Vec::new());
        }
        let part = self.parts.last_mut().expect("just added");
        if part.is_empty() {
            part.push(RawBody::default());
        }
        part.last_mut().expect("just added")
    }

    /// An object of this class starts at `at` (after its tag): takes its number and reads what
    /// is known of it. Gives where to go on.
    fn object(&mut self, class: Class, at: usize) -> usize {
        let number = self.count;
        self.count += 1;
        match class {
            Class::Appearance => {
                if let Some(color) = appearance_at(self.d, at) {
                    self.appearances.insert(number, color);
                    if std::mem::take(&mut self.body_color_follows) {
                        self.last_body().color = Color::Is(color);
                    }
                }
            }
            Class::Part => {
                self.parts.push(Vec::new());
                self.part_objects.insert(number, self.parts.len() - 1);
                self.body_color_follows = false;
                if let Some((_, pointer @ PartPointer::Follows)) = self.components.last_mut() {
                    *pointer = PartPointer::Own(self.parts.len() - 1);
                }
            }
            Class::Body => {
                if self.parts.is_empty() {
                    self.parts.push(Vec::new());
                }
                self.parts.last_mut().expect("just added").push(RawBody::default());
                self.body_color_follows = false;
            }
            Class::BodyProperties => match u16_at(self.d, at) {
                Some(pointer) if pointer != 0 && pointer < 0x7FFF => {
                    self.last_body().color = Color::Object(u32::from(pointer));
                    self.body_color_follows = false;
                }
                // no colour of its own, or one that follows: the next appearance is the body's
                _ => self.body_color_follows = !self.parts.is_empty(),
            },
            Class::Component | Class::SubAssembly => return self.component(class == Class::SubAssembly, at).unwrap_or(at),
            Class::Face | Class::Other => {}
        }
        at
    }

    /// The record of a placed part or assembly at `at`. Gives where to go on.
    fn component(&mut self, assembly: bool, at: usize) -> Option<usize> {
        let d = self.d;
        let (path, mut p) = text_at(d, at)?;
        let mut transform = Transform::default();
        let turned = *d.get(p)? == 1;
        p += 1;
        if turned {
            for (k, value) in transform.rotation.iter_mut().flatten().enumerate() {
                *value = f64_at(d, p + 8 * k)?;
            }
            p += 72;
        }
        for (k, value) in transform.translation.iter_mut().enumerate() {
            *value = f64_at(d, p + 8 * k)?;
        }
        transform.scale = f64_at(d, p + 24)?;
        p += 32;
        if !transform.rotation.iter().flatten().chain(&transform.translation).chain([&transform.scale]).all(|v| v.is_finite()) {
            transform = Transform::default();
        }
        // a flag and ten numbers (the bounds), two bytes, the configuration, twelve bytes, the
        // name and the file, sixty bytes, three texts, twelve bytes, the pointer
        let rest = (|| {
            let (configuration, q) = text_at(d, p + 83)?;
            let (name, q) = text_at(d, q + 12)?;
            let (file, q) = text_at(d, q)?;
            let (_, q) = text_at(d, q + 60)?;
            let (_, q) = text_at(d, q)?;
            let (_, q) = text_at(d, q)?;
            Some((configuration, name, file, q + 12))
        })();
        // the record of the file itself comes first, in the shape of a placed part: its path is its
        // name, without an assembly it is placed in
        let assembly = assembly || !path.contains('@');
        let mut component = Component { path, assembly, transform, ..Component::default() };
        let (pointer, next) = match rest {
            Some((configuration, name, file, q)) => {
                (component.configuration, component.name, component.file) = (configuration, name, file);
                match u16_at(d, q)? {
                    0 => (PartPointer::None, q + 2),
                    0x7FFF => (PartPointer::Object(u32_at(d, q + 2)?), q + 6),
                    n if n < 0x8000 => (PartPointer::Object(u32::from(n)), q + 2),
                    _ => (PartPointer::Follows, q),
                }
            }
            // a record of another shape: its part, if it has one of its own, follows somewhere
            None => (PartPointer::Follows, p),
        };
        self.components.push((component, pointer));
        Some(next)
    }

    /// Reads the bytes from `from` to `to` for the tags of objects.
    fn scan(&mut self, from: usize, to: usize) {
        let d = self.d;
        let mut p = from;
        while p + 2 <= to.min(d.len()) {
            if let Some((name, end)) = class_at(d, p) {
                let class = Class::of(name);
                self.classes.insert(self.count, class);
                self.index.entry(class).or_insert(self.count);
                self.count += 1;
                p = self.object(class, end).max(end);
                continue;
            }
            if let Some((_, end)) = text_at(d, p).filter(|&(_, end)| end <= to) {
                p = end;
                continue;
            }
            // a tag: two bytes, or 7FFF and four when numbers have run past 15 bits
            let tag = match (u16_at(d, p), u32_at(d, p + 2)) {
                (Some(0x7FFF), Some(n)) if n & 0x8000_0000 != 0 => Some((n & 0x7FFF_FFFF, p + 6)),
                (Some(n), _) if n & 0x8000 != 0 && n != 0xFFFF => Some((u32::from(n & 0x7FFF), p + 2)),
                _ => None,
            };
            if let Some((class, at)) = tag.and_then(|(n, at)| Some((*self.classes.get(&n)?, at))) {
                if self.fits(class, at) {
                    p = self.object(class, at).max(at);
                    continue;
                }
            }
            p += 1;
        }
    }

    /// The pointer to an appearance at `at`, and where it ends.
    fn appearance_pointer(&mut self, at: usize) -> Option<(Color, usize)> {
        let d = self.d;
        match u16_at(d, at)? {
            0 => Some((Color::None, at + 2)),
            0x7FFF => {
                let n = u32_at(d, at + 2)?;
                if n & 0x8000_0000 == 0 {
                    return Some((Color::Object(n), at + 6));
                }
                (self.classes.get(&(n & 0x7FFF_FFFF)) == Some(&Class::Appearance)).then_some(())?;
                Some((Color::Is(self.new_appearance(at + 6)?), at + 6))
            }
            n if n < 0x8000 => Some((Color::Object(u32::from(n)), at + 2)),
            n => {
                (Some(n) == self.tag_of(Class::Appearance)).then_some(())?;
                Some((Color::Is(self.new_appearance(at + 2)?), at + 2))
            }
        }
    }

    fn new_appearance(&mut self, at: usize) -> Option<[u8; 3]> {
        let color = appearance_at(self.d, at)?;
        self.appearances.insert(self.count, color);
        self.count += 1;
        Some(color)
    }

    /// What follows the arrays of a face, from `end` to the next face at `next`: the pointer to
    /// its appearance, and where to go on reading.
    fn face_tail(&mut self, end: usize, next: usize) -> (Color, usize) {
        let d = self.d;
        // three numbers, the bounds (ten doubles), ten numbers; an array, a number, two arrays;
        // a number, whether texture coordinates follow, and those; then a count and that many
        // times a number and a pointer to an appearance (one per display state, it seems). The
        // first appearance there is, is taken.
        let listed = (|| {
            let mut p = array_at(d, end + 132)?.2 + 4;
            p = array_at(d, array_at(d, p)?.2)?.2 + 4;
            if u32_at(d, p)? == 1 {
                p = array_at(d, p + 4)?.2;
            } else {
                p += 4;
            }
            let count = u32_at(d, p).filter(|n| (1..=64).contains(n))?;
            p += 4;
            let mut color = Color::None;
            for _ in 0..count {
                (p + 6 <= next).then_some(())?;
                let (pointer, after) = self.appearance_pointer(p + 4)?;
                if color == Color::None {
                    color = pointer;
                }
                p = after;
                if matches!(pointer, Color::Is(_)) {
                    // the appearance itself follows, of unknown length: the pointers after it
                    // are met while reading on
                    break;
                }
            }
            Some((color, p))
        })();
        if let Some(found) = listed {
            return found;
        }
        // faces that do not fit that: numbers and arrays up to a -1, after which the pointer comes
        let mut p = end + 132;
        for _ in 0..40 {
            let Some(word) = u32_at(d, p).filter(|_| p + 6 <= next) else { break };
            if word == 0xFFFF_FFFF {
                if let Some(found) = self.appearance_pointer(p + 4) {
                    return found;
                }
                break;
            }
            p = array_at(d, p).map_or(p + 4, |(_, _, after)| after);
        }
        // or the pointer by its distance to the next face in the same
        // body, 20 bytes, or 28 with three more numbers
        let same_body = u16_at(d, next.wrapping_sub(2)) == self.tag_of(Class::Face) && u16_at(d, next.wrapping_sub(6)) != self.tag_of(Class::Body);
        if next <= d.len() && next >= end + 28 && same_body {
            let at = if u32_at(d, next - 10) == Some(0) {
                Some(next - 20)
            } else if (u32_at(d, next - 18), u32_at(d, next - 14), u32_at(d, next - 10)) == (Some(1), Some(0xFFFF_FFFF), Some(2)) {
                Some(next - 28)
            } else {
                None
            };
            if let Some((color @ (Color::None | Color::Object(_)), after)) = at.and_then(|at| Some((u16_at(d, at)?, at))).filter(|&(n, _)| n < 0x7FFF).and_then(|(_, at)| self.appearance_pointer(at)) {
                return (color, after);
            }
        }
        (Color::None, end)
    }

    /// Walks the stream from `start`; stops after `limit` faces. Gives where it stopped.
    fn run(&mut self, start: usize, faces: &[(usize, usize)], limit: usize) -> usize {
        let mut at = start;
        for (k, &(face, end)) in faces.iter().enumerate().take(limit) {
            if face < at {
                continue;
            }
            self.next_face = face;
            self.scan(at, face);
            let Some((parsed, _)) = face_at(self.d, face) else { continue };
            let next = faces.get(k + 1).map_or(self.d.len(), |n| n.0);
            let (color, after) = self.face_tail(end, next);
            self.last_body().faces.push((parsed, color));
            at = after;
        }
        if limit >= faces.len() {
            self.next_face = usize::MAX;
            self.scan(at, self.d.len());
        }
        at
    }
}

/// The number of the appearance class: the one with which the tag of the second face is the
/// tag of the face class. (The classes before it, of lights and line fonts, are not counted
/// here, so its number is not known from the start.)
fn appearance_index(d: &[u8], start: usize, faces: &[(usize, usize)]) -> u32 {
    let Some(&(second, _)) = faces.get(1) else {
        // one face: only its own appearance matters, and that says the number
        let tag = faces.first().and_then(|&(_, end)| (end + 132..end + 400).find(|&p| u32_at(d, p) == Some(0xFFFF_FFFF) && appearance_at(d, p + 6).is_some()).and_then(|p| u16_at(d, p + 4)));
        return tag.filter(|n| n & 0x8000 != 0).map_or(USUAL_APPEARANCE_INDEX, |n| u32::from(n & 0x7FFF));
    };
    let fits = |first: u32| {
        let mut walk = Walk::new(d, first);
        let at = walk.run(start, faces, 1);
        walk.scan(at, second - 2);
        walk.tag_of(Class::Face).is_some() && walk.tag_of(Class::Face) == u16_at(d, second - 2)
    };
    std::iter::once(USUAL_APPEARANCE_INDEX).chain(1..512).find(|&first| fits(first)).unwrap_or(USUAL_APPEARANCE_INDEX)
}

/// Reads the display list of a file.
pub fn parse(d: &[u8]) -> DisplayList {
    let faces = find_faces(d);
    // the appearance class is defined before anything that is read
    let Some(start) = d.windows(APPEARANCE.len()).position(|w| w == APPEARANCE.as_bytes()).and_then(|at| at.checked_sub(6)) else { return DisplayList::default() };
    let mut walk = Walk::new(d, appearance_index(d, start, &faces));
    walk.run(start, &faces, usize::MAX);

    let mut unresolved_colors = 0;
    let appearances = &walk.appearances;
    let mut color = |color: Color, count: bool| match color {
        Color::None => None,
        Color::Is(rgb) => Some(rgb),
        Color::Object(n) => {
            let found = appearances.get(&n).copied();
            unresolved_colors += usize::from(count && found.is_none());
            found
        }
    };
    let parts: Vec<Part> = walk
        .parts
        .into_iter()
        .map(|bodies| Part {
            bodies: bodies
                .into_iter()
                .map(|body| Body { color: color(body.color, false), faces: body.faces.into_iter().map(|(face, pointer)| Face { color: color(pointer, true), ..face }).collect() })
                .filter(|body| !body.faces.is_empty())
                .collect(),
        })
        .collect();
    let part_objects = &walk.part_objects;
    let components = walk
        .components
        .into_iter()
        .map(|(component, pointer)| {
            let part = match pointer {
                PartPointer::Own(index) => Some(index),
                PartPointer::Object(n) => part_objects.get(&n).copied(),
                PartPointer::None | PartPointer::Follows => None,
            };
            Component { part: part.filter(|&index| !parts[index].bodies.is_empty()), ..component }
        })
        .collect();
    DisplayList { parts, components, unresolved_colors }
}
