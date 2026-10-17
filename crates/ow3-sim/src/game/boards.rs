//! Leaderboards, multiboards and dialogs: what scripts put in them and
//! read back. Nothing is drawn; texts set through the natives of SPEC-0003
//! are stored with trigger strings resolved, so `LeaderboardGetLabelText`
//! and `MultiboardGetTitleText` return the entry text.
//!
//! Dialog buttons are clicked through [`super::Game::click_button`], which
//! runs the triggers registered for the button and its dialog.
//!
//! PROVISIONAL, listed in specs/QUESTIONS.md: `LeaderboardAddItem` always
//! adds a row, also for a player that has one; sorting keeps the order of
//! equal rows; `LeaderboardGetPlayerIndex` is -1 for a player without a
//! row; rows and columns of a multiboard are not limited; each
//! `MultiboardGetItem` makes a new handle; `GetLocalizedString` returns
//! its argument (after resolving a trigger string reference).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use ow3_jass::semantics::Value;
use ow3_jass::vm::{NativeFn, Vm};

use super::{Context, Object, State, fire_trigger, handle, integer, string};

#[derive(Clone, Debug, Default)]
pub struct Leaderboard {
    label: String,
    rows: Vec<Row>,
    size: i32,
    displayed: bool,
}

#[derive(Clone, Debug)]
struct Row {
    label: String,
    value: i32,
    player: i32,
}

#[derive(Clone, Debug, Default)]
pub struct Multiboard {
    title: String,
    rows: i32,
    columns: i32,
    displayed: bool,
    minimized: bool,
    /// Values of cells by (row, column).
    values: BTreeMap<(i32, i32), String>,
}

#[derive(Clone, Debug, Default)]
pub struct Dialog {
    message: String,
    buttons: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct Button {
    pub dialog: u32,
    pub text: String,
    pub hotkey: i32,
    pub quit: bool,
}

/// A trigger registered for a dialog or a button.
#[derive(Clone, Copy, Debug)]
pub enum DialogEvent {
    Dialog { trigger: u32, dialog: u32 },
    Button { trigger: u32, button: u32 },
}

fn text(value: &Value) -> String {
    string(value).unwrap_or("").to_owned()
}

fn leaderboard(state: &mut State, h: Option<u32>) -> Option<&mut Leaderboard> {
    match state.objects.get_mut(&h?)? {
        Object::Leaderboard(lb) => Some(lb),
        _ => None,
    }
}

fn multiboard(state: &mut State, h: Option<u32>) -> Option<&mut Multiboard> {
    match state.objects.get_mut(&h?)? {
        Object::Multiboard(mb) => Some(mb),
        _ => None,
    }
}

fn dialog(state: &mut State, h: Option<u32>) -> Option<&mut Dialog> {
    match state.objects.get_mut(&h?)? {
        Object::Dialog(d) => Some(d),
        _ => None,
    }
}

/// Runs the triggers registered for `button` and for its dialog, in the
/// order they were registered, as player `player` clicked it.
pub(super) fn click(vm: &mut Vm, state: &Rc<RefCell<State>>, button: u32, player: i32) {
    let (dialog, triggers, player) = {
        let mut state = state.borrow_mut();
        let Some(Object::Button(b)) = state.objects.get(&button) else {
            return;
        };
        let dialog = b.dialog;
        let triggers: Vec<u32> = state
            .dialog_events
            .iter()
            .filter_map(|e| match *e {
                DialogEvent::Dialog { trigger, dialog: d } if d == dialog => Some(trigger),
                DialogEvent::Button { trigger, button: b } if b == button => Some(trigger),
                _ => None,
            })
            .collect();
        (dialog, triggers, state.player_handle(player))
    };
    for trigger in triggers {
        let context = Context {
            clicked_button: Some(button),
            clicked_dialog: Some(dialog),
            triggering_player: Some(player),
            ..Context::default()
        };
        fire_trigger(vm, state, trigger, context);
    }
}

#[allow(unused_braces)]
pub(super) fn register(add: &mut impl FnMut(&str, NativeFn), state: &Rc<RefCell<State>>) {
    // Leaderboards.
    native!(
        add,
        state,
        "native CreateLeaderboard takes nothing returns leaderboard",
        |vm, state, args| {
            let lb = Object::Leaderboard(Leaderboard::default());
            Ok(Some(Value::Handle(Some(state.borrow_mut().create(lb)))))
        }
    );
    native!(
        add,
        state,
        "native DestroyLeaderboard takes leaderboard lb returns nothing",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            if leaderboard(&mut state, handle(&args[0])).is_some() {
                state.objects.remove(&handle(&args[0]).expect("found"));
            }
            Ok(None)
        }
    );
    type LbSet = fn(&mut Leaderboard, &[Value], &State);
    let setters: [(&str, LbSet); 9] = [
        (
            "native LeaderboardDisplay takes leaderboard lb, boolean show returns nothing",
            |lb, args, _| lb.displayed = args[1] == Value::Boolean(true),
        ),
        (
            "native LeaderboardSetSizeByItemCount takes leaderboard lb, integer count returns nothing",
            |lb, args, _| lb.size = integer(&args[1]),
        ),
        (
            "native LeaderboardSetLabel takes leaderboard lb, string label returns nothing",
            |lb, args, _| lb.label = text(&args[1]),
        ),
        (
            "native LeaderboardAddItem takes leaderboard lb, string label, integer value, player p returns nothing",
            |lb, args, state| {
                if let Some(player) = state.player_id(handle(&args[3])) {
                    lb.rows.push(Row {
                        label: text(&args[1]),
                        value: integer(&args[2]),
                        player,
                    });
                }
            },
        ),
        (
            "native LeaderboardRemoveItem takes leaderboard lb, integer index returns nothing",
            |lb, args, _| {
                if let Ok(i) = usize::try_from(integer(&args[1]))
                    && i < lb.rows.len()
                {
                    lb.rows.remove(i);
                }
            },
        ),
        (
            "native LeaderboardRemovePlayerItem takes leaderboard lb, player p returns nothing",
            |lb, args, state| {
                let player = state.player_id(handle(&args[1]));
                lb.rows.retain(|r| Some(r.player) != player);
            },
        ),
        (
            "native LeaderboardClear takes leaderboard lb returns nothing",
            |lb, _, _| lb.rows.clear(),
        ),
        (
            "native LeaderboardSetItemValue takes leaderboard lb, integer whichItem, integer val returns nothing",
            |lb, args, _| {
                if let Some(row) = usize::try_from(integer(&args[1]))
                    .ok()
                    .and_then(|i| lb.rows.get_mut(i))
                {
                    row.value = integer(&args[2]);
                }
            },
        ),
        (
            "native LeaderboardSetItemLabel takes leaderboard lb, integer whichItem, string val returns nothing",
            |lb, args, _| {
                if let Some(row) = usize::try_from(integer(&args[1]))
                    .ok()
                    .and_then(|i| lb.rows.get_mut(i))
                {
                    row.label = text(&args[2]);
                }
            },
        ),
    ];
    for (declaration, set) in setters {
        native!(add, state, declaration, |vm, state, args| {
            let mut state = state.borrow_mut();
            if let Some(mut lb) = leaderboard(&mut state, handle(&args[0])).cloned() {
                set(&mut lb, args, &state);
                *leaderboard(&mut state, handle(&args[0])).expect("found") = lb;
            }
            Ok(None)
        });
    }
    for (declaration, key) in [
        (
            "native LeaderboardSortItemsByValue takes leaderboard lb, boolean ascending returns nothing",
            0,
        ),
        (
            "native LeaderboardSortItemsByPlayer takes leaderboard lb, boolean ascending returns nothing",
            1,
        ),
        (
            "native LeaderboardSortItemsByLabel takes leaderboard lb, boolean ascending returns nothing",
            2,
        ),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let ascending = args[1] == Value::Boolean(true);
            let mut state = state.borrow_mut();
            if let Some(lb) = leaderboard(&mut state, handle(&args[0])) {
                lb.rows.sort_by(|a, b| {
                    let order = match key {
                        0 => a.value.cmp(&b.value),
                        1 => a.player.cmp(&b.player),
                        _ => a.label.cmp(&b.label),
                    };
                    if ascending { order } else { order.reverse() }
                });
            }
            Ok(None)
        });
    }
    native!(
        add,
        state,
        "native LeaderboardGetItemCount takes leaderboard lb returns integer",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let count = leaderboard(&mut state, handle(&args[0])).map_or(0, |lb| lb.rows.len());
            Ok(Some(Value::Integer(count as i32)))
        }
    );
    native!(
        add,
        state,
        "native IsLeaderboardDisplayed takes leaderboard lb returns boolean",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let shown = leaderboard(&mut state, handle(&args[0])).is_some_and(|lb| lb.displayed);
            Ok(Some(Value::Boolean(shown)))
        }
    );
    native!(
        add,
        state,
        "native LeaderboardGetLabelText takes leaderboard lb returns string",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            Ok(Some(match leaderboard(&mut state, handle(&args[0])) {
                Some(lb) => Value::string(&lb.label),
                None => Value::String(None),
            }))
        }
    );
    native!(
        add,
        state,
        "native LeaderboardHasPlayerItem takes leaderboard lb, player p returns boolean",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let player = state.player_id(handle(&args[1]));
            let has = leaderboard(&mut state, handle(&args[0]))
                .is_some_and(|lb| lb.rows.iter().any(|r| Some(r.player) == player));
            Ok(Some(Value::Boolean(has)))
        }
    );
    native!(
        add,
        state,
        "native LeaderboardGetPlayerIndex takes leaderboard lb, player p returns integer",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let player = state.player_id(handle(&args[1]));
            let index = leaderboard(&mut state, handle(&args[0]))
                .and_then(|lb| lb.rows.iter().position(|r| Some(r.player) == player));
            Ok(Some(Value::Integer(index.map_or(-1, |i| i as i32))))
        }
    );
    native!(
        add,
        state,
        "native PlayerSetLeaderboard takes player toPlayer, leaderboard lb returns nothing",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            if let Some(p) = state.player_id(handle(&args[0])) {
                state.player_leaderboards.insert(p, handle(&args[1]));
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native PlayerGetLeaderboard takes player toPlayer returns leaderboard",
        |vm, state, args| {
            let state = state.borrow();
            let lb = state
                .player_id(handle(&args[0]))
                .and_then(|p| state.player_leaderboards.get(&p).copied())
                .flatten()
                .filter(|h| matches!(state.objects.get(h), Some(Object::Leaderboard(_))));
            Ok(Some(Value::Handle(lb)))
        }
    );

    // Multiboards.
    native!(
        add,
        state,
        "native CreateMultiboard takes nothing returns multiboard",
        |vm, state, args| {
            let mb = Object::Multiboard(Multiboard::default());
            Ok(Some(Value::Handle(Some(state.borrow_mut().create(mb)))))
        }
    );
    native!(
        add,
        state,
        "native DestroyMultiboard takes multiboard lb returns nothing",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            if multiboard(&mut state, handle(&args[0])).is_some() {
                state.objects.remove(&handle(&args[0]).expect("found"));
            }
            Ok(None)
        }
    );
    type MbSet = fn(&mut Multiboard, &[Value]);
    let setters: [(&str, MbSet); 7] = [
        (
            "native MultiboardDisplay takes multiboard lb, boolean show returns nothing",
            |mb, args| mb.displayed = args[1] == Value::Boolean(true),
        ),
        (
            "native MultiboardMinimize takes multiboard lb, boolean minimize returns nothing",
            |mb, args| mb.minimized = args[1] == Value::Boolean(true),
        ),
        (
            "native MultiboardClear takes multiboard lb returns nothing",
            |mb, _| {
                mb.rows = 0;
                mb.columns = 0;
                mb.values.clear();
            },
        ),
        (
            "native MultiboardSetTitleText takes multiboard lb, string label returns nothing",
            |mb, args| mb.title = text(&args[1]),
        ),
        (
            "native MultiboardSetColumnCount takes multiboard lb, integer count returns nothing",
            |mb, args| mb.columns = integer(&args[1]),
        ),
        (
            "native MultiboardSetRowCount takes multiboard lb, integer count returns nothing",
            |mb, args| mb.rows = integer(&args[1]),
        ),
        (
            "native MultiboardSetItemsValue takes multiboard lb, string value returns nothing",
            |mb, args| {
                for row in 0..mb.rows {
                    for column in 0..mb.columns {
                        mb.values.insert((row, column), text(&args[1]));
                    }
                }
            },
        ),
    ];
    for (declaration, set) in setters {
        native!(add, state, declaration, |vm, state, args| {
            if let Some(mb) = multiboard(&mut state.borrow_mut(), handle(&args[0])) {
                set(mb, args);
            }
            Ok(None)
        });
    }
    type MbGet = fn(&Multiboard) -> Value;
    let getters: [(&str, MbGet); 5] = [
        (
            "native MultiboardGetTitleText takes multiboard lb returns string",
            |mb| Value::string(&mb.title),
        ),
        (
            "native MultiboardGetRowCount takes multiboard lb returns integer",
            |mb| Value::Integer(mb.rows),
        ),
        (
            "native MultiboardGetColumnCount takes multiboard lb returns integer",
            |mb| Value::Integer(mb.columns),
        ),
        (
            "native IsMultiboardDisplayed takes multiboard lb returns boolean",
            |mb| Value::Boolean(mb.displayed),
        ),
        (
            "native IsMultiboardMinimized takes multiboard lb returns boolean",
            |mb| Value::Boolean(mb.minimized),
        ),
    ];
    for (declaration, get) in getters {
        let empty = if declaration.ends_with("string") {
            Value::String(None)
        } else if declaration.ends_with("integer") {
            Value::Integer(0)
        } else {
            Value::Boolean(false)
        };
        native!(add, state, declaration, |vm, state, args| {
            let mut state = state.borrow_mut();
            Ok(Some(
                multiboard(&mut state, handle(&args[0])).map_or(empty.clone(), |mb| get(mb)),
            ))
        });
    }
    native!(
        add,
        state,
        "native MultiboardGetItem takes multiboard lb, integer row, integer column returns multiboarditem",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let Some(board) =
                handle(&args[0]).filter(|&h| multiboard(&mut state, Some(h)).is_some())
            else {
                return Ok(Some(Value::Handle(None)));
            };
            let item = Object::MultiboardItem {
                board,
                row: integer(&args[1]),
                column: integer(&args[2]),
            };
            Ok(Some(Value::Handle(Some(state.create(item)))))
        }
    );
    native!(
        add,
        state,
        "native MultiboardReleaseItem takes multiboarditem mbi returns nothing",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            if let Some(h) = handle(&args[0])
                && matches!(state.objects.get(&h), Some(Object::MultiboardItem { .. }))
            {
                state.objects.remove(&h);
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native MultiboardSetItemValue takes multiboarditem mbi, string val returns nothing",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let cell = match handle(&args[0]).and_then(|h| state.objects.get(&h)) {
                Some(&Object::MultiboardItem { board, row, column }) => Some((board, row, column)),
                _ => None,
            };
            if let Some((board, row, column)) = cell
                && let Some(mb) = multiboard(&mut state, Some(board))
            {
                mb.values.insert((row, column), text(&args[1]));
            }
            Ok(None)
        }
    );

    // Dialogs.
    native!(
        add,
        state,
        "native DialogCreate takes nothing returns dialog",
        |vm, state, args| {
            let d = Object::Dialog(Dialog::default());
            Ok(Some(Value::Handle(Some(state.borrow_mut().create(d)))))
        }
    );
    native!(
        add,
        state,
        "native DialogDestroy takes dialog whichDialog returns nothing",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            if let Some(d) = dialog(&mut state, handle(&args[0])).cloned() {
                for button in d.buttons {
                    state.objects.remove(&button);
                }
                state.objects.remove(&handle(&args[0]).expect("found"));
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native DialogClear takes dialog whichDialog returns nothing",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let buttons = dialog(&mut state, handle(&args[0]))
                .map(|d| std::mem::take(&mut d.buttons))
                .unwrap_or_default();
            for button in buttons {
                state.objects.remove(&button);
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native DialogSetMessage takes dialog whichDialog, string messageText returns nothing",
        |vm, state, args| {
            if let Some(d) = dialog(&mut state.borrow_mut(), handle(&args[0])) {
                d.message = text(&args[1]);
            }
            Ok(None)
        }
    );
    for (declaration, quit) in [
        (
            "native DialogAddButton takes dialog whichDialog, string buttonText, integer hotkey returns button",
            false,
        ),
        (
            "native DialogAddQuitButton takes dialog whichDialog, boolean doScoreScreen, string buttonText, integer hotkey returns button",
            true,
        ),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let mut state = state.borrow_mut();
            let Some(d) = handle(&args[0]).filter(|&h| dialog(&mut state, Some(h)).is_some())
            else {
                return Ok(Some(Value::Handle(None)));
            };
            let (text_arg, hotkey_arg) = if quit { (2, 3) } else { (1, 2) };
            let button = state.create(Object::Button(Button {
                dialog: d,
                text: text(&args[text_arg]),
                hotkey: integer(&args[hotkey_arg]),
                quit,
            }));
            if let Some(d) = dialog(&mut state, Some(d)) {
                d.buttons.push(button);
            }
            Ok(Some(Value::Handle(Some(button))))
        });
    }
    native!(
        add,
        state,
        "native TriggerRegisterDialogEvent takes trigger whichTrigger, dialog whichDialog returns event",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            if let (Some(trigger), Some(dialog)) = (handle(&args[0]), handle(&args[1])) {
                state
                    .dialog_events
                    .push(DialogEvent::Dialog { trigger, dialog });
            }
            Ok(Some(Value::Handle(Some(state.create(Object::Event)))))
        }
    );
    native!(
        add,
        state,
        "native TriggerRegisterDialogButtonEvent takes trigger whichTrigger, button whichButton returns event",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            if let (Some(trigger), Some(button)) = (handle(&args[0]), handle(&args[1])) {
                state
                    .dialog_events
                    .push(DialogEvent::Button { trigger, button });
            }
            Ok(Some(Value::Handle(Some(state.create(Object::Event)))))
        }
    );
    native!(
        add,
        state,
        "constant native GetClickedButton takes nothing returns button",
        |vm, state, args| { Ok(Some(Value::Handle(state.borrow().context.clicked_button))) }
    );
    native!(
        add,
        state,
        "constant native GetClickedDialog takes nothing returns dialog",
        |vm, state, args| { Ok(Some(Value::Handle(state.borrow().context.clicked_dialog))) }
    );

    // Localized strings: the argument itself, after a trigger string
    // reference is resolved (SPEC-0003).
    native!(
        add,
        state,
        "native GetLocalizedString takes string source returns string",
        |vm, state, args| { Ok(Some(args[0].clone())) }
    );
}

impl super::Game<'_> {
    /// Clicks a dialog button for player `player`: the triggers registered
    /// for the button and for its dialog run, with `GetClickedButton`,
    /// `GetClickedDialog` and `GetTriggerPlayer`.
    pub fn click_button(&mut self, button: u32, player: i32) {
        click(&mut self.vm, &self.state, button, player);
        self.park();
    }

    /// Buttons of the dialogs in the order they were added, for tests and
    /// for a user interface.
    pub fn dialog_buttons(&self) -> Vec<(u32, Button)> {
        let state = self.state.borrow();
        let mut buttons: Vec<(u32, Button)> = state
            .objects
            .iter()
            .filter_map(|(&h, o)| match o {
                Object::Button(b) => Some((h, b.clone())),
                _ => None,
            })
            .collect();
        buttons.sort_by_key(|&(h, _)| h);
        buttons
    }
}
