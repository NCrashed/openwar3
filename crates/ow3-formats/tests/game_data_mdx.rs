//! Parses every model of a real Warcraft III 1.27b installation and checks
//! that its parts refer to each other consistently. See `game_data_mpq.rs`
//! for where the game data is expected; the test passes without checking
//! anything when it is absent.

use std::collections::BTreeMap;
use std::path::PathBuf;

use ow3_formats::mdx::Model;
use ow3_formats::mpq::Archive;

/// What is wrong with a parsed model, if anything.
fn inconsistency(model: &Model) -> Option<String> {
    for (g, geoset) in model.geosets.iter().enumerate() {
        let n = geoset.vertices.len();
        // Unlit geometry (selection circles and the like) has no normals.
        if !geoset.normals.is_empty() && geoset.normals.len() != n {
            return Some(format!(
                "geoset {g}: {} normals for {n} vertices",
                geoset.normals.len()
            ));
        }
        if geoset.vertex_groups.len() != n {
            return Some(format!("geoset {g}: vertex groups"));
        }
        if geoset.uv_sets.iter().any(|set| set.len() != n) {
            return Some(format!("geoset {g}: texture coordinates"));
        }
        if let Some(i) = geoset.indices.iter().find(|&&i| usize::from(i) >= n) {
            return Some(format!("geoset {g}: index {i} of {n} vertices"));
        }
        let total: u32 = geoset.face_groups.iter().sum();
        if total as usize != geoset.indices.len() {
            return Some(format!("geoset {g}: face groups"));
        }
        let groups = geoset.matrix_group_sizes.len();
        if geoset
            .vertex_groups
            .iter()
            .any(|&v| usize::from(v) >= groups)
        {
            return Some(format!("geoset {g}: matrix group out of range"));
        }
        let sizes: u32 = geoset.matrix_group_sizes.iter().sum();
        if sizes as usize != geoset.matrix_indices.len() {
            return Some(format!("geoset {g}: matrix indices"));
        }
        if geoset.material_id as usize >= model.materials.len() {
            return Some(format!("geoset {g}: material {}", geoset.material_id));
        }
    }
    for (e, emitter) in model.particle_emitters.iter().enumerate() {
        if emitter.texture_id as usize >= model.textures.len() {
            return Some(format!(
                "particle emitter {e}: texture {}",
                emitter.texture_id
            ));
        }
        if emitter.rows == 0 || emitter.columns == 0 {
            return Some(format!("particle emitter {e}: no texture cells"));
        }
    }
    for (m, material) in model.materials.iter().enumerate() {
        for layer in &material.layers {
            if layer.texture_id as usize >= model.textures.len() {
                return Some(format!("material {m}: texture {}", layer.texture_id));
            }
        }
    }
    None
}

#[test]
fn every_model_parses() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b");
    let mut parsed = 0;
    let mut failures = Vec::new();
    let mut other_chunks: BTreeMap<String, usize> = BTreeMap::new();
    let (mut vertices, mut bones, mut emitters) = (0usize, 0usize, 0usize);
    for archive in ["War3.mpq", "War3x.mpq"] {
        let path = root.join(archive);
        if !path.exists() {
            eprintln!("skipping: {} not found", path.display());
            return;
        }
        let mut mpq = Archive::open_path(&path).unwrap();
        let names: Vec<String> = mpq
            .list()
            .unwrap()
            .into_iter()
            .filter(|n| n.to_ascii_lowercase().ends_with(".mdx"))
            .collect();
        for name in names {
            let data = mpq.read(&name).unwrap();
            match Model::parse(&data) {
                Ok(model) => {
                    if let Some(problem) = inconsistency(&model) {
                        failures.push(format!("{archive} {name}: {problem}"));
                    }
                    for (tag, _) in &model.other_chunks {
                        *other_chunks.entry(tag.to_string()).or_default() += 1;
                    }
                    vertices += model
                        .geosets
                        .iter()
                        .map(|g| g.vertices.len())
                        .sum::<usize>();
                    bones += model.bones.len();
                    emitters += model.particle_emitters.len();
                    parsed += 1;
                }
                Err(e) => failures.push(format!("{archive} {name}: {e}")),
            }
        }
    }
    eprintln!(
        "{parsed} models, {vertices} vertices, {bones} bones, {emitters} particle emitters; unparsed chunks: {other_chunks:?}"
    );
    assert!(parsed > 3000, "{parsed}");
    assert!(emitters > 1000, "{emitters}");
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures[..failures.len().min(30)].join("\n")
    );
}

/// The footman's portrait has the camera SPEC-0028 read from the game.
#[test]
fn the_footman_portrait_camera() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b/War3.mpq");
    if !path.exists() {
        return;
    }
    let mut mpq = Archive::open_path(&path).unwrap();
    let data = mpq
        .read("Units\\Human\\Footman\\Footman_portrait.mdx")
        .unwrap();
    let model = Model::parse(&data).unwrap();
    assert_eq!(model.cameras.len(), 1);
    let camera = &model.cameras[0];
    let near = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.01);
    assert!(near(camera.position, [40.18, 194.43, 88.61]), "{camera:?}");
    assert!(near(camera.target, [8.25, 214.87, 90.07]), "{camera:?}");
    assert!((camera.field_of_view - std::f32::consts::FRAC_PI_4).abs() < 1e-4);
    assert_eq!((camera.near, camera.far), (8.0, 1000.0));
}
