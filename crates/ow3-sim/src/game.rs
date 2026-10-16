//! A running game: scripts on the interpreter, driven by game time.
//!
//! [`Game`] owns the interpreter and the sleeping threads; [`State`] holds
//! everything the natives work on, shared with them. Natives that are not
//! implemented here are stubs returning default values.
//!
//! Behaviour follows SPEC-0002 (time, timers, sleeps) and SPEC-0005
//! (threads and sleeping in callbacks). Points no spec covers yet are
//! marked PROVISIONAL and listed in specs/QUESTIONS.md.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use ow3_jass::ast::Script;
use ow3_jass::check::Program;
use ow3_jass::semantics::Value;
use ow3_jass::vm::{LoadError, NativeFn, Natives, Outcome, RuntimeError, Thread, Vm};
use ow3_math::{add, i2r, mul, sub};

use crate::clock::{CHUNK_MICROS, FrameClock, Speed, Step};
use crate::sleep::SleepQueue;
use crate::timers::{Fired, LONG_PART, TimerId, TimerQueue, raise_timeout, split_timeout};

/// PROVISIONAL: the first handle number. Scripts usually see differences
/// between handles, which do not depend on it.
const FIRST_HANDLE: u32 = 0x0010_0000;

/// What a handle refers to.
#[derive(Clone, Debug)]
pub enum Object {
    Timer(Timer),
    Trigger(Trigger),
    TriggerAction,
    TriggerCondition,
    /// A `boolexpr` made by `Condition` or `Filter`.
    BoolExpr {
        function: usize,
    },
    Event,
    /// A value of a handle-typed enumeration such as `gamespeed` or
    /// `player`, made by `ConvertGameSpeed`, `Player` and the like.
    Enum {
        kind: &'static str,
        value: i32,
    },
}

#[derive(Clone, Debug, Default)]
pub struct Timer {
    queued: Option<TimerId>,
    /// The timeout given to `TimerStart`, raised to at least 0.0001.
    timeout: u32,
    /// Parts of a long timer still to queue after the current one: whole
    /// 120 s parts, then the last part (SPEC-0002).
    parts_left: u32,
    last_part: Option<u32>,
    /// Set by the first start; a timer never started reads 0 throughout.
    started: bool,
    periodic: bool,
    callback: Option<usize>,
    /// Remaining time of a paused timer.
    paused: Option<u32>,
    /// The trigger this timer fires, for `TriggerRegisterTimerEvent`.
    fires_trigger: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct Trigger {
    enabled: bool,
    actions: Vec<usize>,
    conditions: Vec<usize>,
}

/// Event responses of the running thread, kept across its sleeps.
#[derive(Clone, Copy, Debug, Default)]
struct Context {
    expired_timer: Option<u32>,
    triggering_trigger: Option<u32>,
}

/// Work waiting for a sleep check.
#[derive(Debug)]
enum Task {
    Thread {
        thread: Thread,
        context: Context,
    },
    /// A trigger's actions run one after another; `thread` runs action
    /// `next - 1` and the remaining ones follow it.
    Trigger {
        actions: Vec<usize>,
        next: usize,
        thread: Thread,
        context: Context,
    },
}

/// Game state shared with the natives.
pub struct State {
    pub clock: FrameClock,
    pub timers: TimerQueue<u32>,
    objects: HashMap<u32, Object>,
    enums: HashMap<(&'static str, i32), u32>,
    next_handle: u32,
    /// Texts shown to the local player, in order.
    pub output: Vec<String>,
    /// Lines given to `Preload`, in order.
    pub preload: Vec<String>,
    /// Errors that ended threads. The game ends such threads silently.
    pub errors: Vec<RuntimeError>,
    context: Context,
    sleep_time: u64,
    /// Tasks that went to sleep inside natives, with their durations.
    parked: Vec<(Task, u32)>,
}

impl State {
    fn new() -> Self {
        State {
            clock: FrameClock::new(),
            timers: TimerQueue::new(),
            objects: HashMap::new(),
            enums: HashMap::new(),
            next_handle: FIRST_HANDLE,
            output: Vec::new(),
            preload: Vec::new(),
            errors: Vec::new(),
            context: Context::default(),
            sleep_time: 0,
            parked: Vec::new(),
        }
    }

    fn create(&mut self, object: Object) -> u32 {
        let handle = self.next_handle;
        self.next_handle += 1;
        self.objects.insert(handle, object);
        handle
    }

    fn enum_handle(&mut self, kind: &'static str, value: i32) -> u32 {
        if let Some(&handle) = self.enums.get(&(kind, value)) {
            return handle;
        }
        let handle = self.create(Object::Enum { kind, value });
        self.enums.insert((kind, value), handle);
        handle
    }

    fn enum_value(&self, handle: Option<u32>) -> Option<i32> {
        match self.objects.get(&handle?)? {
            Object::Enum { value, .. } => Some(*value),
            _ => None,
        }
    }

    fn timer(&mut self, handle: Option<u32>) -> Option<&mut Timer> {
        match self.objects.get_mut(&handle?)? {
            Object::Timer(timer) => Some(timer),
            _ => None,
        }
    }

    fn trigger(&mut self, handle: Option<u32>) -> Option<&mut Trigger> {
        match self.objects.get_mut(&handle?)? {
            Object::Trigger(trigger) => Some(trigger),
            _ => None,
        }
    }

    /// Starts or restarts a timer in the queue. A timeout of 120 s or more
    /// is run as parts (SPEC-0002).
    fn start_timer(&mut self, handle: u32, timeout: u32) {
        let (n, rest) = split_timeout(raise_timeout(timeout));
        let (first, parts_left, last_part) = if n == 0 {
            (rest, 0, None)
        } else {
            (LONG_PART, n - 1, Some(rest))
        };
        let id = self.timers.start(first, handle);
        if let Some(timer) = self.timer(Some(handle)) {
            timer.queued = Some(id);
            timer.paused = None;
            timer.started = true;
            timer.parts_left = parts_left;
            timer.last_part = last_part;
        }
    }

    /// Queues the next part of a long timer whose current part expired.
    /// Returns false when no part is left and the timer fires.
    fn next_part(&mut self, handle: u32) -> bool {
        let Some(timer) = self.timer(Some(handle)) else {
            return false;
        };
        let part = if timer.parts_left > 0 {
            timer.parts_left -= 1;
            LONG_PART
        } else if let Some(rest) = timer.last_part.take() {
            rest
        } else {
            return false;
        };
        let id = self.timers.start(part, handle);
        if let Some(timer) = self.timer(Some(handle)) {
            timer.queued = Some(id);
        }
        true
    }

    fn stop_timer(&mut self, handle: u32) {
        let queued = self.timer(Some(handle)).and_then(|t| t.queued.take());
        if let Some(id) = queued {
            self.timers.cancel(id);
        }
    }

    /// `TimerGetRemaining`: `sub(e, q)` of a running timer, the saved value
    /// of a paused one, 0 otherwise (SPEC-0002).
    ///
    /// PROVISIONAL: a long timer adds the parts still to run, so that the
    /// whole timeout is reported; the exact operations are not specified.
    fn remaining(&mut self, handle: Option<u32>) -> u32 {
        let now = self.timers.time();
        let Some(timer) = self.timer(handle) else {
            return 0;
        };
        if let Some(paused) = timer.paused {
            return paused;
        }
        let (queued, parts_left, last_part) = (timer.queued, timer.parts_left, timer.last_part);
        let Some(expiry) = queued.and_then(|id| self.timers.expiry(id)) else {
            return 0;
        };
        let mut remaining = sub(expiry, now);
        if parts_left > 0 {
            remaining = add(remaining, mul(i2r(parts_left as i32), LONG_PART));
        }
        if let Some(rest) = last_part {
            remaining = add(remaining, rest);
        }
        remaining
    }
}

pub struct Game<'a> {
    pub vm: Vm<'a>,
    pub state: Rc<RefCell<State>>,
    sleeps: SleepQueue<Task>,
}

impl<'a> Game<'a> {
    /// Loads checked scripts. Natives not implemented here are stubs.
    pub fn load(program: &'a Program, scripts: &[&'a Script]) -> Result<Game<'a>, LoadError> {
        let state = Rc::new(RefCell::new(State::new()));
        let mut natives = Natives::default().stub_missing();
        register_natives(&mut natives, &state, program);
        let vm = Vm::load(program, scripts, &natives)?;
        Ok(Game {
            vm,
            state,
            sleeps: SleepQueue::new(),
        })
    }

    /// Runs a function by name as a new thread, as the game runs `config`
    /// and `main`. Does nothing if the function does not exist.
    pub fn run(&mut self, name: &str) {
        if let Some(index) = self.vm.program().function_index(name) {
            run_new_thread(&mut self.vm, &self.state, index, Context::default(), true);
            self.park();
        }
    }

    /// Advances the game by `micros` of real time.
    /// Lets `micros` of real time pass, in steps of one chunk, so the game
    /// keeps up with it. To simulate a stall, pass the stalled time to
    /// [`Game::add_real_time`] instead; at most 4 s of it are made up.
    pub fn advance(&mut self, micros: u64) {
        let mut left = micros;
        while left > 0 {
            let step = left.min(CHUNK_MICROS);
            left -= step;
            self.add_real_time(step);
        }
    }

    /// Adds real time at once and runs the chunks it makes due.
    pub fn add_real_time(&mut self, micros: u64) {
        self.state.borrow_mut().clock.add_real_time(micros);
        loop {
            let Some(chunk) = self.state.borrow_mut().clock.next_chunk() else {
                break;
            };
            for step in chunk {
                match step {
                    Step::Tick { sleep_time } => {
                        self.state.borrow_mut().sleep_time = sleep_time;
                        self.tick();
                    }
                    Step::SleepCheck { time } => {
                        self.state.borrow_mut().sleep_time = time;
                        for task in self.sleeps.due(time) {
                            continue_task(&mut self.vm, &self.state, task);
                            self.park();
                        }
                    }
                }
            }
        }
    }

    /// Advances the game by whole seconds of real time.
    pub fn advance_seconds(&mut self, seconds: f64) {
        self.advance((seconds * 1_000_000.0).round() as u64);
    }

    /// Moves tasks that went to sleep into the sleep queue.
    fn park(&mut self) {
        let (parked, now) = {
            let mut state = self.state.borrow_mut();
            (std::mem::take(&mut state.parked), state.sleep_time)
        };
        for (task, duration) in parked {
            self.sleeps.sleep(now, duration, task);
        }
    }

    fn tick(&mut self) {
        let mut tick = self.state.borrow_mut().timers.begin_tick();
        loop {
            let Some(fired) = self.state.borrow_mut().timers.next_due(&mut tick) else {
                break;
            };
            self.fire(fired);
            self.park();
        }
        self.state.borrow_mut().timers.end_tick(tick);
    }

    fn fire(&mut self, fired: Fired<u32>) {
        let handle = fired.payload;
        let (callback, trigger) = {
            let mut state = self.state.borrow_mut();
            let Some(timer) = state.timer(Some(handle)) else {
                return;
            };
            timer.queued = None;
            if state.next_part(handle) {
                return;
            }
            let Some(timer) = state.timer(Some(handle)) else {
                return;
            };
            let (periodic, timeout) = (timer.periodic, timer.timeout);
            let (callback, trigger) = (timer.callback, timer.fires_trigger);
            // PROVISIONAL: a periodic timer is restarted before its callback
            // runs, with the queue time at its expiry (SPEC-0002).
            if periodic {
                state.start_timer(handle, timeout);
            }
            (callback, trigger)
        };
        if let Some(trigger) = trigger {
            fire_trigger(&mut self.vm, &self.state, trigger);
        } else if let Some(callback) = callback {
            let context = Context {
                expired_timer: Some(handle),
                ..Context::default()
            };
            // A timer callback that sleeps ends at the sleep (SPEC-0005).
            run_new_thread(&mut self.vm, &self.state, callback, context, false);
        }
    }

    /// Texts shown to the local player so far.
    pub fn output(&self) -> Vec<String> {
        self.state.borrow().output.clone()
    }
}

/// Runs `f` with `context` as the event responses, restoring the previous
/// ones afterwards.
fn with_context<T>(state: &Rc<RefCell<State>>, context: Context, f: impl FnOnce() -> T) -> T {
    let previous = std::mem::replace(&mut state.borrow_mut().context, context);
    let result = f();
    state.borrow_mut().context = previous;
    result
}

/// Starts `function` in a new thread. When it sleeps it is parked if
/// `may_sleep`, otherwise it ends there. Errors end it silently.
fn run_new_thread(
    vm: &mut Vm,
    state: &Rc<RefCell<State>>,
    function: usize,
    context: Context,
    may_sleep: bool,
) {
    match with_context(state, context, || vm.start_thread(function, Vec::new())) {
        Ok((Outcome::Sleep(duration), thread)) if may_sleep => {
            state
                .borrow_mut()
                .parked
                .push((Task::Thread { thread, context }, duration));
        }
        Ok(_) => {}
        Err(error) => state.borrow_mut().errors.push(error),
    }
}

fn continue_task(vm: &mut Vm, state: &Rc<RefCell<State>>, task: Task) {
    match task {
        Task::Thread {
            mut thread,
            context,
        } => match with_context(state, context, || vm.resume(&mut thread)) {
            Ok(Outcome::Sleep(duration)) => state
                .borrow_mut()
                .parked
                .push((Task::Thread { thread, context }, duration)),
            Ok(Outcome::Finished(_)) => {}
            Err(error) => state.borrow_mut().errors.push(error),
        },
        Task::Trigger {
            actions,
            next,
            mut thread,
            context,
        } => {
            let outcome = with_context(state, context, || vm.resume(&mut thread));
            match outcome {
                Ok(Outcome::Sleep(duration)) => {
                    let task = Task::Trigger {
                        actions,
                        next,
                        thread,
                        context,
                    };
                    state.borrow_mut().parked.push((task, duration));
                }
                Ok(Outcome::Finished(_)) => run_actions(vm, state, actions, next, context),
                Err(error) => {
                    state.borrow_mut().errors.push(error);
                    run_actions(vm, state, actions, next, context);
                }
            }
        }
    }
}

/// Runs a trigger's actions from `next`, each in its own thread. An action
/// that sleeps parks the rest; an action that fails is followed by the next
/// one (SPEC-0005).
fn run_actions(
    vm: &mut Vm,
    state: &Rc<RefCell<State>>,
    actions: Vec<usize>,
    mut next: usize,
    context: Context,
) {
    while let Some(&action) = actions.get(next) {
        next += 1;
        match with_context(state, context, || vm.start_thread(action, Vec::new())) {
            Ok((Outcome::Sleep(duration), thread)) => {
                let task = Task::Trigger {
                    actions,
                    next,
                    thread,
                    context,
                };
                state.borrow_mut().parked.push((task, duration));
                return;
            }
            Ok(_) => {}
            Err(error) => state.borrow_mut().errors.push(error),
        }
    }
}

/// `TriggerEvaluate`: every condition must hold. A condition that sleeps
/// counts as false (SPEC-0005). PROVISIONAL: evaluation stops at the first
/// false condition, and each condition runs in its own thread.
fn evaluate_trigger(vm: &mut Vm, state: &Rc<RefCell<State>>, trigger: u32) -> bool {
    let Some(conditions) = state
        .borrow_mut()
        .trigger(Some(trigger))
        .map(|t| t.conditions.clone())
    else {
        return false;
    };
    let context = Context {
        triggering_trigger: Some(trigger),
        ..state.borrow().context
    };
    conditions.into_iter().all(|condition| {
        match with_context(state, context, || vm.start_thread(condition, Vec::new())) {
            Ok((Outcome::Finished(Some(Value::Boolean(true))), _)) => true,
            Ok(_) => false,
            Err(error) => {
                state.borrow_mut().errors.push(error);
                false
            }
        }
    })
}

/// `TriggerExecute`: runs the actions; the caller continues once they end
/// or one sleeps (SPEC-0005).
fn execute_trigger(vm: &mut Vm, state: &Rc<RefCell<State>>, trigger: u32) {
    let Some(actions) = state
        .borrow_mut()
        .trigger(Some(trigger))
        .map(|t| t.actions.clone())
    else {
        return;
    };
    let context = Context {
        triggering_trigger: Some(trigger),
        ..state.borrow().context
    };
    run_actions(vm, state, actions, 0, context);
}

/// A trigger's event happened: if it is enabled and its conditions hold,
/// its actions run.
fn fire_trigger(vm: &mut Vm, state: &Rc<RefCell<State>>, trigger: u32) {
    let enabled = state
        .borrow_mut()
        .trigger(Some(trigger))
        .is_some_and(|t| t.enabled);
    if enabled && evaluate_trigger(vm, state, trigger) {
        execute_trigger(vm, state, trigger);
    }
}

fn handle(value: &Value) -> Option<u32> {
    match value {
        Value::Handle(h) => *h,
        _ => None,
    }
}

fn integer(value: &Value) -> i32 {
    match value {
        Value::Integer(i) => *i,
        _ => 0,
    }
}

fn real(value: &Value) -> u32 {
    match value {
        Value::Real(r) => *r,
        _ => 0,
    }
}

fn code(value: &Value) -> Option<usize> {
    match value {
        Value::Code(c) => *c,
        _ => None,
    }
}

fn string(value: &Value) -> Option<&str> {
    match value {
        Value::String(s) => s.as_deref(),
        _ => None,
    }
}

// `rustfmt` wraps short native bodies in braces inside the `native!` calls.
#[allow(unused_braces)]
fn register_natives(natives: &mut Natives, state: &Rc<RefCell<State>>, program: &Program) {
    let mut add = |declaration: &str, f: NativeFn| natives.register(declaration, f);
    macro_rules! native {
        ($declaration:expr, |$vm:ident, $state:ident, $args:ident| $body:expr) => {{
            let $state = state.clone();
            add(
                $declaration,
                Rc::new(move |$vm: &mut Vm, $args: &[Value]| {
                    #[allow(unused_variables)]
                    let $state = &$state;
                    let _ = (&$vm, $args);
                    $body
                }),
            );
        }};
    }

    // Timers (SPEC-0002).
    native!(
        "native CreateTimer takes nothing returns timer",
        |vm, state, args| {
            let handle = state.borrow_mut().create(Object::Timer(Timer::default()));
            Ok(Some(Value::Handle(Some(handle))))
        }
    );
    native!(
        "native DestroyTimer takes timer whichTimer returns nothing",
        |vm, state, args| {
            if let Some(handle) = handle(&args[0]) {
                let mut state = state.borrow_mut();
                state.stop_timer(handle);
                state.objects.remove(&handle);
            }
            Ok(None)
        }
    );
    native!(
        "native TimerStart takes timer whichTimer, real timeout, boolean periodic, code handlerFunc returns nothing",
        |vm, state, args| {
            let Some(h) = handle(&args[0]) else {
                return Ok(None);
            };
            let mut state = state.borrow_mut();
            state.stop_timer(h);
            let Some(timer) = state.timer(Some(h)) else {
                return Ok(None);
            };
            timer.timeout = raise_timeout(real(&args[1]));
            timer.periodic = args[2] == Value::Boolean(true);
            timer.callback = code(&args[3]);
            state.start_timer(h, real(&args[1]));
            Ok(None)
        }
    );
    native!(
        "native TimerGetRemaining takes timer whichTimer returns real",
        |vm, state, args| {
            Ok(Some(Value::Real(
                state.borrow_mut().remaining(handle(&args[0])),
            )))
        }
    );
    native!(
        "native TimerGetElapsed takes timer whichTimer returns real",
        |vm, state, args| {
            // A timer never started reads 0 (SPEC-0002); `sub(0, 0)` would
            // give -0.0. An expired one-shot timer has remaining 0, so its
            // elapsed time is its timeout.
            let mut state = state.borrow_mut();
            let remaining = state.remaining(handle(&args[0]));
            let Some(timer) = state.timer(handle(&args[0])).filter(|t| t.started) else {
                return Ok(Some(Value::Real(0)));
            };
            let timeout = timer.timeout;
            Ok(Some(Value::Real(sub(timeout, remaining))))
        }
    );
    native!(
        "native TimerGetTimeout takes timer whichTimer returns real",
        |vm, state, args| {
            Ok(Some(Value::Real(
                state
                    .borrow_mut()
                    .timer(handle(&args[0]))
                    .map_or(0, |t| t.timeout),
            )))
        }
    );
    native!(
        "native PauseTimer takes timer whichTimer returns nothing",
        |vm, state, args| {
            let Some(h) = handle(&args[0]) else {
                return Ok(None);
            };
            let mut state = state.borrow_mut();
            if state.timer(Some(h)).is_some_and(|t| t.queued.is_some()) {
                let remaining = state.remaining(Some(h));
                state.stop_timer(h);
                if let Some(timer) = state.timer(Some(h)) {
                    timer.paused = Some(remaining);
                }
            }
            Ok(None)
        }
    );
    native!(
        "native ResumeTimer takes timer whichTimer returns nothing",
        |vm, state, args| {
            let Some(h) = handle(&args[0]) else {
                return Ok(None);
            };
            let mut state = state.borrow_mut();
            if let Some(remaining) = state.timer(Some(h)).and_then(|t| t.paused) {
                state.start_timer(h, remaining);
            }
            Ok(None)
        }
    );
    native!(
        "native GetExpiredTimer takes nothing returns timer",
        |vm, state, args| { Ok(Some(Value::Handle(state.borrow().context.expired_timer))) }
    );

    // Threads (SPEC-0005).
    native!(
        "native TriggerSleepAction takes real timeout returns nothing",
        |vm, state, args| {
            vm.sleep(real(&args[0]));
            Ok(None)
        }
    );
    let functions: HashMap<String, usize> = program
        .functions
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.native && f.parameters.is_empty())
        .map(|(i, f)| (f.name.clone(), i))
        .collect();
    native!(
        "native ExecuteFunc takes string funcName returns nothing",
        |vm, state, args| {
            // PROVISIONAL: an unknown name, or a function that takes arguments,
            // does nothing.
            if let Some(&function) = string(&args[0]).and_then(|name| functions.get(name)) {
                let context = state.borrow().context;
                run_new_thread(vm, state, function, context, true);
            }
            Ok(None)
        }
    );

    // Triggers.
    native!(
        "native CreateTrigger takes nothing returns trigger",
        |vm, state, args| {
            let trigger = Trigger {
                enabled: true,
                actions: Vec::new(),
                conditions: Vec::new(),
            };
            Ok(Some(Value::Handle(Some(
                state.borrow_mut().create(Object::Trigger(trigger)),
            ))))
        }
    );
    native!(
        "native DestroyTrigger takes trigger whichTrigger returns nothing",
        |vm, state, args| {
            if let Some(h) = handle(&args[0]) {
                state.borrow_mut().objects.remove(&h);
            }
            Ok(None)
        }
    );
    native!(
        "native TriggerAddAction takes trigger whichTrigger, code actionFunc returns triggeraction",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            if let (Some(trigger), Some(f)) = (state.trigger(handle(&args[0])), code(&args[1])) {
                trigger.actions.push(f);
            }
            Ok(Some(Value::Handle(Some(
                state.create(Object::TriggerAction),
            ))))
        }
    );
    native!(
        "native TriggerAddCondition takes trigger whichTrigger, boolexpr condition returns triggercondition",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let function = match handle(&args[1]).and_then(|h| state.objects.get(&h)) {
                Some(Object::BoolExpr { function }) => Some(*function),
                _ => None,
            };
            if let (Some(trigger), Some(f)) = (state.trigger(handle(&args[0])), function) {
                trigger.conditions.push(f);
            }
            Ok(Some(Value::Handle(Some(
                state.create(Object::TriggerCondition),
            ))))
        }
    );
    // PROVISIONAL: each call makes a new boolexpr.
    for declaration in [
        "native Condition takes code func returns conditionfunc",
        "native Filter takes code func returns filterfunc",
    ] {
        native!(declaration, |vm, state, args| {
            Ok(Some(Value::Handle(code(&args[0]).map(|function| {
                state.borrow_mut().create(Object::BoolExpr { function })
            }))))
        });
    }
    native!(
        "native TriggerRegisterTimerEvent takes trigger whichTrigger, real timeout, boolean periodic returns event",
        |vm, state, args| {
            let Some(trigger) = handle(&args[0]) else {
                return Ok(None);
            };
            let mut state = state.borrow_mut();
            let timer = Timer {
                timeout: raise_timeout(real(&args[1])),
                periodic: args[2] == Value::Boolean(true),
                fires_trigger: Some(trigger),
                ..Timer::default()
            };
            let h = state.create(Object::Timer(timer));
            state.start_timer(h, real(&args[1]));
            Ok(Some(Value::Handle(Some(state.create(Object::Event)))))
        }
    );
    native!(
        "native TriggerExecute takes trigger whichTrigger returns nothing",
        |vm, state, args| {
            if let Some(trigger) = handle(&args[0]) {
                execute_trigger(vm, state, trigger);
            }
            Ok(None)
        }
    );
    native!(
        "native TriggerEvaluate takes trigger whichTrigger returns boolean",
        |vm, state, args| {
            let result =
                handle(&args[0]).is_some_and(|trigger| evaluate_trigger(vm, state, trigger));
            Ok(Some(Value::Boolean(result)))
        }
    );
    native!(
        "constant native GetTriggeringTrigger takes nothing returns trigger",
        |vm, state, args| {
            Ok(Some(Value::Handle(
                state.borrow().context.triggering_trigger,
            )))
        }
    );
    for (declaration, enabled) in [
        (
            "native EnableTrigger takes trigger whichTrigger returns nothing",
            true,
        ),
        (
            "native DisableTrigger takes trigger whichTrigger returns nothing",
            false,
        ),
    ] {
        native!(declaration, |vm, state, args| {
            if let Some(trigger) = state.borrow_mut().trigger(handle(&args[0])) {
                trigger.enabled = enabled;
            }
            Ok(None)
        });
    }
    native!(
        "native IsTriggerEnabled takes trigger whichTrigger returns boolean",
        |vm, state, args| {
            let enabled = state
                .borrow_mut()
                .trigger(handle(&args[0]))
                .is_some_and(|t| t.enabled);
            Ok(Some(Value::Boolean(enabled)))
        }
    );

    // Game speed and pause (SPEC-0002).
    native!(
        "constant native ConvertGameSpeed takes integer i returns gamespeed",
        |vm, state, args| {
            Ok(Some(Value::Handle(Some(
                state
                    .borrow_mut()
                    .enum_handle("gamespeed", integer(&args[0])),
            ))))
        }
    );
    native!(
        "native SetGameSpeed takes gamespeed whichspeed returns nothing",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            if let Some(index) = state.enum_value(handle(&args[0])) {
                state.clock.speed = Speed::from_index(index);
            }
            Ok(None)
        }
    );
    native!(
        "constant native GetGameSpeed takes nothing returns gamespeed",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let index = state.clock.speed.index();
            Ok(Some(Value::Handle(Some(
                state.enum_handle("gamespeed", index),
            ))))
        }
    );
    native!(
        "native PauseGame takes boolean flag returns nothing",
        |vm, state, args| {
            state.borrow_mut().clock.paused = args[0] == Value::Boolean(true);
            Ok(None)
        }
    );

    // Players and messages. PROVISIONAL: player 0 is the local player, and
    // texts are recorded without resolving trigger strings.
    native!(
        "constant native Player takes integer number returns player",
        |vm, state, args| {
            Ok(Some(Value::Handle(Some(
                state.borrow_mut().enum_handle("player", integer(&args[0])),
            ))))
        }
    );
    native!(
        "constant native GetLocalPlayer takes nothing returns player",
        |vm, state, args| {
            Ok(Some(Value::Handle(Some(
                state.borrow_mut().enum_handle("player", 0),
            ))))
        }
    );
    native!(
        "constant native GetPlayerId takes player whichPlayer returns integer",
        |vm, state, args| {
            Ok(Some(Value::Integer(
                state.borrow().enum_value(handle(&args[0])).unwrap_or(0),
            )))
        }
    );
    for declaration in [
        "native DisplayTimedTextToPlayer takes player toPlayer, real x, real y, real duration, string message returns nothing",
        "native DisplayTextToPlayer takes player toPlayer, real x, real y, string message returns nothing",
    ] {
        native!(declaration, |vm, state, args| {
            let mut state = state.borrow_mut();
            if state.enum_value(handle(&args[0])) == Some(0) {
                let text = string(args.last().expect("message"))
                    .unwrap_or("")
                    .to_owned();
                state.output.push(text);
            }
            Ok(None)
        });
    }
    native!(
        "native Preload takes string filename returns nothing",
        |vm, state, args| {
            let line = string(&args[0]).unwrap_or("").to_owned();
            state.borrow_mut().preload.push(line);
            Ok(None)
        }
    );
    native!(
        "native GetHandleId takes handle h returns integer",
        |vm, state, args| {
            Ok(Some(Value::Integer(
                handle(&args[0]).map_or(0, |h| h as i32),
            )))
        }
    );

    // Conversions (SPEC-0004, SPEC-0005).
    native!(
        "native I2S takes integer i returns string",
        |vm, state, args| { Ok(Some(Value::string(&integer(&args[0]).to_string()))) }
    );
    native!(
        "native I2R takes integer i returns real",
        |vm, state, args| { Ok(Some(Value::Real(ow3_math::i2r(integer(&args[0]))))) }
    );
    native!(
        "native R2I takes real r returns integer",
        |vm, state, args| { Ok(Some(Value::Integer(ow3_math::r2i(real(&args[0]))))) }
    );
    native!(
        "native S2R takes string s returns real",
        |vm, state, args| {
            Ok(Some(Value::Real(ow3_math::s2r(
                string(&args[0]).unwrap_or(""),
            ))))
        }
    );
    native!(
        "native S2I takes string s returns integer",
        |vm, state, args| { Ok(Some(Value::Integer(s2i(string(&args[0]).unwrap_or(""))))) }
    );
    // PROVISIONAL: three decimals for R2S, and Rust's formatting of the
    // nearest decimal for both.
    native!(
        "native R2S takes real r returns string",
        |vm, state, args| {
            Ok(Some(Value::string(&format!(
                "{:.3}",
                f32::from_bits(real(&args[0]))
            ))))
        }
    );
    native!(
        "native R2SW takes real r, integer width, integer precision returns string",
        |vm, state, args| {
            let (width, precision) = (
                integer(&args[1]).max(0) as usize,
                integer(&args[2]).max(0) as usize,
            );
            let value = f32::from_bits(real(&args[0]));
            Ok(Some(Value::string(&format!("{value:width$.precision$}"))))
        }
    );
    native!(
        "native StringLength takes string s returns integer",
        |vm, state, args| {
            Ok(Some(Value::Integer(
                string(&args[0]).map_or(0, str::len) as i32
            )))
        }
    );
}

/// `S2I` (SPEC-0005): leading spaces, an optional sign, decimal digits up to
/// the first other character, saturating at the 32-bit limits.
pub fn s2i(text: &str) -> i32 {
    let text = text.trim_start_matches(' ');
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let mut value: i64 = 0;
    for byte in digits.bytes().take_while(u8::is_ascii_digit) {
        value = (value * 10 + i64::from(byte - b'0')).min(i64::from(i32::MAX) + 1);
    }
    let value = if negative { -value } else { value };
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}
