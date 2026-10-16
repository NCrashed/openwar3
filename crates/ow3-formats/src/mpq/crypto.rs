//! MPQ hashing and table/file encryption.

const CRYPT_TABLE: [u32; 0x500] = build_crypt_table();

const fn build_crypt_table() -> [u32; 0x500] {
    let mut table = [0u32; 0x500];
    let mut seed: u32 = 0x0010_0001;
    let mut index1 = 0;
    while index1 < 0x100 {
        let mut index2 = index1;
        let mut i = 0;
        while i < 5 {
            seed = (seed * 125 + 3) % 0x2A_AAAB;
            let high = (seed & 0xFFFF) << 16;
            seed = (seed * 125 + 3) % 0x2A_AAAB;
            let low = seed & 0xFFFF;
            table[index2] = high | low;
            index2 += 0x100;
            i += 1;
        }
        index1 += 1;
    }
    table
}

/// Which of the four hash functions to apply to a name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashType {
    /// Start index in the hash table.
    TableOffset = 0,
    /// First check value stored in a hash table entry.
    NameA = 1,
    /// Second check value stored in a hash table entry.
    NameB = 2,
    /// Encryption key of a file or table.
    FileKey = 3,
}

/// MPQ names are case-insensitive and treat `/` and `\` as the same separator.
fn normalize(byte: u8) -> u8 {
    match byte {
        b'/' => b'\\',
        b => b.to_ascii_uppercase(),
    }
}

pub fn hash_string(name: &str, hash_type: HashType) -> u32 {
    let offset = (hash_type as usize) << 8;
    let mut seed1: u32 = 0x7FED_7FED;
    let mut seed2: u32 = 0xEEEE_EEEE;
    for byte in name.bytes() {
        let ch = normalize(byte) as u32;
        seed1 = CRYPT_TABLE[offset + ch as usize] ^ seed1.wrapping_add(seed2);
        seed2 = ch
            .wrapping_add(seed1)
            .wrapping_add(seed2)
            .wrapping_add(seed2 << 5)
            .wrapping_add(3);
    }
    seed1
}

fn next_key(key: u32) -> u32 {
    ((!key << 0x15).wrapping_add(0x1111_1111)) | (key >> 0x0B)
}

/// Decrypts whole little-endian `u32` words in place. Trailing bytes that do
/// not fill a word are stored unencrypted and are left as they are.
pub fn decrypt(data: &mut [u8], mut key: u32) {
    let mut seed: u32 = 0xEEEE_EEEE;
    for chunk in data.chunks_exact_mut(4) {
        seed = seed.wrapping_add(CRYPT_TABLE[0x400 + (key & 0xFF) as usize]);
        let value =
            u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) ^ key.wrapping_add(seed);
        key = next_key(key);
        seed = value
            .wrapping_add(seed)
            .wrapping_add(seed << 5)
            .wrapping_add(3);
        chunk.copy_from_slice(&value.to_le_bytes());
    }
}

/// Inverse of [`decrypt`].
#[cfg(test)]
pub fn encrypt(data: &mut [u8], mut key: u32) {
    let mut seed: u32 = 0xEEEE_EEEE;
    for chunk in data.chunks_exact_mut(4) {
        seed = seed.wrapping_add(CRYPT_TABLE[0x400 + (key & 0xFF) as usize]);
        let plain = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        let value = plain ^ key.wrapping_add(seed);
        key = next_key(key);
        seed = plain
            .wrapping_add(seed)
            .wrapping_add(seed << 5)
            .wrapping_add(3);
        chunk.copy_from_slice(&value.to_le_bytes());
    }
}

/// Encryption key of a stored file. The key depends only on the base name,
/// unless the archive asks for it to be adjusted by the file's position.
pub fn file_key(name: &str, block_offset: u32, file_size: u32, adjusted: bool) -> u32 {
    let base = name.rsplit(['\\', '/']).next().unwrap_or(name);
    let key = hash_string(base, HashType::FileKey);
    if adjusted {
        key.wrapping_add(block_offset) ^ file_size
    } else {
        key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_keys_match_published_values() {
        assert_eq!(hash_string("(hash table)", HashType::FileKey), 0xC3AF_3770);
        assert_eq!(hash_string("(block table)", HashType::FileKey), 0xEC83_B3A3);
    }

    #[test]
    fn hashing_ignores_case_and_separator_style() {
        for hash_type in [HashType::TableOffset, HashType::NameA, HashType::NameB] {
            assert_eq!(
                hash_string("units/unitdata.slk", hash_type),
                hash_string("Units\\UnitData.slk", hash_type),
            );
        }
    }

    #[test]
    fn encrypt_then_decrypt_round_trips() {
        let original: Vec<u8> = (0..=42).collect();
        let mut data = original.clone();
        encrypt(&mut data, 0x1234_5678);
        assert_ne!(data[..40], original[..40]);
        assert_eq!(data[40..], original[40..], "tail bytes stay plain");
        decrypt(&mut data, 0x1234_5678);
        assert_eq!(data, original);
    }

    #[test]
    fn file_key_uses_base_name() {
        assert_eq!(
            file_key("Scripts\\common.j", 0, 0, false),
            hash_string("common.j", HashType::FileKey),
        );
        let plain = file_key("a.txt", 0, 0, false);
        assert_eq!(
            file_key("a.txt", 0x100, 7, true),
            plain.wrapping_add(0x100) ^ 7
        );
    }
}
