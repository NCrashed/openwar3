//! Player slots before `config`, after it and in `main` (SPEC-0008), on a
//! small script that declares what it uses.

use ow3_sim::game::Game;
use ow3_sim::game::players::Setup;

const DECLARATIONS: &str = r#"
type agent extends handle
type player extends agent
type mapcontrol extends handle
type playerslotstate extends handle
type alliancetype extends handle
type mapflag extends handle
native Player takes integer number returns player
native DisplayTextToPlayer takes player toPlayer, real x, real y, string message returns nothing
native I2S takes integer i returns string
native GetHandleId takes handle h returns integer
native ConvertMapControl takes integer i returns mapcontrol
native ConvertAllianceType takes integer i returns alliancetype
native ConvertMapFlag takes integer i returns mapflag
native GetPlayerController takes player whichPlayer returns mapcontrol
native SetPlayerController takes player whichPlayer, mapcontrol controlType returns nothing
native GetPlayerSlotState takes player whichPlayer returns playerslotstate
native GetPlayerTeam takes player whichPlayer returns integer
native SetPlayerTeam takes player whichPlayer, integer whichTeam returns nothing
native GetPlayerStartLocation takes player whichPlayer returns integer
native GetPlayerName takes player whichPlayer returns string
native GetLocalPlayer takes nothing returns player
native GetPlayerId takes player whichPlayer returns integer
native IsPlayerAlly takes player whichPlayer, player otherPlayer returns boolean
native SetPlayerAlliance takes player sourcePlayer, player otherPlayer, alliancetype whichAllianceSetting, boolean value returns nothing
native IsMapFlagSet takes mapflag whichMapFlag returns boolean
"#;

const SCRIPT: &str = r#"
globals
    integer array seen
    boolean array allies
endglobals
function Print takes string s returns nothing
    call DisplayTextToPlayer(GetLocalPlayer(), 0, 0, s)
endfunction
function B2I takes boolean b returns integer
    if b then
        return 1
    endif
    return 0
endfunction
function config takes nothing returns nothing
    set seen[0] = GetHandleId(GetPlayerController(Player(0)))
    set seen[1] = GetHandleId(GetPlayerController(Player(12)))
    set seen[2] = GetHandleId(GetPlayerSlotState(Player(0)))
    set seen[3] = GetHandleId(GetPlayerSlotState(Player(12)))
    set seen[4] = GetPlayerTeam(Player(0))
    set seen[5] = GetPlayerTeam(Player(12))
    set seen[6] = GetPlayerStartLocation(Player(0))
    set seen[7] = B2I(IsPlayerAlly(Player(0), Player(0)))
    set seen[8] = B2I(IsPlayerAlly(Player(0), Player(15)))
    set seen[9] = B2I(IsPlayerAlly(Player(0), Player(12)))
    set seen[10] = B2I(IsPlayerAlly(Player(15), Player(3)))
    call SetPlayerController(Player(0), ConvertMapControl(1))
    call SetPlayerController(Player(1), ConvertMapControl(0))
    call SetPlayerController(Player(2), ConvertMapControl(1))
    call SetPlayerController(Player(3), ConvertMapControl(0))
    call SetPlayerTeam(Player(3), 2)
endfunction
function main takes nothing returns nothing
    local integer i = 0
    loop
        exitwhen i > 10
        call Print("before config " + I2S(i) + ": " + I2S(seen[i]))
        set i = i + 1
    endloop
    call Print("local " + I2S(GetPlayerId(GetLocalPlayer())))
    set i = 0
    loop
        exitwhen i > 3
        call Print(I2S(i) + ": slot " + I2S(GetHandleId(GetPlayerSlotState(Player(i)))) + " team " + I2S(GetPlayerTeam(Player(i))) + " " + GetPlayerName(Player(i)))
        set i = i + 1
    endloop
    call SetPlayerAlliance(Player(1), Player(1), ConvertAllianceType(0), false)
    call Print("self ally after unset " + I2S(B2I(IsPlayerAlly(Player(1), Player(1)))))
    call Print("lock alliances " + I2S(B2I(IsMapFlagSet(ConvertMapFlag(1024)))))
endfunction
"#;

#[test]
fn fixed_player_settings() {
    let declarations = ow3_jass::parse(DECLARATIONS).unwrap();
    let script = ow3_jass::parse(SCRIPT).unwrap();
    let refs = [&declarations, &script];
    let program = ow3_jass::check::check(&refs).unwrap();
    let mut game = Game::load(&program, &refs).unwrap();
    {
        let mut setup = Setup::before_config();
        setup.fixed_players = true;
        game.state.borrow_mut().setup = setup;
    }
    game.start().unwrap();
    assert!(game.state.borrow().errors.is_empty());
    assert_eq!(
        game.output(),
        [
            // MAP_CONTROL_NONE and _CREEP, slot states empty and playing,
            // teams 0 and -1, start location -1.
            "before config 0: 5",
            "before config 1: 4",
            "before config 2: 0",
            "before config 3: 1",
            "before config 4: 0",
            "before config 5: -1",
            "before config 6: -1",
            // Passive toward itself and player 15, not toward 12; player
            // 15 toward everyone.
            "before config 7: 1",
            "before config 8: 1",
            "before config 9: 0",
            "before config 10: 1",
            // The first user slot is the local player; computers play,
            // other user slots are empty with team -1 and default names.
            "local 1",
            "0: slot 1 team 0 Player 1",
            "1: slot 1 team 0 Player",
            "2: slot 1 team 0 Player 3",
            "3: slot 0 team -1 Player 4",
            "self ally after unset 0",
            "lock alliances 1",
        ]
    );
}

#[test]
fn string_native_in_config_stops_the_map() {
    let declarations = ow3_jass::parse(DECLARATIONS).unwrap();
    let script = ow3_jass::parse(
        "function config takes nothing returns nothing
            call I2S(1)
        endfunction
        function main takes nothing returns nothing
            call DisplayTextToPlayer(Player(0), 0, 0, \"main ran\")
        endfunction",
    )
    .unwrap();
    let refs = [&declarations, &script];
    let program = ow3_jass::check::check(&refs).unwrap();
    let mut game = Game::load(&program, &refs).unwrap();
    assert!(game.start().is_err());
    assert!(game.output().is_empty());
}
