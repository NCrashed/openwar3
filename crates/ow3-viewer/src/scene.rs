//! What is drawn: a map's ground and the models placed on it, loaded from
//! its archive and the game archives.

use std::collections::BTreeMap;
use std::path::Path;

use ow3_formats::map::terrain::Terrain;
use ow3_formats::mpq::ArchiveChain;
use ow3_render::image::Image;

use crate::models::{self, Group};
use crate::terrain_mesh::{self, Layer};

pub struct Scene {
    pub terrain: Terrain,
    pub textures: Vec<Option<Image>>,
    pub layers: Vec<Layer>,
    /// Models with their placements, and the textures they use by path.
    pub groups: Vec<Group>,
    pub model_textures: BTreeMap<String, Image>,
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
        let placements = crate::objects::placements(&mut chain);
        let (groups, model_textures) = models::load(&mut chain, &placements);
        Ok(Scene {
            terrain,
            textures,
            layers,
            groups,
            model_textures,
        })
    }
}
