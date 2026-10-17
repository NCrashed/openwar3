//! Unit life, facing and position as SPEC-0013 gives them, without game
//! data: every unit type is accepted with 1 hit point and no mana.

use ow3_sim::game::Game;

const DECLARATIONS: &str = r#"
type agent extends handle
type event extends agent
type player extends agent
type widget extends agent
type unit extends widget
type timer extends agent
type trigger extends agent
type triggeraction extends handle
type unitstate extends handle
type unittype extends handle
type widgetevent extends handle
native Player takes integer number returns player
native DisplayTextToPlayer takes player toPlayer, real x, real y, string message returns nothing
native R2S takes real r returns string
native ConvertUnitState takes integer i returns unitstate
native ConvertUnitType takes integer i returns unittype
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native GetUnitX takes unit whichUnit returns real
native GetUnitY takes unit whichUnit returns real
native GetUnitFacing takes unit whichUnit returns real
native SetUnitFacing takes unit whichUnit, real facingAngle returns nothing
native SetUnitPosition takes unit whichUnit, real newX, real newY returns nothing
native GetUnitState takes unit whichUnit, unitstate whichUnitState returns real
native SetUnitState takes unit whichUnit, unitstate whichUnitState, real newVal returns nothing
native GetWidgetLife takes widget whichWidget returns real
native SetWidgetLife takes widget whichWidget, real newLife returns nothing
native IsUnitType takes unit whichUnit, unittype whichUnitType returns boolean
native CreateTimer takes nothing returns timer
native TimerStart takes timer whichTimer, real timeout, boolean periodic, code handlerFunc returns nothing
native CreateTrigger takes nothing returns trigger
native TriggerAddAction takes trigger whichTrigger, code actionFunc returns triggeraction
native TriggerRegisterDeathEvent takes trigger whichTrigger, widget whichWidget returns event
"#;

const SCRIPT: &str = r#"
globals
    unit turning = null
endglobals
function Print takes string s returns nothing
    call DisplayTextToPlayer(Player(0), 0, 0, s)
endfunction
function Life takes unit u returns nothing
    call Print("life " + R2S(GetUnitState(u, ConvertUnitState(0))) + " of " + R2S(GetUnitState(u, ConvertUnitState(1))) + " mana " + R2S(GetUnitState(u, ConvertUnitState(2))))
endfunction
function Died takes nothing returns nothing
    call Print("died")
endfunction
function Later takes nothing returns nothing
    call Print("facing " + R2S(GetUnitFacing(turning)))
endfunction
function main takes nothing returns nothing
    local trigger t = CreateTrigger()
    local unit u = CreateUnit(Player(0), 'hfoo', 100.37, -1000.3, 400)
    call Print("at " + R2S(GetUnitX(u)) + " " + R2S(GetUnitY(u)) + " facing " + R2S(GetUnitFacing(u)))
    call Print("facing " + R2S(GetUnitFacing(CreateUnit(Player(0), 'hfoo', 0, 0, -90))) + " " + R2S(GetUnitFacing(CreateUnit(Player(0), 'hfoo', 0, 0, 720.5))))
    call Life(u)
    call SetUnitState(u, ConvertUnitState(1), 500)
    call SetUnitState(u, ConvertUnitState(2), 50)
    call SetWidgetLife(u, 5)
    call Life(u)
    call SetUnitState(u, ConvertUnitState(0), 0.406)
    call Life(u)
    call TriggerRegisterDeathEvent(t, u)
    call TriggerAddAction(t, function Died)
    call SetUnitState(u, ConvertUnitState(0), 0.405)
    call Print("after the call")
    call Life(u)
    if IsUnitType(u, ConvertUnitType(1)) then
        call Print("dead")
    endif
    set turning = CreateUnit(Player(0), 'hfoo', 0, 0, 10)
    call SetUnitFacing(turning, 90)
    call Print("facing " + R2S(GetUnitFacing(turning)))
    call TimerStart(CreateTimer(), 1, false, function Later)
endfunction
"#;

#[test]
fn unit_states_facing_and_position() {
    let declarations = ow3_jass::parse(DECLARATIONS).unwrap();
    let script = ow3_jass::parse(SCRIPT).unwrap();
    let refs = [&declarations, &script];
    let program = ow3_jass::check::check(&refs).unwrap();
    let mut game = Game::load(&program, &refs).unwrap();
    game.run("main");
    game.advance_seconds(1.5);
    assert!(game.state.borrow().errors.is_empty());
    assert_eq!(
        game.output(),
        [
            "at 100.250 -1000.500 facing 40.000",
            "facing 270.000 0.500",
            "life 1.000 of 1.000 mana 0.000",
            "life 1.000 of 1.000 mana 0.000",
            "life 0.406 of 1.000 mana 0.000",
            "died",
            "after the call",
            "life 0.000 of 1.000 mana 0.000",
            "dead",
            "facing 10.000",
            "facing 90.000",
        ]
    );
}

/// What a viewer sees: units in creation order with their position,
/// facing in radians and death.
#[test]
fn unit_views() {
    let declarations = ow3_jass::parse(DECLARATIONS).unwrap();
    let script = ow3_jass::parse(
        r#"
function main takes nothing returns nothing
    local unit u = CreateUnit(Player(0), 'hfoo', 100, 200, 90)
    call CreateUnit(Player(1), 'hkni', -50, 0, 0)
    call SetUnitState(u, ConvertUnitState(0), 0)
endfunction
"#,
    )
    .unwrap();
    let refs = [&declarations, &script];
    let program = ow3_jass::check::check(&refs).unwrap();
    let mut game = Game::load(&program, &refs).unwrap();
    game.run("main");
    let units = game.units();
    assert_eq!(units.len(), 2);
    let (a, b) = (units[0], units[1]);
    assert_eq!((a.x, a.y, a.owner), (100.0, 200.0, 0));
    assert!((a.facing - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    assert!(!a.alive && a.died_at.is_some());
    assert!(b.alive && b.died_at.is_none() && b.owner == 1);
}
