//! Forces, groups and boolexprs on a small script that declares what it
//! uses, so these tests need no game data.

use ow3_sim::game::Game;

const DECLARATIONS: &str = r#"
type agent extends handle
type player extends agent
type force extends agent
type group extends agent
type widget extends agent
type unit extends widget
type boolexpr extends agent
type conditionfunc extends boolexpr
type filterfunc extends boolexpr
type location extends agent
native Player takes integer number returns player
native GetPlayerId takes player whichPlayer returns integer
native DisplayTextToPlayer takes player toPlayer, real x, real y, string message returns nothing
native I2S takes integer i returns string
native CreateForce takes nothing returns force
native ForceAddPlayer takes force whichForce, player whichPlayer returns nothing
native ForceRemovePlayer takes force whichForce, player whichPlayer returns nothing
native ForceEnumPlayers takes force whichForce, boolexpr filter returns nothing
native ForForce takes force whichForce, code callback returns nothing
native GetEnumPlayer takes nothing returns player
native IsPlayerInForce takes player whichPlayer, force whichForce returns boolean
native CreateGroup takes nothing returns group
native GroupAddUnit takes group whichGroup, unit whichUnit returns nothing
native GroupRemoveUnit takes group whichGroup, unit whichUnit returns nothing
native ForGroup takes group whichGroup, code callback returns nothing
native FirstOfGroup takes group whichGroup returns unit
native GetEnumUnit takes nothing returns unit
native GetFilterUnit takes nothing returns unit
native GroupEnumUnitsInRange takes group whichGroup, real x, real y, real radius, boolexpr filter returns nothing
native GroupEnumUnitsInRangeCounted takes group whichGroup, real x, real y, real radius, boolexpr filter, integer countLimit returns nothing
native GroupEnumUnitsOfPlayer takes group whichGroup, player whichPlayer, boolexpr filter returns nothing
native GroupEnumUnitsInRangeOfLoc takes group whichGroup, location whichLocation, real radius, boolexpr filter returns nothing
native Location takes real x, real y returns location
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native GetUnitTypeId takes unit whichUnit returns integer
native Condition takes code func returns conditionfunc
native Filter takes code func returns filterfunc
native And takes boolexpr operandA, boolexpr operandB returns boolexpr
native Or takes boolexpr operandA, boolexpr operandB returns boolexpr
native Not takes boolexpr operand returns boolexpr
native DestroyBoolExpr takes boolexpr e returns nothing
type trigger extends agent
type triggercondition extends agent
native CreateTrigger takes nothing returns trigger
native TriggerAddCondition takes trigger whichTrigger, boolexpr condition returns triggercondition
native TriggerEvaluate takes trigger whichTrigger returns boolean
native GetHandleId takes handle h returns integer
native ExecuteFunc takes string funcName returns nothing
native ForceClear takes force whichForce returns nothing
"#;

/// Runs `main` of `script` after the declarations and returns the output.
fn run(script: &str) -> Vec<String> {
    let declarations = ow3_jass::parse(DECLARATIONS).unwrap();
    let script = ow3_jass::parse(script).unwrap();
    let refs = [&declarations, &script];
    let program = ow3_jass::check::check(&refs).unwrap();
    let mut game = Game::load(&program, &refs).unwrap();
    game.run("main");
    let errors = game.state.borrow().errors.clone();
    assert!(
        errors.iter().all(|e| e.message.contains("ExecuteFunc")),
        "{errors:?}"
    );
    game.output()
}

const PRINT: &str = r#"
function Print takes string s returns nothing
    call DisplayTextToPlayer(Player(0), 0, 0, s)
endfunction
"#;

#[test]
fn forces_enumerate_players_in_id_order() {
    let output = run(&format!(
        "{PRINT}{}",
        r#"
function Show takes nothing returns nothing
    call Print(I2S(GetPlayerId(GetEnumPlayer())))
endfunction
function main takes nothing returns nothing
    local force f = CreateForce()
    call ForceAddPlayer(f, Player(5))
    call ForceAddPlayer(f, Player(1))
    call ForceAddPlayer(f, Player(3))
    call ForceAddPlayer(f, Player(1))
    call ForceRemovePlayer(f, Player(3))
    call ForForce(f, function Show)
    if IsPlayerInForce(Player(5), f) and not IsPlayerInForce(Player(3), f) then
        call Print("membership ok")
    endif
    call ForceEnumPlayers(f, null)
    call ForForce(f, function Show)
endfunction
"#
    ));
    assert_eq!(output, ["1", "5", "membership ok", "0"]);
}

#[test]
fn groups_enumerate_units_with_filters() {
    let output = run(&format!(
        "globals\n    integer seen = 0\nendglobals\n{PRINT}{}",
        r#"
function Show takes nothing returns nothing
    call Print(I2S(GetUnitTypeId(GetEnumUnit())))
endfunction
function Odd takes nothing returns boolean
    set seen = seen + 1
    return GetUnitTypeId(GetFilterUnit()) / 2 * 2 != GetUnitTypeId(GetFilterUnit())
endfunction
function Big takes nothing returns boolean
    return GetUnitTypeId(GetFilterUnit()) > 2
endfunction
function main takes nothing returns nothing
    local group g = CreateGroup()
    local unit u1 = CreateUnit(Player(0), 1, 0, 0, 0)
    local unit u2 = CreateUnit(Player(1), 2, 100, 0, 0)
    local unit u3 = CreateUnit(Player(0), 3, 0, 300, 0)
    local unit u4 = CreateUnit(Player(0), 4, 500, 0, 0)
    call GroupAddUnit(g, u3)
    call GroupAddUnit(g, u1)
    call GroupAddUnit(g, u3)
    call ForGroup(g, function Show)
    call Print("-")
    call GroupEnumUnitsInRange(g, 0, 0, 300, null)
    call ForGroup(g, function Show)
    call Print("-")
    call GroupEnumUnitsInRange(g, 0, 0, 1000, And(Filter(function Odd), Not(Filter(function Big))))
    call ForGroup(g, function Show)
    call Print("seen " + I2S(seen))
    call GroupEnumUnitsInRange(g, 0, 0, 1000, Or(Filter(function Big), Filter(function Odd)))
    call ForGroup(g, function Show)
    call Print("-")
    call GroupEnumUnitsInRangeCounted(g, 0, 0, 1000, null, 2)
    call ForGroup(g, function Show)
    call Print("-")
    call GroupEnumUnitsOfPlayer(g, Player(1), null)
    call ForGroup(g, function Show)
    call GroupEnumUnitsInRangeOfLoc(g, Location(500, 50), 50, null)
    call ForGroup(g, function Show)
    call GroupRemoveUnit(g, u4)
    if FirstOfGroup(g) == null then
        call Print("empty")
    endif
endfunction
"#
    ));
    assert_eq!(
        output,
        [
            "3", "1", "-", // insertion order
            "1", "2", "3", "-", // the radius is inclusive
            "1", "seen 4", // And stops after a false left side
            "1", "3", "4", "-", // Or skips Odd when Big holds
            "1", "2", "-", // counted
            "2", "4", "empty",
        ]
    );
}

/// SPEC-0006: boolexprs, trigger conditions, handle ids, forces and
/// `ExecuteFunc`.
#[test]
fn spec_0006_rules() {
    let output = run(&format!(
        "globals\n    integer calls = 0\nendglobals\n{PRINT}{}",
        r#"
function Yes takes nothing returns boolean
    set calls = calls + 1
    return true
endfunction
function No takes nothing returns boolean
    set calls = calls + 1
    return false
endfunction
function Show takes nothing returns nothing
    call Print(I2S(GetPlayerId(GetEnumPlayer())))
endfunction
function Takes takes integer i returns nothing
    call Print("never")
endfunction
function Returns takes nothing returns integer
    call Print("returns ran")
    return 1
endfunction
function main takes nothing returns nothing
    local trigger t = CreateTrigger()
    local force f = CreateForce()
    if Condition(function Yes) == Filter(function Yes) then
        call Print("same boolexpr")
    endif
    call TriggerAddCondition(t, Condition(function No))
    call TriggerAddCondition(t, Condition(function Yes))
    if not TriggerEvaluate(t) then
        call Print("false after " + I2S(calls) + " calls")
    endif
    call ForceEnumPlayers(f, And(Condition(function Yes), null))
    call ForForce(f, function Show)
    call ForceEnumPlayers(f, null)
    call ForForce(f, function Show)
    call Print("player 0 id " + I2S(GetHandleId(Player(0))))
    call ExecuteFunc("returns")
    call ExecuteFunc("Returns")
    call ExecuteFunc("Takes")
    call Print("not reached")
endfunction
"#
    ));
    assert_eq!(
        output,
        [
            "same boolexpr",
            // Both conditions run although the first is false.
            "false after 2 calls",
            // And with a null operand is false: the force stays empty.
            // ForceEnumPlayers(f, null) leaves out the neutral players.
            "0",
            "player 0 id 1048584",
            // The name is case-sensitive; the result is discarded.
            "returns ran",
        ]
    );
}

/// SPEC-0006: `IsUnitInRangeXY` subtracts the collision size (a footman,
/// collision 31, 1000 away is in range from 969); SPEC-0019: a unit of
/// `Player(15)` is no enemy of `Player(0)`, one of `Player(1)` is.
#[test]
fn unit_range_and_enemies() {
    let natives =
        "native IsUnitInRangeXY takes unit whichUnit, real x, real y, real distance returns boolean
native IsUnitEnemy takes unit whichUnit, player whichPlayer returns boolean";
    let script = format!(
        "{natives}{PRINT}{}",
        r#"
function Show takes boolean b returns nothing
    if b then
        call Print("true")
    else
        call Print("false")
    endif
endfunction
function main takes nothing returns nothing
    local unit u = CreateUnit(Player(1), 'hfoo', 1000, 0, 0)
    call Show(IsUnitInRangeXY(u, 0, 0, 969))
    call Show(IsUnitInRangeXY(u, 0, 0, 968.9))
    call Show(IsUnitEnemy(u, Player(0)))
    call Show(IsUnitEnemy(CreateUnit(Player(15), 'hfoo', 0, 0, 0), Player(0)))
endfunction
"#
    );
    let declarations = ow3_jass::parse(DECLARATIONS).unwrap();
    let script = ow3_jass::parse(&script).unwrap();
    let refs = [&declarations, &script];
    let program = ow3_jass::check::check(&refs).unwrap();
    let mut game = Game::load(&program, &refs).unwrap();
    let footman = ow3_sim::data::UnitType {
        collision: 31f32.to_bits(),
        ..Default::default()
    };
    let code = i32::from_be_bytes(*b"hfoo");
    game.state.borrow_mut().data.units.insert(code, footman);
    game.run("main");
    assert_eq!(game.output(), ["true", "false", "true", "false"]);
}

/// SPEC-0009: a hero type with hero data starts with 1 skill point, a
/// hero made from a footman with 0, and a non-hero has none.
#[test]
fn hero_skill_points() {
    let natives = "native GetHeroSkillPoints takes unit whichHero returns integer";
    let script = format!(
        "{natives}{PRINT}{}",
        r#"
function Show takes integer id returns nothing
    call Print(I2S(GetHeroSkillPoints(CreateUnit(Player(0), id, 0, 0, 0))))
endfunction
function main takes nothing returns nothing
    call Show('Hpal')
    call Show('H000')
    call Show('Hf00')
    call Show('hfoo')
endfunction
"#
    );
    let declarations = ow3_jass::parse(DECLARATIONS).unwrap();
    let script = ow3_jass::parse(&script).unwrap();
    let refs = [&declarations, &script];
    let program = ow3_jass::check::check(&refs).unwrap();
    let mut game = Game::load(&program, &refs).unwrap();
    for (code, hero_data) in [
        (b"Hpal", true),
        (b"H000", true),
        (b"Hf00", false),
        (b"hfoo", false),
    ] {
        let t = ow3_sim::data::UnitType {
            hero_data,
            hp: 100f32.to_bits(),
            ..Default::default()
        };
        game.state
            .borrow_mut()
            .data
            .units
            .insert(i32::from_be_bytes(*code), t);
    }
    game.run("main");
    assert_eq!(game.output(), ["1", "1", "0", "0"]);
}
