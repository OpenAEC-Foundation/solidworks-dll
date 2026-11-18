//! Reader of SolidWorks parts (`.sldprt`) and assemblies (`.sldasm`).
//!
//! What is read is the display list of the file: the triangles SolidWorks itself saved for
//! showing the model ([`displaylist`]). The exact geometry (the Parasolid bodies), the feature
//! tree, sketches, dimensions and configurations other than the one last shown are not read.
//!
//! [`read`] works on the bytes of one file. An assembly normally holds the tessellation of all
//! its parts; where it does not (a part saved lightweight), [`read_assembly`] asks the caller
//! for the bytes of the part file. Nothing here touches the file system, so the reader also
//! runs in the browser.

#![forbid(unsafe_code)]

pub mod displaylist;
pub mod model;

use displaylist::{Body, DisplayList};
use model::{Kind, Mesh, Scene};
pub use sldw_core::Error;
use std::collections::HashMap;

/// The stream of the display list (stored compressed, as `...__ZLB`).
pub const DISPLAY_LISTS: &str = "Contents/DisplayLists";

/// Reads a part or an assembly from the bytes of its file. Parts of an assembly whose
/// tessellation is not in the assembly are listed in `Scene::info.missing`.
pub fn read(data: &[u8]) -> Result<Scene, Error> {
    read_assembly(data, &mut |_| None)
}

/// Reads a part or an assembly. `fetch` is asked for the bytes of a part file, by its file name
/// (`Bracket.SLDPRT`, without folders), when the assembly does not hold the tessellation of
/// that part; it answers `None` when it does not have the file.
pub fn read_assembly(data: &[u8], fetch: &mut dyn FnMut(&str) -> Option<Vec<u8>>) -> Result<Scene, Error> {
    let list = display_list(data)?;
    let mut scene = Scene::default();
    scene.info.unresolved_colors = list.unresolved_colors;
    let placed: Vec<_> = list.components.iter().filter(|c| !c.assembly).collect();
    if placed.is_empty() {
        let bodies = list.parts.iter().flat_map(|p| &p.bodies);
        for (number, body) in bodies.enumerate() {
            scene.info.bodies += 1;
            scene.info.faces += body.faces.len();
            for shape in shapes(std::slice::from_ref(body)) {
                scene.meshes.push(Mesh { name: format!("Body {}", number + 1), instance: number as u32, ..shape.mesh() });
            }
        }
    } else {
        scene.info.kind = Kind::Assembly;
        scene.info.components = placed.len();
        scene.info.bodies = list.parts.iter().map(|p| p.bodies.len()).sum();
        scene.info.faces = list.parts.iter().flat_map(|p| &p.bodies).map(|b| b.faces.len()).sum();
        let own: Vec<Vec<Shape>> = list.parts.iter().map(|p| shapes(&p.bodies)).collect();
        // the parts asked from the caller, by file name: each asked once
        let mut fetched: HashMap<String, Option<Vec<Shape>>> = HashMap::new();
        for (number, component) in placed.iter().enumerate() {
            let shapes = match component.part {
                Some(part) => Some(&own[part]),
                None => fetched
                    .entry(component.file_name().to_lowercase())
                    .or_insert_with(|| {
                        let list = display_list(&fetch(component.file_name())?).ok()?;
                        let bodies: Vec<Body> = list.parts.into_iter().flat_map(|p| p.bodies).collect();
                        Some(shapes(&bodies)).filter(|s| !s.is_empty())
                    })
                    .as_ref(),
            };
            let Some(shapes) = shapes else {
                scene.info.missing.push(component.file_name().to_string());
                continue;
            };
            let file = component.file_name().to_string();
            let props = [("Component", &component.path), ("Part", &component.name), ("File", &file), ("Configuration", &component.configuration)];
            let props: Vec<(String, String)> = props.iter().filter(|(_, value)| !value.is_empty()).map(|(name, value)| (name.to_string(), value.to_string())).collect();
            for shape in shapes {
                scene.meshes.push(Mesh { name: component.instance_name().to_string(), transform: component.transform.matrix(), props: props.clone(), instance: number as u32, ..shape.mesh() });
            }
        }
    }
    if scene.meshes.is_empty() {
        return Err(Error::NoGeometry(if scene.info.missing.is_empty() { "the file was saved without its display list" } else { "the assembly was saved without the display lists of its parts" }));
    }
    Ok(scene)
}

/// The display list of a file, from its bytes.
pub fn display_list(data: &[u8]) -> Result<DisplayList, Error> {
    let document = sldw_core::Document::open(data)?;
    Ok(displaylist::parse(&document.content(DISPLAY_LISTS)?))
}

/// The triangles of one colour of some bodies.
#[derive(Default)]
struct Shape {
    color: Option<[u8; 3]>,
    positions: Vec<f32>,
    normals: Vec<f32>,
    indices: Vec<u32>,
}

impl Shape {
    fn mesh(&self) -> Mesh {
        let color = self.color.map_or(model::DEFAULT_COLOR, |c| c.map(|v| f32::from(v) / 255.0));
        Mesh { positions: self.positions.clone(), normals: self.normals.clone(), indices: self.indices.clone(), color, ..Mesh::default() }
    }
}

fn subtract(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The faces of these bodies as triangle lists, one per colour, in the order the colours are
/// first met. A face without a colour takes that of its body.
fn shapes(bodies: &[Body]) -> Vec<Shape> {
    let mut out: Vec<Shape> = Vec::new();
    for body in bodies {
        for face in &body.faces {
            let color = face.color.or(body.color);
            let shape = match out.iter().position(|s| s.color == color) {
                Some(k) => &mut out[k],
                None => {
                    out.push(Shape { color, ..Shape::default() });
                    out.last_mut().expect("just added")
                }
            };
            let base = (shape.positions.len() / 3) as u32;
            shape.positions.extend(face.positions.iter().flatten());
            let own_normals = face.normals.len() == face.positions.len();
            if own_normals {
                shape.normals.extend(face.normals.iter().flatten());
            } else {
                shape.normals.resize(shape.positions.len(), 0.0);
            }
            let mut first = 0usize;
            for &length in &face.strips {
                let length = length as usize;
                for k in 0..length.saturating_sub(2) {
                    // every second triangle of a strip runs the other way round
                    let mut corners = if k % 2 == 0 { [first + k, first + k + 1, first + k + 2] } else { [first + k + 1, first + k, first + k + 2] };
                    let [a, b, c] = corners.map(|v| face.positions[v]);
                    let normal = cross(subtract(b, a), subtract(c, a));
                    if normal == [0.0; 3] {
                        continue;
                    }
                    if own_normals {
                        // the normals of the file say which side is out
                        let outward = corners.iter().fold([0.0; 3], |sum, &v| [sum[0] + face.normals[v][0], sum[1] + face.normals[v][1], sum[2] + face.normals[v][2]]);
                        if dot(normal, outward) < 0.0 {
                            corners.swap(1, 2);
                        }
                    } else {
                        for v in corners {
                            let at = (base as usize + v) * 3;
                            for (axis, value) in normal.iter().enumerate() {
                                shape.normals[at + axis] += value;
                            }
                        }
                    }
                    shape.indices.extend(corners.map(|v| base + v as u32));
                }
                first += length;
            }
            if !own_normals {
                for normal in shape.normals[base as usize * 3..].chunks_exact_mut(3) {
                    let length = dot([normal[0], normal[1], normal[2]], [normal[0], normal[1], normal[2]]).sqrt();
                    if length > 0.0 {
                        normal.iter_mut().for_each(|v| *v /= length);
                    }
                }
            }
        }
    }
    out.retain(|s| !s.indices.is_empty());
    out
}

#[cfg(test)]
mod tests;
