//! `sldw`: what a SolidWorks file holds, and its meshes as OBJ for checking in another viewer.
//!
//! ```text
//! sldw info <file>              streams, and what the display list holds
//! sldw obj <file> <out.obj>     the meshes, placed, as Wavefront OBJ (with <out>.mtl)
//! sldw unpack <file> <folder>   every stream as a file, compressed ones inflated
//! ```
//!
//! For an assembly, parts saved without their tessellation are looked for next to the file.

use sldw_core::{zlb, Document};
use sldw_read::model::{Kind, Scene};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let result = match arguments[..] {
        ["info", file] => info(Path::new(file)),
        ["obj", file, out] => obj(Path::new(file), Path::new(out)),
        ["unpack", file, folder] => unpack(Path::new(file), Path::new(folder)),
        _ => Err("usage: sldw info <file> | sldw obj <file> <out.obj> | sldw unpack <file> <folder>".to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn bytes(file: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))
}

/// Reads a file; the parts an assembly asks for are taken from its folder, whatever the case
/// of their names.
fn scene(file: &Path) -> Result<Scene, String> {
    let data = bytes(file)?;
    let folder = file.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut fetch = |name: &str| {
        let wanted = name.to_lowercase();
        let found: PathBuf = std::fs::read_dir(if folder.as_os_str().is_empty() { Path::new(".") } else { &folder })
            .ok()?
            .flatten()
            .map(|e| e.path())
            .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase() == wanted))?;
        std::fs::read(found).ok()
    };
    sldw_read::read_assembly(&data, &mut fetch).map_err(|e| format!("{}: {e}", file.display()))
}

fn info(file: &Path) -> Result<(), String> {
    let data = bytes(file)?;
    let document = Document::open(&data).map_err(|e| format!("{}: {e}", file.display()))?;
    println!("{} ({} bytes)", file.display(), data.len());
    println!("streams:");
    for (path, size) in document.streams() {
        let inflated = document.raw(path).ok().filter(|raw| zlb::is_compressed(raw)).map(|raw| zlb::inflate(&raw).map(|c| c.len()));
        match inflated {
            Some(Ok(length)) => println!("  {path:<48} {size:>10}  inflates to {length}"),
            Some(Err(error)) => println!("  {path:<48} {size:>10}  {error}"),
            None => println!("  {path:<48} {size:>10}"),
        }
    }
    let list = sldw_read::display_list(&data).map_err(|e| format!("{}: {e}", file.display()))?;
    let bodies = || list.parts.iter().flat_map(|p| &p.bodies);
    let triangles: usize = bodies().flat_map(|b| &b.faces).map(|f| f.positions.len() - 2 * f.strips.len()).sum();
    println!(
        "display list: {} parts, {} bodies, {} faces, {} triangles; {} appearance pointers not followed",
        list.parts.len(),
        bodies().count(),
        bodies().map(|b| b.faces.len()).sum::<usize>(),
        triangles,
        list.unresolved_colors
    );
    if !list.components.is_empty() {
        println!("components:");
        for component in &list.components {
            let depth = component.path.matches('/').count();
            let what = match (component.assembly, component.part) {
                (true, _) => "assembly".to_string(),
                (false, Some(part)) => format!("part {part}"),
                (false, None) => "no tessellation".to_string(),
            };
            println!("  {:indent$}{}  [{}]  {}  {what}", "", component.instance_name(), component.configuration, component.file_name(), indent = depth * 2);
        }
    }
    match scene(file) {
        Ok(scene) => {
            let kind = if scene.info.kind == Kind::Assembly { "assembly" } else { "part" };
            println!("read as {kind}: {} meshes, {} triangles, {} placed parts, {} without tessellation", scene.meshes.len(), scene.triangle_count(), scene.info.components, scene.info.missing.len());
            let (mut low, mut high) = ([f32::MAX; 3], [f32::MIN; 3]);
            for point in scene.meshes.iter().flat_map(|m| m.world_positions()).collect::<Vec<_>>().chunks_exact(3) {
                for axis in 0..3 {
                    (low[axis], high[axis]) = (low[axis].min(point[axis]), high[axis].max(point[axis]));
                }
            }
            println!("bounds (m): {low:?} to {high:?}");
        }
        Err(message) => println!("not read: {message}"),
    }
    Ok(())
}

fn obj(file: &Path, out: &Path) -> Result<(), String> {
    let scene = scene(file)?;
    let material_file = out.with_extension("mtl");
    let (mut text, mut materials, mut colors) = (String::new(), String::new(), Vec::<[u8; 3]>::new());
    let _ = writeln!(text, "mtllib {}", material_file.file_name().map(|n| n.to_string_lossy()).unwrap_or_default());
    let mut offset = 1;
    for mesh in &scene.meshes {
        let color = mesh.color.map(|v| (v * 255.0).round() as u8);
        let material = colors.iter().position(|c| *c == color).unwrap_or_else(|| {
            let _ = writeln!(materials, "newmtl color{}\nKd {} {} {}", colors.len(), mesh.color[0], mesh.color[1], mesh.color[2]);
            colors.push(color);
            colors.len() - 1
        });
        let _ = writeln!(text, "o {}\nusemtl color{material}", mesh.name.replace(' ', "_"));
        let (positions, normals) = (mesh.world_positions(), mesh.world_normals());
        positions.chunks_exact(3).for_each(|p| {
            let _ = writeln!(text, "v {} {} {}", p[0], p[1], p[2]);
        });
        normals.chunks_exact(3).for_each(|n| {
            let _ = writeln!(text, "vn {} {} {}", n[0], n[1], n[2]);
        });
        for t in mesh.indices.chunks_exact(3) {
            let [a, b, c] = [t[0], t[1], t[2]].map(|v| v as usize + offset);
            let _ = writeln!(text, "f {a}//{a} {b}//{b} {c}//{c}");
        }
        offset += positions.len() / 3;
    }
    std::fs::write(out, text).map_err(|e| format!("{}: {e}", out.display()))?;
    std::fs::write(&material_file, materials).map_err(|e| format!("{}: {e}", material_file.display()))?;
    println!("{}: {} meshes, {} triangles", out.display(), scene.meshes.len(), scene.triangle_count());
    Ok(())
}

fn unpack(file: &Path, folder: &Path) -> Result<(), String> {
    let data = bytes(file)?;
    let document = Document::open(&data).map_err(|e| format!("{}: {e}", file.display()))?;
    for (path, _) in document.streams() {
        // stream names are data: keep them inside the folder
        let name: String = path.chars().map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ') { c } else { '_' }).collect();
        let content = document.content(path).or_else(|_| document.raw(path)).map_err(|e| format!("{path}: {e}"))?;
        std::fs::create_dir_all(folder).and_then(|()| std::fs::write(folder.join(&name), content)).map_err(|e| format!("{}: {e}", folder.join(&name).display()))?;
    }
    Ok(())
}
