//! What is drawn: a map's ground, cliffs and water and the models placed
//! on it, loaded from its archive and the game archives.

use std::collections::BTreeMap;
use std::path::Path;

use ow3_formats::map::terrain::Terrain;
use ow3_formats::mpq::ArchiveChain;
use ow3_render::image::Image;

use ow3_render::daylight::DayLight;

use crate::models::{self, Animation, Group};

/// The unit light when the script names none.
const DEFAULT_UNIT_LIGHT: &str =
    "Environment\\DNC\\DNCLordaeron\\DNCLordaeronUnit\\DNCLordaeronUnit.mdx";
use crate::terrain_mesh::{self, Layer};

pub struct Scene {
    pub terrain: Terrain,
    pub textures: Vec<Option<Image>>,
    pub layers: Vec<Layer>,
    /// The water surface and its texture, if the map has water.
    pub water: Option<(Layer, Image)>,
    /// Models with their placements (cliffs included), and the textures
    /// they use by path.
    pub groups: Vec<Group>,
    pub model_textures: BTreeMap<String, Image>,
    /// How the models move.
    pub animation: Animation,
    /// The day/night models of the ground and of units (SPEC-0017).
    pub terrain_light: Option<DayLight>,
    pub unit_light: Option<DayLight>,
}

/// The first frame of the water texture `path` (without the frame
/// number), or plain white.
fn water_texture(chain: &mut ArchiveChain, path: &str) -> Image {
    chain
        .read(&format!("{path}00.blp"))
        .and_then(|d| ow3_formats::blp::decode(&d).ok())
        .map_or_else(
            || {
                let mut white = Image::new(1, 1);
                white.set(0, 0, [255; 4]);
                white
            },
            Image::from,
        )
}

impl Scene {
    /// With `without_units`, the units placed in the editor are left out
    /// for the simulation to place.
    /// `day_night` names the light models the script chose; without them
    /// the Lordaeron ones light the map.
    pub fn load(
        game_data: &Path,
        map: &Path,
        without_units: bool,
        day_night: Option<(String, String)>,
    ) -> Result<Scene, String> {
        let mut chain =
            ArchiveChain::open(game_data, map).map_err(|e| format!("{}: {e}", map.display()))?;
        let data = chain
            .read_map_file("war3map.w3e")
            .ok_or("the map has no war3map.w3e")?;
        let terrain = Terrain::parse(&data).map_err(|e| format!("war3map.w3e: {e}"))?;
        let (terrain_model, unit_model) = day_night.unwrap_or_else(|| {
            (
                ow3_render::daylight::DEFAULT_TERRAIN.to_owned(),
                DEFAULT_UNIT_LIGHT.to_owned(),
            )
        });
        let terrain_light = DayLight::load(&mut chain, &terrain_model);
        let unit_light = DayLight::load(&mut chain, &unit_model);
        let textures = ow3_render::terrain::ground_textures(&mut chain, &terrain);
        let cliffs = crate::cliff_mesh::build(&mut chain, &terrain);
        // The corners of cliff tiles show the cliff type's ground.
        let tiles = ow3_render::terrain::ground_tiles(&mut chain, &terrain);
        let painted = crate::cliff_mesh::repaint(&terrain, &tiles, &cliffs.grounds);
        let layers = terrain_mesh::build(&painted, &textures, &cliffs.covered);
        let water = ow3_render::water::water_type(&mut chain, terrain.tileset).map(|kind| {
            let depths = ow3_render::water::depths(&mut chain);
            let mesh = terrain_mesh::water(&terrain, &kind, depths);
            (mesh, water_texture(&mut chain, &kind.texture))
        });
        let water = water.filter(|(mesh, _)| !mesh.indices.is_empty());
        let placements = crate::objects::placements(&mut chain, without_units);
        let (mut groups, mut model_textures, animation) = models::load(&mut chain, &placements);
        model_textures.extend(models::textures(&mut chain, &cliffs.groups));
        groups.extend(cliffs.groups);
        Ok(Scene {
            terrain,
            textures,
            layers,
            water,
            groups,
            model_textures,
            animation,
            terrain_light,
            unit_light,
        })
    }
}
