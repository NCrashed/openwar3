//! Sector decompression.
//!
//! A compressed sector starts with a byte that lists the methods applied to
//! it, one bit per method.

use std::io::Read;

use flate2::read::ZlibDecoder;

use super::{Error, Result};

pub use super::explode::explode;

const ZLIB: u8 = 0x02;
const PKWARE: u8 = 0x08;

pub fn decompress(data: &[u8], expected: usize) -> Result<Vec<u8>> {
    let (&methods, payload) = data
        .split_first()
        .ok_or(Error::Corrupt("empty compressed sector"))?;
    match methods {
        ZLIB => inflate(payload, expected),
        PKWARE => explode(payload, expected),
        other => Err(Error::Unsupported(format!(
            "compression methods {other:#04x}"
        ))),
    }
}

fn inflate(data: &[u8], expected: usize) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(expected);
    ZlibDecoder::new(data)
        .read_to_end(&mut out)
        .map_err(|_| Error::Corrupt("invalid zlib stream"))?;
    Ok(out)
}
