use super::*;

/// Little-endian writer for building test files.
#[derive(Default)]
struct Writer(Vec<u8>);

impl Writer {
    fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn f32(&mut self, v: f32) -> &mut Self {
        self.u32(v.to_bits())
    }
    fn tag(&mut self, t: &[u8; 4]) -> &mut Self {
        self.0.extend_from_slice(t);
        self
    }
    fn text(&mut self, s: &str, len: usize) -> &mut Self {
        let mut bytes = s.as_bytes().to_vec();
        bytes.resize(len, 0);
        self.0.extend_from_slice(&bytes);
        self
    }
    fn extent(&mut self) -> &mut Self {
        for v in [1.0, -1.0, -1.0, -1.0, 1.0, 1.0, 1.0] {
            self.f32(v);
        }
        self
    }
    fn chunk(&mut self, tag: &[u8; 4], body: &Writer) -> &mut Self {
        self.tag(tag).u32(body.0.len() as u32);
        self.0.extend_from_slice(&body.0);
        self
    }
    /// An object with its inclusive size first.
    fn sized(&mut self, body: &Writer) -> &mut Self {
        self.u32(body.0.len() as u32 + 4);
        self.0.extend_from_slice(&body.0);
        self
    }
}

fn sample() -> Vec<u8> {
    let mut file = Writer::default();
    file.tag(b"MDLX");
    file.chunk(b"VERS", Writer::default().u32(800));
    let mut modl = Writer::default();
    modl.text("Box", 80).text("", 260).extent().u32(150);
    file.chunk(b"MODL", &modl);
    let mut seqs = Writer::default();
    seqs.text("Stand", 80)
        .u32(0)
        .u32(1000)
        .f32(0.0)
        .u32(0)
        .f32(0.0)
        .u32(0)
        .extent();
    file.chunk(b"SEQS", &seqs);
    let mut texs = Writer::default();
    texs.u32(0).text("Textures\\Box.blp", 260).u32(0);
    texs.u32(1).text("", 260).u32(0);
    file.chunk(b"TEXS", &texs);
    // One material with one layer whose alpha goes from 1 to 0.
    let mut layer = Writer::default();
    layer.u32(1).u32(0).u32(0).u32(u32::MAX).u32(0).f32(1.0);
    layer.tag(b"KMTA").u32(2).u32(1).u32(u32::MAX);
    layer.u32(0).f32(1.0).u32(1000).f32(0.0);
    let mut material = Writer::default();
    material.u32(0).u32(0).tag(b"LAYS").u32(1).sized(&layer);
    file.chunk(b"MTLS", Writer::default().sized(&material));
    // One triangle.
    let mut geoset = Writer::default();
    geoset.tag(b"VRTX").u32(3);
    for v in [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        geoset.f32(v);
    }
    geoset.tag(b"NRMS").u32(3);
    for _ in 0..3 {
        geoset.f32(0.0).f32(0.0).f32(1.0);
    }
    geoset.tag(b"PTYP").u32(1).u32(4);
    geoset.tag(b"PCNT").u32(1).u32(3);
    geoset.tag(b"PVTX").u32(3);
    geoset.0.extend_from_slice(&[0, 0, 1, 0, 2, 0]);
    geoset.tag(b"GNDX").u32(3);
    geoset.0.extend_from_slice(&[0, 0, 0]);
    geoset.tag(b"MTGC").u32(1).u32(1);
    geoset.tag(b"MATS").u32(1).u32(0);
    geoset.u32(0).u32(0).u32(0).extent().u32(1).extent();
    geoset.tag(b"UVAS").u32(1).tag(b"UVBS").u32(3);
    for v in [0.0, 0.0, 1.0, 0.0, 0.0, 1.0] {
        geoset.f32(v);
    }
    file.chunk(b"GEOS", Writer::default().sized(&geoset));
    // A bone with a Hermite translation.
    let mut node = Writer::default();
    node.text("Bone_Root", 80).u32(0).u32(u32::MAX).u32(0x100);
    node.tag(b"KGTR").u32(1).u32(2).u32(u32::MAX).u32(500);
    for v in [1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0] {
        node.f32(v);
    }
    let mut bone = Writer::default();
    bone.sized(&node).u32(0).u32(u32::MAX);
    file.chunk(b"BONE", &bone);
    file.chunk(b"PIVT", Writer::default().f32(0.5).f32(0.5).f32(0.0));
    file.chunk(b"CAMS", Writer::default().u32(7));
    file.0
}

#[test]
fn parses_a_small_model() {
    let model = Model::parse(&sample()).unwrap();
    assert_eq!(model.version, 800);
    assert_eq!(model.info.name, "Box");
    assert_eq!(model.info.blend_time, 150);
    assert_eq!(model.sequences[0].name, "Stand");
    assert_eq!(model.sequences[0].interval, [0, 1000]);
    assert_eq!(model.textures[0].path, "Textures\\Box.blp");
    assert_eq!(model.textures[1].replaceable_id, 1);
    let layer = &model.materials[0].layers[0];
    assert_eq!(layer.filter_mode, 1);
    assert_eq!(layer.texture_animation_id, None);
    let alpha = &layer.tracks[0];
    assert_eq!(alpha.interpolation, Interpolation::Linear);
    assert_eq!(alpha.keys[1].frame, 1000);
    assert_eq!(alpha.keys[1].reals(), [0.0]);
    let geoset = &model.geosets[0];
    assert_eq!(geoset.vertices.len(), 3);
    assert_eq!(geoset.indices, [0, 1, 2]);
    assert_eq!(geoset.uv_sets[0][1], [1.0, 0.0]);
    assert_eq!(geoset.sequence_extents.len(), 1);
    let bone = &model.bones[0];
    assert_eq!(bone.node.name, "Bone_Root");
    assert_eq!(bone.node.parent_id, None);
    assert_eq!(bone.geoset_id, Some(0));
    let translation = &bone.node.tracks[0];
    assert_eq!(translation.interpolation, Interpolation::Hermite);
    assert_eq!(translation.keys[0].reals(), [1.0, 2.0, 3.0]);
    assert!(translation.keys[0].tangents.is_some());
    assert_eq!(model.pivots, [[0.5, 0.5, 0.0]]);
    assert_eq!(model.other_chunks.len(), 1);
    assert_eq!(&model.other_chunks[0].0.0, b"CAMS");
}

#[test]
fn rejects_bad_data() {
    let data = sample();
    assert!(Model::parse(b"MDLY").is_err());
    for len in [5, 20, data.len() - 1] {
        assert!(Model::parse(&data[..len]).is_err(), "cut at {len}");
    }
}
