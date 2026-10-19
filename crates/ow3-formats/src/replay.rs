//! Replays (`.w3g`) of Warcraft III 1.07 and later, after the public
//! community documentation of the format.
//!
//! A replay holds no state of the game: it is the setup (players, slots,
//! the random seed, the map) and then the actions of the players, sent in
//! time slots of the network, which the game plays again with its own
//! simulation.
//!
//! The file is a header and a stream compressed in blocks of 8 KiB with
//! zlib. The stream starts with the game's records (players, the game
//! name, an encoded string with the settings and the map, the slots and
//! the seed) and goes on with blocks: time slots with the actions of each
//! player, chat, players leaving and checksums.

use flate2::read::ZlibDecoder;
use std::io::Read;

use crate::binary::{Error, Reader, Result};

const MAGIC: &[u8; 28] = b"Warcraft III recorded game\x1a\0";

/// The header of a replay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    /// `WAR3` (Reign of Chaos) or `W3XP` (The Frozen Throne).
    pub product: [u8; 4],
    /// The patch version (27 for 1.27) and the build number.
    pub version: u32,
    pub build: u16,
    /// Bit 0x8000: a multiplayer game.
    pub flags: u16,
    /// The length of the game in milliseconds.
    pub length: u32,
}

/// A player of the game record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Player {
    pub id: u8,
    pub name: String,
    /// The player who saved the replay (the first record).
    pub host: bool,
}

/// A slot of the game start record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    /// The player in the slot (0 for none or a computer).
    pub player: u8,
    /// 0 empty, 1 closed, 2 used.
    pub status: u8,
    pub computer: bool,
    /// The team (12 for observers) and the colour.
    pub team: u8,
    pub color: u8,
    /// Race flags: 0x01 human, 0x02 orc, 0x04 night elf, 0x08 undead,
    /// 0x20 random, 0x40 fixed.
    pub race: u8,
    /// Computer strength: 0 easy, 1 normal, 2 insane.
    pub ai: u8,
    pub handicap: u8,
}

/// The settings of the encoded string: what the map and its creator are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// The settings bytes before the map's checksum (speed, visibility,
    /// observers, teams fixed, ...), as decoded.
    pub flags: Vec<u8>,
    pub map_checksum: u32,
    pub map_path: String,
    pub creator: String,
}

/// An action of a player inside a time slot.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Pause,
    Resume,
    SetSpeed(u8),
    IncreaseSpeed,
    DecreaseSpeed,
    SaveGame(String),
    SaveFinished(u32),
    /// An order without a target (`0x10`): flags, order id (a raw code or
    /// an order number) and two words of unknown use.
    Order {
        flags: u16,
        order: u32,
        unknown: [u32; 2],
    },
    /// An order to a point (`0x11`).
    PointOrder {
        flags: u16,
        order: u32,
        unknown: [u32; 2],
        x: f32,
        y: f32,
    },
    /// An order to a point and an object (`0x12`): the object's two ids.
    TargetOrder {
        flags: u16,
        order: u32,
        unknown: [u32; 2],
        x: f32,
        y: f32,
        target: [u32; 2],
    },
    /// Giving or dropping an item (`0x13`).
    ItemOrder {
        flags: u16,
        order: u32,
        unknown: [u32; 2],
        x: f32,
        y: f32,
        target: [u32; 2],
        item: [u32; 2],
    },
    /// An order with two targets (`0x14`), kept as its bytes.
    TwoTargetOrder(Vec<u8>),
    /// A change of the selection (`0x16`): 1 adds, 2 removes these units.
    Select {
        mode: u8,
        units: Vec<[u32; 2]>,
    },
    /// A control group (`0x17`) made of these units.
    AssignGroup {
        group: u8,
        units: Vec<[u32; 2]>,
    },
    /// A control group selected (`0x18`).
    SelectGroup(u8),
    /// The subgroup of a selection (`0x19`): the type and the unit.
    SelectSubgroup {
        item: u32,
        unit: [u32; 2],
    },
    PreSubselection,
    /// Selecting an item on the ground (`0x1C`).
    SelectItem([u32; 2]),
    CancelHeroRevival([u32; 2]),
    /// A unit leaves the building queue (`0x1E`): slot and type.
    Dequeue {
        slot: u8,
        item: u32,
    },
    /// Alliance flags towards a player (`0x50`).
    Alliance {
        player: u8,
        flags: u32,
    },
    /// Gold and lumber given to a player (`0x51`).
    Transfer {
        player: u8,
        gold: u32,
        lumber: u32,
    },
    /// Chat text the map's triggers listen to (`0x60`).
    TriggerChat(String),
    EscapePressed,
    /// A signal on the minimap (`0x68`).
    MinimapSignal {
        x: f32,
        y: f32,
    },
    /// Any other action, with its bytes; the rest of the player's block is
    /// kept here when its length is not known.
    Other {
        id: u8,
        data: Vec<u8>,
    },
}

/// A block of the replay after the game's records.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The game time advanced by `ms`, with the actions of the players in
    /// this slot, in the order sent.
    TimeSlot {
        ms: u16,
        actions: Vec<(u8, Vec<Action>)>,
        /// What the block adds to the network sum of the turn checksum
        /// (SPEC-0024): `bh(id + bh(w))`, with `w` the block's bytes after
        /// its length folded as words and then bytes.
        network: u32,
    },
    Chat {
        player: u8,
        text: String,
    },
    /// A player left the game.
    Leave {
        player: u8,
        reason: u32,
        result: u32,
    },
    /// A checksum of the game sent with the time slots.
    Checksum(u32),
    /// The start blocks and others whose content is not used.
    Other(u8),
}

/// A replay: its header, players and setup, and what happened.
#[derive(Clone, Debug, PartialEq)]
pub struct Replay {
    pub header: Header,
    pub players: Vec<Player>,
    pub game_name: String,
    pub settings: Settings,
    pub slots: Vec<Slot>,
    /// The random seed of the game.
    pub seed: u32,
    pub events: Vec<Event>,
}

impl Replay {
    pub fn parse(data: &[u8]) -> Result<Replay> {
        let mut r = Reader::new(data);
        if r.bytes(28)? != MAGIC {
            return Err(Error::Invalid {
                what: "replay signature",
                offset: 0,
            });
        }
        let header_size = r.u32()? as usize;
        let _compressed_size = r.u32()?;
        let header_version = r.u32()?;
        if header_version != 1 {
            return Err(Error::UnsupportedVersion(header_version));
        }
        let decompressed_size = r.u32()? as usize;
        let blocks = r.u32()?;
        let mut product = [0u8; 4];
        product.copy_from_slice(r.bytes(4)?);
        product.reverse();
        let version = r.u32()?;
        let build = r.u16()?;
        let flags = r.u16()?;
        let length = r.u32()?;
        let _crc = r.u32()?;
        let header = Header {
            product,
            version,
            build,
            flags,
            length,
        };
        let mut r = Reader::new(data.get(header_size..).ok_or(Error::UnexpectedEnd {
            offset: header_size,
        })?);
        let mut stream = Vec::with_capacity(decompressed_size);
        for _ in 0..blocks {
            let at = header_size + r.position();
            let compressed = usize::from(r.u16()?);
            let size = usize::from(r.u16()?);
            let _checksum = r.u32()?;
            let mut block = Vec::with_capacity(size);
            ZlibDecoder::new(r.bytes(compressed)?)
                .take(size as u64)
                .read_to_end(&mut block)
                .map_err(|_| Error::Invalid {
                    what: "compressed block",
                    offset: at,
                })?;
            stream.extend(block);
        }
        stream.truncate(decompressed_size);
        parse_stream(header, &stream)
    }
}

/// A null-terminated string, read whatever its encoding.
fn text(r: &mut Reader) -> Result<String> {
    let mut bytes = Vec::new();
    loop {
        match r.u8()? {
            0 => return Ok(String::from_utf8_lossy(&bytes).into_owned()),
            b => bytes.push(b),
        }
    }
}

/// The bytes of the encoded string: in every group of 8, the first byte
/// is a mask, and each of the others is one more than its value when its
/// bit of the mask (from bit 1) is clear.
fn decode_string(encoded: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(encoded.len());
    for group in encoded.chunks(8) {
        let mask = group[0];
        for (i, &b) in group[1..].iter().enumerate() {
            out.push(if mask & (1 << (i + 1)) == 0 {
                b.wrapping_sub(1)
            } else {
                b
            });
        }
    }
    out
}

fn player_record(r: &mut Reader) -> Result<Player> {
    let host = r.u8()? == 0;
    let id = r.u8()?;
    let name = text(r)?;
    let extra = usize::from(r.u8()?);
    r.bytes(extra)?;
    Ok(Player { id, name, host })
}

fn parse_stream(header: Header, data: &[u8]) -> Result<Replay> {
    let mut r = Reader::new(data);
    r.u32()?;
    let mut players = vec![player_record(&mut r)?];
    let game_name = text(&mut r)?;
    r.u8()?;
    let start = r.position();
    let encoded = &data[start..]
        .iter()
        .position(|&b| b == 0)
        .map(|n| &data[start..start + n])
        .ok_or(Error::UnexpectedEnd { offset: start })?;
    r.bytes(encoded.len() + 1)?;
    let settings = settings(&decode_string(encoded))?;
    let _player_count = r.u32()?;
    let _game_type = r.u32()?;
    let _language = r.u32()?;
    while data.get(r.position()) == Some(&0x16) {
        r.u8()?;
        players.push(player_record(&mut r)?);
        r.u32()?;
    }
    // The game start record.
    let at = r.position();
    if r.u8()? != 0x19 {
        return Err(Error::Invalid {
            what: "game start record",
            offset: at,
        });
    }
    let _size = r.u16()?;
    let count = r.u8()?;
    let mut slots = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let player = r.u8()?;
        let _download = r.u8()?;
        let status = r.u8()?;
        let computer = r.u8()? != 0;
        let team = r.u8()?;
        let color = r.u8()?;
        let race = r.u8()?;
        let ai = r.u8()?;
        let handicap = r.u8()?;
        slots.push(Slot {
            player,
            status,
            computer,
            team,
            color,
            race,
            ai,
            handicap,
        });
    }
    let seed = r.u32()?;
    let _select_mode = r.u8()?;
    let _start_spots = r.u8()?;
    let events = events(&mut r)?;
    Ok(Replay {
        header,
        players,
        game_name,
        settings,
        slots,
        seed,
        events,
    })
}

fn settings(decoded: &[u8]) -> Result<Settings> {
    let mut r = Reader::new(decoded);
    let flags = r.bytes(9)?.to_vec();
    let map_checksum = r.u32()?;
    let map_path = text(&mut r)?;
    let creator = text(&mut r)?;
    Ok(Settings {
        flags,
        map_checksum,
        map_path,
        creator,
    })
}

fn events(r: &mut Reader) -> Result<Vec<Event>> {
    let mut events = Vec::new();
    while r.remaining() > 0 {
        let at = r.position();
        let id = r.u8()?;
        let event = match id {
            // Padding to the end of the last block.
            0x00 => break,
            0x17 => {
                let reason = r.u32()?;
                let player = r.u8()?;
                let result = r.u32()?;
                r.u32()?;
                Event::Leave {
                    player,
                    reason,
                    result,
                }
            }
            0x1A..=0x1C => {
                r.u32()?;
                Event::Other(id)
            }
            0x1E | 0x1F => {
                let size = usize::from(r.u16()?);
                let block = r.bytes(size)?;
                time_slot(id, block, at)?
            }
            0x20 => {
                let player = r.u8()?;
                let size = usize::from(r.u16()?);
                let mut c = Reader::new(r.bytes(size)?);
                let flags = c.u8()?;
                if flags == 0x20 {
                    c.u32()?;
                }
                Event::Chat {
                    player,
                    text: text(&mut c)?,
                }
            }
            0x22 => {
                let size = usize::from(r.u8()?);
                let block = r.bytes(size)?;
                let mut c = Reader::new(block);
                Event::Checksum(if size >= 4 { c.u32()? } else { 0 })
            }
            0x23 => {
                r.bytes(10)?;
                Event::Other(id)
            }
            0x2F => {
                r.bytes(8)?;
                Event::Other(id)
            }
            _ => {
                return Err(Error::Invalid {
                    what: "replay block",
                    offset: at,
                });
            }
        };
        events.push(event);
    }
    Ok(events)
}

fn time_slot(id: u8, block: &[u8], at: usize) -> Result<Event> {
    let mut r = Reader::new(block);
    let ms = r.u16()?;
    let mut actions = Vec::new();
    while r.remaining() > 0 {
        let player = r.u8()?;
        let size = usize::from(r.u16()?);
        let list = player_actions(r.bytes(size)?).map_err(|e| match e {
            Error::UnexpectedEnd { .. } => Error::Invalid {
                what: "player actions",
                offset: at,
            },
            e => e,
        })?;
        actions.push((player, list));
    }
    let words = block.chunks_exact(4);
    let rest = words.remainder();
    let mut w = 0;
    for word in words {
        w = fold(w, u32::from_le_bytes([word[0], word[1], word[2], word[3]]));
    }
    for &byte in rest {
        w = fold(w, u32::from(byte));
    }
    Ok(Event::TimeSlot {
        ms,
        actions,
        network: bh(u32::from(id).wrapping_add(bh(w))),
    })
}

/// The step of the turn checksum (SPEC-0024): `rol3(h XOR x)`.
pub fn fold(h: u32, x: u32) -> u32 {
    (h ^ x).rotate_left(3)
}

/// The byte hash of the turn checksum (SPEC-0024): the sum over the four
/// bytes `b` of `x` of `(b * 0x7A2D) XOR b`.
pub fn bh(x: u32) -> u32 {
    x.to_le_bytes()
        .iter()
        .map(|&b| (u32::from(b) * 0x7A2D) ^ u32::from(b))
        .fold(0, u32::wrapping_add)
}

fn units(r: &mut Reader) -> Result<Vec<[u32; 2]>> {
    let n = usize::from(r.u16()?);
    (0..n).map(|_| Ok([r.u32()?, r.u32()?])).collect()
}

fn player_actions(block: &[u8]) -> Result<Vec<Action>> {
    let mut r = Reader::new(block);
    let mut out = Vec::new();
    while r.remaining() > 0 {
        let id = r.u8()?;
        let pair = |r: &mut Reader| -> Result<[u32; 2]> { Ok([r.u32()?, r.u32()?]) };
        let action = match id {
            0x01 => Action::Pause,
            0x02 => Action::Resume,
            0x03 => Action::SetSpeed(r.u8()?),
            0x04 => Action::IncreaseSpeed,
            0x05 => Action::DecreaseSpeed,
            0x06 => Action::SaveGame(text(&mut r)?),
            0x07 => Action::SaveFinished(r.u32()?),
            0x10..=0x13 => {
                let flags = r.u16()?;
                let order = r.u32()?;
                let unknown = pair(&mut r)?;
                if id == 0x10 {
                    Action::Order {
                        flags,
                        order,
                        unknown,
                    }
                } else {
                    let (x, y) = (r.f32()?, r.f32()?);
                    match id {
                        0x11 => Action::PointOrder {
                            flags,
                            order,
                            unknown,
                            x,
                            y,
                        },
                        0x12 => Action::TargetOrder {
                            flags,
                            order,
                            unknown,
                            x,
                            y,
                            target: pair(&mut r)?,
                        },
                        _ => Action::ItemOrder {
                            flags,
                            order,
                            unknown,
                            x,
                            y,
                            target: pair(&mut r)?,
                            item: pair(&mut r)?,
                        },
                    }
                }
            }
            0x14 => Action::TwoTargetOrder(r.bytes(43)?.to_vec()),
            0x16 => {
                let mode = r.u8()?;
                Action::Select {
                    mode,
                    units: units(&mut r)?,
                }
            }
            0x17 => {
                let group = r.u8()?;
                Action::AssignGroup {
                    group,
                    units: units(&mut r)?,
                }
            }
            0x18 => {
                let group = r.u8()?;
                r.u8()?;
                Action::SelectGroup(group)
            }
            0x19 => Action::SelectSubgroup {
                item: r.u32()?,
                unit: pair(&mut r)?,
            },
            0x1A => Action::PreSubselection,
            0x1C => {
                r.u8()?;
                Action::SelectItem(pair(&mut r)?)
            }
            0x1D => Action::CancelHeroRevival(pair(&mut r)?),
            0x1E => Action::Dequeue {
                slot: r.u8()?,
                item: r.u32()?,
            },
            0x50 => Action::Alliance {
                player: r.u8()?,
                flags: r.u32()?,
            },
            0x51 => Action::Transfer {
                player: r.u8()?,
                gold: r.u32()?,
                lumber: r.u32()?,
            },
            0x60 => {
                pair(&mut r)?;
                Action::TriggerChat(text(&mut r)?)
            }
            0x61 => Action::EscapePressed,
            0x68 => {
                let (x, y) = (r.f32()?, r.f32()?);
                r.u32()?;
                Action::MinimapSignal { x, y }
            }
            _ => {
                // Known lengths of the actions not interpreted.
                let len = match id {
                    0x1B => Some(9),
                    0x21 => Some(8),
                    0x20 | 0x22..=0x26 | 0x29..=0x2C | 0x2F..=0x32 => Some(0),
                    0x27 | 0x28 | 0x2D => Some(5),
                    0x2E => Some(4),
                    0x62 => Some(12),
                    0x66 | 0x67 => Some(0),
                    0x69 | 0x6A => Some(16),
                    0x75 => Some(1),
                    _ => None,
                };
                let len = len.unwrap_or(r.remaining());
                Action::Other {
                    id,
                    data: r.bytes(len)?.to_vec(),
                }
            }
        };
        out.push(action);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0024, verification 1.
    #[test]
    fn byte_hash() {
        assert_eq!(bh(0x0000_0001), 0x7A2C);
        assert_eq!(bh(0x0000_0100), 0x7A2C);
        assert_eq!(bh(0x0000_00FF), 0x79_B22C);
        assert_eq!(bh(0x1234_5678), 0x83_B8F0);
        assert_eq!(bh(0xFFFF_FFFF), 0x1E6_C8B0);
    }

    #[test]
    fn encoded_string() {
        // Mask 0b0000_0100: the second byte is kept, the others less one.
        assert_eq!(decode_string(&[0x04, 0x02, 0x05, 0x04]), [1, 5, 3]);
    }

    /// Actions of every interpreted kind and an unknown one, which takes the
    /// rest of the block.
    #[test]
    fn actions() {
        let mut block = vec![0x01, 0x03, 2, 0x11];
        block.extend(3u16.to_le_bytes());
        block.extend(0x000D_0003u32.to_le_bytes());
        block.extend([0xFF; 8]);
        block.extend(100.0f32.to_le_bytes());
        block.extend((-50.0f32).to_le_bytes());
        block.extend([0x16, 1, 1, 0, 7, 0, 0, 0, 8, 0, 0, 0]);
        block.extend([0x99, 1, 2]);
        let actions = player_actions(&block).unwrap();
        assert_eq!(
            actions,
            [
                Action::Pause,
                Action::SetSpeed(2),
                Action::PointOrder {
                    flags: 3,
                    order: 0x000D_0003,
                    unknown: [u32::MAX; 2],
                    x: 100.0,
                    y: -50.0,
                },
                Action::Select {
                    mode: 1,
                    units: vec![[7, 8]],
                },
                Action::Other {
                    id: 0x99,
                    data: vec![1, 2],
                },
            ]
        );
    }
}
