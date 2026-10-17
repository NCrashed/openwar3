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
