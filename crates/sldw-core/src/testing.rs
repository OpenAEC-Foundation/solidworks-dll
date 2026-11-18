//! Builders of small files for tests, here and in the crates on top: a compound file from a
//! list of streams, and a compressed stream from its content. They write what the readers of
//! this crate read; they are no general writers.

use std::io::Write;

const SECTOR: usize = 512;
const MINI_SECTOR: usize = 64;
const MINI_CUTOFF: usize = 4096;
const END: u32 = 0xFFFF_FFFE;
const NONE: u32 = 0xFFFF_FFFF;

struct Node {
    name: String,
    kind: u8,
    right: u32,
    child: u32,
    start: u32,
    size: u32,
    data: Vec<u8>,
}

/// The entry `name` in storage `parent`, added when it is not there yet.
fn child(nodes: &mut Vec<Node>, parent: usize, name: &str, kind: u8) -> usize {
    let (mut at, mut last) = (nodes[parent].child, None);
    while at != NONE {
        if nodes[at as usize].name == name {
            return at as usize;
        }
        last = Some(at as usize);
        at = nodes[at as usize].right;
    }
    nodes.push(Node { name: name.to_string(), kind, right: NONE, child: NONE, start: END, size: 0, data: Vec::new() });
    let index = nodes.len() - 1;
    match last {
        Some(last) => nodes[last].right = index as u32,
        None => nodes[parent].child = index as u32,
    }
    index
}

/// A compound file with these streams (path with `/` between storages, content). Streams under
/// 4096 bytes go to the mini stream, as in real files.
pub fn compound_file(streams: &[(&str, &[u8])]) -> Vec<u8> {
    let mut nodes = vec![Node { name: "Root Entry".into(), kind: 5, right: NONE, child: NONE, start: END, size: 0, data: Vec::new() }];
    for (path, data) in streams {
        let parts: Vec<&str> = path.split('/').collect();
        let mut at = 0;
        for (k, part) in parts.iter().enumerate() {
            let last = k + 1 == parts.len();
            at = child(&mut nodes, at, part, if last { 2 } else { 1 });
            if last {
                nodes[at].data = data.to_vec();
                nodes[at].size = data.len() as u32;
            }
        }
    }
    // the small streams, one after the other in the mini stream
    let (mut mini, mut mini_fat) = (Vec::new(), Vec::<u32>::new());
    for node in nodes.iter_mut().filter(|n| n.kind == 2 && !n.data.is_empty() && n.data.len() < MINI_CUTOFF) {
        let (first, count) = (mini_fat.len() as u32, node.data.len().div_ceil(MINI_SECTOR));
        node.start = first;
        mini_fat.extend((0..count as u32).map(|k| if k + 1 == count as u32 { END } else { first + k + 1 }));
        mini.append(&mut node.data);
        mini.resize(mini_fat.len() * MINI_SECTOR, 0);
    }
    let mut mini_fat_bytes: Vec<u8> = mini_fat.iter().flat_map(|v| v.to_le_bytes()).collect();
    mini_fat_bytes.resize(mini_fat_bytes.len().div_ceil(SECTOR) * SECTOR, 0xFF);
    nodes[0].size = mini.len() as u32;
    // sectors: the allocation table, the directory, the mini table, the mini stream, the
    // large streams
    let sectors = |bytes: usize| bytes.div_ceil(SECTOR);
    let directory_sectors = nodes.len().div_ceil(4);
    let large: Vec<usize> = (0..nodes.len()).filter(|&k| nodes[k].kind == 2 && !nodes[k].data.is_empty()).collect();
    let content = directory_sectors + sectors(mini_fat_bytes.len()) + sectors(mini.len()) + large.iter().map(|&k| sectors(nodes[k].data.len())).sum::<usize>();
    let fat_sectors = (1..).find(|f| f * (SECTOR / 4) >= f + content).unwrap();
    assert!(fat_sectors <= 109, "too large for this builder");
    let mut fat = vec![0xFFFF_FFFDu32; fat_sectors];
    let mut body = Vec::new();
    // a chain of sectors for these bytes; where it starts
    let place = |bytes: &[u8], fat: &mut Vec<u32>, body: &mut Vec<u8>| -> u32 {
        if bytes.is_empty() {
            return END;
        }
        let (first, count) = (fat.len() as u32, sectors(bytes.len()) as u32);
        fat.extend((0..count).map(|k| if k + 1 == count { END } else { first + k + 1 }));
        body.extend_from_slice(bytes);
        body.resize(body.len().div_ceil(SECTOR) * SECTOR, 0);
        first
    };
    let directory_start = place(&vec![0; directory_sectors * SECTOR], &mut fat, &mut body);
    let mini_fat_start = place(&mini_fat_bytes, &mut fat, &mut body);
    nodes[0].start = place(&mini, &mut fat, &mut body);
    for k in large {
        let data = std::mem::take(&mut nodes[k].data);
        nodes[k].start = place(&data, &mut fat, &mut body);
    }
    // the directory, now that every stream has its place
    let mut directory = Vec::new();
    for node in &nodes {
        let mut entry = [0u8; 128];
        let units: Vec<u16> = node.name.encode_utf16().collect();
        for (k, unit) in units.iter().enumerate() {
            entry[2 * k..2 * k + 2].copy_from_slice(&unit.to_le_bytes());
        }
        entry[0x40..0x42].copy_from_slice(&((units.len() as u16 + 1) * 2).to_le_bytes());
        entry[0x42] = node.kind;
        entry[0x43] = 1;
        entry[0x44..0x48].copy_from_slice(&NONE.to_le_bytes());
        entry[0x48..0x4C].copy_from_slice(&node.right.to_le_bytes());
        entry[0x4C..0x50].copy_from_slice(&node.child.to_le_bytes());
        entry[0x74..0x78].copy_from_slice(&node.start.to_le_bytes());
        entry[0x78..0x7C].copy_from_slice(&node.size.to_le_bytes());
        directory.extend_from_slice(&entry);
    }
    while directory.len() < directory_sectors * SECTOR {
        let mut entry = [0u8; 128];
        entry[0x44..0x50].fill(0xFF);
        directory.extend_from_slice(&entry);
    }
    let at = (directory_start as usize - fat_sectors) * SECTOR;
    body[at..at + directory.len()].copy_from_slice(&directory);

    let mut file = vec![0u8; SECTOR];
    file[..8].copy_from_slice(&crate::cfb::MAGIC);
    file[0x18..0x20].copy_from_slice(&[0x3E, 0, 3, 0, 0xFE, 0xFF, 9, 0]);
    file[0x20] = 6;
    file[0x2C..0x30].copy_from_slice(&(fat_sectors as u32).to_le_bytes());
    file[0x30..0x34].copy_from_slice(&directory_start.to_le_bytes());
    file[0x38..0x3C].copy_from_slice(&(MINI_CUTOFF as u32).to_le_bytes());
    file[0x3C..0x40].copy_from_slice(&mini_fat_start.to_le_bytes());
    file[0x40..0x44].copy_from_slice(&(sectors(mini_fat_bytes.len()) as u32).to_le_bytes());
    file[0x44..0x48].copy_from_slice(&END.to_le_bytes());
    file[0x4C..].fill(0xFF);
    for k in 0..fat_sectors {
        file[0x4C + 4 * k..0x50 + 4 * k].copy_from_slice(&(k as u32).to_le_bytes());
    }
    fat.resize(fat_sectors * SECTOR / 4, NONE);
    file.extend(fat.iter().flat_map(|v| v.to_le_bytes()));
    file.extend_from_slice(&body);
    file
}

/// A compressed stream holding `content`, in blocks of at most `block` bytes of it.
pub fn zlb(content: &[u8], block: usize) -> Vec<u8> {
    let mut out = crate::zlb::SIGNATURE.to_vec();
    for chunk in content.chunks(block.max(1)) {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(chunk).expect("writing to memory");
        let packed = encoder.finish().expect("writing to memory");
        out.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
        out.extend_from_slice(&(packed.len() as u32).to_le_bytes());
        out.extend_from_slice(&packed);
    }
    if content.is_empty() {
        out.extend_from_slice(&[0; 8]);
    }
    out
}
