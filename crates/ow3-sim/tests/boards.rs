//! Leaderboards, multiboards and dialogs on a small script that declares
//! what it uses.

use ow3_sim::game::Game;

const DECLARATIONS: &str = r#"
type agent extends handle
type event extends agent
type player extends agent
type trigger extends agent
type triggeraction extends handle
type leaderboard extends agent
type multiboard extends agent
type multiboarditem extends agent
type dialog extends agent
type button extends agent
native Player takes integer number returns player
native GetPlayerId takes player whichPlayer returns integer
native DisplayTextToPlayer takes player toPlayer, real x, real y, string message returns nothing
native I2S takes integer i returns string
native CreateTrigger takes nothing returns trigger
native TriggerAddAction takes trigger whichTrigger, code actionFunc returns triggeraction
native GetTriggerPlayer takes nothing returns player
native CreateLeaderboard takes nothing returns leaderboard
native LeaderboardAddItem takes leaderboard lb, string label, integer value, player p returns nothing
native LeaderboardSortItemsByValue takes leaderboard lb, boolean ascending returns nothing
native LeaderboardGetPlayerIndex takes leaderboard lb, player p returns integer
native LeaderboardGetItemCount takes leaderboard lb returns integer
native LeaderboardRemovePlayerItem takes leaderboard lb, player p returns nothing
native LeaderboardHasPlayerItem takes leaderboard lb, player p returns boolean
native PlayerSetLeaderboard takes player toPlayer, leaderboard lb returns nothing
native PlayerGetLeaderboard takes player toPlayer returns leaderboard
native CreateMultiboard takes nothing returns multiboard
native MultiboardSetRowCount takes multiboard lb, integer count returns nothing
native MultiboardSetColumnCount takes multiboard lb, integer count returns nothing
native MultiboardGetRowCount takes multiboard lb returns integer
native MultiboardGetColumnCount takes multiboard lb returns integer
native MultiboardDisplay takes multiboard lb, boolean show returns nothing
native IsMultiboardDisplayed takes multiboard lb returns boolean
native DialogCreate takes nothing returns dialog
native DialogAddButton takes dialog whichDialog, string buttonText, integer hotkey returns button
native DialogAddQuitButton takes dialog whichDialog, boolean doScoreScreen, string buttonText, integer hotkey returns button
native TriggerRegisterDialogEvent takes trigger whichTrigger, dialog whichDialog returns event
native TriggerRegisterDialogButtonEvent takes trigger whichTrigger, button whichButton returns event
native GetClickedButton takes nothing returns button
native GetClickedDialog takes nothing returns dialog
"#;

const SCRIPT: &str = r#"
globals
    dialog d = null
    button yes = null
endglobals
function Print takes string s returns nothing
    call DisplayTextToPlayer(Player(0), 0, 0, s)
endfunction
function OnDialog takes nothing returns nothing
    if GetClickedDialog() == d and GetClickedButton() == yes then
        call Print("dialog: yes by " + I2S(GetPlayerId(GetTriggerPlayer())))
    endif
endfunction
function OnYes takes nothing returns nothing
    call Print("button: yes")
endfunction
function main takes nothing returns nothing
    local leaderboard lb = CreateLeaderboard()
    local multiboard mb = CreateMultiboard()
    local trigger t = CreateTrigger()
    call LeaderboardAddItem(lb, "a", 5, Player(0))
    call LeaderboardAddItem(lb, "b", 9, Player(1))
    call LeaderboardAddItem(lb, "c", 7, Player(2))
    call LeaderboardSortItemsByValue(lb, false)
    call Print("index of 1: " + I2S(LeaderboardGetPlayerIndex(lb, Player(1))) + ", of 0: " + I2S(LeaderboardGetPlayerIndex(lb, Player(0))))
    call LeaderboardRemovePlayerItem(lb, Player(1))
    if not LeaderboardHasPlayerItem(lb, Player(1)) then
        call Print("rows " + I2S(LeaderboardGetItemCount(lb)))
    endif
    call PlayerSetLeaderboard(Player(0), lb)
    if PlayerGetLeaderboard(Player(0)) == lb and PlayerGetLeaderboard(Player(1)) == null then
        call Print("player 0 sees it")
    endif
    call MultiboardSetRowCount(mb, 3)
    call MultiboardSetColumnCount(mb, 2)
    call MultiboardDisplay(mb, true)
    if IsMultiboardDisplayed(mb) then
        call Print("multiboard " + I2S(MultiboardGetRowCount(mb)) + "x" + I2S(MultiboardGetColumnCount(mb)))
    endif
    set d = DialogCreate()
    set yes = DialogAddButton(d, "Yes", 'Y')
    call DialogAddQuitButton(d, true, "Quit", 'Q')
    call TriggerRegisterDialogEvent(t, d)
    call TriggerAddAction(t, function OnDialog)
    set t = CreateTrigger()
    call TriggerRegisterDialogButtonEvent(t, yes)
    call TriggerAddAction(t, function OnYes)
endfunction
"#;

#[test]
fn leaderboards_multiboards_and_dialogs() {
    let declarations = ow3_jass::parse(DECLARATIONS).unwrap();
    let script = ow3_jass::parse(SCRIPT).unwrap();
    let refs = [&declarations, &script];
    let program = ow3_jass::check::check(&refs).unwrap();
    let mut game = Game::load(&program, &refs).unwrap();
    game.run("main");
    let buttons = game.dialog_buttons();
    assert_eq!(buttons.len(), 2);
    assert_eq!(
        (buttons[0].1.text.as_str(), buttons[0].1.quit),
        ("Yes", false)
    );
    assert_eq!(
        (buttons[1].1.text.as_str(), buttons[1].1.quit),
        ("Quit", true)
    );
    game.click_button(buttons[1].0, 0);
    game.click_button(buttons[0].0, 3);
    assert!(game.state.borrow().errors.is_empty());
    assert_eq!(
        game.output(),
        [
            "index of 1: 0, of 0: 2",
            "rows 2",
            "player 0 sees it",
            "multiboard 3x2",
            // The quit button runs only the dialog's trigger, which ignores
            // it; the yes button runs both, in the order registered.
            "dialog: yes by 3",
            "button: yes",
        ]
    );
}

const SPEC_0010: &str = r#"
globals
    dialog d = null
    timer t = null
endglobals
function Print takes string s returns nothing
    call DisplayTextToPlayer(Player(0), 0, 0, s)
endfunction
function Rows takes leaderboard lb returns string
    return I2S(LeaderboardGetItemCount(lb)) + " | " + I2S(LeaderboardGetPlayerIndex(lb, Player(0))) + " " + I2S(LeaderboardGetPlayerIndex(lb, Player(1))) + " " + I2S(LeaderboardGetPlayerIndex(lb, Player(2)))
endfunction
function Responses takes string who returns nothing
    call Print(who + " button " + I2S(GetHandleId(GetClickedButton())) + " dialog " + I2S(GetHandleId(GetClickedDialog())) + " player " + I2S(GetPlayerId(GetTriggerPlayer())) + " event " + I2S(GetHandleId(GetTriggerEventId())))
endfunction
function D0 takes nothing returns nothing
    call Responses("d0")
endfunction
function B1 takes nothing returns nothing
    call Responses("b1")
endfunction
function D2 takes nothing returns nothing
    call Print("d2")
endfunction
function Expired takes nothing returns nothing
    call Print("timer expired")
endfunction
function main takes nothing returns nothing
    local leaderboard lb = CreateLeaderboard()
    local trigger tr
    local button b
    call LeaderboardAddItem(lb, "", 1, Player(0))
    call LeaderboardAddItem(lb, "", 5, Player(1))
    call LeaderboardAddItem(lb, "", 10, Player(0))
    call LeaderboardSetSizeByItemCount(lb, 5)
    call LeaderboardAddItem(lb, "", 3, Player(2))
    call Print(Rows(lb))
    call LeaderboardRemovePlayerItem(lb, Player(0))
    call Print(Rows(lb))
    call LeaderboardRemoveItem(lb, 0)
    call LeaderboardRemoveItem(lb, 7)
    call Print(Rows(lb))
    set d = DialogCreate()
    set b = DialogAddButton(d, "Alpha", 65)
    set tr = CreateTrigger()
    call TriggerRegisterDialogEvent(tr, d)
    call TriggerAddAction(tr, function D0)
    set tr = CreateTrigger()
    call TriggerRegisterDialogButtonEvent(tr, b)
    call TriggerAddAction(tr, function B1)
    set tr = CreateTrigger()
    call TriggerRegisterDialogEvent(tr, d)
    call TriggerAddAction(tr, function D2)
    set t = CreateTimer()
    call TimerStart(t, 2.0, false, function Expired)
    call DialogDisplay(Player(0), d, true)
endfunction
"#;

/// SPEC-0010: leaderboard rows, the order and responses of a click, and
/// game time stopped while a dialog is open.
#[test]
fn spec_0010_rows_clicks_and_pause() {
    let declarations = format!(
        "{DECLARATIONS}
type timer extends agent
type eventid extends handle
native GetHandleId takes handle h returns integer
native LeaderboardSetSizeByItemCount takes leaderboard lb, integer count returns nothing
native LeaderboardRemoveItem takes leaderboard lb, integer index returns nothing
native DialogDisplay takes player whichPlayer, dialog whichDialog, boolean flag returns nothing
native GetTriggerEventId takes nothing returns eventid
native CreateTimer takes nothing returns timer
native TimerStart takes timer whichTimer, real timeout, boolean periodic, code handlerFunc returns nothing
"
    );
    let declarations = ow3_jass::parse(&declarations).unwrap();
    let script = ow3_jass::parse(SPEC_0010).unwrap();
    let refs = [&declarations, &script];
    let program = ow3_jass::check::check(&refs).unwrap();
    let mut game = Game::load(&program, &refs).unwrap();
    game.run("main");
    game.advance_seconds(20.0);
    let paused = game.output().len();
    let (button, _) = game.dialog_buttons()[0].clone();
    game.click_button(button, 0);
    game.advance_seconds(3.0);
    assert!(game.state.borrow().errors.is_empty());
    let output = game.output();
    assert_eq!(paused, 3, "{output:#?}");
    let ids = |s: &str| s.replace(&button.to_string(), "B");
    let output: Vec<String> = output.iter().map(|l| ids(l)).collect();
    assert_eq!(output[..3], ["4 | 0 1 3", "3 | 1 0 2", "2 | 0 -1 1"]);
    let d0 = &output[3];
    assert!(
        d0.starts_with("d0 button B dialog ") && d0.ends_with(" player 0 event 92"),
        "{d0}"
    );
    assert_eq!(
        output[4..],
        [
            "d2",
            "b1 button 0 dialog 0 player 0 event 90",
            "timer expired"
        ]
    );
}
