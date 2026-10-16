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
"#;

/// Runs `main` of `script` after the declarations and returns the output.
fn run(script: &str) -> Vec<String> {
    let declarations = ow3_jass::parse(DECLARATIONS).unwrap();
    let script = ow3_jass::parse(script).unwrap();
    let refs = [&declarations, &script];
    let program = ow3_jass::check::check(&refs).unwrap();
    let mut game = Game::load(&program, &refs).unwrap();
    game.run("main");
    assert!(game.state.borrow().errors.is_empty());
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
