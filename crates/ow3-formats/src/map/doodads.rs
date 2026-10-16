//! `war3map.doo`: doodads (trees, rocks, destructibles) and special doodads
//! placed on the map.
//!
//! Supports format version 7 (Reign of Chaos maps) and 8 (Frozen Throne
//! maps up to 1.27). Layout reference: the community W3X/W3M format notes
//! (originally by Zepir, revised on Hive Workshop).

use crate::binary::{Error, RawCode, Reader, Result};

const MAGIC: &[u8; 4] = b"W3do";
pub const VERSION_REIGN_OF_CHAOS: u32 = 7;
pub const VERSION_FROZEN_THRONE: u32 = 8;

#[derive(Clone, Debug, PartialEq)]
pub struct Doodads {
    pub format_version: u32,
    pub subversion: u32,
    pub doodads: Vec<Doodad>,
    pub special_format_version: u32,
    pub special: Vec<SpecialDoodad>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Doodad {
    pub kind: RawCode,
    pub variation: i32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    /// Facing in radians.
    pub angle: f32,
    pub scale: [f32; 3],
    /// In community notes: 0 invisible and non-solid, 1 visible and
    /// non-solid, 2 normal.
    pub flags: u8,
    /// Life in percent of the maximum.
    pub life_percent: u8,
    /// Frozen Throne only: index of a map random item table, or -1.
    pub item_table: i32,
    /// Frozen Throne only: item sets dropped when destroyed.
    pub dropped_item_sets: Vec<Vec<DroppedItem>>,
    /// Number the editor uses to refer to this doodad, for example in
    /// triggers.
    pub editor_id: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DroppedItem {
    pub item: RawCode,
    /// Percentage chance.
    pub chance: i32,
}

/// Doodads that are part of the terrain, such as cliff and ramp details.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpecialDoodad {
    pub kind: RawCode,
    pub z: i32,
    /// Position in tiles.
    pub x: i32,
    pub y: i32,
}

impl Doodads {
    pub fn parse(data: &[u8]) -> Result<Doodads> {
        let mut r = Reader::new(data);
        if r.bytes(4)? != MAGIC {
            return Err(Error::Invalid {
                what: "doodad signature",
                offset: 0,
            });
        }
        let format_version = r.u32()?;
        let tft = match format_version {
            VERSION_REIGN_OF_CHAOS => false,
            VERSION_FROZEN_THRONE => true,
            other => return Err(Error::UnsupportedVersion(other)),
        };
        let subversion = r.u32()?;
        let min_size = if tft { 50 } else { 42 };
        let doodads = r.list(min_size, |r| {
            let kind = r.raw_code()?;
            let variation = r.i32()?;
            let (x, y, z) = (r.f32()?, r.f32()?, r.f32()?);
            let angle = r.f32()?;
            let scale = [r.f32()?, r.f32()?, r.f32()?];
            let flags = r.u8()?;
            let life_percent = r.u8()?;
            let (item_table, dropped_item_sets) = if tft {
                let table = r.i32()?;
                let sets = r.list(4, |r| {
                    r.list(8, |r| {
                        Ok(DroppedItem {
                            item: r.raw_code()?,
                            chance: r.i32()?,
                        })
                    })
                })?;
                (table, sets)
            } else {
                (-1, Vec::new())
            };
            Ok(Doodad {
                kind,
                variation,
                x,
                y,
                z,
                angle,
                scale,
                flags,
                life_percent,
                item_table,
                dropped_item_sets,
                editor_id: r.i32()?,
            })
        })?;
        let special_format_version = r.u32()?;
        let special = r.list(16, |r| {
            Ok(SpecialDoodad {
                kind: r.raw_code()?,
                z: r.i32()?,
                x: r.i32()?,
                y: r.i32()?,
            })
        })?;
        r.finish()?;
        Ok(Doodads {
            format_version,
            subversion,
            doodads,
            special_format_version,
            special,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(version: u32, count: u32) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        for v in [version, 11, count] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }

    fn doodad_common(out: &mut Vec<u8>, kind: &[u8; 4]) {
        out.extend_from_slice(kind);
        out.extend_from_slice(&3i32.to_le_bytes());
        for v in [64.0f32, -32.0, 8.0, 1.5, 1.0, 1.0, 2.0] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&[2, 100]);
    }

    fn special_section(out: &mut Vec<u8>) {
        for v in [0u32, 1] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(b"YOlb");
        for v in [0i32, 12, 34] {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }

    #[test]
    fn parses_reign_of_chaos_doodads() {
        let mut data = header(VERSION_REIGN_OF_CHAOS, 1);
        doodad_common(&mut data, b"LTlt");
        data.extend_from_slice(&42i32.to_le_bytes());
        special_section(&mut data);

        let parsed = Doodads::parse(&data).unwrap();
        let d = &parsed.doodads[0];
        assert_eq!((d.kind, d.variation), (RawCode(*b"LTlt"), 3));
        assert_eq!((d.x, d.y, d.z, d.angle), (64.0, -32.0, 8.0, 1.5));
        assert_eq!(d.scale, [1.0, 1.0, 2.0]);
        assert_eq!((d.flags, d.life_percent, d.editor_id), (2, 100, 42));
        assert_eq!((d.item_table, d.dropped_item_sets.len()), (-1, 0));
        let s = parsed.special[0];
        assert_eq!((s.kind, s.x, s.y), (RawCode(*b"YOlb"), 12, 34));
    }

    #[test]
    fn parses_frozen_throne_dropped_items() {
        let mut data = header(VERSION_FROZEN_THRONE, 1);
        doodad_common(&mut data, b"LTbr");
        data.extend_from_slice(&(-1i32).to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes()); // one set
        data.extend_from_slice(&2u32.to_le_bytes()); // two items
        data.extend_from_slice(b"ratc");
        data.extend_from_slice(&75i32.to_le_bytes());
        data.extend_from_slice(b"rde1");
        data.extend_from_slice(&25i32.to_le_bytes());
        data.extend_from_slice(&7i32.to_le_bytes());
        special_section(&mut data);

        let parsed = Doodads::parse(&data).unwrap();
        let d = &parsed.doodads[0];
        assert_eq!(d.editor_id, 7);
        assert_eq!(
            d.dropped_item_sets,
            [vec![
                DroppedItem {
                    item: RawCode(*b"ratc"),
                    chance: 75
                },
                DroppedItem {
                    item: RawCode(*b"rde1"),
                    chance: 25
                },
            ]]
        );
    }

    #[test]
    fn rejects_other_versions() {
        assert_eq!(
            Doodads::parse(&header(9, 0)),
            Err(Error::UnsupportedVersion(9))
        );
    }
}
