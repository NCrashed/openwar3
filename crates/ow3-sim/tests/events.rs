//! Death events and items on a small script that declares what it uses,
//! so these tests need no game data.

use ow3_sim::game::Game;

const DECLARATIONS: &str = r#"
type agent extends handle
type event extends agent
type player extends agent
type widget extends agent
type unit extends widget
type item extends widget
type trigger extends agent
type triggeraction extends handle
type boolexpr extends agent
type filterfunc extends boolexpr
type eventid extends handle
type unitevent extends eventid
type playerunitevent extends eventid
type rect extends agent
native Player takes integer number returns player
native GetPlayerId takes player whichPlayer returns integer
native DisplayTextToPlayer takes player toPlayer, real x, real y, string message returns nothing
native I2S takes integer i returns string
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native KillUnit takes unit whichUnit returns nothing
native GetUnitTypeId takes unit whichUnit returns integer
native GetWidgetLife takes widget whichWidget returns real
native R2S takes real r returns string
native CreateTrigger takes nothing returns trigger
native DisableTrigger takes trigger whichTrigger returns nothing
native TriggerAddAction takes trigger whichTrigger, code actionFunc returns triggeraction
native TriggerRegisterUnitEvent takes trigger whichTrigger, unit whichUnit, unitevent whichEvent returns event
native TriggerRegisterPlayerUnitEvent takes trigger whichTrigger, player whichPlayer, playerunitevent whichPlayerUnitEvent, boolexpr filter returns event
native TriggerRegisterDeathEvent takes trigger whichTrigger, widget whichWidget returns event
native GetTriggerUnit takes nothing returns unit
native GetTriggerPlayer takes nothing returns player
native GetTriggerWidget takes nothing returns widget
native GetTriggerEventId takes nothing returns eventid
native GetHandleId takes handle h returns integer
native GetFilterUnit takes nothing returns unit
native Filter takes code func returns filterfunc
native ConvertUnitEvent takes integer i returns unitevent
native ConvertPlayerUnitEvent takes integer i returns playerunitevent
native CreateItem takes integer itemid, real x, real y returns item
native RemoveItem takes item whichItem returns nothing
native GetItemTypeId takes item i returns integer
native Rect takes real minx, real miny, real maxx, real maxy returns rect
native EnumItemsInRect takes rect r, boolexpr filter, code actionFunc returns nothing
native GetEnumItem takes nothing returns item
native GetFilterItem takes nothing returns item
"#;

const SCRIPT: &str = r#"
function Print takes string s returns nothing
    call DisplayTextToPlayer(Player(0), 0, 0, s)
endfunction
function OnUnit takes nothing returns nothing
    call Print("unit event " + I2S(GetHandleId(GetTriggerEventId())) + " type " + I2S(GetUnitTypeId(GetTriggerUnit())))
endfunction
function OnPlayer takes nothing returns nothing
    call Print("player event " + I2S(GetHandleId(GetTriggerEventId())) + " player " + I2S(GetPlayerId(GetTriggerPlayer())) + " type " + I2S(GetUnitTypeId(GetTriggerUnit())))
endfunction
function OnWidget takes nothing returns nothing
    call Print("widget event " + I2S(GetHandleId(GetTriggerEventId())) + " life " + R2S(GetWidgetLife(GetTriggerWidget())))
endfunction
function Never takes nothing returns nothing
    call Print("disabled trigger ran")
endfunction
function OnlyType2 takes nothing returns boolean
    return GetUnitTypeId(GetFilterUnit()) == 2
endfunction
function ShowItem takes nothing returns nothing
    call Print("item " + I2S(GetItemTypeId(GetEnumItem())))
endfunction
function NotSeven takes nothing returns boolean
    return GetItemTypeId(GetFilterItem()) != 7
endfunction
function main takes nothing returns nothing
    local unit a = CreateUnit(Player(1), 1, 0, 0, 0)
    local unit b = CreateUnit(Player(1), 2, 0, 0, 0)
    local trigger t = CreateTrigger()
    call TriggerRegisterUnitEvent(t, a, ConvertUnitEvent(53))
    call TriggerAddAction(t, function OnUnit)
    set t = CreateTrigger()
    call TriggerRegisterPlayerUnitEvent(t, Player(1), ConvertPlayerUnitEvent(20), Filter(function OnlyType2))
    call TriggerAddAction(t, function OnPlayer)
    set t = CreateTrigger()
    call TriggerRegisterDeathEvent(t, b)
    call TriggerAddAction(t, function OnWidget)
    set t = CreateTrigger()
    call TriggerRegisterDeathEvent(t, a)
    call TriggerAddAction(t, function Never)
    call DisableTrigger(t)
    call KillUnit(a)
    call Print("-")
    call KillUnit(b)
    call KillUnit(b)
    call Print("-")
    call CreateItem(5, 10, 10)
    call CreateItem(7, 20, 20)
    call CreateItem(6, 500, 500)
    call RemoveItem(CreateItem(8, 0, 0))
    call EnumItemsInRect(Rect(0, 0, 100, 100), Filter(function NotSeven), function ShowItem)
endfunction
"#;

#[test]
fn death_events_and_items() {
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
            // Unit a: its unit event; the player event filter wants type 2;
            // the death event's trigger is disabled.
            "unit event 53 type 1",
            "-",
            // Unit b: player event, then death event, in the order they
            // were registered; a second KillUnit does nothing.
            "player event 20 player 1 type 2",
            "widget event 89 life 0.000",
            "-",
            // Items in the rect that pass the filter, in creation order.
            "item 5",
        ]
    );
}
