use std::collections::BTreeMap;
use std::path::PathBuf;

use ow3_formats::replay::Replay;
use ow3_sim::map::Map;
use ow3_sim::replay::Playback;

fn game_data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b")
}

#[test]
fn probe() {
    let name = std::env::var("OW3_NAME").unwrap_or_else(|_| "c-SBV-1".into());
    let offset: u64 = std::env::var("OW3_OFF")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(201);
    let Ok(text) = std::fs::read_to_string(game_data().join(format!("replays/{name}.log.tsv")))
    else {
        return;
    };
    let mut logged: BTreeMap<usize, BTreeMap<u64, [f32; 2]>> = BTreeMap::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let mut w: Vec<&str> = line.split('\t').collect();
        if w.len() > 1 && w[0].parse::<u64>().is_err() {
            w.swap(0, 1);
        }
        if w.get(1) != Some(&"p") {
            continue;
        }
        let (Some(t), Some(u), Some(x), Some(y)) = (
            w[0].parse::<u64>().ok(),
            w[2].parse::<usize>().ok(),
            w[3].parse::<f32>().ok(),
            w[4].parse::<f32>().ok(),
        ) else {
            continue;
        };
        logged.entry(u).or_default().insert(t + offset, [x, y]);
    }
    let dir = game_data().join("replays");
    let replay = Replay::parse(&std::fs::read(dir.join(format!("{name}.w3g"))).unwrap()).unwrap();
    let map_path = game_data().join(replay.settings.map_path.replace('\\', "/"));
    let map = Map::load(&game_data(), &map_path).unwrap();
    let program = map.check().unwrap();
    let mut playback = Playback::new(&program, &map, &replay).unwrap();
    let mut at: BTreeMap<usize, BTreeMap<u64, [f32; 2]>> = BTreeMap::new();
    while playback.step_tick() {
        let tick = playback.game.ticks();
        if let Ok(r) = std::env::var("OW3_TRACE")
            && {
                let v: Vec<u64> = r.split(',').map(|s| s.parse().unwrap()).collect();
                (v[0]..v[1]).contains(&tick)
            }
        {
            let u = &playback.game.units()[std::env::var("OW3_U")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(5)];
            eprintln!(
                "TR {tick} {:.4} {:.4} f {:.4} o {}",
                u.x, u.y, u.facing, u.order
            );
        }
        for (i, u) in playback.game.units().iter().enumerate() {
            at.entry(i).or_default().insert(tick, [u.x, u.y]);
        }
    }
    for (unit, list) in &logged {
        let miss = list.iter().find(|(t, p)| {
            at.get(unit)
                .and_then(|a| a.get(t))
                .is_none_or(|q| (q[0] - p[0]).abs() > 0.01 || (q[1] - p[1]).abs() > 0.01)
        });
        if let Some((t, p)) = miss {
            let q = at.get(unit).and_then(|a| a.get(t));
            eprintln!("unit {unit}: first miss at {t}: game {p:?}, ours {q:?}");
        }
    }
}

#[test]
fn homing() {
    let Ok(mut archives) = ow3_sim::map::Archives::game(&game_data()) else {
        return;
    };
    let data = ow3_sim::data::ObjectData::load(&mut archives, false).unwrap();
    for id in [*b"ntrg", *b"earc"] {
        let t = &data.units[&i32::from_be_bytes(id)];
        eprintln!(
            "{:?} speed {} homing {} coll {} range {:?} castpt {} abil {:?}",
            id.map(char::from),
            f32::from_bits(t.missile_speed),
            t.missile_homing,
            f32::from_bits(t.collision),
            t.weapon.as_ref().map(|w| f32::from_bits(w.range)),
            f32::from_bits(t.cast_point),
            t.abilities
                .iter()
                .map(|a| a.to_be_bytes().map(char::from))
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn map_area() {
    let path = game_data().join("Maps/FrozenThrone/(2)EchoIsles.w3x");
    let Ok(map) = Map::load(&game_data(), &path) else {
        return;
    };
    let b = &map.world_bounds;
    eprintln!(
        "bounds {} {} {} {} complements {:?}",
        b.min_x, b.min_y, b.max_x, b.max_y, map.info.camera_bounds_complements
    );
}

#[test]
fn minimap_bars() {
    let Ok(data) = std::fs::read(std::env::var("OW3_BLP").unwrap_or_default()) else {
        return;
    };
    let img = ow3_formats::blp::decode(&data).unwrap();
    let (w, h) = (img.width as usize, img.height as usize);
    let lum = |x: usize, y: usize| {
        let p = &img.rgba[(y * w + x) * 4..];
        p[0] as u32 + p[1] as u32 + p[2] as u32
    };
    eprintln!("size {w}x{h}");
    for y in (0..h).step_by(8) {
        let s: u32 = (0..w).map(|x| lum(x, y)).sum();
        eprint!("{} ", s / w as u32);
    }
    eprintln!();
    for x in (0..w).step_by(8) {
        let s: u32 = (0..h).map(|y| lum(x, y)).sum();
        eprint!("{} ", s / h as u32);
    }
    eprintln!();
}

#[test]
fn farm_sequences() {
    let Ok(mut archives) = ow3_sim::map::Archives::game(&game_data()) else {
        return;
    };
    let Some(data) = archives.read_game_file("Buildings\\Human\\Farm\\Farm.mdx") else {
        return;
    };
    let m = ow3_formats::mdx::Model::parse(&data).unwrap();
    for (i, s) in m.sequences.iter().enumerate() {
        eprintln!("{i} {:?} {:?}", s.name, s.interval);
    }
    eprintln!(
        "geosets {} anims {}",
        m.geosets.len(),
        m.geoset_animations.len()
    );
}

#[test]
fn ahrp() {
    let Ok(mut archives) = ow3_sim::map::Archives::game(&game_data()) else {
        return;
    };
    let data = ow3_sim::data::ObjectData::load(&mut archives, false).unwrap();
    let a = &data.abilities[&i32::from_be_bytes(*b"Ahrp")];
    eprintln!(
        "Ahrp range {} area {} d {:?}",
        f32::from_bits(a.range),
        f32::from_bits(a.area),
        a.data.map(f32::from_bits)
    );
    for id in [*b"hhou", *b"hbar", *b"hpea"] {
        let t = &data.units[&i32::from_be_bytes(id)];
        eprintln!(
            "{:?} coll {}",
            id.map(char::from),
            f32::from_bits(t.collision)
        );
    }
}

#[test]
fn gold_log() {
    let name = "c-SBZ-1";
    let Ok(text) = std::fs::read_to_string(game_data().join(format!("replays/{name}.log.tsv")))
    else {
        return;
    };
    let mut log: Vec<(u64, i32, i32)> = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let w: Vec<&str> = line.split('\t').collect();
        if w.get(1) == Some(&"g") {
            log.push((
                w[0].parse().unwrap(),
                w[2].parse().unwrap(),
                w[3].parse().unwrap(),
            ));
        }
    }
    let dir = game_data().join("replays");
    let replay = Replay::parse(&std::fs::read(dir.join(format!("{name}.w3g"))).unwrap()).unwrap();
    let map_path = game_data().join(replay.settings.map_path.replace('\\', "/"));
    let map = Map::load(&game_data(), &map_path).unwrap();
    let program = map.check().unwrap();
    let mut playback = Playback::new(&program, &map, &replay).unwrap();
    let mut ours: BTreeMap<u64, (i32, i32)> = BTreeMap::new();
    while playback.step_tick() {
        let tick = playback.game.ticks();
        ours.insert(
            tick,
            (
                playback.game.player_state(0, 1),
                playback.game.player_state(0, 2),
            ),
        );
    }
    let mut shown = 0;
    for (t, g, l) in log {
        let o = ours.get(&(t + 201)).copied();
        if o != Some((g, l)) {
            eprintln!("log {t}: game {g} {l}, ours {o:?}");
            shown += 1;
            if shown > 12 {
                break;
            }
        }
    }
}

#[test]
fn map_script() {
    let Ok(name) = std::env::var("OW3_MAP") else {
        return;
    };
    let dir = game_data().join("replays");
    let replay = Replay::parse(&std::fs::read(dir.join(format!("{name}.w3g"))).unwrap()).unwrap();
    let map_path = game_data().join(replay.settings.map_path.replace('\\', "/"));
    eprintln!("MAP {}", map_path.display());
    let mut chain = ow3_formats::mpq::ArchiveChain::open(&game_data(), &map_path).unwrap();
    let text = chain.read("war3map.j").unwrap();
    std::fs::write(std::env::var("OW3_OUT").unwrap(), text).unwrap();
}

#[test]
fn ability_code() {
    let Ok(mut archives) = ow3_sim::map::Archives::game(&game_data()) else {
        return;
    };
    let data = ow3_sim::data::ObjectData::load(&mut archives, false).unwrap();
    for id in [*b"ANth", *b"AOww"] {
        let a = &data.abilities[&i32::from_be_bytes(id)];
        eprintln!(
            "{:?} code {:?} cost {:?}",
            id.map(char::from),
            a.code.to_be_bytes().map(char::from),
            a.per_level.first().map(|l| l.cost)
        );
    }
}
