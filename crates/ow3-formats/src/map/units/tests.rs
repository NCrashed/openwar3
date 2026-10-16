use super::*;

#[derive(Default)]
struct Writer(Vec<u8>);

impl Writer {
    fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
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
}

fn file(version: u32, units: usize) -> Writer {
    let mut w = Writer::default();
    w.code(MAGIC).i32(version as i32).i32(11).i32(units as i32);
    w
}

/// Fields from the code up to and including the mana.
fn unit_start(w: &mut Writer, kind: &[u8; 4], owner: i32) {
    w.code(kind)
        .i32(1)
        .f32(-512.0)
        .f32(256.0)
        .f32(0.0)
        .f32(4.71);
    w.f32(1.0).f32(1.0).f32(1.0).u8(2).i32(owner).u8(0).u8(0);
    w.i32(-1).i32(-1);
}

#[test]
fn parses_a_frozen_throne_hero() {
    let mut w = file(VERSION_FROZEN_THRONE, 1);
    unit_start(&mut w, b"Hpal", 3);
    w.i32(-1).i32(1).i32(1).code(b"ratc").i32(100); // one dropped set
    w.i32(12500).f32(-1.0).i32(5).i32(22).i32(13).i32(17);
    w.i32(2).i32(0).code(b"rde1").i32(5).code(b"pghe");
    w.i32(1).code(b"AHhb").i32(1).i32(2);
    w.i32(0).i32(-1); // not random: any level, class 0xFF
    w.i32(-1).i32(-1).i32(42);

    let parsed = PlacedUnits::parse(&w.0).unwrap();
    let u = &parsed.units[0];
    assert_eq!((u.kind, u.owner, u.variation), (RawCode(*b"Hpal"), 3, 1));
    assert_eq!((u.x, u.y, u.angle), (-512.0, 256.0, 4.71));
    assert_eq!((u.hit_points, u.mana, u.item_table), (-1, -1, -1));
    assert_eq!(
        u.dropped_item_sets,
        [vec![DroppedItem {
            item: RawCode(*b"ratc"),
            chance: 100
        }]]
    );
    assert_eq!(
        (u.gold, u.target_acquisition, u.hero_level),
        (12500, -1.0, 5)
    );
    assert_eq!(u.hero_attributes, Some([22, 13, 17]));
    assert_eq!(
        u.inventory[1],
        InventoryItem {
            slot: 5,
            item: RawCode(*b"pghe")
        }
    );
    assert_eq!(
        u.abilities,
        [AbilityChange {
            ability: RawCode(*b"AHhb"),
            autocast: true,
            level: 2
        }]
    );
    assert_eq!(
        u.random,
        RandomChoice::ByLevel {
            level: -1,
            item_class: 0xFF
        }
    );
    assert_eq!(
        (u.custom_color, u.waygate_destination, u.editor_id),
        (-1, -1, 42)
    );
}

#[test]
fn parses_every_random_choice_kind() {
    let mut w = file(VERSION_FROZEN_THRONE, 4);
    let randoms: [&dyn Fn(&mut Writer); 4] = [
        &|w| {
            w.i32(0).i32(0x0300_0002); // level 2, item class 3
        },
        &|w| {
            w.i32(1).i32(4).i32(1);
        },
        &|w| {
            w.i32(2).i32(2).code(b"nfrl").i32(60).code(b"nfrs").i32(40);
        },
        &|w| {
            w.i32(0x281);
        },
    ];
    for random in randoms {
        unit_start(&mut w, b"uDNR", 12);
        w.i32(-1)
            .i32(0)
            .i32(0)
            .f32(-1.0)
            .i32(1)
            .i32(0)
            .i32(0)
            .i32(0);
        w.i32(0).i32(0);
        random(&mut w);
        w.i32(-1).i32(-1).i32(0);
    }
    let units = PlacedUnits::parse(&w.0).unwrap().units;
    assert_eq!(
        units[0].random,
        RandomChoice::ByLevel {
            level: 2,
            item_class: 3
        }
    );
    assert_eq!(
        units[1].random,
        RandomChoice::FromTable {
            group: 4,
            position: 1
        }
    );
    assert_eq!(
        units[2].random,
        RandomChoice::FromList(vec![(RawCode(*b"nfrl"), 60), (RawCode(*b"nfrs"), 40)])
    );
    assert_eq!(units[3].random, RandomChoice::None { kind: 0x281 });
}

#[test]
fn parses_a_reign_of_chaos_start_location() {
    let mut w = file(VERSION_REIGN_OF_CHAOS, 1);
    unit_start(&mut w, b"sloc", 0);
    w.i32(0).i32(0).f32(-1.0).i32(1).i32(0).i32(0);
    w.i32(-1); // random kind -1, no data
    w.i32(-1).i32(-1).i32(0);
    let u = &PlacedUnits::parse(&w.0).unwrap().units[0];
    assert_eq!(u.kind, RawCode(*b"sloc"));
    assert_eq!((u.item_table, u.hero_attributes), (-1, None));
    assert_eq!(u.random, RandomChoice::None { kind: -1 });
}

#[test]
fn rejects_other_versions() {
    assert_eq!(
        PlacedUnits::parse(&file(9, 0).0),
        Err(Error::UnsupportedVersion(9))
    );
}
