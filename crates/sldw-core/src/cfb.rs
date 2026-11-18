//! The compound file (the "OLE2" structured storage container): sectors chained by a file
//! allocation table, a directory of storages and streams, and a mini stream for small streams.
//! Read from the public description of the format ([MS-CFB]).
//!
//! Every chain is walked with a bound, so a damaged or hostile file cannot loop for ever, and
//! no stream is larger than the file that holds it.

use crate::Error;

/// The first eight bytes of a compound file.
pub const MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

const END_OF_CHAIN: u32 = 0xFFFF_FFFE;
const NO_STREAM: u32 = 0xFFFF_FFFF;
/// More directory entries than any real file has.
const MAX_ENTRIES: usize = 1 << 20;

/// A stream of the file.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Its path: the names of its storages and its own, separated by `/`.
    pub path: String,
    /// Its size in bytes.
    pub size: u64,
    start: u32,
}

/// An opened compound file. Keeps the bytes it needs: the tables and the mini stream; streams
/// are copied out on demand.
pub struct CompoundFile {
    data: Vec<u8>,
    sector: usize,
    mini_sector: usize,
    mini_cutoff: u64,
    fat: Vec<u32>,
    mini_fat: Vec<u32>,
    mini_stream: Vec<u8>,
    entries: Vec<Entry>,
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

impl CompoundFile {
    pub fn open(data: &[u8]) -> Result<CompoundFile, Error> {
        if data.len() < 512 || data[..8] != MAGIC {
            return Err(Error::NotACompoundFile);
        }
        let header = |at: usize| u32_at(data, at).ok_or(Error::Damaged("header"));
        let sector_shift = u16_at(data, 0x1E).ok_or(Error::Damaged("header"))?;
        let mini_shift = u16_at(data, 0x20).ok_or(Error::Damaged("header"))?;
        if !(9..=16).contains(&sector_shift) || mini_shift >= sector_shift {
            return Err(Error::Damaged("sector size"));
        }
        let mut file = CompoundFile {
            data: data.to_vec(),
            sector: 1 << sector_shift,
            mini_sector: 1 << mini_shift,
            mini_cutoff: u64::from(header(0x38)?),
            fat: Vec::new(),
            mini_fat: Vec::new(),
            mini_stream: Vec::new(),
            entries: Vec::new(),
        };
        // the sectors of the allocation table: 109 in the header, the rest in a chain of
        // sectors of their own
        let mut fat_sectors: Vec<u32> = (0..109).filter_map(|k| u32_at(data, 0x4C + 4 * k)).filter(|&s| s < END_OF_CHAIN - 1).collect();
        let (mut next, per_sector) = (header(0x44)?, file.sector / 4 - 1);
        let mut guard = 0;
        while next < END_OF_CHAIN - 1 {
            let sector = file.sector_bytes(next).ok_or(Error::Damaged("allocation table"))?;
            fat_sectors.extend((0..per_sector).filter_map(|k| u32_at(sector, 4 * k)).filter(|&s| s < END_OF_CHAIN - 1));
            next = u32_at(sector, 4 * per_sector).ok_or(Error::Damaged("allocation table"))?;
            guard += 1;
            if guard > data.len() / file.sector {
                return Err(Error::Damaged("allocation table loops"));
            }
        }
        let mut fat = Vec::with_capacity(fat_sectors.len() * file.sector / 4);
        for sector in fat_sectors {
            let bytes = file.sector_bytes(sector).ok_or(Error::Damaged("allocation table"))?;
            fat.extend(bytes.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])));
        }
        file.fat = fat;
        let directory = file.chain(header(0x30)?, u64::MAX)?;
        let mini_fat = file.chain(header(0x3C)?, u64::MAX)?;
        file.mini_fat = mini_fat.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        file.read_directory(&directory)?;
        Ok(file)
    }

    /// The streams of the file.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Whether the file has a stream of this path.
    pub fn has(&self, path: &str) -> bool {
        self.entries.iter().any(|e| e.path == path)
    }

    /// The bytes of a stream.
    pub fn stream(&self, path: &str) -> Result<Vec<u8>, Error> {
        let entry = self.entries.iter().find(|e| e.path == path).ok_or_else(|| Error::MissingStream(path.to_string()))?;
        if entry.size < self.mini_cutoff {
            return self.mini_chain(entry.start, entry.size);
        }
        self.chain(entry.start, entry.size)
    }

    fn sector_bytes(&self, sector: u32) -> Option<&[u8]> {
        let start = (sector as usize).checked_add(1)?.checked_mul(self.sector)?;
        // the last sector of a file may be cut short
        let end = start.checked_add(self.sector)?.min(self.data.len());
        self.data.get(start..end)
    }

    /// The bytes of the chain of sectors that starts at `start`, at most `size` of them.
    fn chain(&self, start: u32, size: u64) -> Result<Vec<u8>, Error> {
        let limit = size.min(self.data.len() as u64) as usize;
        let mut out = Vec::with_capacity(limit.min(1 << 24));
        let (mut sector, mut steps) = (start, 0usize);
        while sector < END_OF_CHAIN - 1 && out.len() < limit {
            let bytes = self.sector_bytes(sector).ok_or(Error::Damaged("sector outside the file"))?;
            out.extend_from_slice(&bytes[..bytes.len().min(limit - out.len())]);
            sector = *self.fat.get(sector as usize).ok_or(Error::Damaged("sector outside the allocation table"))?;
            steps += 1;
            if steps > self.fat.len() {
                return Err(Error::Damaged("sector chain loops"));
            }
        }
        if size != u64::MAX && (out.len() as u64) < size {
            return Err(Error::Damaged("stream cut short"));
        }
        Ok(out)
    }

    /// The same for a small stream, kept in the mini stream.
    fn mini_chain(&self, start: u32, size: u64) -> Result<Vec<u8>, Error> {
        let limit = size as usize;
        let mut out = Vec::with_capacity(limit);
        let (mut sector, mut steps) = (start, 0usize);
        while sector < END_OF_CHAIN - 1 && out.len() < limit {
            let at = sector as usize * self.mini_sector;
            let bytes = self.mini_stream.get(at..(at + self.mini_sector).min(self.mini_stream.len())).ok_or(Error::Damaged("mini sector outside the mini stream"))?;
            out.extend_from_slice(&bytes[..bytes.len().min(limit - out.len())]);
            sector = *self.mini_fat.get(sector as usize).ok_or(Error::Damaged("mini sector outside its table"))?;
            steps += 1;
            if steps > self.mini_fat.len() {
                return Err(Error::Damaged("mini sector chain loops"));
            }
        }
        if out.len() < limit {
            return Err(Error::Damaged("stream cut short"));
        }
        Ok(out)
    }

    /// Walks the directory: a tree of storages, the entries of each a binary tree of siblings.
    fn read_directory(&mut self, directory: &[u8]) -> Result<(), Error> {
        struct Raw {
            name: String,
            kind: u8,
            left: u32,
            right: u32,
            child: u32,
            start: u32,
            size: u64,
        }
        let version = u16_at(&self.data, 0x1A).unwrap_or(3);
        let raw: Vec<Raw> = directory
            .chunks_exact(128)
            .take(MAX_ENTRIES)
            .map(|e| {
                let length = (usize::from(u16::from_le_bytes([e[0x40], e[0x41]])).saturating_sub(2)).min(62);
                let units: Vec<u16> = e[..length].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                let word = |at: usize| u32::from_le_bytes([e[at], e[at + 1], e[at + 2], e[at + 3]]);
                // version 3 files keep 32 bits of size; the upper half may hold rubbish
                let size = if version >= 4 { u64::from(word(0x78)) | u64::from(word(0x7C)) << 32 } else { u64::from(word(0x78)) };
                Raw { name: String::from_utf16_lossy(&units), kind: e[0x42], left: word(0x44), right: word(0x48), child: word(0x4C), start: word(0x74), size }
            })
            .collect();
        let root = raw.first().filter(|r| r.kind == 5).ok_or(Error::Damaged("no root entry"))?;
        self.mini_stream = self.chain(root.start, root.size)?;
        // (entry, path of its storage); each entry is visited once
        let mut seen = vec![false; raw.len()];
        let mut todo = vec![(root.child, String::new())];
        while let Some((index, prefix)) = todo.pop() {
            let Some(entry) = raw.get(index as usize) else { continue };
            if index == NO_STREAM || std::mem::replace(&mut seen[index as usize], true) {
                continue;
            }
            todo.push((entry.left, prefix.clone()));
            todo.push((entry.right, prefix.clone()));
            let path = format!("{prefix}{}", entry.name);
            match entry.kind {
                1 => todo.push((entry.child, format!("{path}/"))),
                2 => self.entries.push(Entry { path, size: entry.size, start: entry.start }),
                _ => {}
            }
        }
        self.entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::compound_file;

    #[test]
    fn small_and_large_streams_come_back_whole() {
        let large: Vec<u8> = (0..20_000u32).map(|k| (k % 251) as u8).collect();
        let small: Vec<u8> = (0..300u32).map(|k| (k % 7) as u8).collect();
        let file = compound_file(&[("Large", &large), ("Storage/Small", &small), ("Storage/Empty", &[]), ("Storage/Deeper/Tiny", b"x")]);
        let opened = CompoundFile::open(&file).unwrap();
        let paths: Vec<&str> = opened.entries().iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["Large", "Storage/Deeper/Tiny", "Storage/Empty", "Storage/Small"]);
        assert_eq!(opened.stream("Large").unwrap(), large);
        assert_eq!(opened.stream("Storage/Small").unwrap(), small);
        assert_eq!(opened.stream("Storage/Empty").unwrap(), Vec::<u8>::new());
        assert_eq!(opened.stream("Storage/Deeper/Tiny").unwrap(), b"x");
        assert!(opened.has("Large") && !opened.has("Storage"));
    }

    #[test]
    fn a_file_cut_short_is_damaged_not_a_panic() {
        let large = vec![9u8; 20_000];
        let file = compound_file(&[("Large", &large), ("Small", b"abc")]);
        for cut in [511, 512, 1024, 2048, file.len() - 600] {
            match CompoundFile::open(&file[..cut]) {
                Ok(opened) => assert!(opened.stream("Large").is_err(), "cut at {cut}"),
                Err(error) => assert!(matches!(error, Error::Damaged(_) | Error::NotACompoundFile), "cut at {cut}: {error}"),
            }
        }
    }

    #[test]
    fn a_looping_chain_ends() {
        let mut file = compound_file(&[("Large", &[1u8; 9000])]);
        // every sector of the allocation table points at sector 0
        let fat_sector = u32::from_le_bytes(file[0x4C..0x50].try_into().unwrap()) as usize;
        let at = (fat_sector + 1) * 512;
        file[at..at + 512].fill(0);
        // no stream is read past its size, so the walk ends, with rubbish or an error
        match CompoundFile::open(&file) {
            Ok(opened) => assert!(opened.stream("Large").map_or(true, |bytes| bytes.len() == 9000)),
            Err(error) => assert!(matches!(error, Error::Damaged(_))),
        }
    }
}
