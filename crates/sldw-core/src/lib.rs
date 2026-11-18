//! Container layer of the SolidWorks reader.
//!
//! A SolidWorks part (`.sldprt`) or assembly (`.sldasm`) is a compound file: a small file
//! system of named streams in storages ([`cfb`]). Streams whose name ends in `__ZLB` are
//! compressed ([`zlb`]). [`Document`] opens a file and hands out its streams, inflated where
//! needed.
//!
//! Everything works on bytes in memory: no file system, no `unsafe`.

#![forbid(unsafe_code)]

pub mod cfb;
pub mod testing;
pub mod zlb;

use std::fmt;

/// Why a file could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The bytes are not a compound file. Files in the newer SolidWorks container (no compound
    /// file header) end here: that container is not read yet.
    NotACompoundFile,
    /// The compound file is damaged or cut short.
    Damaged(&'static str),
    /// A compressed stream does not inflate.
    Compression(&'static str),
    /// The file has no stream of this name.
    MissingStream(String),
    /// The file holds nothing that can be drawn.
    NoGeometry(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotACompoundFile => write!(f, "not a compound file (the newer SolidWorks container is not read yet)"),
            Error::Damaged(what) => write!(f, "damaged compound file: {what}"),
            Error::Compression(what) => write!(f, "compressed stream does not inflate: {what}"),
            Error::MissingStream(name) => write!(f, "no stream {name}"),
            Error::NoGeometry(what) => write!(f, "nothing to draw: {what}"),
        }
    }
}

impl std::error::Error for Error {}

/// The suffix of the names of compressed streams.
pub const ZLB: &str = "__ZLB";

/// An opened SolidWorks file: its streams by path (`Contents/DisplayLists__ZLB`).
pub struct Document {
    file: cfb::CompoundFile,
}

impl Document {
    /// Opens the bytes of a `.sldprt` or `.sldasm` file.
    pub fn open(data: &[u8]) -> Result<Document, Error> {
        Ok(Document { file: cfb::CompoundFile::open(data)? })
    }

    /// The streams of the file: path (storages separated by `/`) and size in bytes as stored.
    pub fn streams(&self) -> impl Iterator<Item = (&str, u64)> {
        self.file.entries().iter().map(|e| (e.path.as_str(), e.size))
    }

    /// The bytes of a stream as stored.
    pub fn raw(&self, path: &str) -> Result<Vec<u8>, Error> {
        self.file.stream(path)
    }

    /// The content of a stream: `path` without the `__ZLB` suffix finds the stream stored
    /// either plain or compressed, and gives its bytes inflated.
    pub fn content(&self, path: &str) -> Result<Vec<u8>, Error> {
        let plain = path.strip_suffix(ZLB).unwrap_or(path);
        let compressed = format!("{plain}{ZLB}");
        if self.file.has(&compressed) {
            return zlb::inflate(&self.file.stream(&compressed)?);
        }
        self.file.stream(plain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_gives_plain_and_compressed_streams() {
        let text = b"tessellation ".repeat(700);
        let packed = testing::zlb(&text, 4096);
        let file = testing::compound_file(&[("Header2", b"head"), ("Contents/DisplayLists__ZLB", &packed), ("Contents/Definition", &[7u8; 5000])]);
        let document = Document::open(&file).unwrap();
        let mut names: Vec<_> = document.streams().map(|(name, size)| (name.to_string(), size)).collect();
        names.sort();
        assert_eq!(names, [("Contents/Definition".to_string(), 5000), ("Contents/DisplayLists__ZLB".to_string(), packed.len() as u64), ("Header2".to_string(), 4)]);
        assert_eq!(document.content("Contents/DisplayLists").unwrap(), text);
        assert_eq!(document.content("Contents/DisplayLists__ZLB").unwrap(), text);
        assert_eq!(document.raw("Contents/DisplayLists__ZLB").unwrap(), packed);
        assert_eq!(document.content("Header2").unwrap(), b"head");
        assert_eq!(document.content("Contents/Definition").unwrap(), vec![7u8; 5000]);
        assert_eq!(document.content("Contents/Nothing"), Err(Error::MissingStream("Contents/Nothing".into())));
    }

    #[test]
    fn other_bytes_are_not_a_compound_file() {
        assert!(matches!(Document::open(b"PK\x03\x04 not this"), Err(Error::NotACompoundFile)));
        assert!(matches!(Document::open(&[]), Err(Error::NotACompoundFile)));
    }
}
