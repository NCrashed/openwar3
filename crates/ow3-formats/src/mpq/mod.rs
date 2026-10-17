//! Read-only access to MPQ archives, the container format of Warcraft III
//! game data and maps.
//!
//! Supports format version 0 (the one Warcraft III writes) and the 64-bit
//! offsets of version 1. Maps that prepend their own header are handled by
//! scanning for the MPQ signature on 512-byte boundaries.

mod chain;
mod compression;
mod crypto;
mod explode;

use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use crypto::{HashType, decrypt, file_key, hash_string};

pub use chain::{ArchiveChain, GAME_ARCHIVES};

const ARCHIVE_SIGNATURE: u32 = 0x1A51_504D; // "MPQ\x1A"
const USER_DATA_SIGNATURE: u32 = 0x1B51_504D; // "MPQ\x1B"
const HEADER_ALIGNMENT: u64 = 0x200;
const HEADER_V0_SIZE: u32 = 0x20;
const HEADER_V1_SIZE: u32 = 0x2C;

const HASH_ENTRY_EMPTY: u32 = 0xFFFF_FFFF;
const HASH_ENTRY_DELETED: u32 = 0xFFFF_FFFE;
const NEUTRAL_LOCALE: u16 = 0;

const FLAG_IMPLODE: u32 = 0x0000_0100;
const FLAG_COMPRESS: u32 = 0x0000_0200;
const FLAG_ENCRYPTED: u32 = 0x0001_0000;
const FLAG_FIX_KEY: u32 = 0x0002_0000;
const FLAG_SINGLE_UNIT: u32 = 0x0100_0000;
const FLAG_DELETE_MARKER: u32 = 0x0200_0000;
const FLAG_SECTOR_CRC: u32 = 0x0400_0000;
const FLAG_EXISTS: u32 = 0x8000_0000;

const LISTFILE: &str = "(listfile)";

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// No MPQ header was found.
    NotAnArchive,
    /// The archive structure is inconsistent.
    Corrupt(&'static str),
    FileNotFound(String),
    /// A valid feature this reader does not implement yet.
    Unsupported(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(err) => write!(f, "I/O error: {err}"),
            Error::NotAnArchive => write!(f, "no MPQ header found"),
            Error::Corrupt(what) => write!(f, "corrupt archive: {what}"),
            Error::FileNotFound(name) => write!(f, "file not found in archive: {name}"),
            Error::Unsupported(what) => write!(f, "unsupported: {what}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug)]
pub struct Header {
    /// Absolute position of the MPQ header in the underlying stream. All
    /// offsets inside the archive are relative to it.
    pub archive_offset: u64,
    pub format_version: u16,
    pub sector_size: u32,
    pub hash_table_offset: u64,
    pub block_table_offset: u64,
    pub hash_table_entries: u32,
    pub block_table_entries: u32,
}

#[derive(Clone, Copy, Debug)]
struct HashEntry {
    name_a: u32,
    name_b: u32,
    locale: u16,
    block_index: u32,
}

#[derive(Clone, Copy, Debug)]
struct BlockEntry {
    offset: u32,
    compressed_size: u32,
    file_size: u32,
    flags: u32,
}

impl BlockEntry {
    fn has(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }
}

pub struct Archive<R> {
    reader: R,
    header: Header,
    hash_table: Vec<HashEntry>,
    block_table: Vec<BlockEntry>,
}

impl Archive<BufReader<File>> {
    pub fn open_path(path: impl AsRef<Path>) -> Result<Self> {
        Archive::open(BufReader::new(File::open(path)?))
    }
}

impl<R: Read + Seek> Archive<R> {
    pub fn open(mut reader: R) -> Result<Self> {
        let header = find_header(&mut reader)?;
        let hash_table = read_table(
            &mut reader,
            header.archive_offset + header.hash_table_offset,
            header.hash_table_entries,
            "(hash table)",
        )?
        .chunks_exact(16)
        .map(|e| HashEntry {
            name_a: le_u32(e, 0),
            name_b: le_u32(e, 4),
            locale: le_u16(e, 8),
            block_index: le_u32(e, 12),
        })
        .collect();
        let block_table = read_table(
            &mut reader,
            header.archive_offset + header.block_table_offset,
            header.block_table_entries,
            "(block table)",
        )?
        .chunks_exact(16)
        .map(|e| BlockEntry {
            offset: le_u32(e, 0),
            compressed_size: le_u32(e, 4),
            file_size: le_u32(e, 8),
            flags: le_u32(e, 12),
        })
        .collect();
        Ok(Archive {
            reader,
            header,
            hash_table,
            block_table,
        })
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn contains(&self, name: &str) -> bool {
        self.find_block(name).is_some()
    }

    /// Uncompressed size of a file, if the archive contains it.
    pub fn file_size(&self, name: &str) -> Option<u64> {
        self.find_block(name).map(|b| u64::from(b.file_size))
    }

    /// File names from the archive's `(listfile)`. MPQ archives store only
    /// hashes of names, so files missing from the list cannot be enumerated.
    pub fn list(&mut self) -> Result<Vec<String>> {
        let data = self.read(LISTFILE)?;
        Ok(String::from_utf8_lossy(&data)
            .split(['\r', '\n', ';'])
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect())
    }

    pub fn read(&mut self, name: &str) -> Result<Vec<u8>> {
        let block = self
            .find_block(name)
            .ok_or_else(|| Error::FileNotFound(name.to_owned()))?;
        self.read_block(name, block)
    }

    fn find_block(&self, name: &str) -> Option<BlockEntry> {
        let count = self.hash_table.len();
        if count == 0 {
            return None;
        }
        let start = hash_string(name, HashType::TableOffset) as usize % count;
        let name_a = hash_string(name, HashType::NameA);
        let name_b = hash_string(name, HashType::NameB);

        let mut fallback = None;
        for i in 0..count {
            let entry = self.hash_table[(start + i) % count];
            match entry.block_index {
                HASH_ENTRY_EMPTY => break,
                HASH_ENTRY_DELETED => continue,
                _ => {}
            }
            if entry.name_a != name_a || entry.name_b != name_b {
                continue;
            }
            let Some(block) = self.block_table.get(entry.block_index as usize) else {
                continue;
            };
            if !block.has(FLAG_EXISTS) || block.has(FLAG_DELETE_MARKER) {
                continue;
            }
            if entry.locale == NEUTRAL_LOCALE {
                return Some(*block);
            }
            fallback.get_or_insert(*block);
        }
        fallback
    }

    fn read_block(&mut self, name: &str, block: BlockEntry) -> Result<Vec<u8>> {
        let file_size = block.file_size as usize;
        if file_size == 0 {
            return Ok(Vec::new());
        }
        let position = self.header.archive_offset + u64::from(block.offset);
        let key = block
            .has(FLAG_ENCRYPTED)
            .then(|| file_key(name, block.offset, block.file_size, block.has(FLAG_FIX_KEY)));

        let mut raw = vec![0u8; block.compressed_size as usize];
        self.reader.seek(SeekFrom::Start(position))?;
        self.reader.read_exact(&mut raw)?;

        if block.has(FLAG_SINGLE_UNIT) {
            if let Some(key) = key {
                decrypt(&mut raw, key);
            }
            return decode_unit(raw, file_size, block.flags);
        }

        let sector_size = self.header.sector_size as usize;
        let sector_count = file_size.div_ceil(sector_size);
        let mut out = Vec::with_capacity(file_size);

        if !block.has(FLAG_COMPRESS | FLAG_IMPLODE) {
            // Stored sectors follow each other without an offset table.
            if raw.len() < file_size {
                return Err(Error::Corrupt("stored file is shorter than its size"));
            }
            for (index, sector) in raw[..file_size].chunks(sector_size).enumerate() {
                let mut sector = sector.to_vec();
                if let Some(key) = key {
                    decrypt(&mut sector, key.wrapping_add(index as u32));
                }
                out.extend_from_slice(&sector);
            }
            return Ok(out);
        }

        let table_entries = sector_count + 1 + usize::from(block.has(FLAG_SECTOR_CRC));
        let table_len = table_entries * 4;
        if raw.len() < table_len {
            return Err(Error::Corrupt("sector offset table exceeds file data"));
        }
        let mut table = raw[..table_len].to_vec();
        if let Some(key) = key {
            decrypt(&mut table, key.wrapping_sub(1));
        }
        let offsets: Vec<usize> = (0..=sector_count)
            .map(|i| le_u32(&table, i * 4) as usize)
            .collect();

        for index in 0..sector_count {
            let (start, end) = (offsets[index], offsets[index + 1]);
            if start > end || end > raw.len() {
                return Err(Error::Corrupt("sector offset out of range"));
            }
            let mut sector = raw[start..end].to_vec();
            if let Some(key) = key {
                decrypt(&mut sector, key.wrapping_add(index as u32));
            }
            let expected = sector_size.min(file_size - index * sector_size);
            out.extend_from_slice(&decode_unit(sector, expected, block.flags)?);
        }
        Ok(out)
    }
}

/// Decompresses one sector, or a whole single-unit file. Data that is not
/// smaller than its expected size was stored without compression.
fn decode_unit(data: Vec<u8>, expected: usize, flags: u32) -> Result<Vec<u8>> {
    let decoded = if data.len() >= expected {
        data
    } else if flags & FLAG_COMPRESS != 0 {
        compression::decompress(&data, expected)?
    } else if flags & FLAG_IMPLODE != 0 {
        compression::explode(&data, expected)?
    } else {
        return Err(Error::Corrupt(
            "data is shorter than its size but not compressed",
        ));
    };
    if decoded.len() < expected {
        return Err(Error::Corrupt("decompressed data is shorter than expected"));
    }
    Ok(decoded)
}

fn find_header<R: Read + Seek>(reader: &mut R) -> Result<Header> {
    let length = reader.seek(SeekFrom::End(0))?;
    let mut offset = 0;
    while offset + u64::from(HEADER_V0_SIZE) <= length {
        reader.seek(SeekFrom::Start(offset))?;
        let mut signature = [0u8; 4];
        reader.read_exact(&mut signature)?;
        match u32::from_le_bytes(signature) {
            ARCHIVE_SIGNATURE => return read_header(reader, offset, length),
            USER_DATA_SIGNATURE => {
                let mut fields = [0u8; 8];
                reader.read_exact(&mut fields)?;
                let header_offset = offset + u64::from(le_u32(&fields, 4));
                if header_offset + u64::from(HEADER_V0_SIZE) <= length {
                    reader.seek(SeekFrom::Start(header_offset))?;
                    reader.read_exact(&mut signature)?;
                    if u32::from_le_bytes(signature) == ARCHIVE_SIGNATURE {
                        return read_header(reader, header_offset, length);
                    }
                }
            }
            _ => {}
        }
        offset += HEADER_ALIGNMENT;
    }
    Err(Error::NotAnArchive)
}

/// Reads the header whose signature was just consumed at `offset`.
fn read_header<R: Read + Seek>(reader: &mut R, offset: u64, length: u64) -> Result<Header> {
    let mut fields = [0u8; (HEADER_V1_SIZE - 4) as usize];
    let available = (length - offset - 4).min(fields.len() as u64) as usize;
    reader.read_exact(&mut fields[..available])?;

    let header_size = le_u32(&fields, 0);
    let format_version = le_u16(&fields, 8);
    let sector_size_shift = le_u16(&fields, 10);
    let mut hash_table_offset = u64::from(le_u32(&fields, 12));
    let mut block_table_offset = u64::from(le_u32(&fields, 16));

    // Warcraft III ignores the version field and some maps put garbage in
    // it, so the high offset words are trusted only when everything agrees.
    if format_version == 1 && header_size >= HEADER_V1_SIZE && available == fields.len() {
        hash_table_offset |= u64::from(le_u16(&fields, 36)) << 32;
        block_table_offset |= u64::from(le_u16(&fields, 38)) << 32;
    }
    let sector_size = 512u32
        .checked_shl(u32::from(sector_size_shift))
        .ok_or(Error::Corrupt("sector size shift too large"))?;

    Ok(Header {
        archive_offset: offset,
        format_version,
        sector_size,
        hash_table_offset,
        block_table_offset,
        hash_table_entries: le_u32(&fields, 20),
        block_table_entries: le_u32(&fields, 24),
    })
}

fn read_table<R: Read + Seek>(
    reader: &mut R,
    position: u64,
    entries: u32,
    key_name: &str,
) -> Result<Vec<u8>> {
    let len =
        usize::try_from(u64::from(entries) * 16).map_err(|_| Error::Corrupt("table too large"))?;
    let mut data = vec![0u8; len];
    reader.seek(SeekFrom::Start(position))?;
    reader.read_exact(&mut data)?;
    decrypt(&mut data, hash_string(key_name, HashType::FileKey));
    Ok(data)
}

fn le_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn le_u16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

#[cfg(test)]
mod tests;
