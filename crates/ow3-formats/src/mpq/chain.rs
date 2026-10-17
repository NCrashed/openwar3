//! A map archive and the game archives, searched by priority.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use super::{Archive, Result};

/// Game archives by priority, highest first (SPEC-0008).
pub const GAME_ARCHIVES: [&str; 4] = ["War3Patch.mpq", "War3xlocal.mpq", "War3x.mpq", "War3.mpq"];

/// Archives searched in order: the map (if any), then the game archives
/// found in a game directory. Missing game archives are skipped.
pub struct ArchiveChain {
    archives: Vec<Archive<BufReader<File>>>,
    has_map: bool,
}

impl ArchiveChain {
    /// The archive at `map` above the game archives in `game_data`.
    pub fn open(game_data: &Path, map: &Path) -> Result<ArchiveChain> {
        let mut chain = ArchiveChain::game(game_data)?;
        chain.archives.insert(0, Archive::open_path(map)?);
        chain.has_map = true;
        Ok(chain)
    }

    /// The game archives in `game_data` only.
    pub fn game(game_data: &Path) -> Result<ArchiveChain> {
        let mut archives = Vec::new();
        for name in GAME_ARCHIVES {
            let path = game_data.join(name);
            if path.exists() {
                archives.push(Archive::open_path(&path)?);
            }
        }
        Ok(ArchiveChain {
            archives,
            has_map: false,
        })
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
}

impl ArchiveChain {
    /// The first copy of `name` in the game archives, skipping the map.
    pub fn read_game_file(&mut self, name: &str) -> Option<Vec<u8>> {
        let skip = usize::from(self.has_map);
        self.archives
            .iter_mut()
            .skip(skip)
            .find(|a| a.contains(name))
            .and_then(|a| a.read(name).ok())
    }
}
