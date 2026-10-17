//! What is drawn: a map's ground, loaded from its archive and the game
//! archives.

use std::path::Path;

use ow3_formats::map::terrain::Terrain;
use ow3_formats::mpq::ArchiveChain;
use ow3_render::image::Image;

use crate::terrain_mesh::{self, Layer};

pub struct Scene {
    pub terrain: Terrain,
    pub textures: Vec<Option<Image>>,
    pub layers: Vec<Layer>,
}

impl Scene {
    pub fn load(game_data: &Path, map: &Path) -> Result<Scene, String> {
        let mut chain =
            ArchiveChain::open(game_data, map).map_err(|e| format!("{}: {e}", map.display()))?;
        let data = chain
            .read_map_file("war3map.w3e")
            .ok_or("the map has no war3map.w3e")?;
        let terrain = Terrain::parse(&data).map_err(|e| format!("war3map.w3e: {e}"))?;
        let textures = ow3_render::terrain::ground_textures(&mut chain, &terrain);
        let layers = terrain_mesh::build(&terrain, &textures);
        Ok(Scene {
            terrain,
            textures,
            layers,
        })
    }
}
