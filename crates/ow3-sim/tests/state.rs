//! Natives that keep state for scripts: tech, player and game states,
//! timer expiry events, unit data, rects and camera bounds, and objects
//! the simulation does not model yet. The script declares what it uses.

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
type location extends agent
type rect extends agent
type sound extends agent
type dialog extends agent
type playerstate extends handle
type fgamestate extends handle
type version extends handle
native Player takes integer number returns player
native DisplayTextToPlayer takes player toPlayer, real x, real y, string message returns nothing
native I2S takes integer i returns string
native R2S takes real r returns string
native GetHandleId takes handle h returns integer
native ConvertPlayerState takes integer i returns playerstate
native ConvertFGameState takes integer i returns fgamestate
native SetPlayerTechMaxAllowed takes player whichPlayer, integer techid, integer maximum returns nothing
native GetPlayerTechMaxAllowed takes player whichPlayer, integer techid returns integer
native AddPlayerTechResearched takes player whichPlayer, integer techid, integer levels returns nothing
native GetPlayerTechResearched takes player whichPlayer, integer techid, boolean specificonly returns boolean
native SetPlayerState takes player whichPlayer, playerstate whichPlayerState, integer value returns nothing
native GetPlayerState takes player whichPlayer, playerstate whichPlayerState returns integer
native SetFloatGameState takes fgamestate whichFloatGameState, real value returns nothing
native GetFloatGameState takes fgamestate whichFloatGameState returns real
native VersionGet takes nothing returns version
native CreateTimer takes nothing returns timer
native TimerStart takes timer whichTimer, real timeout, boolean periodic, code handlerFunc returns nothing
native GetExpiredTimer takes nothing returns timer
native CreateTrigger takes nothing returns trigger
native TriggerAddAction takes trigger whichTrigger, code actionFunc returns triggeraction
native TriggerRegisterTimerExpireEvent takes trigger whichTrigger, timer t returns event
native Location takes real x, real y returns location
native GetLocationX takes location whichLocation returns real
native CreateUnitAtLoc takes player id, integer unitid, location whichLocation, real face returns unit
native GetUnitLoc takes unit whichUnit returns location
native SetUnitX takes unit whichUnit, real newX returns nothing
native GetUnitX takes unit whichUnit returns real
native SetUnitOwner takes unit whichUnit, player whichPlayer, boolean changeColor returns nothing
native GetOwningPlayer takes unit whichUnit returns player
native GetPlayerId takes player whichPlayer returns integer
native SetUnitUserData takes unit whichUnit, integer data returns nothing
native GetUnitUserData takes unit whichUnit returns integer
native GetPlayerUnitCount takes player whichPlayer, boolean includeIncomplete returns integer
native Rect takes real minx, real miny, real maxx, real maxy returns rect
native GetRectCenterX takes rect whichRect returns real
native SetCameraBounds takes real x1, real y1, real x2, real y2, real x3, real y3, real x4, real y4 returns nothing
native GetCameraBoundMinX takes nothing returns real
native GetCameraBoundMaxY takes nothing returns real
native CreateSound takes string fileName, boolean looping, boolean is3D, boolean stopwhenoutofrange, integer fadeInRate, integer fadeOutRate, string eaxSetting returns sound
native DialogCreate takes nothing returns dialog
"#;

const SCRIPT: &str = r#"
globals
    timer clock = null
endglobals
function Print takes string s returns nothing
    call DisplayTextToPlayer(Player(0), 0, 0, s)
endfunction
function Callback takes nothing returns nothing
    call Print("callback")
endfunction
function OnExpire takes nothing returns nothing
    if GetExpiredTimer() == clock then
        call Print("expire event")
    endif
endfunction
function main takes nothing returns nothing
    local trigger t = CreateTrigger()
    local unit u
    call Print("tech " + I2S(GetPlayerTechMaxAllowed(Player(0), 'hfoo')))
    call SetPlayerTechMaxAllowed(Player(0), 'hfoo', 3)
    call Print("tech " + I2S(GetPlayerTechMaxAllowed(Player(0), 'hfoo')))
    if not GetPlayerTechResearched(Player(0), 'Rhme', true) then
        call AddPlayerTechResearched(Player(0), 'Rhme', 1)
    endif
    if GetPlayerTechResearched(Player(0), 'Rhme', true) then
        call Print("researched")
    endif
    call SetPlayerState(Player(1), ConvertPlayerState(1), 500)
    call Print("gold " + I2S(GetPlayerState(Player(1), ConvertPlayerState(1))) + " " + I2S(GetPlayerState(Player(0), ConvertPlayerState(1))))
    call SetFloatGameState(ConvertFGameState(2), 12.5)
    call Print("time of day " + R2S(GetFloatGameState(ConvertFGameState(2))))
    call Print("version " + I2S(GetHandleId(VersionGet())))
    set u = CreateUnitAtLoc(Player(0), 'hfoo', Location(100, 200), 0)
    call SetUnitX(u, 300)
    call SetUnitOwner(u, Player(2), true)
    call SetUnitUserData(u, 42)
    call Print("unit at " + R2S(GetLocationX(GetUnitLoc(u))) + " owner " + I2S(GetPlayerId(GetOwningPlayer(u))) + " data " + I2S(GetUnitUserData(u)) + " count " + I2S(GetPlayerUnitCount(Player(2), true)))
    call Print("centre " + R2S(GetRectCenterX(Rect(-10, 0, 30, 0))))
    call SetCameraBounds(-100, -50, 200, 80, -100, 80, 200, -50)
    call Print("camera " + R2S(GetCameraBoundMinX()) + " " + R2S(GetCameraBoundMaxY()))
    if CreateSound("x", false, false, false, 0, 0, "") != null and DialogCreate() != null then
        call Print("objects are not null")
    endif
    set clock = CreateTimer()
    call TriggerRegisterTimerExpireEvent(t, clock)
    call TriggerAddAction(t, function OnExpire)
    call TimerStart(clock, 0.5, false, function Callback)
endfunction
"#;

#[test]
fn state_natives() {
    let declarations = ow3_jass::parse(DECLARATIONS).unwrap();
    let script = ow3_jass::parse(SCRIPT).unwrap();
    let refs = [&declarations, &script];
    let program = ow3_jass::check::check(&refs).unwrap();
    let mut game = Game::load(&program, &refs).unwrap();
    game.run("main");
    game.advance_seconds(1.0);
    assert!(game.state.borrow().errors.is_empty());
    assert_eq!(
        game.output(),
        [
            "tech -1",
            "tech 3",
            "researched",
            "gold 500 0",
            "time of day 12.500",
            "version 1",
            "unit at 300.000 owner 2 data 42 count 1",
            "centre 10.000",
            "camera -100.000 80.000",
            "objects are not null",
            "callback",
            "expire event",
        ]
    );
}
