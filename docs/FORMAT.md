# What is known about the format

Everything here was worked out from files (see the clean-room rules in the README). "Seen" means
seen in the sample set: 110 files of one design, saved in 2016.

## Container

A part or assembly is a compound file ([MS-CFB]): storages and streams. The streams seen:

| Stream | What |
|---|---|
| `Contents/DisplayLists__ZLB` | The tessellation for display; read (below) |
| `Contents/Config-0-Partition` | The bodies of configuration 0 (Parasolid), compressed in a way of its own; not read |
| `Contents/Config-0-GhostPartition`, `Config-0-LWDATA`, `Config-0-ResolvedFeatures` | Not read |
| `Contents/Config-0`, `Contents/CMgr`, `Contents/Definition`, `Header2` | The document as an archive of objects; not read |
| `Contents/Config-0-MatesList` | Assemblies only; not read |
| `swXmlContents/COMPINSTANCETREE__ZLB` | Assemblies only: XML with every model, its references, their transforms relative to the parent, suppressed and hidden flags; not read (the display list has the places) |
| `swXmlContents/Features__ZLB`, `KeyWords__ZLB`, `Tables__ZLB` | XML; not read |
| `PreviewPNG`, `Preview` | A picture of the model as it was saved |
| `SummaryInformation`, `DocumentSummaryInformation`, `Config-0-Properties`, `ISolidWorksInformation` | Property sets; not read |
| `_DL_VERSION_6000/DLUpdateStamp`, `_MO_VERSION_6000/...` | Version stamps and history |

Files in the newer container (no compound file header) have not been seen.

## Compressed streams (`...__ZLB`)

```text
16 bytes   signature 23 1D D5 71 DA 81 48 A2 A8 58 98 B2 1B 89 EF 99
repeated:  u32 size of the content of the block (at most 0x100000)
           u32 size of the compressed bytes
           the compressed bytes: a zlib stream
```

An empty stream is the signature and eight zero bytes.

## The display list

An archive of objects, numbered as MFC archives number them: an object starts with a tag, either
a new class (`FF FF`, u16 version, u16 length, the name) or `0x8000 | n` for the class numbered
`n`. Classes and objects take the next number of one sequence, from 1, in the order they are
written. A pointer is a tag followed by the object, the number of an earlier object, or 0.
Numbers past 0x7FFE are written as `FF 7F` and a u32. Text is `FF FE FF`, a length (u8; `FF` and
a u16 for long text) and UTF-16.

All numbers are little endian; lengths are metres; Y is up.

The stream starts with bounds, the environment (`uiUserModelEnv_c`), line fonts
(`uiLineFontMgr_c`, `uiLFConfig_c`), lights (`moDirectionLight_c`) and the appearance of the
document (`moVisualProperties_c`). In most samples the appearance class is number 42, in some 41.

### Appearance: `moVisualProperties_c`

```text
u8 red, u8 green, u8 blue, u8 0
u32 0, u32 (0 or C0C0C0)
text, text (material name), doubles, flags, more text (texture and material files) ...
```

Only the colour is read.

### Part file

```text
... FF FF FF FF
u16 number of bodies
bodies
```

### Body: `uoTempBodyTessData_c`

```text
u16 number of faces
faces
u32 0, u32, u32 0, u32 1, i32 -1
uoBodyPropInfo_c: pointer to the appearance of the body (0: none), u32, u32 6, ...
                  in part files: ... i32 -1, an appearance (the material's)
                  (bodies with several display states have a count and that many of these, numbered)
```

### Face: `uoTempFaceTessData_c`

Arrays are headed by `u32 element size, u32 type (8: integers, 100: floats), u32 2, u32 count`.

```text
u32 number of triangles, u32 number of strips
array u32   the number of vertices of each triangle strip
array 3 f32 the positions of the strip vertices
array 3 f32 their normals
array u32   (vertices + triangles entries; about edges)
array u32   (one per strip)
array u8    (vertices + triangles entries)
u32 0, u32 0, u32 1
10 doubles  bounds: a point, the maximum, the minimum, a radius
10 u32      flags and identifiers
array 8 bytes (empty in the samples), u32 1, array 3 f32, array 3 f32 (empty)
u32 0
u32 1 when texture coordinates follow, [array f32: the texture coordinates]
u32 count, then that many times: i32 (-1, or 1, 2, ...), pointer to an appearance
u32, u32, u32 flag, [when flag is 1: i32 -1, u32 2], u32
```

Most faces have one pointer (numbered -1); some have two or three, numbered from 1 (one per
display state, it seems). The reader takes the first that points at an appearance.

The triangles of a strip of `n` vertices are `(k, k+1, k+2)` for even `k` and `(k+1, k, k+2)`
for odd `k`; which side is out is taken from the normals.

### Assembly

A tree of placed components. `uoTempAssemblySHDData_c` is a placed part,
`uoTempSubAssemblySHDData_c` a placed assembly, followed by a count and its components.

```text
text        path of the component: Arm-1@Robot/Bracket-2@Arm
u8          1 when a rotation follows
[9 doubles  rotation, rows; a point p of the part lies at p . rotation (p a row vector)]
3 doubles   translation, in the coordinates of the file (not of the parent assembly)
double      scale
u8, 10 doubles (bounds in the coordinates of the part), u16
text        configuration
12 bytes
text        name of the document
text        path of its file on the computer that saved the assembly
60 bytes
text, text, text (the name again)
12 bytes
pointer     to uoTempPartTessData_c: the tessellation of the part (0 for a placed assembly)
```

`uoTempPartTessData_c` is `u16 number of bodies` and the bodies. A part placed more than once
is written once; the other components point at it by its number.

### How the reader walks it

The reader does not know the layout of every class, so it cannot read the stream object by
object. It finds the faces by the shape of their arrays and searches the bytes between them for
the tags of the classes above, accepting a tag only when what follows fits (a text after a
component, a face count and a face after a body, a colour and a text after an appearance). Every
accepted tag and class takes its number, so that pointers land. The number of the first class
it meets, the appearance class, is found by trying: the right one makes the tag before the
second face the tag of the face class.

In the sample set (110 files, 47 533 faces) every pointer to an appearance lands. In the largest
assembly (385 placed parts) three placed parts, all in a configuration with a name of its own,
have no tessellation in the assembly; the reader asks the caller for their part files, whose
display list is that of the configuration the part was saved in.
