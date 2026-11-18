//! The model a file is read to: triangle meshes, each with its name, colour, place and
//! properties.
//!
//! Lengths are metres and the axes are those of the file: SolidWorks draws Y up.

/// What a file holds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scene {
    pub meshes: Vec<Mesh>,
    pub info: Info,
}

/// What kind of document a file is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Kind {
    /// A part: bodies of its own.
    #[default]
    Part,
    /// An assembly: parts and other assemblies, placed.
    Assembly,
}

/// What was found in a file, and what was left out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Info {
    pub kind: Kind,
    /// The placed parts of an assembly (0 for a part).
    pub components: usize,
    /// The bodies read (those of each part once, however often it is placed).
    pub bodies: usize,
    /// The faces read, likewise.
    pub faces: usize,
    /// The files of placed parts whose tessellation is neither in the assembly nor was given by
    /// the caller: those parts are missing from the meshes. One entry per placed part.
    pub missing: Vec<String>,
    /// Faces whose appearance is a reference that could not be followed: drawn in the colour of
    /// their body, or the default colour.
    pub unresolved_colors: usize,
}

/// The colour of faces without an appearance of their own: the grey-blue SolidWorks gives a
/// new part.
pub const DEFAULT_COLOR: [f32; 3] = [202.0 / 255.0, 209.0 / 255.0, 238.0 / 255.0];

/// The identity transform.
pub const IDENTITY: [f64; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

/// Triangles of one colour: of a body of a part, or of a placed part of an assembly.
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    /// The name of the placed part (`Bracket-2`), or `Body 1` in a part.
    pub name: String,
    /// xyz per vertex, in the coordinates of the part.
    pub positions: Vec<f32>,
    /// xyz per vertex, unit length, as the file has them.
    pub normals: Vec<f32>,
    /// Triangle list, counter-clockwise seen from outside.
    pub indices: Vec<u32>,
    /// sRGB, 0..1.
    pub color: [f32; 3],
    /// Where the part is placed in the assembly: a 4 x 4 matrix, row by row, that takes a
    /// column vector in the coordinates of the part to the coordinates of the file. The
    /// identity in a part.
    pub transform: [f64; 16],
    /// Name and value: `Component` (the path through the assemblies), `Part`, `File`,
    /// `Configuration`.
    pub props: Vec<(String, String)>,
    /// The placed part these triangles belong to: the meshes of one placed part (one per
    /// colour) share this number. The body number (from 0) in a part.
    pub instance: u32,
}

impl Default for Mesh {
    fn default() -> Self {
        Mesh { name: String::new(), positions: Vec::new(), normals: Vec::new(), indices: Vec::new(), color: DEFAULT_COLOR, transform: IDENTITY, props: Vec::new(), instance: 0 }
    }
}

impl Mesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// The positions in the coordinates of the file: `transform` applied.
    pub fn world_positions(&self) -> Vec<f32> {
        let m = &self.transform;
        self.positions
            .chunks_exact(3)
            .flat_map(|p| {
                let (x, y, z) = (f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
                [0, 4, 8].map(|r| (m[r] * x + m[r + 1] * y + m[r + 2] * z + m[r + 3]) as f32)
            })
            .collect()
    }

    /// The normals in the coordinates of the file: turned with `transform`, unit length.
    pub fn world_normals(&self) -> Vec<f32> {
        let m = &self.transform;
        self.normals
            .chunks_exact(3)
            .flat_map(|n| {
                let (x, y, z) = (f64::from(n[0]), f64::from(n[1]), f64::from(n[2]));
                let v = [0, 4, 8].map(|r| m[r] * x + m[r + 1] * y + m[r + 2] * z);
                let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                v.map(|c| if length > 0.0 { (c / length) as f32 } else { 0.0 })
            })
            .collect()
    }
}

impl Scene {
    pub fn triangle_count(&self) -> usize {
        self.meshes.iter().map(Mesh::triangle_count).sum()
    }
}
