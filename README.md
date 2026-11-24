# solidworks-dll

Native library that reads SolidWorks parts (`.sldprt`) and assemblies (`.sldasm`) without
SolidWorks, for use by [open-bim-suite](https://github.com/OpenAEC-Foundation/open-bim-suite) and
other tools.

**Status:** pre-alpha. Parts and assemblies saved as compound files open as coloured
triangle meshes, read from the display list SolidWorks saves with the model. All 110 files of
the sample set open (87 parts, 23 assemblies).

## Scope

- Input: `.sldprt` and `.sldasm` files that are compound files (the "OLE2" container), as
  SolidWorks wrote them up to at least 2016.
- Output: triangle meshes, each with a name, a colour, its place in the assembly and
  properties (component path, part, file, configuration), exposed through Rust and a command
  line tool.
- `sldw_read::read(&[u8])` works on bytes, so the reader also runs in the browser (wasm). An
  assembly that was saved without the tessellation of some of its parts asks the caller for
  those part files: `sldw_read::read_assembly(&[u8], fetch)`.

### What is read

- The display list (`Contents/DisplayLists`): the triangle strips, with normals, of every face
  of every body, as SolidWorks tessellated them for the configuration and display state that
  were active when the file was saved.
- The colour of each face: its own appearance, or that of its body.
- In an assembly: every placed part with its place (rotation, translation, scale), the path of
  the component through the sub-assemblies, its configuration, and the name and file of its
  part. A part placed many times is tessellated once.

### What is not read

- Files in the newer SolidWorks container (no compound file header): they are refused with a
  message that says so. No sample of them has been seen yet.
- The exact geometry (the Parasolid bodies in `Contents/Config-*-Partition`), so no curved
  surfaces finer than SolidWorks tessellated them, no edges, no measurements on exact faces.
- Other configurations and display states than the one the file was saved in.
- Textures and the other properties of an appearance (shininess, transparency): only its
  colour.
- Hidden and suppressed components are not told apart: what the display list holds is drawn.
- A part an assembly holds no tessellation of is drawn from its own file as that file was
  saved, which may be another configuration than the assembly places.
- The feature tree, sketches, dimensions, mates, custom properties, drawings (`.slddrw`).

## Crates

| Crate | Role |
|---|---|
| `sldw-core` | Container layer: the compound file, its streams, the compressed (`__ZLB`) streams |
| `sldw-read` | The display list to meshes: `read`, `read_assembly`, the `model` |
| `sldw-cli` | `sldw` command: `info` (what a file holds), `obj` (its meshes as OBJ), `unpack` (its streams) |

See [docs/FORMAT.md](docs/FORMAT.md) for what is known about the format.

```text
cargo run -p sldw-cli -- info Part.SLDPRT
cargo run -p sldw-cli -- obj Assembly.SLDASM out.obj
```

## Clean-room rules

The reader is built only from black-box analysis of files.

- Never open, disassemble, or decompile SolidWorks, its add-ins or other third-party binaries;
  no memory dumps.
- Never consult third-party parsers of these files, SDK headers that describe the file format,
  or leaked or licensed format documentation.
- Contributors who have seen such material do not write reader code.
- Public documents that are not about the SolidWorks format are used and named where they are:
  Microsoft's description of the compound file ([MS-CFB]), the zlib format (RFC 1950), and the
  way MFC archives number classes and objects, which the display list follows.

## Test data

Never commit `.sldprt`, `.sldasm` or `.slddrw` files, or anything derived from them (exports,
unpacked streams, screenshots). Sample files live outside the repository; `.gitignore` refuses
the file types. Unit tests write their own bytes. The corpus test runs over a folder of samples
when `SLDW_SAMPLES` names it, and says how many of the files open:

```text
SLDW_SAMPLES=C:\sldw-samples cargo test -p sldw-read --test corpus -- --nocapture
```

## Legal

SolidWorks is a registered trademark of Dassault Systèmes SolidWorks Corporation. This project
is not affiliated with or endorsed by it. Before any public release: legal review and a patent
freedom-to-operate check.
