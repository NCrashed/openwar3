use jpeg_encoder::{ColorType, Encoder};

use super::*;

/// Assembles a BLP1 file from mip level payloads placed one after another.
fn build(
    content: Content,
    alpha_bits: u32,
    size: (u32, u32),
    prefix: &[u8],
    levels: &[Vec<u8>],
) -> Vec<u8> {
    let mut out = vec![0u8; HEADER_SIZE];
    out[..4].copy_from_slice(MAGIC);
    let content = match content {
        Content::Jpeg => 0u32,
        Content::Paletted => 1,
    };
    for (at, value) in [
        (0x04, content),
        (0x08, alpha_bits),
        (0x0C, size.0),
        (0x10, size.1),
        (0x14, 5),
        (0x18, 1),
    ] {
        out[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(prefix);
    for (i, level) in levels.iter().enumerate() {
        let offset = out.len() as u32;
        out[0x1C + i * 4..][..4].copy_from_slice(&offset.to_le_bytes());
        out[0x5C + i * 4..][..4].copy_from_slice(&(level.len() as u32).to_le_bytes());
        out.extend_from_slice(level);
    }
    out
}

/// Palette with entry `i` = (R, G, B) = (i, 255 - i, i / 2), stored as BGR0.
fn palette(entries: usize) -> Vec<u8> {
    (0..entries)
        .flat_map(|i| [(i / 2) as u8, 255 - i as u8, i as u8, 0])
        .collect()
}

fn rgb(index: u8) -> [u8; 3] {
    [index, 255 - index, index / 2]
}

#[test]
fn paletted_with_8_bit_alpha_and_mipmaps() {
    let mip0 = [
        vec![0, 1, 2, 3, 4, 5, 6, 7],
        vec![0, 50, 100, 150, 200, 250, 255, 1],
    ]
    .concat();
    let mip1 = vec![9, 10, 77, 88];
    let mip2 = vec![11, 99];
    let file = build(
        Content::Paletted,
        8,
        (4, 2),
        &palette(256),
        &[mip0, mip1, mip2],
    );

    let levels = decode_mipmaps(&file).unwrap();
    assert_eq!(levels.len(), 3);
    assert_eq!((levels[0].width, levels[0].height), (4, 2));
    assert_eq!((levels[1].width, levels[1].height), (2, 1));
    assert_eq!((levels[2].width, levels[2].height), (1, 1));
    assert_eq!(
        &levels[0].rgba[4..8],
        &[rgb(1)[0], rgb(1)[1], rgb(1)[2], 50]
    );
    assert_eq!(
        &levels[1].rgba[4..8],
        &[rgb(10)[0], rgb(10)[1], rgb(10)[2], 88]
    );
    assert_eq!(levels[2].rgba, [rgb(11)[0], rgb(11)[1], rgb(11)[2], 99]);
}

#[test]
fn paletted_alpha_is_packed_least_significant_bit_first() {
    let indices = vec![0u8; 8];
    let one_bit = build(
        Content::Paletted,
        1,
        (8, 1),
        &palette(256),
        &[[indices.clone(), vec![0b1000_0101]].concat()],
    );
    let alpha: Vec<u8> = decode(&one_bit)
        .unwrap()
        .rgba
        .iter()
        .skip(3)
        .step_by(4)
        .copied()
        .collect();
    assert_eq!(alpha, [255, 0, 255, 0, 0, 0, 0, 255]);

    let four_bit = build(
        Content::Paletted,
        4,
        (4, 1),
        &palette(256),
        &[[vec![0; 4], vec![0x2F, 0x0A]].concat()],
    );
    let alpha: Vec<u8> = decode(&four_bit)
        .unwrap()
        .rgba
        .iter()
        .skip(3)
        .step_by(4)
        .copied()
        .collect();
    assert_eq!(alpha, [0xFF, 0x22, 0xAA, 0x00]);
}

#[test]
fn declared_alpha_that_does_not_fit_means_opaque() {
    let file = build(
        Content::Paletted,
        8,
        (2, 2),
        &palette(256),
        &[vec![1, 2, 3, 4]],
    );
    let image = decode(&file).unwrap();
    assert!(image.rgba.iter().skip(3).step_by(4).all(|&a| a == 255));
    assert_eq!(&image.rgba[..3], &rgb(1));
}

#[test]
fn short_palette_is_padded_with_black() {
    let file = build(Content::Paletted, 0, (2, 1), &palette(16), &[vec![15, 200]]);
    let image = decode(&file).unwrap();
    assert_eq!(
        image.rgba,
        [rgb(15)[0], rgb(15)[1], rgb(15)[2], 255, 0, 0, 0, 255]
    );
}

#[test]
fn mip_chain_stops_at_the_first_bad_slot() {
    let mut file = build(
        Content::Paletted,
        0,
        (4, 4),
        &palette(256),
        &[vec![1; 16], vec![2; 4], vec![3; 1]],
    );
    // Point mip 2 back into mip 0.
    let mip0_offset = le_u32(&file, 0x1C);
    file[0x1C + 8..][..4].copy_from_slice(&mip0_offset.to_le_bytes());
    assert_eq!(decode_mipmaps(&file).unwrap().len(), 2);

    // Garbage in a slot beyond the full chain is ignored.
    let mut file = build(
        Content::Paletted,
        0,
        (2, 1),
        &palette(256),
        &[vec![1; 2], vec![2; 1], vec![3; 1]],
    );
    file[0x1C + 8..][..4].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
    assert_eq!(decode_mipmaps(&file).unwrap().len(), 2);
}

#[test]
fn missing_mip_0_is_an_error() {
    let mut file = build(Content::Paletted, 0, (2, 1), &palette(256), &[vec![1; 2]]);
    file[0x1C..0x20].copy_from_slice(&0x10u32.to_le_bytes()); // into the header
    assert!(matches!(decode(&file), Err(Error::Corrupt(_))));
}

#[test]
fn rejects_bad_headers() {
    let file = build(Content::Paletted, 0, (2, 1), &palette(256), &[vec![1; 2]]);
    let mut blp2 = file.clone();
    blp2[3] = b'2';
    assert!(matches!(decode(&blp2), Err(Error::InvalidHeader(_))));
    let mut zero_width = file.clone();
    zero_width[0x0C..0x10].copy_from_slice(&0u32.to_le_bytes());
    assert!(matches!(decode(&zero_width), Err(Error::InvalidHeader(_))));
    let big = build(Content::Paletted, 0, (9000, 9000), &[], &[]);
    assert!(matches!(decode(&big), Err(Error::TooLarge { .. })));
}

/// Encodes BGRA planes as a four-component JPEG. The encoder stores
/// `255 - value` for CMYK input, so the input is inverted first to make the
/// stream hold the planes as they are, like BLP files do.
fn bgra_jpeg(width: u16, height: u16, bgra: &[u8]) -> Vec<u8> {
    let inverted: Vec<u8> = bgra.iter().map(|v| 255 - v).collect();
    let mut out = Vec::new();
    Encoder::new(&mut out, 100)
        .encode(&inverted, width, height, ColorType::Cmyk)
        .unwrap();
    out
}

/// A level filled with one RGBA colour, as BGRA planes.
fn flat_bgra(width: u16, height: u16, [r, g, b, a]: [u8; 4]) -> Vec<u8> {
    [b, g, r, a].repeat(usize::from(width) * usize::from(height))
}

/// Splits whole JPEG streams into a shared header and per-level remainders.
fn jpeg_blp(alpha_bits: u32, size: (u32, u32), streams: &[Vec<u8>], shared_len: usize) -> Vec<u8> {
    let shared = &streams[0][..shared_len];
    assert!(streams.iter().all(|s| s.starts_with(shared)));
    let prefix = [(shared_len as u32).to_le_bytes().to_vec(), shared.to_vec()].concat();
    let levels: Vec<Vec<u8>> = streams.iter().map(|s| s[shared_len..].to_vec()).collect();
    build(Content::Jpeg, alpha_bits, size, &prefix, &levels)
}

fn assert_close(actual: &[u8], expected: [u8; 4]) {
    for (pixel, chunk) in actual.chunks_exact(4).enumerate() {
        for (c, (&got, want)) in chunk.iter().zip(expected).enumerate() {
            assert!(
                got.abs_diff(want) <= 2,
                "pixel {pixel} channel {c}: {got} vs {want}"
            );
        }
    }
}

#[test]
fn jpeg_planes_map_to_rgba_and_alpha_is_used() {
    let colour = [200, 100, 30, 128];
    let mip0 = bgra_jpeg(16, 16, &flat_bgra(16, 16, colour));
    let mip1 = bgra_jpeg(8, 8, &flat_bgra(8, 8, colour));
    let file = jpeg_blp(8, (16, 16), &[mip0, mip1], 2); // share the SOI marker only
    let levels = decode_mipmaps(&file).unwrap();
    assert_eq!(levels.len(), 2);
    assert_close(&levels[0].rgba, colour);
    assert_close(&levels[1].rgba, colour);
}

#[test]
fn jpeg_alpha_is_ignored_when_unused_or_not_declared() {
    for plane in [0x00, 0xFF] {
        let jpeg = bgra_jpeg(8, 8, &flat_bgra(8, 8, [10, 20, 30, plane]));
        let image = decode(&jpeg_blp(8, (8, 8), &[jpeg], 0)).unwrap();
        assert_close(&image.rgba, [10, 20, 30, 255]);
    }
    let jpeg = bgra_jpeg(8, 8, &flat_bgra(8, 8, [10, 20, 30, 77]));
    let image = decode(&jpeg_blp(0, (8, 8), &[jpeg], 0)).unwrap();
    assert_close(&image.rgba, [10, 20, 30, 255]);
}

#[test]
fn jpeg_frame_smaller_than_level_is_padded_with_transparent_black() {
    let jpeg = bgra_jpeg(8, 8, &flat_bgra(8, 8, [90, 90, 90, 200]));
    let image = decode(&jpeg_blp(8, (16, 8), &[jpeg], 0)).unwrap();
    assert_eq!((image.width, image.height), (16, 8));
    for row in image.rgba.chunks_exact(16 * 4) {
        assert_close(&row[..8 * 4], [90, 90, 90, 200]);
        assert!(row[8 * 4..].iter().all(|&v| v == 0));
    }
}

#[test]
fn jpeg_frame_larger_than_level_is_an_error() {
    let jpeg = bgra_jpeg(16, 16, &flat_bgra(16, 16, [1, 2, 3, 4]));
    assert!(matches!(
        decode(&jpeg_blp(8, (8, 8), &[jpeg], 0)),
        Err(Error::Corrupt(_))
    ));
}
