//! Timer natives on a small script that declares what it uses (SPEC-0002):
//! periodic restarts, `TimerStart` and `DestroyTimer` inside the callback,
//! pause and resume.

use ow3_sim::game::Game;

const DECLARATIONS: &str = r#"
type agent extends handle
type player extends agent
type timer extends agent
native Player takes integer number returns player
native DisplayTextToPlayer takes player toPlayer, real x, real y, string message returns nothing
native R2SW takes real r, integer width, integer precision returns string
native I2S takes integer i returns string
native CreateTimer takes nothing returns timer
native DestroyTimer takes timer whichTimer returns nothing
native TimerStart takes timer whichTimer, real timeout, boolean periodic, code handlerFunc returns nothing
native TimerGetElapsed takes timer whichTimer returns real
native TimerGetRemaining takes timer whichTimer returns real
native TimerGetTimeout takes timer whichTimer returns real
native PauseTimer takes timer whichTimer returns nothing
native ResumeTimer takes timer whichTimer returns nothing
native GetExpiredTimer takes nothing returns timer
"#;

const SCRIPT: &str = r#"
globals
    timer clock = null
    timer long = null
    integer fires = 0
endglobals
function Print takes string s returns nothing
    call DisplayTextToPlayer(Player(0), 0, 0, s)
endfunction
function Now takes nothing returns string
    return R2SW(TimerGetElapsed(clock), 1, 2)
endfunction
function Tick takes nothing returns nothing
    local timer t = GetExpiredTimer()
    set fires = fires + 1
    call Print("tick " + I2S(fires) + " at " + Now() + " remaining " + R2SW(TimerGetRemaining(t), 1, 2) + " elapsed " + R2SW(TimerGetElapsed(t), 1, 2))
    if fires == 2 then
        call TimerStart(t, 0.3, true, function Tick)
    endif
    if fires == 4 then
        call DestroyTimer(t)
    endif
endfunction
function LongFired takes nothing returns nothing
    call Print("long fired at " + Now())
endfunction
function Pause takes nothing returns nothing
    call PauseTimer(long)
    call Print("paused, remaining " + R2SW(TimerGetRemaining(long), 1, 2))
endfunction
function Resume takes nothing returns nothing
    call ResumeTimer(long)
endfunction
function Look takes nothing returns nothing
    call Print("2 s after resume: elapsed " + R2SW(TimerGetElapsed(long), 1, 2) + " remaining " + R2SW(TimerGetRemaining(long), 1, 2))
endfunction
function main takes nothing returns nothing
    set clock = CreateTimer()
    call TimerStart(clock, 1000.0, false, null)
    call TimerStart(CreateTimer(), 0.5, true, function Tick)
    set long = CreateTimer()
    call TimerStart(long, 130.0, false, function LongFired)
    call TimerStart(CreateTimer(), 5.0, false, function Pause)
    call TimerStart(CreateTimer(), 15.0, false, function Resume)
    call TimerStart(CreateTimer(), 17.0, false, function Look)
endfunction
"#;

#[test]
fn periodic_restart_pause_and_resume() {
    let declarations = ow3_jass::parse(DECLARATIONS).unwrap();
    let script = ow3_jass::parse(SCRIPT).unwrap();
    let refs = [&declarations, &script];
    let program = ow3_jass::check::check(&refs).unwrap();
    let mut game = Game::load(&program, &refs).unwrap();
    game.run("main");
    game.advance_seconds(150.0);
    assert!(game.state.borrow().errors.is_empty());
    assert_eq!(
        game.output(),
        [
            // Inside the callback the timer reads 0 remaining and its
            // period elapsed; it starts again after the callback.
            "tick 1 at 0.50 remaining 0.00 elapsed 0.50",
            "tick 2 at 1.00 remaining 0.00 elapsed 0.50",
            // TimerStart in the callback replaced the period.
            "tick 3 at 1.30 remaining 0.00 elapsed 0.30",
            // DestroyTimer in the callback stopped it.
            "tick 4 at 1.60 remaining 0.00 elapsed 0.30",
            "paused, remaining 125.00",
            // After ResumeTimer, elapsed counts from the resume.
            "2 s after resume: elapsed 2.00 remaining 123.00",
            "long fired at 140.00",
        ]
    );
}
