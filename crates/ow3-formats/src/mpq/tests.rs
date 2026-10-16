use std::io::{Cursor, Write};

use flate2::Compression;
use flate2::write::ZlibEncoder;

use super::crypto::encrypt;
use super::*;

#[derive(Clone)]
struct TestFile {
    name: &'static str,
    data: Vec<u8>,
    locale: u16,
    compress: bool,
    encrypt: bool,
    fix_key: bool,
    single_unit: bool,
}

impl TestFile {
    fn new(name: &'static str, data: impl Into<Vec<u8>>) -> Self {
        TestFile {
            name,
            data: data.into(),
            locale: NEUTRAL_LOCALE,
            compress: false,
            encrypt: false,
            fix_key: false,
            single_unit: false,
        }
    }
    fn compressed(mut self) -> Self {
        self.compress = true;
        self
    }
    fn encrypted(mut self, fix_key: bool) -> Self {
        self.encrypt = true;
        self.fix_key = fix_key;
        self
    }
    fn single_unit(mut self) -> Self {
        self.single_unit = true;
        self
    }
    fn locale(mut self, locale: u16) -> Self {
        self.locale = locale;
        self
    }
}

/// Builds a format version 0 archive the way the format documentation
/// describes it. `prefix` bytes are placed before the MPQ header, as maps do.
fn build(files: &[TestFile], sector_shift: u16, prefix: &[u8]) -> Vec<u8> {
    let archive_offset = prefix.len();
    let sector_size = 512usize << sector_shift;
    let mut out = prefix.to_vec();
    out.resize(archive_offset + HEADER_V0_SIZE as usize, 0);

    let mut blocks = Vec::new();
    for file in files {
        let offset = (out.len() - archive_offset) as u32;
        let size = file.data.len() as u32;
        let mut flags = FLAG_EXISTS;
        let key = file.encrypt.then(|| {
            flags |= FLAG_ENCRYPTED;
            if file.fix_key {
                flags |= FLAG_FIX_KEY;
            }
            file_key(file.name, offset, size, file.fix_key)
        });
        if file.compress {
            flags |= FLAG_COMPRESS;
        }

        let stored = if file.single_unit {
            flags |= FLAG_SINGLE_UNIT;
            let mut unit = maybe_compress(&file.data, file.compress);
            if let Some(key) = key {
                encrypt(&mut unit, key);
            }
            unit
        } else if file.compress {
            let sectors: Vec<Vec<u8>> = file
                .data
                .chunks(sector_size)
                .enumerate()
                .map(|(i, chunk)| {
                    let mut sector = maybe_compress(chunk, true);
                    if let Some(key) = key {
                        encrypt(&mut sector, key.wrapping_add(i as u32));
                    }
                    sector
                })
                .collect();
            let mut table = Vec::new();
            let mut position = (sectors.len() as u32 + 1) * 4;
            table.extend_from_slice(&position.to_le_bytes());
            for sector in &sectors {
                position += sector.len() as u32;
                table.extend_from_slice(&position.to_le_bytes());
            }
            if let Some(key) = key {
                encrypt(&mut table, key.wrapping_sub(1));
            }
            table.into_iter().chain(sectors.concat()).collect()
        } else {
            file.data
                .chunks(sector_size)
                .enumerate()
                .flat_map(|(i, chunk)| {
                    let mut sector = chunk.to_vec();
                    if let Some(key) = key {
                        encrypt(&mut sector, key.wrapping_add(i as u32));
                    }
                    sector
                })
                .collect()
        };

        blocks.push((offset, stored.len() as u32, size, flags));
        out.extend_from_slice(&stored);
    }

    let hash_count = (files.len() * 2).next_power_of_two().max(4);
    let mut hash_table = vec![[HASH_ENTRY_EMPTY; 4]; hash_count];
    for (index, file) in files.iter().enumerate() {
        let mut slot = hash_string(file.name, HashType::TableOffset) as usize % hash_count;
        while hash_table[slot][3] != HASH_ENTRY_EMPTY {
            slot = (slot + 1) % hash_count;
        }
        hash_table[slot] = [
            hash_string(file.name, HashType::NameA),
            hash_string(file.name, HashType::NameB),
            u32::from(file.locale),
            index as u32,
        ];
    }

    let hash_table_offset = (out.len() - archive_offset) as u32;
    let mut table: Vec<u8> = hash_table
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    encrypt(&mut table, hash_string("(hash table)", HashType::FileKey));
    out.extend_from_slice(&table);

    let block_table_offset = (out.len() - archive_offset) as u32;
    let mut table: Vec<u8> = blocks
        .iter()
        .flat_map(|&(a, b, c, d)| [a, b, c, d])
        .flat_map(u32::to_le_bytes)
        .collect();
    encrypt(&mut table, hash_string("(block table)", HashType::FileKey));
    out.extend_from_slice(&table);

    let archive_size = (out.len() - archive_offset) as u32;
    let mut header = Vec::new();
    header.extend_from_slice(&ARCHIVE_SIGNATURE.to_le_bytes());
    header.extend_from_slice(&HEADER_V0_SIZE.to_le_bytes());
    header.extend_from_slice(&archive_size.to_le_bytes());
    header.extend_from_slice(&0u16.to_le_bytes());
    header.extend_from_slice(&sector_shift.to_le_bytes());
    header.extend_from_slice(&hash_table_offset.to_le_bytes());
    header.extend_from_slice(&block_table_offset.to_le_bytes());
    header.extend_from_slice(&(hash_count as u32).to_le_bytes());
    header.extend_from_slice(&(blocks.len() as u32).to_le_bytes());
    out[archive_offset..archive_offset + header.len()].copy_from_slice(&header);
    out
}

/// Compresses with zlib when that saves space, as archivers do.
fn maybe_compress(data: &[u8], compress: bool) -> Vec<u8> {
    if !compress {
        return data.to_vec();
    }
    let mut encoder = ZlibEncoder::new(vec![ZLIB_METHOD], Compression::best());
    encoder.write_all(data).unwrap();
    let packed = encoder.finish().unwrap();
    if packed.len() < data.len() {
        packed
    } else {
        data.to_vec()
    }
}

const ZLIB_METHOD: u8 = 0x02;

fn open(bytes: Vec<u8>) -> Archive<Cursor<Vec<u8>>> {
    Archive::open(Cursor::new(bytes)).expect("archive opens")
}

/// Text that compresses well and spans several 512-byte sectors.
fn sample_text(len: usize) -> Vec<u8> {
    b"native GetUnitState takes unit whichUnit returns real\n"
        .iter()
        .copied()
        .cycle()
        .take(len)
        .collect()
}

/// Bytes that zlib cannot shrink, so sectors end up stored raw.
fn noise(len: usize) -> Vec<u8> {
    let mut state = 0x2545_F491u32;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect()
}

#[test]
fn reads_files_in_every_storage_layout() {
    let files = vec![
        TestFile::new("stored.txt", sample_text(1300)),
        TestFile::new("Units\\compressed.slk", sample_text(5000)).compressed(),
        TestFile::new("mixed.bin", [sample_text(700), noise(900)].concat()).compressed(),
        TestFile::new("single.txt", sample_text(3000))
            .compressed()
            .single_unit(),
        TestFile::new("Scripts\\enc.j", sample_text(2000))
            .compressed()
            .encrypted(false),
        TestFile::new("fixkey.j", sample_text(2100))
            .compressed()
            .encrypted(true),
        TestFile::new("enc-stored.bin", noise(1500)).encrypted(true),
        TestFile::new("enc-single.txt", sample_text(800))
            .single_unit()
            .encrypted(true),
        TestFile::new("empty.txt", Vec::new()),
    ];
    let mut archive = open(build(&files, 0, &[]));
    for file in &files {
        assert_eq!(archive.read(file.name).unwrap(), file.data, "{}", file.name);
        assert_eq!(archive.file_size(file.name), Some(file.data.len() as u64));
    }
}

#[test]
fn lookup_ignores_case_and_separator_style() {
    let files = vec![TestFile::new("Units\\UnitData.slk", "data")];
    let mut archive = open(build(&files, 3, &[]));
    assert_eq!(archive.read("units/unitdata.SLK").unwrap(), b"data");
}

#[test]
fn finds_header_after_a_map_prefix() {
    let mut prefix = b"HM3W".to_vec();
    prefix.resize(1024, 0);
    let files = vec![TestFile::new("war3map.j", sample_text(900)).compressed()];
    let mut archive = open(build(&files, 3, &prefix));
    assert_eq!(archive.header().archive_offset, 1024);
    assert_eq!(archive.read("war3map.j").unwrap(), sample_text(900));
}

#[test]
fn follows_user_data_header() {
    let files = vec![TestFile::new("a.txt", "hello")];
    let mut bytes = build(&files, 3, &[0; 512]);
    let mut user_data = USER_DATA_SIGNATURE.to_le_bytes().to_vec();
    user_data.extend_from_slice(&16u32.to_le_bytes()); // user data size
    user_data.extend_from_slice(&512u32.to_le_bytes()); // header offset
    bytes[..user_data.len()].copy_from_slice(&user_data);
    let mut archive = open(bytes);
    assert_eq!(archive.header().archive_offset, 512);
    assert_eq!(archive.read("a.txt").unwrap(), b"hello");
}

#[test]
fn prefers_the_neutral_locale() {
    let files = vec![
        TestFile::new("Units\\name.txt", "russian").locale(0x419),
        TestFile::new("Units\\name.txt", "neutral"),
    ];
    let mut archive = open(build(&files, 3, &[]));
    assert_eq!(archive.read("Units\\name.txt").unwrap(), b"neutral");
}

#[test]
fn lists_names_from_listfile() {
    let files = vec![
        TestFile::new("a.txt", "a"),
        TestFile::new("b\\c.txt", "c"),
        TestFile::new(LISTFILE, "a.txt\r\nb\\c.txt;\r\n\r\n"),
    ];
    let mut archive = open(build(&files, 3, &[]));
    assert_eq!(archive.list().unwrap(), ["a.txt", "b\\c.txt"]);
}

#[test]
fn reports_missing_files() {
    let mut archive = open(build(&[TestFile::new("a.txt", "a")], 3, &[]));
    assert!(!archive.contains("b.txt"));
    assert!(matches!(archive.read("b.txt"), Err(Error::FileNotFound(name)) if name == "b.txt"));
}

#[test]
fn rejects_data_without_a_header() {
    let result = Archive::open(Cursor::new(vec![0u8; 4096]));
    assert!(matches!(result, Err(Error::NotAnArchive)));
}
