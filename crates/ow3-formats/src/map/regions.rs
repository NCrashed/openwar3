//! `war3map.w3r`: named rectangular regions, used by triggers.
//!
//! Supports format version 5. Layout reference: the community W3X/W3M
//! format notes (originally by Zepir, revised on Hive Workshop).

use crate::binary::{Error, RawCode, Reader, Result};

pub const VERSION: u32 = 5;

#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    /// World coordinates of the rectangle.
    pub left: f32,
    pub bottom: f32,
    pub right: f32,
    pub top: f32,
    pub name: String,
    /// Number the editor and the generated script use for this region.
    pub index: i32,
    /// Weather effect inside the region, or `None`.
    pub weather: Option<RawCode>,
    /// Name of an ambient sound from `war3map.w3s`, or empty.
    pub ambient_sound: String,
    /// Editor colour as stored: blue, green, red.
    pub color_bgr: [u8; 3],
    /// Byte after the colour; 0xFF in every shipped map.
    pub end: u8,
}

pub fn parse(data: &[u8]) -> Result<Vec<Region>> {
    let mut r = Reader::new(data);
    let version = r.u32()?;
    if version != VERSION {
        return Err(Error::UnsupportedVersion(version));
    }
    let regions = r.list(33, |r| {
        Ok(Region {
            left: r.f32()?,
            bottom: r.f32()?,
            right: r.f32()?,
            top: r.f32()?,
            name: r.string()?,
            index: r.i32()?,
            weather: Some(r.raw_code()?).filter(|code| code.0 != [0; 4]),
            ambient_sound: r.string()?,
            color_bgr: [r.u8()?, r.u8()?, r.u8()?],
            end: r.u8()?,
        })
    })?;
    r.finish()?;
    Ok(regions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_regions() {
        let mut data = VERSION.to_le_bytes().to_vec();
        data.extend_from_slice(&1u32.to_le_bytes());
        for v in [-256.0f32, -128.0, 512.0, 640.0] {
            data.extend_from_slice(&v.to_le_bytes());
        }
        data.extend_from_slice(b"Spawn\0");
        data.extend_from_slice(&3i32.to_le_bytes());
        data.extend_from_slice(b"RAhr");
        data.extend_from_slice(b"gg_snd_Rain\0");
        data.extend_from_slice(&[10, 20, 30, 0xFF]);

        let regions = parse(&data).unwrap();
        let region = &regions[0];
        assert_eq!((region.left, region.top), (-256.0, 640.0));
        assert_eq!((region.name.as_str(), region.index), ("Spawn", 3));
        assert_eq!(region.weather, Some(RawCode(*b"RAhr")));
        assert_eq!(region.ambient_sound, "gg_snd_Rain");
        assert_eq!((region.color_bgr, region.end), ([10, 20, 30], 0xFF));
    }

    #[test]
    fn rejects_other_versions() {
        assert_eq!(
            parse(&4u32.to_le_bytes()),
            Err(Error::UnsupportedVersion(4))
        );
    }
}
