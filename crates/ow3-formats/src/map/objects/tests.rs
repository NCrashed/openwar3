use super::*;

#[derive(Default)]
struct Writer(Vec<u8>);

impl Writer {
    fn u32(&mut self, v: u32) -> &mut Self {
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

#[test]
fn parses_unit_changes_and_custom_units() {
    let mut w = Writer::default();
    w.u32(2);
    // Original table: the footman gets 500 hit points.
    w.u32(1).code(b"hfoo").u32(0).u32(1);
    w.code(b"uhpm").u32(0).u32(500).u32(0);
    // Custom table: h000 derived from the knight, with a name, a real and an
    // unreal field, and an end marker that repeats the object's code.
    w.u32(1).code(b"hkni").code(b"h000").u32(3);
    w.code(b"unam").u32(3).str("Paladin").code(b"h000");
    w.code(b"umvs").u32(1);
    w.0.extend_from_slice(&350.5f32.to_le_bytes());
    w.u32(0);
    w.code(b"uaen").u32(2);
    w.0.extend_from_slice(&0.25f32.to_le_bytes());
    w.code(b"hkni");

    let parsed = ObjectModifications::parse(&w.0, ObjectKind::Units).unwrap();
    assert_eq!(parsed.format_version, 2);
    let footman = &parsed.original[0];
    assert_eq!((footman.base, footman.id), (RawCode(*b"hfoo"), None));
    assert_eq!(footman.modifications[0].value, Value::Integer(500));
    assert_eq!(footman.modifications[0].level, None);

    let custom = &parsed.custom[0];
    assert_eq!(custom.id, Some(RawCode(*b"h000")));
    let values: Vec<_> = custom
        .modifications
        .iter()
        .map(|m| m.value.clone())
        .collect();
    assert_eq!(
        values,
        [
            Value::String("Paladin".into()),
            Value::Real(350.5),
            Value::Unreal(0.25)
        ]
    );
    let markers: Vec<_> = custom.modifications.iter().map(|m| m.end_marker).collect();
    assert_eq!(
        markers,
        [RawCode(*b"h000"), RawCode([0; 4]), RawCode(*b"hkni")]
    );
}

#[test]
fn ability_changes_carry_level_and_data_column() {
    let mut w = Writer::default();
    w.u32(1).u32(0).u32(1).code(b"AHbz").code(b"A000").u32(1);
    w.code(b"Hbz1").u32(1).u32(3).u32(1);
    w.0.extend_from_slice(&12.0f32.to_le_bytes());
    w.u32(0);

    let parsed = ObjectModifications::parse(&w.0, ObjectKind::Abilities).unwrap();
    let m = &parsed.custom[0].modifications[0];
    assert_eq!((m.level, m.data_column), (Some(3), Some(1)));
    assert_eq!(m.value, Value::Real(12.0));
}

/// An empty string makes a modification only 13 bytes long; the count check
/// must still accept it.
#[test]
fn accepts_many_empty_string_fields() {
    let mut w = Writer::default();
    w.u32(2).u32(0).u32(1).code(b"rwiz").code(b"I000").u32(3);
    for field in [b"utip", b"utub", b"ides"] {
        w.code(field).u32(3).str("").u32(0);
    }
    let parsed = ObjectModifications::parse(&w.0, ObjectKind::Items).unwrap();
    assert_eq!(parsed.custom[0].modifications.len(), 3);
}

#[test]
fn rejects_unknown_versions_and_value_types() {
    let mut w = Writer::default();
    w.u32(3);
    assert_eq!(
        ObjectModifications::parse(&w.0, ObjectKind::Units),
        Err(Error::UnsupportedVersion(3))
    );

    let mut w = Writer::default();
    w.u32(2)
        .u32(1)
        .code(b"hfoo")
        .u32(0)
        .u32(1)
        .code(b"uhpm")
        .u32(9)
        .u32(0)
        .u32(0);
    w.u32(0);
    assert!(matches!(
        ObjectModifications::parse(&w.0, ObjectKind::Units),
        Err(Error::Invalid {
            what: "value type",
            ..
        })
    ));
}

#[test]
fn file_names_and_levels() {
    assert_eq!(ObjectKind::Abilities.file_name(), "war3map.w3a");
    let with_levels: Vec<_> = ObjectKind::ALL
        .into_iter()
        .filter(|k| k.has_levels())
        .collect();
    assert_eq!(
        with_levels,
        [
            ObjectKind::Doodads,
            ObjectKind::Abilities,
            ObjectKind::Upgrades
        ]
    );
}
