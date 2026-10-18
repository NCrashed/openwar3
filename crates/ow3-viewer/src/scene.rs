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
    /// The water surface, the frames of its texture and its type, if the
    /// map has water.
    pub water: Option<(Layer, Vec<Image>, ow3_render::water::WaterType)>,
    /// Models with their placements (cliffs included), and the textures
    /// they use by path.
    pub groups: Vec<Group>,
    pub model_textures: BTreeMap<String, Image>,
    /// How the models move.
    pub animation: Animation,
    /// The day/night models of the ground and of units (SPEC-0017).
    pub terrain_light: Option<DayLight>,
    pub unit_light: Option<DayLight>,
    /// The shadows the editor baked (`war3map.shd`), if any.
    pub shadows: Option<ow3_formats::map::shadows::Shadows>,
}

/// The frames of the water texture of `kind`: `<texFile>00.blp` to
/// `numTex - 1` (SPEC-0014). A frame that is missing, or of another size
/// than the first, is plain white, and so is a texture without frames.
fn water_frames(chain: &mut ArchiveChain, kind: &ow3_render::water::WaterType) -> Vec<Image> {
    let white = || {
        let mut white = Image::new(1, 1);
        white.set(0, 0, [255; 4]);
        white
    };
    let mut frames: Vec<Image> = Vec::new();
    for n in 0..kind.frames {
        let frame = chain
            .read(&format!("{}{n:02}.blp", kind.texture))
            .and_then(|d| ow3_formats::blp::decode(&d).ok())
            .map(Image::from);
        match (frame, frames.first()) {
            (Some(f), None) => frames.push(f),
            (Some(f), Some(first)) if (f.width, f.height) == (first.width, first.height) => {
                frames.push(f)
            }
            (_, Some(first)) => {
                let mut blank = Image::new(first.width, first.height);
                for y in 0..first.height {
                    for x in 0..first.width {
                        blank.set(x, y, [255; 4]);
                    }
                }
                frames.push(blank);
            }
            (None, None) => frames.push(white()),
        }
    }
    if frames.is_empty() {
        frames.push(white());
    }
    frames
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
        let shadows = chain.read_map_file("war3map.shd").and_then(|d| {
            ow3_formats::map::shadows::Shadows::parse(&d, terrain.width - 1, terrain.height - 1)
                .ok()
        });
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
            let frames = water_frames(&mut chain, &kind);
            (mesh, frames, kind)
        });
        let water = water.filter(|(mesh, ..)| !mesh.indices.is_empty());
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
            shadows,
        })
    }
}
