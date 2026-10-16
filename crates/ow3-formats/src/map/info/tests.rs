use super::*;

#[derive(Default)]
struct Writer(Vec<u8>);

impl Writer {
    fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn i32(&mut self, v: i32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn f32(&mut self, v: f32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn code(&mut self, v: &[u8; 4]) -> &mut Self {
        self.0.extend_from_slice(v);
        self
    }
    fn str(&mut self, v: &str) -> &mut Self {
        self.0.extend_from_slice(v.as_bytes());
        self.0.push(0);
        self
    }
}

/// Everything up to and including the tileset byte.
fn common_header(w: &mut Writer, version: u32) {
    w.u32(version).u32(7).u32(6059);
    w.str("Name").str("Author").str("Description").str("1-4");
    for v in [-1.0, -2.0, 1.0, 2.0, -1.0, 2.0, 1.0, -2.0] {
        w.f32(v);
    }
    for v in [6, 6, 4, 8] {
        w.i32(v);
    }
    w.i32(96).i32(64).u32(0x9C1E).u8(b'L');
}

/// Players, forces, upgrades, tech and one random unit table.
fn common_tail(w: &mut Writer) {
    w.u32(2);
    for (n, race) in [(0, 1), (1, 4)] {
        w.i32(n)
            .i32(1)
            .i32(race)
            .u32(1)
            .str("Player")
            .f32(128.0)
            .f32(-64.0)
            .u32(0)
            .u32(2);
    }
    w.u32(1).u32(0b11).u32(0b11).str("Force");
    w.u32(1).u32(1).code(b"Rhme").i32(0).i32(2);
    w.u32(1).u32(2).code(b"hfoo");
    w.u32(1).i32(0).str("Table");
    w.u32(2).i32(0).i32(1); // columns: unit, building
    w.u32(1).i32(100).code(b"hfoo").code(b"htow");
}

#[test]
fn parses_reign_of_chaos_info() {
    let mut w = Writer::default();
    common_header(&mut w, VERSION_REIGN_OF_CHAOS);
    w.i32(3).str("Loading").str("Title").str("Sub").i32(-1);
    w.str("Prologue").str("PTitle").str("PSub");
    common_tail(&mut w);

    let info = MapInfo::parse(&w.0).unwrap();
    assert_eq!(
        (info.name.as_str(), info.saves, info.editor_version),
        ("Name", 7, 6059)
    );
    assert_eq!(info.camera_bounds[3], 2.0);
    assert_eq!(info.camera_bounds_complements, [6, 6, 4, 8]);
    assert_eq!(
        (info.playable_width, info.playable_height, info.tileset),
        (96, 64, b'L')
    );
    assert_eq!(info.loading_screen.text, "Loading");
    assert_eq!(info.loading_screen.preset, -1);
    assert_eq!(info.prologue.preset, 3, "campaign background");
    assert_eq!(info.prologue.subtitle, "PSub");
    assert_eq!(
        (info.game_data_set, info.environment.is_some()),
        (None, false)
    );
    assert_eq!(info.players.len(), 2);
    assert_eq!(info.players[1].race, 4);
    assert!(info.players[1].fixed_start_position);
    assert_eq!(
        (
            info.players[0].start_x,
            info.players[0].ally_high_priorities
        ),
        (128.0, 2)
    );
    assert_eq!(info.forces[0].players, 0b11);
    assert_eq!(info.upgrades[0].upgrade, RawCode(*b"Rhme"));
    assert_eq!(info.tech[0].tech, RawCode(*b"hfoo"));
    let table = &info.random_unit_tables[0];
    assert_eq!(table.column_kinds, [0, 1]);
    assert_eq!(
        table.rows[0].objects,
        [RawCode(*b"hfoo"), RawCode(*b"htow")]
    );
    assert!(info.random_item_tables.is_empty());
}

#[test]
fn parses_frozen_throne_info() {
    let mut w = Writer::default();
    common_header(&mut w, VERSION_FROZEN_THRONE);
    w.i32(2)
        .str("Model.mdx")
        .str("Loading")
        .str("Title")
        .str("Sub");
    w.i32(1).str("Prologue.mdx").str("P").str("PT").str("PS");
    w.i32(1)
        .f32(3000.0)
        .f32(5000.0)
        .f32(0.5)
        .u8(10)
        .u8(20)
        .u8(30)
        .u8(255);
    w.code(b"RAhr")
        .str("Lordaeron")
        .u8(b'X')
        .u8(1)
        .u8(2)
        .u8(3)
        .u8(4);
    common_tail(&mut w);
    w.u32(1).i32(5).str("Items");
    w.u32(1).u32(2).i32(60).code(b"ratc").i32(40).code(b"rde1");

    let info = MapInfo::parse(&w.0).unwrap();
    assert_eq!(info.loading_screen.preset, 2);
    assert_eq!(info.loading_screen.custom_model, "Model.mdx");
    assert_eq!(info.game_data_set, Some(1));
    assert_eq!(info.prologue.custom_model, "Prologue.mdx");
    let env = info.environment.unwrap();
    assert_eq!((env.fog_style, env.fog_end_z), (1, 5000.0));
    assert_eq!(
        env.fog_color,
        Rgba {
            r: 10,
            g: 20,
            b: 30,
            a: 255
        }
    );
    assert_eq!(env.weather, Some(RawCode(*b"RAhr")));
    assert_eq!(
        (env.sound_environment.as_str(), env.light_environment),
        ("Lordaeron", b'X')
    );
    let items = &info.random_item_tables[0];
    assert_eq!((items.number, items.name.as_str()), (5, "Items"));
    assert_eq!(
        items.sets[0][1],
        RandomItem {
            chance: 40,
            item: RawCode(*b"rde1")
        }
    );
}

#[test]
fn missing_weather_is_none() {
    let mut w = Writer::default();
    common_header(&mut w, VERSION_FROZEN_THRONE);
    w.i32(-1)
        .str("")
        .str("")
        .str("")
        .str("")
        .i32(0)
        .str("")
        .str("")
        .str("")
        .str("");
    w.i32(0).f32(0.0).f32(0.0).f32(0.0).u8(0).u8(0).u8(0).u8(0);
    w.u32(0).str("").u8(0).u8(0).u8(0).u8(0).u8(0);
    w.u32(0).u32(0).u32(0).u32(0).u32(0).u32(0);
    let info = MapInfo::parse(&w.0).unwrap();
    assert_eq!(info.environment.unwrap().weather, None);
    assert!(info.players.is_empty());
}

#[test]
fn rejects_other_versions_and_trailing_bytes() {
    assert_eq!(
        MapInfo::parse(&28u32.to_le_bytes()),
        Err(Error::UnsupportedVersion(28))
    );

    let mut w = Writer::default();
    common_header(&mut w, VERSION_REIGN_OF_CHAOS);
    w.i32(-1)
        .str("")
        .str("")
        .str("")
        .i32(-1)
        .str("")
        .str("")
        .str("");
    w.u32(0).u32(0).u32(0).u32(0).u32(0);
    let end = w.0.len();
    w.u8(0);
    assert_eq!(
        MapInfo::parse(&w.0),
        Err(Error::TrailingData { offset: end })
    );
}
