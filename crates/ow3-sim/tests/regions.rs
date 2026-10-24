//! Regions and their enter and leave events (SPEC-0049) on small scripts
//! that declare what they use, so these tests need no game data.

use ow3_sim::game::Game;

const DECLARATIONS: &str = r#"
type agent extends handle
type event extends agent
type player extends agent
type widget extends agent
type unit extends widget
type trigger extends agent
type triggeraction extends handle
type boolexpr extends agent
type filterfunc extends boolexpr
type eventid extends handle
type region extends agent
type rect extends agent
type timer extends agent
native Player takes integer number returns player
native DisplayTextToPlayer takes player toPlayer, real x, real y, string message returns nothing
native I2S takes integer i returns string
native R2S takes real r returns string
native GetHandleId takes handle h returns integer
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native GetUnitTypeId takes unit whichUnit returns integer
native GetUnitX takes unit whichUnit returns real
native SetUnitX takes unit whichUnit, real newX returns nothing
native IssuePointOrder takes unit whichUnit, string order, real x, real y returns boolean
native CreateTrigger takes nothing returns trigger
native TriggerAddAction takes trigger whichTrigger, code actionFunc returns triggeraction
native Filter takes code func returns filterfunc
native GetFilterUnit takes nothing returns unit
native GetTriggerUnit takes nothing returns unit
native GetTriggerEventId takes nothing returns eventid
native Rect takes real minx, real miny, real maxx, real maxy returns rect
native CreateRegion takes nothing returns region
native RegionAddRect takes region whichRegion, rect r returns nothing
native IsPointInRegion takes region whichRegion, real x, real y returns boolean
native IsUnitInRegion takes region whichRegion, unit whichUnit returns boolean
native TriggerRegisterEnterRegion takes trigger whichTrigger, region whichRegion, boolexpr filter returns event
native TriggerRegisterLeaveRegion takes trigger whichTrigger, region whichRegion, boolexpr filter returns event
native GetTriggeringRegion takes nothing returns region
native GetEnteringUnit takes nothing returns unit
native GetLeavingUnit takes nothing returns unit
native CreateTimer takes nothing returns timer
native GetExpiredTimer takes nothing returns timer
native TimerStart takes timer whichTimer, real timeout, boolean periodic, code handlerFunc returns nothing
globals
    region A = null
    region B = null
    unit U = null
    timer Clock = null
    integer N = 0
endglobals
function Print takes string s returns nothing
    call DisplayTextToPlayer(Player(0), 0, 0, s)
endfunction
function B2S takes boolean b returns string
    if b then
        return "true"
    endif
    return "false"
endfunction
function Square takes nothing returns region
    local region r = CreateRegion()
    call RegionAddRect(r, Rect(0, 0, 256, 256))
    return r
endfunction
"#;

fn game(script: &str) -> Game<'static> {
    let declarations = Box::leak(Box::new(ow3_jass::parse(DECLARATIONS).unwrap()));
    let script = Box::leak(Box::new(ow3_jass::parse(script).unwrap()));
    let refs: &'static [&'static _] = Box::leak(Box::new([&*declarations, &*script]));
    let program = Box::leak(Box::new(ow3_jass::check::check(refs).unwrap()));
    let game = Game::load(program, refs).unwrap();
    game.state.borrow_mut().world_bounds =
        [-12288.0f32, -12288.0, 12288.0, 12288.0].map(f32::to_bits);
    game
}

fn assert_no_errors(game: &Game) {
    assert!(
        game.state.borrow().errors.is_empty(),
        "{:?}",
        game.state.borrow().errors
    );
}

/// Verification 1 and 2: the cells of a rect, and `CreateUnit` inside a
/// region runs the filter inside the call and the action later.
#[test]
fn create_unit_runs_the_filter_at_once_and_the_action_later() {
    let mut game = game(
        r#"
function Accept takes nothing returns boolean
    call Print("filter " + I2S(GetUnitTypeId(GetFilterUnit())) + " " + B2S(GetTriggerUnit() == null))
    return true
endfunction
function Entered takes nothing returns nothing
    call Print("enter " + I2S(GetHandleId(GetTriggerEventId())) + " " + I2S(GetUnitTypeId(GetTriggerUnit())) + " " + B2S(GetTriggeringRegion() == A) + " " + B2S(GetEnteringUnit() == U) + " " + B2S(GetLeavingUnit() == null))
endfunction
function main takes nothing returns nothing
    local trigger t = CreateTrigger()
    set A = Square()
    call Print(B2S(IsPointInRegion(A, 287.9, 100)) + " " + B2S(IsPointInRegion(A, -0.1, 100)) + " " + B2S(IsPointInRegion(A, 288, 100)))
    call TriggerRegisterEnterRegion(t, A, Filter(function Accept))
    call TriggerAddAction(t, function Entered)
    set U = CreateUnit(Player(0), 1, 100, 100, 0)
    call Print("created " + B2S(IsUnitInRegion(A, U)))
endfunction
"#,
    );
    game.run("main");
    assert_no_errors(&game);
    assert_eq!(
        game.output(),
        ["true false false", "filter 1 true", "created true"]
    );
    game.step();
    assert_eq!(game.output()[3..], ["enter 5 1 true true true"]);
}

/// Verification 3: in, out, in, out by `SetUnitX` in one call. The
/// filters run at the crossings; the enter actions, then the leave
/// actions run later and read the final position.
#[test]
fn each_crossing_is_an_event() {
    let mut game = game(
        r#"
function EnterFilter takes nothing returns boolean
    call Print("enter filter " + R2S(GetUnitX(GetFilterUnit())))
    return true
endfunction
function LeaveFilter takes nothing returns boolean
    call Print("leave filter " + R2S(GetUnitX(GetFilterUnit())))
    return true
endfunction
function Entered takes nothing returns nothing
    call Print("enter " + R2S(GetUnitX(GetTriggerUnit())) + " " + B2S(IsUnitInRegion(A, GetTriggerUnit())))
endfunction
function Left takes nothing returns nothing
    call Print("leave " + I2S(GetHandleId(GetTriggerEventId())) + " " + R2S(GetUnitX(GetLeavingUnit())))
endfunction
function main takes nothing returns nothing
    local trigger t = CreateTrigger()
    set A = Square()
    call TriggerRegisterEnterRegion(t, A, Filter(function EnterFilter))
    call TriggerAddAction(t, function Entered)
    set t = CreateTrigger()
    call TriggerRegisterLeaveRegion(t, A, Filter(function LeaveFilter))
    call TriggerAddAction(t, function Left)
    set U = CreateUnit(Player(0), 1, -100, 100, 0)
    call SetUnitX(U, 100)
    call SetUnitX(U, -100)
    call SetUnitX(U, 100)
    call SetUnitX(U, -100)
    call Print("moved")
endfunction
"#,
    );
    game.run("main");
    assert_no_errors(&game);
    assert_eq!(
        game.output(),
        [
            "enter filter 100.000",
            "leave filter -100.000",
            "enter filter 100.000",
            "leave filter -100.000",
            "moved",
        ]
    );
    game.step();
    assert_eq!(
        game.output()[5..],
        [
            "enter -100.000 false",
            "enter -100.000 false",
            "leave 6 -100.000",
            "leave 6 -100.000",
        ]
    );
}

/// The order of c-SAR: regions A and B, B made first; triggers 1 on A, 2
/// on B, 3 on A with a filter for footmen (type 1), 4 on A and B; one
/// call creates a footman and a rifleman (type 2) inside both and a
/// footman inside B only.
#[test]
fn queued_events_go_by_region_trigger_and_entry() {
    let mut game = game(
        r#"
function Footman takes nothing returns boolean
    return GetUnitTypeId(GetFilterUnit()) == 1
endfunction
function Name takes nothing returns string
    if GetTriggeringRegion() == A then
        return " A"
    endif
    return " B"
endfunction
function T1 takes nothing returns nothing
    call Print("1 " + I2S(GetUnitTypeId(GetTriggerUnit())) + Name())
endfunction
function T2 takes nothing returns nothing
    call Print("2 " + I2S(GetUnitTypeId(GetTriggerUnit())) + Name())
endfunction
function T3 takes nothing returns nothing
    call Print("3 " + I2S(GetUnitTypeId(GetTriggerUnit())) + Name())
endfunction
function T4 takes nothing returns nothing
    call Print("4 " + I2S(GetUnitTypeId(GetTriggerUnit())) + Name())
endfunction
function main takes nothing returns nothing
    local trigger t
    set B = CreateRegion()
    call RegionAddRect(B, Rect(0, 0, 512, 512))
    set A = Square()
    set t = CreateTrigger()
    call TriggerRegisterEnterRegion(t, A, null)
    call TriggerAddAction(t, function T1)
    set t = CreateTrigger()
    call TriggerRegisterEnterRegion(t, B, null)
    call TriggerAddAction(t, function T2)
    set t = CreateTrigger()
    call TriggerRegisterEnterRegion(t, A, Filter(function Footman))
    call TriggerAddAction(t, function T3)
    set t = CreateTrigger()
    call TriggerRegisterEnterRegion(t, A, null)
    call TriggerRegisterEnterRegion(t, B, null)
    call TriggerAddAction(t, function T4)
    call CreateUnit(Player(0), 1, 100, 100, 0)
    call CreateUnit(Player(0), 2, 150, 150, 0)
    call CreateUnit(Player(0), 1, 400, 400, 0)
endfunction
"#,
    );
    game.run("main");
    game.step();
    assert_no_errors(&game);
    assert_eq!(
        game.output(),
        [
            "1 1 A", "1 2 A", "3 1 A", "4 1 A", "4 2 A", "2 1 B", "2 2 B", "2 1 B", "4 1 B",
            "4 2 B", "4 1 B",
        ]
    );
}

/// The slot of an event on the timer queue (c-SAS): a 0 s timer that the
/// filter starts comes before the event, the event before a 0 s timer
/// started after `CreateUnit` returned, and all after the callback that
/// made the unit.
#[test]
fn an_event_is_queued_as_a_zero_timer() {
    let mut game = game(
        r#"
function Label takes nothing returns nothing
    call Print("timer " + I2S(GetHandleId(GetExpiredTimer()) - GetHandleId(Clock)))
endfunction
function Accept takes nothing returns boolean
    call TimerStart(CreateTimer(), 0, false, function Label)
    return true
endfunction
function Entered takes nothing returns nothing
    call Print("enter")
endfunction
function Make takes nothing returns nothing
    call CreateUnit(Player(0), 1, 100, 100, 0)
    call TimerStart(CreateTimer(), 0, false, function Label)
    call TimerStart(CreateTimer(), 0.002, false, function Label)
    call Print("made")
endfunction
function main takes nothing returns nothing
    local trigger t = CreateTrigger()
    set A = Square()
    call TriggerRegisterEnterRegion(t, A, Filter(function Accept))
    call TriggerAddAction(t, function Entered)
    set Clock = CreateTimer()
    call TimerStart(Clock, 0.5, false, function Make)
endfunction
"#,
    );
    game.run("main");
    while game.ticks() < 120 {
        game.step();
    }
    assert_no_errors(&game);
    // The handles after `Clock`: 1 the unit, 2 the timer the filter
    // started, 3 and 4 those started after `CreateUnit`.
    assert_eq!(
        game.output(),
        ["made", "timer 2", "enter", "timer 3", "timer 4"]
    );
}

/// Verification 4: a walker enters at a commit of its path agent, after
/// the first tick at which its position is inside, and leaves the same
/// way.
#[test]
fn a_walker_enters_at_a_commit() {
    let mut game = game(
        r#"
function Entered takes nothing returns nothing
    call Print("enter " + R2S(GetUnitX(GetTriggerUnit())))
endfunction
function Left takes nothing returns nothing
    call Print("leave " + R2S(GetUnitX(GetTriggerUnit())))
endfunction
function Accept takes nothing returns boolean
    call Print("filter " + R2S(GetUnitX(GetFilterUnit())))
    return true
endfunction
function main takes nothing returns nothing
    local trigger t = CreateTrigger()
    set A = Square()
    call TriggerRegisterEnterRegion(t, A, Filter(function Accept))
    call TriggerAddAction(t, function Entered)
    set t = CreateTrigger()
    call TriggerRegisterLeaveRegion(t, A, Filter(function Accept))
    call TriggerAddAction(t, function Left)
    set U = CreateUnit(Player(0), 'hfoo', -110, 100, 0)
    call IssuePointOrder(U, "move", 600, 100)
endfunction
"#,
    );
    {
        let mut state = game.state.borrow_mut();
        let bits = f32::to_bits;
        state.data.units.insert(
            i32::from_be_bytes(*b"hfoo"),
            ow3_sim::data::UnitType {
                hp: bits(420.0),
                speed: bits(270.0),
                turn_rate: bits(0.6),
                prop_window: bits(60.0),
                collision: bits(16.0),
                ..Default::default()
            },
        );
    }
    game.run("main");
    assert_no_errors(&game);
    // (tick, x at the end of the tick, filter lines printed in the tick)
    let mut ticks = Vec::new();
    while game.ticks() < 1000 {
        let before = game.output().len();
        game.step();
        let x = game.units()[0].x;
        let printed: Vec<String> = game.output()[before..]
            .iter()
            .filter(|l| l.starts_with("filter"))
            .cloned()
            .collect();
        ticks.push((game.ticks(), x, printed));
    }
    let first = |inside: &dyn Fn(f32) -> bool| ticks.iter().find(|t| inside(t.1)).unwrap().0;
    let tick_of = |n: usize| {
        ticks
            .iter()
            .filter(|t| !t.2.is_empty())
            .map(|t| t.0)
            .nth(n)
            .expect("a filter call")
    };
    let (inside, outside) = (first(&|x| x >= 0.0), first(&|x| x >= 288.0));
    let (enter, leave) = (tick_of(0), tick_of(1));
    assert!(
        enter > inside && enter < inside + 6,
        "inside at {inside}, entered at {enter}"
    );
    assert!(
        leave > outside && leave < outside + 6,
        "outside at {outside}, left at {leave}"
    );
    // Both at commits, a whole number of update cycles apart.
    assert_eq!((leave - enter) % 6, 0);
    let lines = game.output();
    let actions: Vec<&str> = lines
        .iter()
        .filter(|l| l.starts_with("enter") || l.starts_with("leave"))
        .map(|l| l.split(' ').next().unwrap())
        .collect();
    assert_eq!(actions, ["enter", "leave"]);
}
