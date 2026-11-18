//! Compressed streams (`...__ZLB`): a 16 byte signature, then blocks, each the size of its
//! content (u32), the size of its compressed bytes (u32) and those bytes, a zlib stream. A
//! block holds at most a megabyte of content; an empty stream is the signature and one block of
//! two zeros.

use crate::Error;
use std::io::Read;

/// The first sixteen bytes of every compressed stream.
pub const SIGNATURE: [u8; 16] = [0x23, 0x1D, 0xD5, 0x71, 0xDA, 0x81, 0x48, 0xA2, 0xA8, 0x58, 0x98, 0xB2, 0x1B, 0x89, 0xEF, 0x99];

/// More content than any stream of a real file inflates to: a bound against streams that are
/// built to exhaust memory.
pub const MAX_CONTENT: usize = 1 << 31;

/// Whether the bytes of a stream start as a compressed stream does.
pub fn is_compressed(stream: &[u8]) -> bool {
    stream.len() >= 16 && stream[..16] == SIGNATURE
}

/// The content of a compressed stream.
pub fn inflate(stream: &[u8]) -> Result<Vec<u8>, Error> {
    if !is_compressed(stream) {
        return Err(Error::Compression("no signature"));
    }
    let word = |at: usize| stream.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let (mut at, mut out) = (16, Vec::new());
    while let (Some(size), Some(packed)) = (word(at), word(at + 4)) {
        at += 8;
        if packed == 0 {
            // an empty block; anything after it is padding
            if size != 0 {
                return Err(Error::Compression("block without bytes"));
            }
            continue;
        }
        let bytes = stream.get(at..at + packed).ok_or(Error::Compression("block cut short"))?;
        if out.len() + size > MAX_CONTENT {
            return Err(Error::Compression("content too large"));
        }
        let before = out.len();
        // one byte more than promised shows a block that lies about its size
        flate2::read::ZlibDecoder::new(bytes).take(size as u64 + 1).read_to_end(&mut out).map_err(|_| Error::Compression("not a zlib stream"))?;
        if out.len() - before != size {
            return Err(Error::Compression("block of another size than it says"));
        }
        at += packed;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::zlb;

    #[test]
    fn blocks_inflate_to_the_content() {
        let content: Vec<u8> = (0..10_000u32).flat_map(|k| (k * k).to_le_bytes()).collect();
        for block in [100_000, 4096, 1000] {
            assert_eq!(inflate(&zlb(&content, block)).unwrap(), content, "blocks of {block}");
        }
    }

    #[test]
    fn an_empty_stream_is_empty() {
        let mut stream = SIGNATURE.to_vec();
        stream.extend_from_slice(&[0; 8]);
        assert_eq!(inflate(&stream).unwrap(), Vec::<u8>::new());
        assert_eq!(inflate(&zlb(&[], 4096)).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn damage_is_an_error() {
        let good = zlb(&[42u8; 5000], 4096);
        assert_eq!(inflate(&good[4..]), Err(Error::Compression("no signature")));
        assert_eq!(inflate(&good[..good.len() - 3]), Err(Error::Compression("block cut short")));
        // a block that says it is smaller than it is
        let mut lying = good.clone();
        lying[16..20].copy_from_slice(&100u32.to_le_bytes());
        assert_eq!(inflate(&lying), Err(Error::Compression("block of another size than it says")));
        let mut broken = good;
        broken[24..40].fill(0xAA);
        assert!(inflate(&broken).is_err());
    }
}
