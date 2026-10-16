//! `war3map.w3c`: camera setups placed in the editor.
//!
//! Supports format version 0, used by every map up to 1.27. Layout
//! reference: the community W3X/W3M format notes (originally by Zepir,
//! revised on Hive Workshop).

use crate::binary::{Error, Reader, Result};

pub const VERSION: u32 = 0;

#[derive(Clone, Debug, PartialEq)]
pub struct Camera {
    pub target_x: f32,
    pub target_y: f32,
    pub z_offset: f32,
    /// In degrees.
    pub rotation: f32,
    /// Angle of attack, in degrees.
    pub angle_of_attack: f32,
    pub distance: f32,
    pub roll: f32,
    pub field_of_view: f32,
    pub far_clipping: f32,
    /// A float after the far clipping distance whose meaning is not
    /// documented.
    pub unknown: f32,
    pub name: String,
}

pub fn parse(data: &[u8]) -> Result<Vec<Camera>> {
    let mut r = Reader::new(data);
    let version = r.u32()?;
    if version != VERSION {
        return Err(Error::UnsupportedVersion(version));
    }
    let cameras = r.list(41, |r| {
        Ok(Camera {
            target_x: r.f32()?,
            target_y: r.f32()?,
            z_offset: r.f32()?,
            rotation: r.f32()?,
            angle_of_attack: r.f32()?,
            distance: r.f32()?,
            roll: r.f32()?,
            field_of_view: r.f32()?,
            far_clipping: r.f32()?,
            unknown: r.f32()?,
            name: r.string()?,
        })
    })?;
    r.finish()?;
    Ok(cameras)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cameras() {
        let mut data = VERSION.to_le_bytes().to_vec();
        data.extend_from_slice(&1u32.to_le_bytes());
        for v in [
            100.0f32, -200.0, 0.0, 90.0, 304.0, 1650.0, 0.0, 70.0, 5000.0, 100.0,
        ] {
            data.extend_from_slice(&v.to_le_bytes());
        }
        data.extend_from_slice(b"Camera 001\0");
        let camera = &parse(&data).unwrap()[0];
        assert_eq!((camera.target_x, camera.target_y), (100.0, -200.0));
        assert_eq!((camera.angle_of_attack, camera.distance), (304.0, 1650.0));
        assert_eq!((camera.far_clipping, camera.unknown), (5000.0, 100.0));
        assert_eq!(camera.name, "Camera 001");
    }
}
