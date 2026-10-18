//! MDX models (format version 800, as in 1.27b), after the public
//! community documentation of the format.
//!
//! A file is `MDLX` followed by chunks: a four-character tag, a byte size
//! and the data. Parsed here: `VERS`, `MODL`, `SEQS`, `GLBS`, `TEXS`,
//! `MTLS`, `GEOS`, `GEOA`, `BONE`, `HELP`, `ATCH`, `PIVT`, `LITE` and `PRE2`. Other chunks
//! (particle emitters, ribbons, events, cameras, collision shapes,
//! texture animations) are kept as raw bytes.
//!
//! Animated values are stored as tracks of 32-bit words: the number of
//! words per key depends on the track's tag (3 for a translation, 4 for a
//! rotation quaternion, 1 for an alpha and so on).

use crate::binary::{Error, RawCode, Reader, Result};

pub const VERSION: u32 = 800;
const NAME_LEN: usize = 80;
const PATH_LEN: usize = 260;

/// A bounding sphere radius and box.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Extent {
    pub radius: f32,
    pub min: [f32; 3],
    pub max: [f32; 3],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelInfo {
    pub name: String,
    pub animation_file: String,
    pub extent: Extent,
    pub blend_time: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sequence {
    pub name: String,
    /// First and last frame, in milliseconds.
    pub interval: [u32; 2],
    pub move_speed: f32,
    /// 1: does not loop.
    pub flags: u32,
    pub rarity: f32,
    pub sync_point: u32,
    pub extent: Extent,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Texture {
    /// 0 for a texture file; otherwise a texture chosen by the game, such
    /// as 1 for team colour and 2 for team glow.
    pub replaceable_id: u32,
    pub path: String,
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interpolation {
    None,
    Linear,
    Hermite,
    Bezier,
}

/// One key of a track: a frame and its values, with in and out tangents
/// for Hermite and Bezier interpolation.
#[derive(Clone, Debug, PartialEq)]
pub struct Key {
    pub frame: i32,
    pub value: Vec<u32>,
    pub tangents: Option<(Vec<u32>, Vec<u32>)>,
}

/// An animated value.
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub tag: RawCode,
    pub interpolation: Interpolation,
    /// Index into the global sequences, if the track follows one.
    pub global_sequence: Option<u32>,
    pub keys: Vec<Key>,
}

impl Key {
    /// The value as reals (for tracks of reals).
    pub fn reals(&self) -> Vec<f32> {
        self.value.iter().map(|&w| f32::from_bits(w)).collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub filter_mode: u32,
    pub shading_flags: u32,
    pub texture_id: u32,
    /// `None` without a texture animation.
    pub texture_animation_id: Option<u32>,
    pub coord_id: u32,
    pub alpha: f32,
    pub tracks: Vec<Track>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub priority_plane: i32,
    pub flags: u32,
    pub layers: Vec<Layer>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Geoset {
    pub vertices: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// Primitive type of each face group (4: triangles).
    pub face_types: Vec<u32>,
    /// Number of indices in each face group.
    pub face_groups: Vec<u32>,
    pub indices: Vec<u16>,
    /// The matrix group of each vertex.
    pub vertex_groups: Vec<u8>,
    /// Size of each matrix group, and the bone (node) indices of all
    /// groups one after another.
    pub matrix_group_sizes: Vec<u32>,
    pub matrix_indices: Vec<u32>,
    pub material_id: u32,
    pub selection_group: u32,
    /// 4: not selectable.
    pub selection_flags: u32,
    pub extent: Extent,
    /// The extent in each sequence.
    pub sequence_extents: Vec<Extent>,
    /// Texture coordinates of each vertex, one list per set.
    pub uv_sets: Vec<Vec<[f32; 2]>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeosetAnimation {
    pub alpha: f32,
    pub flags: u32,
    pub color: [f32; 3],
    pub geoset_id: u32,
    pub tracks: Vec<Track>,
}

/// The part every bone, helper, attachment and other object shares.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub name: String,
    pub object_id: u32,
    pub parent_id: Option<u32>,
    pub flags: u32,
    /// Translation (`KGTR`), rotation (`KGRT`) and scaling (`KGSC`).
    pub tracks: Vec<Track>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bone {
    pub node: Node,
    pub geoset_id: Option<u32>,
    pub geoset_animation_id: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Attachment {
    pub node: Node,
    pub path: String,
    pub attachment_id: u32,
    /// Visibility (`KATV`).
    pub tracks: Vec<Track>,
}

/// A light object (`LITE`).
#[derive(Clone, Debug, PartialEq)]
pub struct Light {
    pub node: Node,
    /// 0 omnidirectional, 1 directional, 2 ambient.
    pub kind: u32,
    pub attenuation: [f32; 2],
    /// As stored: blue, green, red.
    pub color: [f32; 3],
    pub intensity: f32,
    pub ambient_color: [f32; 3],
    pub ambient_intensity: f32,
    /// `KLAS`, `KLAE`, `KLAC`, `KLAI`, `KLBI`, `KLBC`, `KLAV`.
    pub tracks: Vec<Track>,
}

/// A particle emitter of the second kind (`PRE2`): it sends out textured
/// quads from its node, which age through three segments of colour,
/// alpha and size.
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleEmitter2 {
    pub node: Node,
    /// Units per second, its variation as a share, the half angle of the
    /// cone of directions in degrees, and the fall in units per second
    /// squared.
    pub speed: f32,
    pub variation: f32,
    pub latitude: f32,
    pub gravity: f32,
    /// Seconds a particle lives.
    pub lifespan: f32,
    /// Particles per second.
    pub emission_rate: f32,
    /// The size of the area particles start in.
    pub width: f32,
    pub length: f32,
    /// 0 blend, 1 additive, 2 modulate, 3 modulate 2x, 4 alpha key.
    pub filter_mode: u32,
    /// The texture is cut into `rows` by `columns` cells.
    pub rows: u32,
    pub columns: u32,
    /// 0 head, 1 tail, 2 both.
    pub head_or_tail: u32,
    pub tail_length: f32,
    /// The share of the life at which the middle segment is reached.
    pub time: f32,
    /// Red, green, blue, for the start, middle and end.
    pub segment_colors: [[f32; 3]; 3],
    pub segment_alphas: [u8; 3],
    pub segment_scaling: [f32; 3],
    /// Cells of the texture shown over the life: start, end and repeat,
    /// for the head in its first and second part and for the tail.
    pub head_intervals: [u32; 3],
    pub head_decay_intervals: [u32; 3],
    pub tail_intervals: [u32; 3],
    pub tail_decay_intervals: [u32; 3],
    pub texture_id: u32,
    pub squirt: u32,
    pub priority_plane: i32,
    pub replaceable_id: u32,
    /// `KP2S`, `KP2R`, `KP2L`, `KP2G`, `KP2E`, `KP2N`, `KP2W`, `KP2V`.
    pub tracks: Vec<Track>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
    pub version: u32,
    pub info: ModelInfo,
    pub sequences: Vec<Sequence>,
    /// Durations of the global sequences, in milliseconds.
    pub global_sequences: Vec<u32>,
    pub textures: Vec<Texture>,
    pub materials: Vec<Material>,
    pub geosets: Vec<Geoset>,
    pub geoset_animations: Vec<GeosetAnimation>,
    pub bones: Vec<Bone>,
    pub helpers: Vec<Node>,
    pub attachments: Vec<Attachment>,
    pub lights: Vec<Light>,
    pub particle_emitters: Vec<ParticleEmitter2>,
    pub pivots: Vec<[f32; 3]>,
    /// Chunks not parsed, with their data, in file order.
    pub other_chunks: Vec<(RawCode, Vec<u8>)>,
}

/// `0xFFFFFFFF` means none.
fn optional(value: u32) -> Option<u32> {
    (value != u32::MAX).then_some(value)
}

fn vec3(r: &mut Reader) -> Result<[f32; 3]> {
    Ok([r.f32()?, r.f32()?, r.f32()?])
}

fn extent(r: &mut Reader) -> Result<Extent> {
    Ok(Extent {
        radius: r.f32()?,
        min: vec3(r)?,
        max: vec3(r)?,
    })
}

/// Words per value of a track with this tag.
fn track_width(tag: &[u8; 4]) -> Option<usize> {
    Some(match tag {
        b"KGTR" | b"KGSC" | b"KTAT" | b"KTAS" | b"KGAC" | b"KLAC" | b"KLBC" | b"KRCO" | b"KCTR"
        | b"KTTR" => 3,
        b"KGRT" | b"KTAR" => 4,
        b"KMTA" | b"KMTF" | b"KGAO" | b"KATV" | b"KLAS" | b"KLAE" | b"KLAI" | b"KLBI" | b"KLAV"
        | b"KP2S" | b"KP2R" | b"KP2L" | b"KP2G" | b"KP2E" | b"KP2N" | b"KP2W" | b"KP2V"
        | b"KPEE" | b"KPEG" | b"KPLN" | b"KPLT" | b"KPEL" | b"KPES" | b"KPEV" | b"KRHA"
        | b"KRHB" | b"KRAL" | b"KRTX" | b"KRVS" | b"KCRL" => 1,
        _ => return None,
    })
}

fn track(r: &mut Reader) -> Result<Track> {
    let offset = r.position();
    let tag = r.raw_code()?;
    let width = track_width(&tag.0).ok_or(Error::Invalid {
        what: "track tag",
        offset,
    })?;
    let count = r.u32()? as usize;
    let interpolation = match r.u32()? {
        0 => Interpolation::None,
        1 => Interpolation::Linear,
        2 => Interpolation::Hermite,
        3 => Interpolation::Bezier,
        _ => {
            return Err(Error::Invalid {
                what: "interpolation",
                offset,
            });
        }
    };
    let global_sequence = optional(r.u32()?);
    let tangents = matches!(
        interpolation,
        Interpolation::Hermite | Interpolation::Bezier
    );
    let key_size = 4 + 4 * width * if tangents { 3 } else { 1 };
    if count.saturating_mul(key_size) > r.remaining() {
        return Err(Error::Invalid {
            what: "key count",
            offset,
        });
    }
    let words = |r: &mut Reader| (0..width).map(|_| r.u32()).collect::<Result<Vec<u32>>>();
    let mut keys = Vec::with_capacity(count);
    for _ in 0..count {
        let frame = r.i32()?;
        let value = words(r)?;
        let tangents = if tangents {
            Some((words(r)?, words(r)?))
        } else {
            None
        };
        keys.push(Key {
            frame,
            value,
            tangents,
        });
    }
    Ok(Track {
        tag,
        interpolation,
        global_sequence,
        keys,
    })
}

/// Tracks until `end`.
fn tracks_until(r: &mut Reader, end: usize) -> Result<Vec<Track>> {
    let mut tracks = Vec::new();
    while r.position() < end {
        tracks.push(track(r)?);
    }
    check_end(r, end, "object size")?;
    Ok(tracks)
}

fn check_end(r: &Reader, end: usize, what: &'static str) -> Result<()> {
    if r.position() == end {
        Ok(())
    } else {
        Err(Error::Invalid {
            what,
            offset: r.position(),
        })
    }
}

/// The end of an object whose size, counting its own size field, starts
/// at `start`.
fn object_end(r: &Reader, start: usize, size: u32) -> Result<usize> {
    let end = start + size as usize;
    if size < 4 || end > r.position() + r.remaining() {
        return Err(Error::Invalid {
            what: "object size",
            offset: start,
        });
    }
    Ok(end)
}

fn light(r: &mut Reader) -> Result<Light> {
    let start = r.position();
    let size = r.u32()?;
    let end = object_end(r, start, size)?;
    let node = node(r)?;
    let kind = r.u32()?;
    let attenuation = [r.f32()?, r.f32()?];
    let color = vec3(r)?;
    let intensity = r.f32()?;
    let ambient_color = vec3(r)?;
    let ambient_intensity = r.f32()?;
    let tracks = tracks_until(r, end)?;
    Ok(Light {
        node,
        kind,
        attenuation,
        color,
        intensity,
        ambient_color,
        ambient_intensity,
        tracks,
    })
}

fn particle_emitter2(r: &mut Reader) -> Result<ParticleEmitter2> {
    let start = r.position();
    let size = r.u32()?;
    let end = object_end(r, start, size)?;
    let node = node(r)?;
    let speed = r.f32()?;
    let variation = r.f32()?;
    let latitude = r.f32()?;
    let gravity = r.f32()?;
    let lifespan = r.f32()?;
    let emission_rate = r.f32()?;
    let width = r.f32()?;
    let length = r.f32()?;
    let filter_mode = r.u32()?;
    let rows = r.u32()?;
    let columns = r.u32()?;
    let head_or_tail = r.u32()?;
    let tail_length = r.f32()?;
    let time = r.f32()?;
    let segment_colors = [vec3(r)?, vec3(r)?, vec3(r)?];
    let segment_alphas = [r.u8()?, r.u8()?, r.u8()?];
    let segment_scaling = vec3(r)?;
    let mut interval = || -> Result<[u32; 3]> { Ok([r.u32()?, r.u32()?, r.u32()?]) };
    let head_intervals = interval()?;
    let head_decay_intervals = interval()?;
    let tail_intervals = interval()?;
    let tail_decay_intervals = interval()?;
    let texture_id = r.u32()?;
    let squirt = r.u32()?;
    let priority_plane = r.u32()? as i32;
    let replaceable_id = r.u32()?;
    let tracks = tracks_until(r, end)?;
    Ok(ParticleEmitter2 {
        node,
        speed,
        variation,
        latitude,
        gravity,
        lifespan,
        emission_rate,
        width,
        length,
        filter_mode,
        rows,
        columns,
        head_or_tail,
        tail_length,
        time,
        segment_colors,
        segment_alphas,
        segment_scaling,
        head_intervals,
        head_decay_intervals,
        tail_intervals,
        tail_decay_intervals,
        texture_id,
        squirt,
        priority_plane,
        replaceable_id,
        tracks,
    })
}

fn node(r: &mut Reader) -> Result<Node> {
    let start = r.position();
    let size = r.u32()?;
    let end = object_end(r, start, size)?;
    let name = r.fixed_string(NAME_LEN)?;
    let object_id = r.u32()?;
    let parent_id = optional(r.u32()?);
    let flags = r.u32()?;
    let tracks = tracks_until(r, end)?;
    Ok(Node {
        name,
        object_id,
        parent_id,
        flags,
        tracks,
    })
}

fn model_info(r: &mut Reader) -> Result<ModelInfo> {
    Ok(ModelInfo {
        name: r.fixed_string(NAME_LEN)?,
        animation_file: r.fixed_string(PATH_LEN)?,
        extent: extent(r)?,
        blend_time: r.u32()?,
    })
}

fn sequence(r: &mut Reader) -> Result<Sequence> {
    Ok(Sequence {
        name: r.fixed_string(NAME_LEN)?,
        interval: [r.u32()?, r.u32()?],
        move_speed: r.f32()?,
        flags: r.u32()?,
        rarity: r.f32()?,
        sync_point: r.u32()?,
        extent: extent(r)?,
    })
}

fn texture(r: &mut Reader) -> Result<Texture> {
    Ok(Texture {
        replaceable_id: r.u32()?,
        path: r.fixed_string(PATH_LEN)?,
        flags: r.u32()?,
    })
}

fn expect_tag(r: &mut Reader, tag: &[u8; 4]) -> Result<()> {
    let offset = r.position();
    if &r.raw_code()?.0 == tag {
        Ok(())
    } else {
        Err(Error::Invalid {
            what: "sub-chunk tag",
            offset,
        })
    }
}

fn material(r: &mut Reader) -> Result<Material> {
    let start = r.position();
    let size = r.u32()?;
    let end = object_end(r, start, size)?;
    let priority_plane = r.i32()?;
    let flags = r.u32()?;
    expect_tag(r, b"LAYS")?;
    let layers = r.list(28, |r| {
        let start = r.position();
        let size = r.u32()?;
        let end = object_end(r, start, size)?;
        let filter_mode = r.u32()?;
        let shading_flags = r.u32()?;
        let texture_id = r.u32()?;
        let texture_animation_id = optional(r.u32()?);
        let coord_id = r.u32()?;
        let alpha = r.f32()?;
        let tracks = tracks_until(r, end)?;
        Ok(Layer {
            filter_mode,
            shading_flags,
            texture_id,
            texture_animation_id,
            coord_id,
            alpha,
            tracks,
        })
    })?;
    check_end(r, end, "material size")?;
    Ok(Material {
        priority_plane,
        flags,
        layers,
    })
}

/// A tagged sub-chunk: the tag, a count and that many items.
fn tagged<T>(
    r: &mut Reader,
    tag: &[u8; 4],
    item_size: usize,
    item: impl FnMut(&mut Reader) -> Result<T>,
) -> Result<Vec<T>> {
    expect_tag(r, tag)?;
    r.list(item_size, item)
}

fn geoset(r: &mut Reader) -> Result<Geoset> {
    let start = r.position();
    let size = r.u32()?;
    let end = object_end(r, start, size)?;
    let vertices = tagged(r, b"VRTX", 12, vec3)?;
    let normals = tagged(r, b"NRMS", 12, vec3)?;
    let face_types = tagged(r, b"PTYP", 4, |r| r.u32())?;
    let face_groups = tagged(r, b"PCNT", 4, |r| r.u32())?;
    let indices = tagged(r, b"PVTX", 2, |r| r.u16())?;
    let vertex_groups = tagged(r, b"GNDX", 1, |r| r.u8())?;
    let matrix_group_sizes = tagged(r, b"MTGC", 4, |r| r.u32())?;
    let matrix_indices = tagged(r, b"MATS", 4, |r| r.u32())?;
    let material_id = r.u32()?;
    let selection_group = r.u32()?;
    let selection_flags = r.u32()?;
    let geoset_extent = extent(r)?;
    let sequence_extents = r.list(28, extent)?;
    expect_tag(r, b"UVAS")?;
    let sets = r.u32()?;
    let mut uv_sets = Vec::new();
    for _ in 0..sets {
        uv_sets.push(tagged(r, b"UVBS", 8, |r| Ok([r.f32()?, r.f32()?]))?);
    }
    check_end(r, end, "geoset size")?;
    Ok(Geoset {
        vertices,
        normals,
        face_types,
        face_groups,
        indices,
        vertex_groups,
        matrix_group_sizes,
        matrix_indices,
        material_id,
        selection_group,
        selection_flags,
        extent: geoset_extent,
        sequence_extents,
        uv_sets,
    })
}

fn geoset_animation(r: &mut Reader) -> Result<GeosetAnimation> {
    let start = r.position();
    let size = r.u32()?;
    let end = object_end(r, start, size)?;
    let alpha = r.f32()?;
    let flags = r.u32()?;
    let color = vec3(r)?;
    let geoset_id = r.u32()?;
    let tracks = tracks_until(r, end)?;
    Ok(GeosetAnimation {
        alpha,
        flags,
        color,
        geoset_id,
        tracks,
    })
}

fn attachment(r: &mut Reader) -> Result<Attachment> {
    let start = r.position();
    let size = r.u32()?;
    let end = object_end(r, start, size)?;
    let node = node(r)?;
    let path = r.fixed_string(PATH_LEN)?;
    let attachment_id = r.u32()?;
    let tracks = tracks_until(r, end)?;
    Ok(Attachment {
        node,
        path,
        attachment_id,
        tracks,
    })
}

/// Items one after another until the chunk ends.
fn items<T>(r: &mut Reader, mut item: impl FnMut(&mut Reader) -> Result<T>) -> Result<Vec<T>> {
    let mut items = Vec::new();
    while r.remaining() > 0 {
        items.push(item(r)?);
    }
    Ok(items)
}

/// Fixed-size items filling the chunk.
fn fixed<T>(
    r: &mut Reader,
    size: usize,
    offset: usize,
    item: impl FnMut(&mut Reader) -> Result<T>,
) -> Result<Vec<T>> {
    if r.remaining() % size != 0 {
        return Err(Error::Invalid {
            what: "chunk size",
            offset,
        });
    }
    items(r, item)
}

impl Model {
    pub fn parse(data: &[u8]) -> Result<Model> {
        let mut r = Reader::new(data);
        if &r.raw_code()?.0 != b"MDLX" {
            return Err(Error::Invalid {
                what: "MDX magic",
                offset: 0,
            });
        }
        let mut model = Model::default();
        while r.remaining() > 0 {
            let offset = r.position();
            let tag = r.raw_code()?;
            let size = r.u32()? as usize;
            let mut c = Reader::new(r.bytes(size)?);
            match &tag.0 {
                b"VERS" => {
                    model.version = c.u32()?;
                    if model.version != VERSION {
                        return Err(Error::UnsupportedVersion(model.version));
                    }
                }
                b"MODL" => model.info = model_info(&mut c)?,
                b"SEQS" => model.sequences = fixed(&mut c, 132, offset, sequence)?,
                b"GLBS" => model.global_sequences = fixed(&mut c, 4, offset, |r| r.u32())?,
                b"TEXS" => model.textures = fixed(&mut c, 268, offset, texture)?,
                b"MTLS" => model.materials = items(&mut c, material)?,
                b"GEOS" => model.geosets = items(&mut c, geoset)?,
                b"GEOA" => model.geoset_animations = items(&mut c, geoset_animation)?,
                b"BONE" => {
                    model.bones = items(&mut c, |r| {
                        Ok(Bone {
                            node: node(r)?,
                            geoset_id: optional(r.u32()?),
                            geoset_animation_id: optional(r.u32()?),
                        })
                    })?;
                }
                b"HELP" => model.helpers = items(&mut c, node)?,
                b"ATCH" => model.attachments = items(&mut c, attachment)?,
                b"PIVT" => model.pivots = fixed(&mut c, 12, offset, vec3)?,
                b"LITE" => model.lights = items(&mut c, light)?,
                b"PRE2" => model.particle_emitters = items(&mut c, particle_emitter2)?,
                _ => {
                    model.other_chunks.push((tag, c.bytes(size)?.to_vec()));
                }
            }
            c.finish()?;
        }
        Ok(model)
    }
}

#[cfg(test)]
mod tests;
