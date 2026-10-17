//! `UnitDamageTarget` and the damaged event (SPEC-0015), without game
//! data: every unit has 1 hit point, armor 0, and every multiplier is 1.

use ow3_sim::game::Game;

const DECLARATIONS: &str = r#"
type agent extends handle
type event extends agent
type player extends agent
type widget extends agent
type unit extends widget
type item extends widget
type destructable extends widget
type trigger extends agent
type triggeraction extends handle
type eventid extends handle
type unitevent extends eventid
type attacktype extends handle
type damagetype extends handle
type weapontype extends handle
native Player takes integer number returns player
native DisplayTextToPlayer takes player toPlayer, real x, real y, string message returns nothing
native R2S takes real r returns string
native I2S takes integer i returns string
native GetHandleId takes handle h returns integer
native ConvertUnitEvent takes integer i returns unitevent
native ConvertAttackType takes integer i returns attacktype
native ConvertDamageType takes integer i returns damagetype
native ConvertWeaponType takes integer i returns weapontype
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native GetWidgetLife takes widget whichWidget returns real
native CreateItem takes integer itemid, real x, real y returns item
native CreateDestructable takes integer objectid, real x, real y, real face, real scale, integer variation returns destructable
native CreateTrigger takes nothing returns trigger
native TriggerAddAction takes trigger whichTrigger, code actionFunc returns triggeraction
native TriggerRegisterUnitEvent takes trigger whichTrigger, unit whichUnit, unitevent whichEvent returns event
native TriggerRegisterDeathEvent takes trigger whichTrigger, widget whichWidget returns event
native GetTriggerUnit takes nothing returns unit
native GetTriggerEventId takes nothing returns eventid
native GetEventDamage takes nothing returns real
native GetEventDamageSource takes nothing returns unit
native GetKillingUnit takes nothing returns unit
native UnitDamageTarget takes unit whichUnit, widget target, real amount, boolean attack, boolean ranged, attacktype attackType, damagetype damageType, weapontype weaponType returns boolean
"#;

const SCRIPT: &str = r#"
globals
    unit source = null
    unit target = null
endglobals
function Print takes string s returns nothing
    call DisplayTextToPlayer(Player(0), 0, 0, s)
endfunction
function Damaged takes nothing returns nothing
    local string from = "other"
    if GetEventDamageSource() == source then
        set from = "source"
    endif
    call Print("damaged " + R2S(GetEventDamage()) + " life " + R2S(GetWidgetLife(GetTriggerUnit())) + " from " + from + " event " + I2S(GetHandleId(GetTriggerEventId())))
endfunction
function Died takes nothing returns nothing
    if GetKillingUnit() == source then
        call Print("killed by the source")
    endif
endfunction
function ItemDied takes nothing returns nothing
    if GetKillingUnit() == null then
        call Print("item died, no killer")
    endif
endfunction
function Hit takes real amount returns nothing
    local boolean done = UnitDamageTarget(source, target, amount, true, false, ConvertAttackType(2), ConvertDamageType(4), ConvertWeaponType(0))
    if done then
        call Print("done, life " + R2S(GetWidgetLife(target)))
    else
        call Print("not done")
    endif
endfunction
function main takes nothing returns nothing
    local trigger damaged = CreateTrigger()
    local trigger died = CreateTrigger()
    local item rat
    local destructable tree
    local trigger itemdied = CreateTrigger()
    set source = CreateUnit(Player(0), 'hfoo', 0, 0, 0)
    set target = CreateUnit(Player(1), 'hfoo', 100, 0, 0)
    call TriggerRegisterUnitEvent(damaged, target, ConvertUnitEvent(52))
    call TriggerAddAction(damaged, function Damaged)
    call TriggerRegisterUnitEvent(died, target, ConvertUnitEvent(53))
    call TriggerAddAction(died, function Died)
    call Hit(0.5)
    call Hit(0.2)
    call Hit(1)
    if not UnitDamageTarget(null, source, 1, true, false, ConvertAttackType(2), ConvertDamageType(4), ConvertWeaponType(0)) then
        call Print("no source")
    endif
    set rat = CreateItem('ratc', 0, 0)
    call UnitDamageTarget(source, rat, 0.25, true, false, ConvertAttackType(5), ConvertDamageType(4), ConvertWeaponType(0))
    set tree = CreateDestructable('LTlt', 0, 0, 0, 1, 0)
    call UnitDamageTarget(source, tree, 0.75, true, false, ConvertAttackType(2), ConvertDamageType(4), ConvertWeaponType(0))
    call Print("item " + R2S(GetWidgetLife(rat)) + " tree " + R2S(GetWidgetLife(tree)))
    call TriggerRegisterDeathEvent(itemdied, rat)
    call TriggerAddAction(itemdied, function ItemDied)
    call UnitDamageTarget(source, rat, 5, true, false, ConvertAttackType(5), ConvertDamageType(4), ConvertWeaponType(0))
    call Print("item life " + R2S(GetWidgetLife(rat)))
endfunction
"#;

#[test]
fn damage_and_events() {
    let declarations = ow3_jass::parse(DECLARATIONS).unwrap();
    let script = ow3_jass::parse(SCRIPT).unwrap();
    let refs = [&declarations, &script];
    let program = ow3_jass::check::check(&refs).unwrap();
    let mut game = Game::load(&program, &refs).unwrap();
    game.run("main");
    assert!(game.state.borrow().errors.is_empty());
    assert_eq!(
        game.output(),
        [
            "damaged 0.500 life 1.000 from source event 52",
            "done, life 0.500",
            // 0.3 left: 0.405 or less is death, with the source as killer.
            "damaged 0.200 life 0.500 from source event 52",
            "killed by the source",
            "done, life 0.000",
            "not done",
            "no source",
            "item 0.750 tree 0.250",
            "item died, no killer",
            "item life 0.000",
        ]
    );
}

/// The two copies of the constants differ in spell damage against heroes
/// (SPEC-0015): 0.70 for melee maps, 0.75 for the others.
#[test]
fn melee_and_custom_constants() {
    let game_data =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b");
    if !game_data.join("War3Patch.mpq").exists() {
        return;
    }
    let mut archives = ow3_sim::map::Archives::game(&game_data).unwrap();
    let hero = 5;
    let spells = 0;
    let melee = ow3_sim::combat::Combat::load(&mut archives, true);
    let custom = ow3_sim::combat::Combat::load(&mut archives, false);
    assert_eq!(melee.bonus[spells][hero], 0x3F33_3334);
    assert_eq!(custom.bonus[spells][hero], 0.75f32.to_bits());
    assert_eq!(melee.defense_armor, 0x3D75_C290);
    // A tree's armor type, Wood, is outside the lists; the footman's is
    // one of them.
    let data = ow3_sim::data::ObjectData::load(&mut archives, true).unwrap();
    let code = |c: &[u8; 4]| i32::from_be_bytes(*c);
    assert_eq!(data.destructable_armor[&code(b"LTlt")], None);
    assert!(data.units[&code(b"hfoo")].armor_type.is_some());
    // 100 pierce damage against medium armor 10: 46.88 as R2SW shows it.
    let d = custom.damage(100f32.to_bits(), 10f32.to_bits(), Some(1), 2, 4);
    assert_eq!(f32::from_bits(d), 46.874_992);
}

/// The example of SPEC-0016: a knight of the custom data set, after
/// `SetRandomSeed(777)`, against medium armor 10.
#[test]
fn knight_hits() {
    let game_data =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b");
    if !game_data.join("War3Patch.mpq").exists() {
        return;
    }
    let mut archives = ow3_sim::map::Archives::game(&game_data).unwrap();
    let data = ow3_sim::data::ObjectData::load(&mut archives, false).unwrap();
    let combat = ow3_sim::combat::Combat::load(&mut archives, false);
    let knight = &data.units[&i32::from_be_bytes(*b"hkni")];
    let weapon = knight.weapon.as_ref().unwrap();
    assert_eq!((weapon.damage, weapon.dice, weapon.sides), (25, 2, 5));
    assert_eq!(f32::from_bits(weapon.cooldown), 1.5);
    let mut dice = ow3_sim::random::Random::reseed(777).1[ow3_sim::random::ATTACK_DICE].clone();
    let hits: Vec<f32> = (0..6)
        .map(|_| {
            let amount = ow3_sim::combat::hit_amount(weapon, 0, &mut dice);
            let d = combat.damage(amount, 10f32.to_bits(), Some(1), weapon.attack_type, 4);
            f32::from_bits(d)
        })
        .collect();
    // The spec lists these rounded; with the armor factor 0x3F1FFFFF of
    // SPEC-0015 they are a few units of the last place below.
    let expected = [29.0625, 30.0, 29.0625, 29.0625, 30.0, 27.1875];
    for (hit, expected) in hits.iter().zip(expected) {
        assert!((hit - expected).abs() < 1e-4, "{hits:?}");
    }
    // The melee data set has the knight at 28 and 1.4 s.
    let melee = ow3_sim::data::ObjectData::load(&mut archives, true).unwrap();
    let weapon = melee.units[&i32::from_be_bytes(*b"hkni")]
        .weapon
        .clone()
        .unwrap();
    assert_eq!((weapon.damage, f32::from_bits(weapon.cooldown)), (28, 1.4));
}
