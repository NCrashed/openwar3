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
    let (mut vertices, mut bones) = (0usize, 0usize);
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
                    parsed += 1;
                }
                Err(e) => failures.push(format!("{archive} {name}: {e}")),
            }
        }
    }
    eprintln!(
        "{parsed} models, {vertices} vertices, {bones} bones; unparsed chunks: {other_chunks:?}"
    );
    assert!(parsed > 3000, "{parsed}");
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures[..failures.len().min(30)].join("\n")
    );
}
