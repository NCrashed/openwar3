//! Decoder for BLP1, the texture format of Warcraft III.
//!
//! Format references: "BLP Specifications (WC3)" by Dr Super Good on Hive
//! Workshop, the BLP page on wowdev.wiki, and the format notes of the
//! JuicyPixels-blp library (BSD-3), which document the deviations found in
//! real files. Images are returned as 8-bit RGBA.

use std::fmt;

use jpeg_decoder::{ColorTransform, Decoder, PixelFormat};

const MAGIC: &[u8; 4] = b"BLP1";
const HEADER_SIZE: usize = 0x9C;
const MIP_SLOTS: usize = 16;
const PALETTE_ENTRIES: usize = 256;
const MAX_SIDE: u32 = 65535;
/// Default limit on the full size image, checked before anything is
/// allocated, because the header size fields are not trustworthy.
pub const DEFAULT_MAX_PIXELS: u64 = 8192 * 8192;

#[derive(Debug)]
pub enum Error {
    /// Not a BLP1 file, or a header field outside its valid range.
    InvalidHeader(&'static str),
    /// The full size image is larger than the caller allowed.
    TooLarge { width: u32, height: u32 },
    /// Mip level 0 is missing or damaged.
    Corrupt(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidHeader(what) => write!(f, "invalid BLP header: {what}"),
            Error::TooLarge { width, height } => {
                write!(f, "BLP image {width}x{height} exceeds the size limit")
            }
            Error::Corrupt(what) => write!(f, "corrupt BLP: {what}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Content {
    Jpeg,
    Paletted,
}

#[derive(Clone, Debug)]
pub struct Header {
    pub content: Content,
    pub alpha_bits: u32,
    pub width: u32,
    pub height: u32,
    /// No known effect on decoding; kept for tools that rewrite files.
    pub extra: u32,
    pub has_mipmaps: u32,
    pub mip_offsets: [u32; MIP_SLOTS],
    pub mip_sizes: [u32; MIP_SLOTS],
}

/// An 8-bit RGBA image, rows top to bottom.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    fn transparent(width: u32, height: u32) -> Image {
        Image {
            width,
            height,
            rgba: vec![0; width as usize * height as usize * 4],
        }
    }
}

pub fn read_header(data: &[u8]) -> Result<Header> {
    if data.len() < HEADER_SIZE {
        return Err(Error::InvalidHeader("file shorter than the header"));
    }
    if &data[..4] != MAGIC {
        return Err(Error::InvalidHeader("not a BLP1 file"));
    }
    let content = match le_u32(data, 0x04) {
        0 => Content::Jpeg,
        1 => Content::Paletted,
        _ => return Err(Error::InvalidHeader("unknown content type")),
    };
    let (width, height) = (le_u32(data, 0x0C), le_u32(data, 0x10));
    if !(1..=MAX_SIDE).contains(&width) || !(1..=MAX_SIDE).contains(&height) {
        return Err(Error::InvalidHeader("width or height out of range"));
    }
    Ok(Header {
        content,
        alpha_bits: le_u32(data, 0x08),
        width,
        height,
        extra: le_u32(data, 0x14),
        has_mipmaps: le_u32(data, 0x18),
        mip_offsets: std::array::from_fn(|i| le_u32(data, 0x1C + i * 4)),
        mip_sizes: std::array::from_fn(|i| le_u32(data, 0x5C + i * 4)),
    })
}

impl Header {
    /// Number of levels in a full mip chain: until the larger side reaches 1.
    pub fn full_chain_len(&self) -> usize {
        (32 - self.width.max(self.height).leading_zeros()) as usize
    }

    pub fn level_size(&self, level: usize) -> (u32, u32) {
        ((self.width >> level).max(1), (self.height >> level).max(1))
    }

    /// Byte ranges of the usable mip levels. The chain ends at the first slot
    /// that is empty, runs past the end of the file, points into the header,
    /// or overlaps an earlier level.
    fn level_ranges(&self, file_len: usize) -> Vec<std::ops::Range<usize>> {
        let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
        for level in 0..self.full_chain_len().min(MIP_SLOTS) {
            let start = self.mip_offsets[level] as usize;
            let size = self.mip_sizes[level] as usize;
            let end = start.saturating_add(size);
            let overlaps = ranges.iter().any(|r| start < r.end && r.start < end);
            if size == 0 || start < HEADER_SIZE || end > file_len || overlaps {
                break;
            }
            ranges.push(start..end);
        }
        ranges
    }
}

/// Decodes the full size image.
pub fn decode(data: &[u8]) -> Result<Image> {
    decode_levels(data, DEFAULT_MAX_PIXELS, 1).map(|mut levels| levels.swap_remove(0))
}

/// Decodes the full size image followed by its mipmaps. Decoding stops at the
/// first damaged level, so element `i` is always level `i`.
pub fn decode_mipmaps(data: &[u8]) -> Result<Vec<Image>> {
    decode_levels(data, DEFAULT_MAX_PIXELS, MIP_SLOTS)
}

pub fn decode_levels(data: &[u8], max_pixels: u64, max_levels: usize) -> Result<Vec<Image>> {
    let header = read_header(data)?;
    if u64::from(header.width) * u64::from(header.height) > max_pixels {
        return Err(Error::TooLarge {
            width: header.width,
            height: header.height,
        });
    }
    let mut ranges = header.level_ranges(data.len());
    ranges.truncate(max_levels);
    if ranges.is_empty() {
        return Err(Error::Corrupt(
            "mip level 0 is missing or out of range".into(),
        ));
    }
    match header.content {
        Content::Paletted => decode_paletted(&header, data, &ranges),
        Content::Jpeg => decode_jpeg(&header, data, &ranges),
    }
}

fn decode_paletted(
    header: &Header,
    data: &[u8],
    ranges: &[std::ops::Range<usize>],
) -> Result<Vec<Image>> {
    // Some tools write fewer than 256 palette entries and start mip 0 early.
    let palette_end = ranges[0].start.min(HEADER_SIZE + PALETTE_ENTRIES * 4);
    let mut palette = [[0u8; 3]; PALETTE_ENTRIES];
    for (entry, bgr) in palette
        .iter_mut()
        .zip(data[HEADER_SIZE..palette_end].chunks_exact(4))
    {
        *entry = [bgr[2], bgr[1], bgr[0]];
    }

    let pixels0 = level_pixels(header, 0);
    let declared_bits = match header.alpha_bits {
        bits @ (1 | 4 | 8) => bits,
        _ => 0,
    };
    // A declared alpha list that does not fit in mip 0 means there is none.
    let alpha_bits = if ranges[0].len() >= pixels0 + alpha_len(pixels0, declared_bits) {
        declared_bits
    } else {
        0
    };

    let mut levels = Vec::new();
    for (level, range) in ranges.iter().enumerate() {
        let pixels = level_pixels(header, level);
        let bytes = &data[range.clone()];
        if bytes.len() < pixels + alpha_len(pixels, alpha_bits) {
            if level == 0 {
                return Err(Error::Corrupt("mip level 0 is too short".into()));
            }
            break;
        }
        let (indices, alpha) = bytes.split_at(pixels);
        let (width, height) = header.level_size(level);
        let mut rgba = Vec::with_capacity(pixels * 4);
        for (i, &index) in indices.iter().enumerate() {
            rgba.extend_from_slice(&palette[usize::from(index)]);
            rgba.push(alpha_value(alpha, i, alpha_bits));
        }
        levels.push(Image {
            width,
            height,
            rgba,
        });
    }
    Ok(levels)
}

fn level_pixels(header: &Header, level: usize) -> usize {
    let (width, height) = header.level_size(level);
    width as usize * height as usize
}

fn alpha_len(pixels: usize, bits: u32) -> usize {
    (pixels * bits as usize).div_ceil(8)
}

/// Alpha values are packed least significant bit first.
fn alpha_value(alpha: &[u8], pixel: usize, bits: u32) -> u8 {
    if bits == 0 {
        return 0xFF;
    }
    let bit = pixel * bits as usize;
    let raw = alpha[bit / 8] >> (bit % 8);
    match bits {
        1 => (raw & 0x01) * 0xFF,
        4 => (raw & 0x0F) * 0x11,
        8 => raw,
        _ => 0xFF,
    }
}

fn decode_jpeg(
    header: &Header,
    data: &[u8],
    ranges: &[std::ops::Range<usize>],
) -> Result<Vec<Image>> {
    let shared_len = data
        .get(HEADER_SIZE..HEADER_SIZE + 4)
        .map(|b| le_u32(b, 0) as usize)
        .ok_or_else(|| Error::Corrupt("missing JPEG header size".into()))?;
    let shared = data
        .get(HEADER_SIZE + 4..HEADER_SIZE + 4 + shared_len)
        .ok_or_else(|| Error::Corrupt("JPEG header runs past the end of the file".into()))?;

    let mut levels = Vec::new();
    let mut use_alpha = None;
    for (level, range) in ranges.iter().enumerate() {
        let stream = [shared, &data[range.clone()]].concat();
        let (width, height) = header.level_size(level);
        let decoded = match decode_jpeg_level(&stream, width, height) {
            Ok(decoded) => decoded,
            Err(err) if level == 0 => return Err(err),
            Err(_) => break,
        };
        // Decided once from mip 0, so every level of a texture looks the same.
        let use_alpha = *use_alpha
            .get_or_insert_with(|| header.alpha_bits == 8 && decoded.alpha_plane_is_meaningful());
        levels.push(decoded.into_rgba(width, height, use_alpha));
    }
    Ok(levels)
}

struct JpegLevel {
    width: u32,
    height: u32,
    format: PixelFormat,
    pixels: Vec<u8>,
}

fn decode_jpeg_level(stream: &[u8], width: u32, height: u32) -> Result<JpegLevel> {
    let mut decoder = Decoder::new(stream);
    decoder
        .read_info()
        .map_err(|err| Error::Corrupt(format!("JPEG frame header: {err}")))?;
    let info = decoder
        .info()
        .ok_or_else(|| Error::Corrupt("JPEG without a frame header".into()))?;
    let (frame_width, frame_height) = (u32::from(info.width), u32::from(info.height));
    if frame_width > width || frame_height > height {
        return Err(Error::Corrupt(format!(
            "JPEG frame {frame_width}x{frame_height} is larger than its {width}x{height} level"
        )));
    }
    match info.pixel_format {
        // The four components are the B, G, R and A planes, not ink. Forcing
        // the CMYK transform makes the output independent of an Adobe marker;
        // it only inverts each sample, which is undone below.
        PixelFormat::CMYK32 => decoder.set_color_transform(ColorTransform::CMYK),
        PixelFormat::L16 => {
            return Err(Error::Corrupt("12-bit JPEG is not supported".into()));
        }
        PixelFormat::RGB24 | PixelFormat::L8 => {}
    }
    let mut pixels = decoder
        .decode()
        .map_err(|err| Error::Corrupt(format!("JPEG data: {err}")))?;
    if info.pixel_format == PixelFormat::CMYK32 {
        pixels.iter_mut().for_each(|v| *v = 255 - *v);
    }
    Ok(JpegLevel {
        width: frame_width,
        height: frame_height,
        format: info.pixel_format,
        pixels,
    })
}

impl JpegLevel {
    /// Tools fill an unused alpha plane with all 0x00 or all 0xFF; either
    /// means opaque. Any other plane, even a uniform one, is real alpha.
    fn alpha_plane_is_meaningful(&self) -> bool {
        if self.format != PixelFormat::CMYK32 {
            return true;
        }
        let mut alpha = self.pixels.iter().skip(3).step_by(4);
        match alpha.next() {
            None => false,
            Some(&first) => alpha.any(|&a| a != first) || (first != 0x00 && first != 0xFF),
        }
    }

    /// Converts to RGBA and pads a frame smaller than its level with
    /// transparent black, as Warcraft III does.
    fn into_rgba(self, width: u32, height: u32, use_alpha: bool) -> Image {
        let mut image = Image::transparent(width, height);
        let row = self.width as usize;
        for y in 0..self.height as usize {
            for x in 0..row {
                let i = y * row + x;
                let out = &mut image.rgba[(y * width as usize + x) * 4..][..4];
                match self.format {
                    PixelFormat::CMYK32 => {
                        let p = &self.pixels[i * 4..][..4];
                        out.copy_from_slice(&[
                            p[2],
                            p[1],
                            p[0],
                            if use_alpha { p[3] } else { 0xFF },
                        ]);
                    }
                    PixelFormat::RGB24 => {
                        let p = &self.pixels[i * 3..][..3];
                        out.copy_from_slice(&[p[0], p[1], p[2], 0xFF]);
                    }
                    PixelFormat::L8 | PixelFormat::L16 => {
                        let l = self.pixels[i];
                        out.copy_from_slice(&[l, l, l, 0xFF]);
                    }
                }
            }
        }
        image
    }
}

fn le_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

#[cfg(test)]
mod tests;
