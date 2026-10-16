//! Decompressor for the PKWARE Data Compression Library "implode" format.
//!
//! Format reference: Mark Adler's description in `blast.c` (zlib license),
//! distributed with zlib under `contrib/blast`.
//!
//! The stream starts with two bytes: literal mode (0 = raw bytes, 1 = Huffman
//! coded) and dictionary size in bits (4, 5 or 6). The rest is a bit stream,
//! least significant bit first, of literals and length/distance pairs.

use super::{Error, Result};

const MAX_CODE_BITS: usize = 13;
const END_OF_STREAM: usize = 519;

/// Code lengths in run-length form: each byte holds a length in the low
/// nibble and a repeat count minus one in the high nibble.
const LITERAL_LENGTHS: [u8; 98] = [
    11, 124, 8, 7, 28, 7, 188, 13, 76, 4, 10, 8, 12, 10, 12, 10, 8, 23, 8, 9, 7, 6, 7, 8, 7, 6, 55,
    8, 23, 24, 12, 11, 7, 9, 11, 12, 6, 7, 22, 5, 7, 24, 6, 11, 9, 6, 7, 22, 7, 11, 38, 7, 9, 8,
    25, 11, 8, 11, 9, 12, 8, 12, 5, 38, 5, 38, 5, 11, 7, 5, 6, 21, 6, 10, 53, 8, 7, 24, 10, 27, 44,
    253, 253, 253, 252, 252, 252, 13, 12, 45, 12, 45, 12, 61, 12, 45, 44, 173,
];
const LENGTH_LENGTHS: [u8; 6] = [2, 35, 36, 53, 38, 23];
const DISTANCE_LENGTHS: [u8; 7] = [2, 20, 53, 230, 247, 151, 248];

const LENGTH_BASE: [usize; 16] = [3, 2, 4, 5, 6, 7, 8, 9, 10, 12, 16, 24, 40, 72, 136, 264];
const LENGTH_EXTRA_BITS: [u32; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8];

/// Canonical Huffman code: how many codes have each length, and the symbols
/// ordered by code length, then by symbol value.
struct Code {
    counts: [u16; MAX_CODE_BITS + 1],
    symbols: Vec<u16>,
}

impl Code {
    fn from_run_lengths(runs: &[u8]) -> Code {
        let lengths: Vec<usize> = runs
            .iter()
            .flat_map(|&run| {
                std::iter::repeat_n(usize::from(run & 0x0F), usize::from(run >> 4) + 1)
            })
            .collect();
        let mut counts = [0u16; MAX_CODE_BITS + 1];
        for &length in &lengths {
            counts[length] += 1;
        }
        let mut symbols: Vec<u16> = (0..lengths.len() as u16)
            .filter(|&s| lengths[usize::from(s)] != 0)
            .collect();
        symbols.sort_by_key(|&s| lengths[usize::from(s)]);
        Code { counts, symbols }
    }

    #[cfg(test)]
    fn symbol_count(&self) -> usize {
        self.counts[1..].iter().map(|&c| usize::from(c)).sum()
    }
}

struct BitReader<'a> {
    data: &'a [u8],
    position: usize,
    buffer: u32,
    available: u32,
}

impl BitReader<'_> {
    fn bits(&mut self, count: u32) -> Result<u32> {
        while self.available < count {
            let byte = *self
                .data
                .get(self.position)
                .ok_or(Error::Corrupt("implode stream ends early"))?;
            self.buffer |= u32::from(byte) << self.available;
            self.position += 1;
            self.available += 8;
        }
        let value = self.buffer & ((1 << count) - 1);
        self.buffer >>= count;
        self.available -= count;
        Ok(value)
    }

    /// Codes are stored with their bits inverted.
    fn decode(&mut self, code: &Code) -> Result<usize> {
        let (mut value, mut first, mut index) = (0usize, 0usize, 0usize);
        for &count in &code.counts[1..] {
            let count = usize::from(count);
            value |= (self.bits(1)? ^ 1) as usize;
            if value < first + count {
                return Ok(usize::from(code.symbols[index + value - first]));
            }
            index += count;
            first = (first + count) << 1;
            value <<= 1;
        }
        Err(Error::Corrupt("invalid implode code"))
    }
}

pub fn explode(data: &[u8], expected: usize) -> Result<Vec<u8>> {
    let literals = Code::from_run_lengths(&LITERAL_LENGTHS);
    let lengths = Code::from_run_lengths(&LENGTH_LENGTHS);
    let distances = Code::from_run_lengths(&DISTANCE_LENGTHS);

    let mut input = BitReader {
        data,
        position: 0,
        buffer: 0,
        available: 0,
    };
    let coded_literals = match input.bits(8)? {
        0 => false,
        1 => true,
        _ => return Err(Error::Corrupt("invalid implode literal mode")),
    };
    let dictionary_bits = input.bits(8)?;
    if !(4..=6).contains(&dictionary_bits) {
        return Err(Error::Corrupt("invalid implode dictionary size"));
    }

    let mut out = Vec::with_capacity(expected);
    loop {
        if input.bits(1)? == 0 {
            let literal = if coded_literals {
                input.decode(&literals)? as u8
            } else {
                input.bits(8)? as u8
            };
            out.push(literal);
        } else {
            let symbol = input.decode(&lengths)?;
            let length = LENGTH_BASE[symbol] + input.bits(LENGTH_EXTRA_BITS[symbol])? as usize;
            if length == END_OF_STREAM {
                break;
            }
            let low_bits = if length == 2 { 2 } else { dictionary_bits };
            let distance =
                (input.decode(&distances)? << low_bits) + input.bits(low_bits)? as usize + 1;
            if distance > out.len() {
                return Err(Error::Corrupt("implode distance before start of output"));
            }
            let start = out.len() - distance;
            for i in 0..length {
                out.push(out[start + i]);
            }
        }
        if out.len() > expected {
            return Err(Error::Corrupt("implode output exceeds expected size"));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each table must describe a complete prefix code over its alphabet.
    #[test]
    fn code_tables_are_complete() {
        for (runs, alphabet) in [
            (&LITERAL_LENGTHS[..], 256),
            (&LENGTH_LENGTHS[..], 16),
            (&DISTANCE_LENGTHS[..], 64),
        ] {
            let code = Code::from_run_lengths(runs);
            assert_eq!(code.symbol_count(), alphabet);
            let kraft: f64 = (1..=MAX_CODE_BITS)
                .map(|len| f64::from(code.counts[len]) / f64::from(1u32 << len))
                .sum();
            assert_eq!(kraft, 1.0);
        }
    }

    /// Hand-assembled stream with raw literals: "AB", then a copy of length 4
    /// at distance 2, then the end marker.
    #[test]
    fn decodes_raw_literals_and_back_references() {
        let mut bits = BitWriter::default();
        bits.push(0, 8); // raw literals
        bits.push(4, 8); // 1 KiB dictionary
        for byte in *b"AB" {
            bits.push(0, 1);
            bits.push(u32::from(byte), 8);
        }
        bits.push(1, 1);
        bits.push_code(&Code::from_run_lengths(&LENGTH_LENGTHS), 2); // base 4, no extra bits
        bits.push_code(&Code::from_run_lengths(&DISTANCE_LENGTHS), 0);
        bits.push(1, 4); // distance (0 << 4) + 1 + 1 = 2
        bits.push(1, 1);
        bits.push_code(&Code::from_run_lengths(&LENGTH_LENGTHS), 15); // base 264
        bits.push(255, 8); // 264 + 255 = end of stream
        assert_eq!(explode(&bits.finish(), 6).unwrap(), b"ABABAB");
    }

    #[derive(Default)]
    struct BitWriter {
        bytes: Vec<u8>,
        used: u32,
    }

    impl BitWriter {
        fn push(&mut self, value: u32, count: u32) {
            for i in 0..count {
                if self.used % 8 == 0 {
                    self.bytes.push(0);
                }
                let bit = ((value >> i) & 1) as u8;
                *self.bytes.last_mut().unwrap() |= bit << (self.used % 8);
                self.used += 1;
            }
        }

        /// Writes the inverted canonical code of `symbol`, most significant bit first.
        fn push_code(&mut self, code: &Code, symbol: u16) {
            let (mut first, mut index) = (0u32, 0usize);
            for len in 1..=MAX_CODE_BITS {
                let count = u32::from(code.counts[len]);
                let slice = &code.symbols[index..index + count as usize];
                if let Some(pos) = slice.iter().position(|&s| s == symbol) {
                    let value = first + pos as u32;
                    for bit in (0..len).rev() {
                        self.push(((value >> bit) & 1) ^ 1, 1);
                    }
                    return;
                }
                index += count as usize;
                first = (first + count) << 1;
            }
            panic!("symbol {symbol} has no code");
        }

        fn finish(self) -> Vec<u8> {
            self.bytes
        }
    }
}
