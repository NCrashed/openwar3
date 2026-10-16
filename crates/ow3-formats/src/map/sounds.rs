//! `war3map.w3s`: sound and music definitions used by triggers.
//!
//! Supports format version 1, used by every map up to 1.27. Layout
//! reference: the community W3X/W3M format notes (originally by Zepir,
//! revised on Hive Workshop). Several fields have no documented meaning;
//! they are kept as stored.

use crate::binary::{Error, Reader, Result};

pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq)]
pub struct Sound {
    /// Variable name, such as `gg_snd_Rain`.
    pub name: String,
    pub file: String,
    /// EAX effect, such as `DefaultEAXON`.
    pub eax_effect: String,
    /// In community notes: 0x1 looping, 0x2 3D sound, 0x4 stop when out of
    /// range, 0x8 music.
    pub flags: u32,
    pub fade_in_rate: i32,
    pub fade_out_rate: i32,
    /// -1 for the default volume.
    pub volume: i32,
    pub pitch: f32,
    pub unknown_a: f32,
    pub unknown_b: i32,
    pub channel: i32,
    pub min_distance: f32,
    pub max_distance: f32,
    pub distance_cutoff: f32,
    pub unknown_c: f32,
    pub unknown_d: f32,
    pub unknown_e: i32,
    pub unknown_f: f32,
    pub unknown_g: f32,
    pub unknown_h: f32,
}

pub fn parse(data: &[u8]) -> Result<Vec<Sound>> {
    let mut r = Reader::new(data);
    let version = r.u32()?;
    if version != VERSION {
        return Err(Error::UnsupportedVersion(version));
    }
    let sounds = r.list(75, |r| {
        Ok(Sound {
            name: r.string()?,
            file: r.string()?,
            eax_effect: r.string()?,
            flags: r.u32()?,
            fade_in_rate: r.i32()?,
            fade_out_rate: r.i32()?,
            volume: r.i32()?,
            pitch: r.f32()?,
            unknown_a: r.f32()?,
            unknown_b: r.i32()?,
            channel: r.i32()?,
            min_distance: r.f32()?,
            max_distance: r.f32()?,
            distance_cutoff: r.f32()?,
            unknown_c: r.f32()?,
            unknown_d: r.f32()?,
            unknown_e: r.i32()?,
            unknown_f: r.f32()?,
            unknown_g: r.f32()?,
            unknown_h: r.f32()?,
        })
    })?;
    r.finish()?;
    Ok(sounds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sounds() {
        let mut data = VERSION.to_le_bytes().to_vec();
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(b"gg_snd_Rain\0Ambient\\Rain.wav\0DefaultEAXON\0");
        data.extend_from_slice(&3u32.to_le_bytes());
        for v in [10i32, 10, -1] {
            data.extend_from_slice(&v.to_le_bytes());
        }
        data.extend_from_slice(&1.0f32.to_le_bytes());
        data.extend_from_slice(&0.0f32.to_le_bytes());
        data.extend_from_slice(&(-1i32).to_le_bytes());
        data.extend_from_slice(&0i32.to_le_bytes());
        for v in [600.0f32, 10000.0, 3000.0, 0.0, 0.0] {
            data.extend_from_slice(&v.to_le_bytes());
        }
        data.extend_from_slice(&127i32.to_le_bytes());
        for v in [0.0f32, 0.0, 0.0] {
            data.extend_from_slice(&v.to_le_bytes());
        }

        let sound = &parse(&data).unwrap()[0];
        assert_eq!(sound.file, "Ambient\\Rain.wav");
        assert_eq!((sound.flags, sound.volume, sound.pitch), (3, -1, 1.0));
        assert_eq!((sound.min_distance, sound.distance_cutoff), (600.0, 3000.0));
        assert_eq!(sound.unknown_e, 127);
    }
}
