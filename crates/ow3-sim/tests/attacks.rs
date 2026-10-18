//! Attacks of units (SPEC-0016) with the custom data set of the game:
//! a knight hitting a target of medium armor 10 after
//! `SetRandomSeed(777)`. Skipped without game data.

use ow3_sim::game::Game;

const SCRIPT: &str = r#"
type agent extends handle
type event extends agent
type player extends agent
type widget extends agent
type unit extends widget
type timer extends agent
type trigger extends agent
type triggeraction extends handle
type eventid extends handle
type unitevent extends eventid
native Player takes integer number returns player
native DisplayTextToPlayer takes player toPlayer, real x, real y, string message returns nothing
native R2S takes real r returns string
native R2SW takes real r, integer width, integer precision returns string
native SetRandomSeed takes integer seed returns nothing
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native CreateTimer takes nothing returns timer
native TimerStart takes timer whichTimer, real timeout, boolean periodic, code handlerFunc returns nothing
native TimerGetElapsed takes timer whichTimer returns real
native CreateTrigger takes nothing returns trigger
native TriggerAddAction takes trigger whichTrigger, code actionFunc returns triggeraction
native TriggerRegisterUnitEvent takes trigger whichTrigger, unit whichUnit, unitevent whichEvent returns event
native ConvertUnitEvent takes integer i returns unitevent
native GetEventDamage takes nothing returns real
native IssueTargetOrder takes unit whichUnit, string order, widget targetWidget returns boolean
native IssueImmediateOrder takes unit whichUnit, string order returns boolean
globals
    timer clock = null
    unit knight = null
    integer hits = 0
endglobals
function Print takes string s returns nothing
    call DisplayTextToPlayer(Player(0), 0, 0, s)
endfunction
function Hit takes nothing returns nothing
    set hits = hits + 1
    call Print(R2S(GetEventDamage()) + " at " + R2SW(TimerGetElapsed(clock), 1, 3))
    if hits == 6 then
        call IssueImmediateOrder(knight, "stop")
    endif
endfunction
function Nothing takes nothing returns nothing
endfunction
function main takes nothing returns nothing
    local trigger t = CreateTrigger()
    local unit target
    set knight = CreateUnit(Player(0), 'hkni', 0, 0, 0)
    set target = CreateUnit(Player(15), 'hfoo', 100, 0, 180)
    // After the units, which draw from the generator of the dice.
    call SetRandomSeed(777)
    call TriggerRegisterUnitEvent(t, target, ConvertUnitEvent(52))
    call TriggerAddAction(t, function Hit)
    set clock = CreateTimer()
    call TimerStart(clock, 100, false, function Nothing)
    if IssueTargetOrder(knight, "attack", target) then
        call Print("attacking")
    endif
endfunction
"#;

#[test]
fn knight_attacks() {
    let game_data =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b");
    if !game_data.join("War3Patch.mpq").exists() {
        return;
    }
    let mut archives = ow3_sim::map::Archives::game(&game_data).unwrap();
    let mut data = ow3_sim::data::ObjectData::load(&mut archives, false).unwrap();
    // The target: plenty of life, medium armor 10.
    let footman = data.units.get_mut(&i32::from_be_bytes(*b"hfoo")).unwrap();
    footman.hp = 100_000f32.to_bits();
    footman.armor = 10f32.to_bits();
    footman.armor_type = Some(1);
    let script = ow3_jass::parse(SCRIPT).unwrap();
    let refs = [&script];
    let program = ow3_jass::check::check(&refs).unwrap();
    let mut game = Game::load(&program, &refs).unwrap();
    game.state.borrow_mut().data = data;
    game.state.borrow_mut().combat = ow3_sim::combat::Combat::load(&mut archives, false);
    game.run("main");
    while game.ticks() < 3000 {
        game.advance(25_000);
    }
    assert!(game.state.borrow().errors.is_empty());
    // Rounded as R2S shows them; hits 1.5 s apart, the first one in tick
    // 151 (SPEC-0019, "When it starts": the order of `main` at tick 0, the
    // delay of seed 777, 6.58 ticks, counted from tick 7, the swing at
    // the update of tick 19 and the hit dmgpt1, 132 ticks, later).
    let output = game.output();
    let hits: Vec<(&str, f32)> = output[1..]
        .iter()
        .map(|l| {
            let (damage, at) = l.split_once(" at ").unwrap();
            (damage, at.parse().unwrap())
        })
        .collect();
    assert_eq!(output[0], "attacking");
    assert_eq!(
        hits.iter().map(|h| h.0).collect::<Vec<_>>(),
        ["29.062", "30.000", "29.062", "29.062", "30.000", "27.187"]
    );
    assert!(
        hits.windows(2)
            .all(|w| (w[1].1 - w[0].1 - 1.5).abs() < 0.001)
    );
}
