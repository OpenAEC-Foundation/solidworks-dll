//! Tests on display lists written here, byte by byte, in the layout the reader expects.

use crate::displaylist::{self, APPEARANCE, BODY, BODY_PROPERTIES, COMPONENT, FACE, PART, SUB_ASSEMBLY};
use crate::model::{Kind, DEFAULT_COLOR};
use crate::{read, read_assembly, Error, DISPLAY_LISTS};
use std::collections::HashMap;

/// A pointer to an object, as written.
#[derive(Clone, Copy)]
enum Pointer {
    None,
    Object(u32),
    /// A new appearance of this colour follows.
    New([u8; 3]),
}

/// A display list being written: keeps the numbers of classes and objects as the file does.
struct Stream {
    bytes: Vec<u8>,
    count: u32,
    classes: HashMap<&'static str, u32>,
}

const BLUE: [u8; 3] = [102, 183, 255];
const GREY: [u8; 3] = [202, 209, 238];
const BLACK: [u8; 3] = [20, 20, 20];

impl Stream {
    /// A stream whose appearance class gets number `first`: lights and line fonts, which the
    /// reader does not count, come before it. Starts with the appearance of the document.
    fn new(first: u32) -> Stream {
        let mut stream = Stream { bytes: Vec::new(), count: first, classes: HashMap::new() };
        stream.bytes.extend_from_slice(&[1, 0, 0, 0, 1, 0, 0, 0]);
        stream.bytes.extend_from_slice(b"\xff\xff\x01\x00\x12\x00moDirectionLight_c");
        stream.doubles(&[0.1, 0.3, 0.3]);
        stream.appearance(GREY);
        stream
    }

    fn u16(&mut self, v: u16) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }

    fn u32s(&mut self, values: &[u32]) {
        values.iter().for_each(|v| self.bytes.extend_from_slice(&v.to_le_bytes()));
    }

    fn doubles(&mut self, values: &[f64]) {
        values.iter().for_each(|v| self.bytes.extend_from_slice(&v.to_le_bytes()));
    }

    fn text(&mut self, text: &str) {
        let units: Vec<u16> = text.encode_utf16().collect();
        self.bytes.extend_from_slice(&[0xFF, 0xFE, 0xFF, units.len() as u8]);
        units.iter().for_each(|u| self.bytes.extend_from_slice(&u.to_le_bytes()));
    }

    /// The tag of a new object of this class (the definition of the class the first time);
    /// the number of the object.
    fn tag(&mut self, class: &'static str) -> u32 {
        match self.classes.get(class) {
            Some(&n) => self.u16(0x8000 | n as u16),
            None => {
                self.bytes.extend_from_slice(&[0xFF, 0xFF, 1, 0, class.len() as u8, 0]);
                self.bytes.extend_from_slice(class.as_bytes());
                self.classes.insert(class, self.count);
                self.count += 1;
            }
        }
        self.count += 1;
        self.count - 1
    }

    fn appearance(&mut self, color: [u8; 3]) -> u32 {
        let number = self.tag(APPEARANCE);
        self.bytes.extend_from_slice(&[color[0], color[1], color[2], 0]);
        self.u32s(&[0, 0x00C0_C0C0]);
        self.text("");
        self.text("Steel");
        self.doubles(&[1.0, 1.0, 0.5, 0.4]);
        self.text("defaultplastic");
        self.u32s(&[1, 0xFFFF_FFFF, 0]);
        number
    }

    fn pointer(&mut self, pointer: Pointer) {
        match pointer {
            Pointer::None => self.u16(0),
            Pointer::Object(n) => self.u16(n as u16),
            Pointer::New(color) => {
                self.appearance(color);
            }
        }
    }

    fn array(&mut self, size: u32, kind: u32, count: u32) {
        self.u32s(&[size, kind, 2, count]);
    }

    /// A face of these triangle strips, all its normals `normal`; the number of the face.
    fn face(&mut self, strips: &[&[[f32; 3]]], normal: [f32; 3], color: Pointer) -> u32 {
        let number = self.tag(FACE);
        let vertices: u32 = strips.iter().map(|s| s.len() as u32).sum();
        let triangles = vertices - 2 * strips.len() as u32;
        self.u32s(&[triangles, strips.len() as u32]);
        self.array(4, 8, strips.len() as u32);
        strips.iter().for_each(|s| self.u32s(&[s.len() as u32]));
        self.array(12, 100, vertices);
        strips.iter().flat_map(|s| s.iter().flatten()).for_each(|v| self.bytes.extend_from_slice(&v.to_le_bytes()));
        self.array(12, 100, vertices);
        (0..vertices).flat_map(|_| normal).for_each(|v| self.bytes.extend_from_slice(&v.to_le_bytes()));
        self.array(4, 8, vertices + triangles);
        self.u32s(&vec![7; (vertices + triangles) as usize]);
        self.array(4, 8, strips.len() as u32);
        self.u32s(&vec![4; strips.len()]);
        self.array(1, 8, vertices + triangles);
        self.bytes.extend(std::iter::repeat_n(1u8, (vertices + triangles) as usize));
        // what follows the arrays: numbers, the bounds, numbers, three empty arrays
        self.u32s(&[0, 0, 1]);
        self.doubles(&[0.5; 10]);
        self.u32s(&[1, 1, 0xAB75_035A, 0, 1, 0, 0, 0, 0xF, 1]);
        self.array(8, 100, 0);
        self.u32s(&[1]);
        self.array(12, 100, 0);
        self.array(12, 100, 0);
        self.u32s(&[0, 0, 1, 0xFFFF_FFFF]);
        self.pointer(color);
        match color {
            Pointer::None => self.u32s(&[0x41EE, 0x34, 0, 0x77]),
            _ => self.u32s(&[0x0060_AB75, 0x67, 1, 0xFFFF_FFFF, 2, 0x55B8]),
        }
        number
    }

    fn body(&mut self, faces: u16) {
        self.tag(BODY);
        self.u16(faces);
    }

    /// What follows the faces of a body: its properties, which point at its appearance.
    fn body_end(&mut self, color: Pointer) {
        self.u32s(&[0, 0x65, 0, 1, 0xFFFF_FFFF]);
        self.tag(BODY_PROPERTIES);
        match color {
            Pointer::New(_) => self.u16(0),
            other => self.pointer(other),
        }
        self.u32s(&[1, 6, 1, 0, 0]);
        self.u16(0);
        self.u32s(&[1, 0xFFFF_FFFF]);
        if let Pointer::New(color) = color {
            self.appearance(color);
        }
    }

    fn part(&mut self, bodies: u16) -> u32 {
        let number = self.tag(PART);
        self.u16(bodies);
        number
    }

    /// The record of a placed part (or assembly). With `Pointer::New` its part follows.
    fn component(&mut self, assembly: bool, path: &str, rotation: Option<[f64; 9]>, translation: [f64; 3], name: &str, file: &str, part: Pointer) {
        self.tag(if assembly { SUB_ASSEMBLY } else { COMPONENT });
        self.text(path);
        self.bytes.push(u8::from(rotation.is_some()));
        if let Some(rotation) = rotation {
            self.doubles(&rotation);
        }
        self.doubles(&translation);
        self.doubles(&[1.0]);
        self.bytes.push(0);
        self.doubles(&[0.25; 10]);
        self.u16(0);
        self.text("Default");
        self.u32s(&[0, 0x87, 0x56B0_9C91]);
        self.text(name);
        self.text(file);
        self.u32s(&[0, 0, 6]);
        self.doubles(&[1000.0]);
        self.u32s(&[1, 0, 1, 0, 1, 0, 0, 0xA4, 0xFFFF_FFFF, 0]);
        self.text("");
        self.text("");
        self.text(name);
        self.u32s(&[0, 0, 0]);
        match part {
            Pointer::New(_) => {}
            other => {
                self.pointer(other);
                self.u32s(&[0xFFFF_FFFF]);
                self.u16(if assembly { 2 } else { 0 });
            }
        }
    }

    /// The bytes of a file with this display list.
    fn file(&self) -> Vec<u8> {
        let stream = sldw_core::testing::zlb(&self.bytes, 4096);
        sldw_core::testing::compound_file(&[("Header2", b"header"), (&format!("{DISPLAY_LISTS}__ZLB"), &stream)])
    }
}

/// A unit square in the plane z = `z`, as one strip.
fn square(z: f32) -> [[f32; 3]; 4] {
    [[0.0, 0.0, z], [1.0, 0.0, z], [0.0, 1.0, z], [1.0, 1.0, z]]
}

/// A part of one body: a face with an appearance of its own, one that points at that
/// appearance, and one without, which takes the colour of the body.
fn part(first: u32, body_color: Pointer) -> Stream {
    let mut s = Stream::new(first);
    s.u32s(&[0xFFFF_FFFF]);
    s.u16(1);
    s.body(3);
    let (top, strip) = (square(1.0), [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
    let face = s.face(&[&top], [0.0, 0.0, 1.0], Pointer::New(BLUE));
    s.face(&[&square(0.0)], [0.0, 0.0, -1.0], Pointer::Object(face + 1));
    s.face(&[&strip, &strip], [0.0, 0.0, 1.0], Pointer::None);
    s.body_end(body_color);
    s
}

fn color(rgb: [u8; 3]) -> [f32; 3] {
    rgb.map(|v| f32::from(v) / 255.0)
}

#[test]
fn a_part_gives_a_mesh_per_colour_of_its_body() {
    let scene = read(&part(42, Pointer::New(BLACK)).file()).unwrap();
    assert_eq!((scene.info.kind, scene.info.bodies, scene.info.faces, scene.info.unresolved_colors), (Kind::Part, 1, 3, 0));
    assert!(scene.info.missing.is_empty());
    assert_eq!(scene.meshes.len(), 2);
    let (blue, black) = (&scene.meshes[0], &scene.meshes[1]);
    assert_eq!((blue.name.as_str(), blue.color, blue.triangle_count()), ("Body 1", color(BLUE), 4));
    assert_eq!((black.color, black.triangle_count(), black.positions.len()), (color(BLACK), 2, 18));
    assert_eq!(blue.transform, crate::model::IDENTITY);
    assert_eq!(blue.normals.len(), blue.positions.len());
    assert_eq!(scene.triangle_count(), 6);
}

#[test]
fn triangles_face_the_way_of_their_normals() {
    let scene = read(&part(42, Pointer::None).file()).unwrap();
    let blue = &scene.meshes[0];
    // the square at z = 1 looks up, the one at z = 0 down
    for triangle in blue.indices.chunks_exact(3) {
        let [a, b, c] = [0, 1, 2].map(|k| [0, 1, 2].map(|axis| blue.positions[triangle[k] as usize * 3 + axis]));
        let normal = crate::cross(crate::subtract(b, a), crate::subtract(c, a));
        assert_eq!(normal[2] > 0.0, a[2] == 1.0, "{a:?} {b:?} {c:?}");
    }
    // without an appearance anywhere: the default colour
    assert_eq!(scene.meshes[1].color, DEFAULT_COLOR);
}

#[test]
fn a_body_points_at_its_appearance() {
    let mut s = Stream::new(42);
    s.u32s(&[0xFFFF_FFFF]);
    s.u16(1);
    s.body(2);
    let first = s.face(&[&square(0.0)], [0.0, 0.0, 1.0], Pointer::New(BLUE));
    s.face(&[&square(1.0)], [0.0, 0.0, 1.0], Pointer::None);
    s.body_end(Pointer::Object(first + 1));
    let list = displaylist::parse(&s.bytes);
    assert_eq!(list.parts.len(), 1);
    assert_eq!(list.parts[0].bodies[0].color, Some(BLUE));
    assert_eq!(list.parts[0].bodies[0].faces[1].color, None);
    assert_eq!(read(&s.file()).unwrap().meshes.len(), 1);
}

#[test]
fn the_number_of_the_appearance_class_is_found() {
    for first in [42, 37, 60] {
        let scene = read(&part(first, Pointer::New(BLACK)).file()).unwrap();
        assert_eq!(scene.info.unresolved_colors, 0, "first {first}");
        assert_eq!(scene.meshes.iter().map(|m| m.color).collect::<Vec<_>>(), [color(BLUE), color(BLACK)], "first {first}");
    }
}

/// An assembly: a placed assembly `Arm-1` holding the part `Plate` twice (the second turned a
/// quarter about z and moved), and a part `Bolt` saved without its tessellation.
fn assembly() -> Stream {
    let mut s = Stream::new(42);
    s.u32s(&[0xFFFF_FFFF]);
    s.u16(3);
    s.component(true, "Arm-1@Robot", None, [0.0, 0.0, 5.0], "Arm", "C:\\cad\\Arm.SLDASM", Pointer::None);
    s.component(false, "Arm-1@Robot/Plate-1@Arm", None, [0.0, 0.0, 5.0], "Plate", "C:\\cad\\Plate.SLDPRT", Pointer::New(GREY));
    let plate = s.part(1);
    s.body(2);
    let face = s.face(&[&square(0.0)], [0.0, 0.0, 1.0], Pointer::New(BLUE));
    s.face(&[&square(1.0)], [0.0, 0.0, 1.0], Pointer::Object(face + 1));
    s.body_end(Pointer::None);
    s.u32s(&[0xFFFF_FFFF]);
    s.u16(0);
    // x to y, y to -x
    let quarter = [0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    s.component(false, "Arm-1@Robot/Plate-2@Arm", Some(quarter), [10.0, 0.0, 5.0], "Plate", "C:\\cad\\Plate.SLDPRT", Pointer::Object(plate));
    s.component(false, "Bolt-1@Robot", None, [0.0, 2.0, 0.0], "Bolt", "C:\\cad\\parts\\Bolt.SLDPRT", Pointer::None);
    s
}

#[test]
fn an_assembly_places_its_parts() {
    let scene = read(&assembly().file()).unwrap();
    assert_eq!((scene.info.kind, scene.info.components, scene.info.bodies, scene.info.faces), (Kind::Assembly, 3, 1, 2));
    assert_eq!(scene.info.missing, ["Bolt.SLDPRT"]);
    assert_eq!(scene.info.unresolved_colors, 0);
    assert_eq!(scene.meshes.len(), 2);
    let (first, second) = (&scene.meshes[0], &scene.meshes[1]);
    assert_eq!((first.name.as_str(), second.name.as_str()), ("Plate-1", "Plate-2"));
    assert_eq!((first.instance, second.instance), (0, 1));
    assert_eq!((first.color, first.triangle_count()), (color(BLUE), 4));
    // the same triangles, placed differently
    assert_eq!(first.positions, second.positions);
    assert_eq!(&first.world_positions()[..6], [0.0, 0.0, 5.0, 1.0, 0.0, 5.0]);
    assert_eq!(&second.world_positions()[..6], [10.0, 0.0, 5.0, 10.0, 1.0, 5.0]);
    assert_eq!(&second.world_normals()[..3], [0.0, 0.0, 1.0]);
    let prop = |name: &str| second.props.iter().find(|p| p.0 == name).map(|p| p.1.as_str());
    assert_eq!(prop("Component"), Some("Arm-1@Robot/Plate-2@Arm"));
    assert_eq!((prop("Part"), prop("File"), prop("Configuration")), (Some("Plate"), Some("Plate.SLDPRT"), Some("Default")));
}

#[test]
fn a_part_without_tessellation_is_asked_from_the_caller() {
    let bolt = part(42, Pointer::New(BLACK)).file();
    let mut asked = Vec::new();
    let scene = read_assembly(&assembly().file(), &mut |name| {
        asked.push(name.to_string());
        (name == "Bolt.SLDPRT").then(|| bolt.clone())
    })
    .unwrap();
    assert_eq!(asked, ["Bolt.SLDPRT"]);
    assert!(scene.info.missing.is_empty());
    let bolts: Vec<_> = scene.meshes.iter().filter(|m| m.name == "Bolt-1").collect();
    assert_eq!(bolts.len(), 2);
    assert_eq!((bolts[0].instance, bolts[0].color, bolts[1].color), (2, color(BLUE), color(BLACK)));
    assert_eq!(&bolts[0].world_positions()[..3], [0.0, 2.0, 1.0]);
    assert_eq!(scene.triangle_count(), 8 + 6);
}

#[test]
fn a_file_without_display_list_has_nothing_to_draw() {
    let empty = sldw_core::testing::zlb(&[], 4096);
    let file = sldw_core::testing::compound_file(&[(&format!("{DISPLAY_LISTS}__ZLB"), &empty)]);
    assert!(matches!(read(&file), Err(Error::NoGeometry(_))));
    let none = sldw_core::testing::compound_file(&[("Header2", b"header")]);
    assert_eq!(read(&none).unwrap_err(), Error::MissingStream(DISPLAY_LISTS.to_string()));
    assert_eq!(read(b"not a solidworks file").unwrap_err(), Error::NotACompoundFile);
}

#[test]
fn a_stream_cut_anywhere_reads_without_a_panic() {
    let stream = assembly().bytes;
    for cut in (0..stream.len()).step_by(3) {
        let list = displaylist::parse(&stream[..cut]);
        assert!(list.parts.len() <= 1 && list.components.len() <= 4, "cut at {cut}");
    }
    // and with bytes turned over
    for at in (0..stream.len()).step_by(11) {
        let mut damaged = stream.clone();
        damaged[at] ^= 0xFF;
        displaylist::parse(&damaged);
    }
}
