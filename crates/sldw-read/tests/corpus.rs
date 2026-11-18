//! Runs over the sample set: every `.sldprt` and `.sldasm` under `SLDW_SAMPLES`, and says how
//! many of them open. Sample files are kept out of the repository (see the README); without
//! `SLDW_SAMPLES` the test passes without doing anything.
//!
//! ```text
//! SLDW_SAMPLES=C:\sldw-samples cargo test -p sldw-read --test corpus -- --nocapture
//! ```

use std::path::{Path, PathBuf};

fn collect(folder: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    for path in entries.flatten().map(|e| e.path()) {
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("sldprt") || e.eq_ignore_ascii_case("sldasm")) {
            out.push(path);
        }
    }
}

#[test]
fn the_samples_open() {
    let Some(folder) = std::env::var_os("SLDW_SAMPLES") else { return };
    let mut files = Vec::new();
    collect(Path::new(&folder), &mut files);
    files.sort();
    assert!(!files.is_empty(), "no .sldprt or .sldasm under SLDW_SAMPLES");
    let (mut opened, mut triangles, mut unresolved, mut missing, mut faces) = (0, 0, 0, 0, 0);
    for file in &files {
        let name = file.file_name().unwrap_or_default().to_string_lossy();
        let data = std::fs::read(file).expect("a sample file");
        // parts an assembly was saved without are taken from the folder of the assembly
        let mut fetch = |part: &str| {
            let wanted = part.to_lowercase();
            let found = files.iter().find(|f| f.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase() == wanted))?;
            std::fs::read(found).ok()
        };
        match sldw_read::read_assembly(&data, &mut fetch) {
            Ok(scene) => {
                opened += 1;
                triangles += scene.triangle_count();
                unresolved += scene.info.unresolved_colors;
                missing += scene.info.missing.len();
                faces += scene.info.faces;
                println!(
                    "ok    {name}: {} meshes, {} triangles, {} placed parts, {} missing, {} colours not followed",
                    scene.meshes.len(),
                    scene.triangle_count(),
                    scene.info.components,
                    scene.info.missing.len(),
                    scene.info.unresolved_colors
                );
                for mesh in &scene.meshes {
                    assert_eq!(mesh.positions.len(), mesh.normals.len(), "{name}");
                    assert!(mesh.indices.iter().all(|&i| (i as usize) < mesh.positions.len() / 3), "{name}");
                    assert!(mesh.positions.iter().chain(&mesh.transform.map(|v| v as f32)).all(|v| v.is_finite()), "{name}");
                }
            }
            Err(error) => println!("FAIL  {name}: {error}"),
        }
    }
    println!(
        "{opened} of {} files open; {triangles} triangles; {faces} faces, {unresolved} of them with a colour that could not be followed; {missing} placed parts without tessellation",
        files.len()
    );
    assert_eq!(opened, files.len(), "not every sample opens");
}
