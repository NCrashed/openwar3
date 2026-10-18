//! A map archive and the game archives, searched by priority.

use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek};
use std::path::Path;

use super::{Archive, Result};

/// Game archives by priority, highest first (SPEC-0008).
pub const GAME_ARCHIVES: [&str; 4] = ["War3Patch.mpq", "War3xlocal.mpq", "War3x.mpq", "War3.mpq"];

/// What an archive of the chain is read from: a file, or the bytes of an
/// archive inside another one.
trait Source: Read + Seek {}
impl<T: Read + Seek> Source for T {}

/// Archives searched in order: the map (if any), the archive of the map's
/// tileset (once mounted), then the game archives found in a game
/// directory. Missing game archives are skipped.
pub struct ArchiveChain {
    archives: Vec<Archive<Box<dyn Source>>>,
    has_map: bool,
    has_tileset: bool,
}

fn file(path: &Path) -> Result<Archive<Box<dyn Source>>> {
    let reader: Box<dyn Source> = Box::new(BufReader::new(File::open(path)?));
    Archive::open(reader)
}

impl ArchiveChain {
    /// The archive at `map` above the game archives in `game_data`.
    pub fn open(game_data: &Path, map: &Path) -> Result<ArchiveChain> {
        let mut chain = ArchiveChain::game(game_data)?;
        chain.archives.insert(0, file(map)?);
        chain.has_map = true;
        Ok(chain)
    }

    /// The game archives in `game_data` only.
    pub fn game(game_data: &Path) -> Result<ArchiveChain> {
        let mut archives = Vec::new();
        for name in GAME_ARCHIVES {
            let path = game_data.join(name);
            if path.exists() {
                archives.push(file(&path)?);
            }
        }
        Ok(ArchiveChain {
            archives,
            has_map: false,
            has_tileset: false,
        })
    }

    /// Mounts the archive of tileset `letter` (`<letter>.mpq` inside the
    /// game archives) below the map and above the game archives
    /// (SPEC-0014 rule 7, SPEC-0008 "Archive order"). PROVISIONAL: above
    /// `War3Patch.mpq` too, whose place relative to it is not measured.
    /// False when there is no such archive or one is mounted already.
    pub fn mount_tileset(&mut self, letter: u8) -> bool {
        if self.has_tileset {
            return false;
        }
        let name = format!("{}.mpq", char::from(letter));
        let Some(data) = self.read_game_file(&name) else {
            return false;
        };
        let reader: Box<dyn Source> = Box::new(Cursor::new(data));
        let Ok(archive) = Archive::open(reader) else {
            return false;
        };
        self.archives.insert(usize::from(self.has_map), archive);
        self.has_tileset = true;
        true
    }

    /// The first copy of `name`, or `None` if no archive has it.
    pub fn read(&mut self, name: &str) -> Option<Vec<u8>> {
        self.archives
            .iter_mut()
            .find(|a| a.contains(name))
            .and_then(|a| a.read(name).ok())
    }

    /// `name` from the map archive only.
    pub fn read_map_file(&mut self, name: &str) -> Option<Vec<u8>> {
        if !self.has_map {
            return None;
        }
        let map = &mut self.archives[0];
        map.contains(name).then(|| map.read(name).ok()).flatten()
    }

    /// Whether an archive has `name`.
    pub fn contains(&self, name: &str) -> bool {
        self.archives.iter().any(|a| a.contains(name))
    }

    /// The first copy of `name` in the tileset and game archives, skipping
    /// the map.
    pub fn read_game_file(&mut self, name: &str) -> Option<Vec<u8>> {
        let skip = usize::from(self.has_map);
        self.archives
            .iter_mut()
            .skip(skip)
            .find(|a| a.contains(name))
            .and_then(|a| a.read(name).ok())
    }
}
