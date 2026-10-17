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
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;

use ow3_formats::map::strings::RESOLVING_NATIVES;
use ow3_jass::ast::Script;
use ow3_jass::check::{Program, Type};
use ow3_jass::semantics::Value;
use ow3_jass::vm::{LoadError, NativeFn, Natives, Outcome, RuntimeError, Thread, Vm};
use ow3_math::{add, div, i2r, mul, sub};

use crate::clock::{CHUNK_MICROS, FrameClock, Speed, Step};
use crate::data::ObjectData;
use crate::map::Map;
use crate::sleep::SleepQueue;
use crate::timers::{Fired, LONG_PART, TimerId, TimerQueue, raise_timeout, split_timeout};

/// Registers a native: `add` is the registering closure and `state` the
/// shared state, which the body sees as `state`.
macro_rules! native {
    ($add:ident, $shared:ident, $declaration:expr, |$vm:ident, $state:ident, $args:ident| $body:expr) => {{
        let $state = $shared.clone();
        $add(
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

mod enumerate;
pub mod players;
mod widgets;

/// The first handle id (SPEC-0006).
const FIRST_HANDLE: u32 = 0x0010_0000;
/// PROVISIONAL: ids the game takes before the players, so that `Player(0)`
/// is 0x100008 as in SPEC-0006; the 16 players follow in order.
const RESERVED_HANDLES: u32 = 8;

/// What a handle refers to.
#[derive(Clone, Debug)]
pub enum Object {
    Timer(Timer),
    Trigger(Trigger),
    TriggerAction,
    TriggerCondition,
    BoolExpr(BoolExpr),
    Event,
    /// A player slot, made on first use by `Player` and the like.
    Player(i32),
    /// Player ids, enumerated in increasing order.
    Force(BTreeSet<i32>),
    /// Unit handles in the order they were added.
    Group(Vec<u32>),
    Unit(Unit),
    Destructable(Destructable),
    Item(Item),
    Location {
        x: u32,
        y: u32,
    },
    Rect {
        min_x: u32,
        min_y: u32,
        max_x: u32,
        max_y: u32,
    },
}

/// A `boolexpr`: a function made by `Condition` or `Filter`, or `And`,
/// `Or` and `Not` of other boolexprs.
#[derive(Clone, Copy, Debug)]
pub enum BoolExpr {
    Function(usize),
    And(Option<u32>, Option<u32>),
    Or(Option<u32>, Option<u32>),
    Not(Option<u32>),
}

/// PROVISIONAL: a placeholder destructable, as placed in `war3map.doo`.
#[derive(Clone, Debug)]
pub struct Destructable {
    type_id: i32,
    x: u32,
    y: u32,
    /// Hit points as a bit pattern, and the most there can be.
    life: u32,
    max_life: u32,
    alive: bool,
}

/// PROVISIONAL: a placeholder item lying on the ground.
#[derive(Clone, Debug)]
pub struct Item {
    type_id: i32,
    x: u32,
    y: u32,
}

/// PROVISIONAL: a placeholder unit with what the enumerations need, until
/// units are specified.
#[derive(Clone, Debug)]
pub struct Unit {
    owner: i32,
    type_id: i32,
    x: u32,
    y: u32,
    facing: u32,
    /// Hit points as a bit pattern, and the most there can be.
    life: u32,
    max_life: u32,
    alive: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Timer {
    queued: Option<TimerId>,
    /// The timeout given to `TimerStart`, raised to at least 0.0001: what
    /// `TimerGetTimeout` reads and the period of a periodic timer.
    timeout: u32,
    /// Length of the current run: the timeout, or after `ResumeTimer` the
    /// time that was left at the pause (SPEC-0002).
    run: u32,
    /// The part of the run in the queue now, and the parts before it. A
    /// run under 120 s is one part.
    part: u32,
    done: u32,
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
    /// Boolexpr handles, tested in the order added.
    conditions: Vec<u32>,
}

/// Event responses of the running thread, kept across its sleeps.
#[derive(Clone, Copy, Debug, Default)]
struct Context {
    expired_timer: Option<u32>,
    triggering_trigger: Option<u32>,
    enum_player: Option<u32>,
    filter_player: Option<u32>,
    enum_unit: Option<u32>,
    filter_unit: Option<u32>,
    enum_destructable: Option<u32>,
    filter_destructable: Option<u32>,
    enum_item: Option<u32>,
    filter_item: Option<u32>,
    /// Responses of the event that started the thread.
    event_id: Option<i32>,
    triggering_unit: Option<u32>,
    triggering_player: Option<u32>,
    triggering_widget: Option<u32>,
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
    /// Handles of the player slots made so far, by id.
    players: BTreeMap<i32, u32>,
    /// Player slots and the rest of the lobby and `config` setup.
    pub setup: players::Setup,
    /// PROVISIONAL: random numbers, until the game's generator is specified.
    pub random: Random,
    /// The boolexpr made by `Condition` or `Filter` for each function: the
    /// same function gives the same handle (SPEC-0006).
    conditions: HashMap<usize, u32>,
    /// Units in the order they were created.
    units: Vec<u32>,
    /// Destructables in the order they were placed or created.
    destructables: Vec<u32>,
    /// Items on the ground in the order they were created.
    items: Vec<u32>,
    /// Event registrations in the order they were made.
    events: Vec<widgets::Registration>,
    /// Unit, destructable and item types of the map.
    pub data: ObjectData,
    /// The whole map, as bit patterns of reals: min x, min y, max x, max y.
    world_bounds: [u32; 4],
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
        let mut state = State {
            clock: FrameClock::new(),
            timers: TimerQueue::new(),
            objects: HashMap::new(),
            players: BTreeMap::new(),
            setup: players::Setup::single_player(),
            random: Random::new(0),
            units: Vec::new(),
            destructables: Vec::new(),
            items: Vec::new(),
            events: Vec::new(),
            data: ObjectData::default(),
            world_bounds: [0; 4],
            conditions: HashMap::new(),
            next_handle: FIRST_HANDLE + RESERVED_HANDLES,
            output: Vec::new(),
            preload: Vec::new(),
            errors: Vec::new(),
            context: Context::default(),
            sleep_time: 0,
            parked: Vec::new(),
        };
        for id in 0..players::SLOTS {
            state.player_handle(id);
        }
        state
    }

    fn create(&mut self, object: Object) -> u32 {
        let handle = self.next_handle;
        self.next_handle += 1;
        self.objects.insert(handle, object);
        handle
    }

    /// The handle of player slot `id`, made on first use.
    fn player_handle(&mut self, id: i32) -> u32 {
        if let Some(&handle) = self.players.get(&id) {
            return handle;
        }
        let handle = self.create(Object::Player(id));
        self.players.insert(id, handle);
        handle
    }

    fn player_id(&self, handle: Option<u32>) -> Option<i32> {
        match self.objects.get(&handle?)? {
            Object::Player(id) => Some(*id),
            _ => None,
        }
    }

    fn force(&mut self, handle: Option<u32>) -> Option<&mut BTreeSet<i32>> {
        match self.objects.get_mut(&handle?)? {
            Object::Force(players) => Some(players),
            _ => None,
        }
    }

    fn group(&mut self, handle: Option<u32>) -> Option<&mut Vec<u32>> {
        match self.objects.get_mut(&handle?)? {
            Object::Group(units) => Some(units),
            _ => None,
        }
    }

    fn unit(&self, handle: Option<u32>) -> Option<&Unit> {
        match self.objects.get(&handle?)? {
            Object::Unit(unit) => Some(unit),
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

    /// Starts a run of `length` seconds. A run of 120 s or more is queued
    /// as parts (SPEC-0002).
    fn start_timer(&mut self, handle: u32, length: u32) {
        let length = raise_timeout(length);
        let (n, rest) = split_timeout(length);
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
            timer.run = length;
            timer.part = first;
            timer.done = 0;
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
        timer.done = add(timer.done, timer.part);
        timer.part = part;
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
    /// PROVISIONAL: a long timer reads the length of its run minus the
    /// parts done and the time spent in the current part, which keeps
    /// `TimerGetElapsed` exact as in test PZ; the game's operations are not
    /// specified.
    fn remaining(&mut self, handle: Option<u32>) -> u32 {
        let now = self.timers.time();
        let Some(timer) = self.timer(handle) else {
            return 0;
        };
        if let Some(paused) = timer.paused {
            return paused;
        }
        let (queued, run, part, done) = (timer.queued, timer.run, timer.part, timer.done);
        let long = timer.parts_left > 0 || timer.last_part.is_some() || done != 0;
        let Some(expiry) = queued.and_then(|id| self.timers.expiry(id)) else {
            return 0;
        };
        let left = sub(expiry, now);
        if long {
            sub(run, add(done, sub(part, left)))
        } else {
            left
        }
    }

    /// `TimerGetElapsed`: the run length minus the remaining time while it
    /// runs, the timeout minus the saved time while paused, the timeout
    /// once expired, and 0 before the first start (SPEC-0002).
    fn elapsed(&mut self, handle: Option<u32>) -> u32 {
        let remaining = self.remaining(handle);
        let Some(timer) = self.timer(handle).filter(|t| t.started) else {
            return 0;
        };
        let base = if timer.queued.is_some() {
            timer.run
        } else {
            timer.timeout
        };
        sub(base, remaining)
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
        Game::with_state(program, scripts, State::new())
    }

    /// Loads a map's checked scripts, with the players, trigger strings,
    /// destructables and bounds of the map, as single player. Run `config`
    /// and `main` next, or use [`Game::start`].
    pub fn load_map(program: &'a Program, map: &'a Map) -> Result<Game<'a>, LoadError> {
        let mut state = State::new();
        state.setup = players::Setup::from_map(map);
        let b = &map.world_bounds;
        state.world_bounds = [b.min_x, b.min_y, b.max_x, b.max_y].map(f32::to_bits);
        state.data = map.data.clone();
        for d in &map.destructables {
            // PROVISIONAL: placed with the life percentage of the editor.
            let max_life = state
                .data
                .destructables
                .get(&d.type_id)
                .copied()
                .unwrap_or(0);
            let life = div(mul(max_life, i2r(i32::from(d.life_percent))), i2r(100));
            let destructable = Destructable {
                type_id: d.type_id,
                x: d.x.to_bits(),
                y: d.y.to_bits(),
                life,
                max_life,
                alive: true,
            };
            let handle = state.create(Object::Destructable(destructable));
            state.destructables.push(handle);
        }
        Game::with_state(program, &map.script_refs(), state)
    }

    /// Runs `config`, then `main`, as the game does when a map starts.
    pub fn start(&mut self) {
        self.run("config");
        self.run("main");
    }

    fn with_state(
        program: &'a Program,
        scripts: &[&'a Script],
        state: State,
    ) -> Result<Game<'a>, LoadError> {
        let state = Rc::new(RefCell::new(state));
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
            (timer.callback, timer.fires_trigger)
        };
        if let Some(trigger) = trigger {
            fire_trigger(&mut self.vm, &self.state, trigger, Context::default());
        } else if let Some(callback) = callback {
            let context = Context {
                expired_timer: Some(handle),
                ..Context::default()
            };
            // A timer callback that sleeps ends at the sleep (SPEC-0005).
            run_new_thread(&mut self.vm, &self.state, callback, context, false);
        }
        // A periodic timer starts again after its callback, one period
        // after the expiry that fired, unless the callback started,
        // destroyed or paused it (SPEC-0002). The queue time is still that
        // expiry.
        let mut state = self.state.borrow_mut();
        let restart = state
            .timer(Some(handle))
            .filter(|t| t.periodic && t.queued.is_none() && t.paused.is_none())
            .map(|t| t.timeout);
        if let Some(period) = restart {
            state.start_timer(handle, period);
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
fn evaluate_trigger(vm: &mut Vm, state: &Rc<RefCell<State>>, trigger: u32, base: Context) -> bool {
    let Some(conditions) = state
        .borrow_mut()
        .trigger(Some(trigger))
        .map(|t| t.conditions.clone())
    else {
        return false;
    };
    let context = Context {
        triggering_trigger: Some(trigger),
        ..base
    };
    // SPEC-0006: every condition is evaluated, also after a false one.
    let results: Vec<bool> = conditions
        .into_iter()
        .map(|condition| test_boolexpr(vm, state, Some(condition), context))
        .collect();
    results.into_iter().all(|ok| ok)
}

/// Runs a boolean function in its own thread: true only if it returns
/// true. One that sleeps ends there and counts as false, as one that ends
/// by an error (SPEC-0005).
fn run_test(vm: &mut Vm, state: &Rc<RefCell<State>>, function: usize, context: Context) -> bool {
    match with_context(state, context, || vm.start_thread(function, Vec::new())) {
        Ok((Outcome::Finished(Some(Value::Boolean(true))), _)) => true,
        Ok(_) => false,
        Err(error) => {
            state.borrow_mut().errors.push(error);
            false
        }
    }
}

/// Tests a boolexpr given to a native; a `null` filter accepts everything.
fn test_boolexpr(
    vm: &mut Vm,
    state: &Rc<RefCell<State>>,
    expr: Option<u32>,
    context: Context,
) -> bool {
    expr.is_none() || test_operand(vm, state, expr, context)
}

/// Tests a boolexpr operand. `And` and `Or` test the right operand only
/// when the left one does not decide; a `null` or destroyed operand of
/// `And` is false (SPEC-0006). PROVISIONAL: also of `Or` and `Not`.
fn test_operand(
    vm: &mut Vm,
    state: &Rc<RefCell<State>>,
    expr: Option<u32>,
    context: Context,
) -> bool {
    let object = expr.and_then(|h| state.borrow().objects.get(&h).cloned());
    let Some(Object::BoolExpr(expr)) = object else {
        return false;
    };
    match expr {
        BoolExpr::Function(function) => run_test(vm, state, function, context),
        BoolExpr::And(a, b) => {
            test_operand(vm, state, a, context) && test_operand(vm, state, b, context)
        }
        BoolExpr::Or(a, b) => {
            test_operand(vm, state, a, context) || test_operand(vm, state, b, context)
        }
        BoolExpr::Not(a) => !test_operand(vm, state, a, context),
    }
}

/// `TriggerExecute`: runs the actions; the caller continues once they end
/// or one sleeps (SPEC-0005).
fn execute_trigger(vm: &mut Vm, state: &Rc<RefCell<State>>, trigger: u32, base: Context) {
    let Some(actions) = state
        .borrow_mut()
        .trigger(Some(trigger))
        .map(|t| t.actions.clone())
    else {
        return;
    };
    let context = Context {
        triggering_trigger: Some(trigger),
        ..base
    };
    run_actions(vm, state, actions, 0, context);
}

/// A trigger's event happened: if it is enabled and its conditions hold,
/// its actions run.
fn fire_trigger(vm: &mut Vm, state: &Rc<RefCell<State>>, trigger: u32, base: Context) {
    let enabled = state
        .borrow_mut()
        .trigger(Some(trigger))
        .is_some_and(|t| t.enabled);
    if enabled && evaluate_trigger(vm, state, trigger, base) {
        execute_trigger(vm, state, trigger, base);
    }
}

fn handle(value: &Value) -> Option<u32> {
    match value {
        Value::Handle(h) => *h,
        _ => None,
    }
}

/// The handle of a `Convert*` value, whose id is the integer itself.
fn converted(value: i32) -> Value {
    Value::Handle(Some(value as u32))
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
    // Natives that resolve trigger strings get their arguments resolved.
    let shared = state.clone();
    let mut add = |declaration: &str, f: NativeFn| {
        let f = if RESOLVING_NATIVES.contains(&players::native_name(declaration)) {
            players::resolving(f, shared.clone())
        } else {
            f
        };
        natives.register(declaration, f);
    };

    // Timers (SPEC-0002).
    native!(
        add,
        state,
        "native CreateTimer takes nothing returns timer",
        |vm, state, args| {
            let handle = state.borrow_mut().create(Object::Timer(Timer::default()));
            Ok(Some(Value::Handle(Some(handle))))
        }
    );
    native!(
        add,
        state,
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
        add,
        state,
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
        add,
        state,
        "native TimerGetRemaining takes timer whichTimer returns real",
        |vm, state, args| {
            Ok(Some(Value::Real(
                state.borrow_mut().remaining(handle(&args[0])),
            )))
        }
    );
    native!(
        add,
        state,
        "native TimerGetElapsed takes timer whichTimer returns real",
        |vm, state, args| {
            Ok(Some(Value::Real(
                state.borrow_mut().elapsed(handle(&args[0])),
            )))
        }
    );
    native!(
        add,
        state,
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
        add,
        state,
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
        add,
        state,
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
        add,
        state,
        "native GetExpiredTimer takes nothing returns timer",
        |vm, state, args| { Ok(Some(Value::Handle(state.borrow().context.expired_timer))) }
    );

    // Threads (SPEC-0005).
    native!(
        add,
        state,
        "native TriggerSleepAction takes real timeout returns nothing",
        |vm, state, args| {
            vm.sleep(real(&args[0]));
            Ok(None)
        }
    );
    // Script functions by name, with whether they take parameters.
    let functions: HashMap<String, (usize, bool)> = program
        .functions
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.native)
        .map(|(i, f)| (f.name.clone(), (i, !f.parameters.is_empty())))
        .collect();
    native!(
        add,
        state,
        "native ExecuteFunc takes string funcName returns nothing",
        |vm, state, args| {
            // SPEC-0006: the name is case-sensitive, an unknown name does
            // nothing, and a returned value is discarded. PROVISIONAL: a
            // function with parameters ends the caller (the game showed no
            // further output).
            match string(&args[0]).and_then(|name| functions.get(name)) {
                Some(&(_, true)) => Err(vm.fail("ExecuteFunc of a function with parameters")),
                Some(&(function, false)) => {
                    let context = state.borrow().context;
                    run_new_thread(vm, state, function, context, true);
                    Ok(None)
                }
                None => Ok(None),
            }
        }
    );

    // Triggers.
    native!(
        add,
        state,
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
        add,
        state,
        "native DestroyTrigger takes trigger whichTrigger returns nothing",
        |vm, state, args| {
            if let Some(h) = handle(&args[0]) {
                state.borrow_mut().objects.remove(&h);
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
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
        add,
        state,
        "native TriggerAddCondition takes trigger whichTrigger, boolexpr condition returns triggercondition",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let condition = handle(&args[1])
                .filter(|h| matches!(state.objects.get(h), Some(Object::BoolExpr(_))));
            if let (Some(trigger), Some(c)) = (state.trigger(handle(&args[0])), condition) {
                trigger.conditions.push(c);
            }
            Ok(Some(Value::Handle(Some(
                state.create(Object::TriggerCondition),
            ))))
        }
    );
    // The same function gives the same boolexpr, through either native
    // (SPEC-0006). PROVISIONAL: after `DestroyBoolExpr` a new one is made.
    for declaration in [
        "native Condition takes code func returns conditionfunc",
        "native Filter takes code func returns filterfunc",
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let Some(function) = code(&args[0]) else {
                return Ok(Some(Value::Handle(None)));
            };
            let mut state = state.borrow_mut();
            let existing = state
                .conditions
                .get(&function)
                .copied()
                .filter(|h| matches!(state.objects.get(h), Some(Object::BoolExpr(_))));
            let h = existing.unwrap_or_else(|| {
                let h = state.create(Object::BoolExpr(BoolExpr::Function(function)));
                state.conditions.insert(function, h);
                h
            });
            Ok(Some(Value::Handle(Some(h))))
        });
    }
    native!(
        add,
        state,
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
        add,
        state,
        "native TriggerExecute takes trigger whichTrigger returns nothing",
        |vm, state, args| {
            if let Some(trigger) = handle(&args[0]) {
                let base = state.borrow().context;
                execute_trigger(vm, state, trigger, base);
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native TriggerEvaluate takes trigger whichTrigger returns boolean",
        |vm, state, args| {
            let result = handle(&args[0]).is_some_and(|trigger| {
                let base = state.borrow().context;
                evaluate_trigger(vm, state, trigger, base)
            });
            Ok(Some(Value::Boolean(result)))
        }
    );
    native!(
        add,
        state,
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
        native!(add, state, declaration, |vm, state, args| {
            if let Some(trigger) = state.borrow_mut().trigger(handle(&args[0])) {
                trigger.enabled = enabled;
            }
            Ok(None)
        });
    }
    native!(
        add,
        state,
        "native IsTriggerEnabled takes trigger whichTrigger returns boolean",
        |vm, state, args| {
            let enabled = state
                .borrow_mut()
                .trigger(handle(&args[0]))
                .is_some_and(|t| t.enabled);
            Ok(Some(Value::Boolean(enabled)))
        }
    );

    // A `Convert*` value is not an object: its handle id is the integer
    // itself (test B1). This covers every `ConvertX takes integer returns
    // some handle type` native the scripts declare.
    for function in &program.functions {
        if function.native
            && function.name.starts_with("Convert")
            && function.parameters == [Type::Integer]
            && let Some(returns @ Type::Handle(_)) = function.returns
        {
            let declaration = format!(
                "constant native {} takes integer i returns {}",
                function.name,
                program.type_name(returns)
            );
            native!(add, state, &declaration, |vm, state, args| {
                Ok(Some(converted(integer(&args[0]))))
            });
        }
    }

    // Game speed and pause (SPEC-0002).
    native!(
        add,
        state,
        "native SetGameSpeed takes gamespeed whichspeed returns nothing",
        |vm, state, args| {
            if let Some(index) = handle(&args[0]) {
                state.borrow_mut().clock.speed = Speed::from_index(index as i32);
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "constant native GetGameSpeed takes nothing returns gamespeed",
        |vm, state, args| { Ok(Some(converted(state.borrow().clock.speed.index()))) }
    );
    native!(
        add,
        state,
        "native PauseGame takes boolean flag returns nothing",
        |vm, state, args| {
            state.borrow_mut().clock.paused = args[0] == Value::Boolean(true);
            Ok(None)
        }
    );

    // Players and messages. Texts shown to the local player are recorded;
    // trigger strings in them are resolved (SPEC-0003).
    native!(
        add,
        state,
        "constant native Player takes integer number returns player",
        |vm, state, args| {
            // PROVISIONAL: ids outside the 16 player slots give null.
            let id = integer(&args[0]);
            Ok(Some(Value::Handle(
                (0..16)
                    .contains(&id)
                    .then(|| state.borrow_mut().player_handle(id)),
            )))
        }
    );
    native!(
        add,
        state,
        "constant native GetPlayerId takes player whichPlayer returns integer",
        |vm, state, args| {
            Ok(Some(Value::Integer(
                state.borrow().player_id(handle(&args[0])).unwrap_or(0),
            )))
        }
    );
    for declaration in [
        "native DisplayTimedTextToPlayer takes player toPlayer, real x, real y, real duration, string message returns nothing",
        "native DisplayTextToPlayer takes player toPlayer, real x, real y, string message returns nothing",
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let mut state = state.borrow_mut();
            if state.player_id(handle(&args[0])) == Some(state.setup.local_player) {
                let text = string(args.last().expect("message"))
                    .unwrap_or("")
                    .to_owned();
                state.output.push(text);
            }
            Ok(None)
        });
    }
    native!(
        add,
        state,
        "native Preload takes string filename returns nothing",
        |vm, state, args| {
            let line = string(&args[0]).unwrap_or("").to_owned();
            state.borrow_mut().preload.push(line);
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native GetHandleId takes handle h returns integer",
        |vm, state, args| {
            Ok(Some(Value::Integer(
                handle(&args[0]).map_or(0, |h| h as i32),
            )))
        }
    );

    // Conversions (SPEC-0004, SPEC-0005).
    native!(
        add,
        state,
        "native I2S takes integer i returns string",
        |vm, state, args| { Ok(Some(Value::string(&integer(&args[0]).to_string()))) }
    );
    native!(
        add,
        state,
        "native I2R takes integer i returns real",
        |vm, state, args| { Ok(Some(Value::Real(ow3_math::i2r(integer(&args[0]))))) }
    );
    native!(
        add,
        state,
        "native R2I takes real r returns integer",
        |vm, state, args| { Ok(Some(Value::Integer(ow3_math::r2i(real(&args[0]))))) }
    );
    native!(
        add,
        state,
        "native S2R takes string s returns real",
        |vm, state, args| {
            Ok(Some(Value::Real(ow3_math::s2r(
                string(&args[0]).unwrap_or(""),
            ))))
        }
    );
    native!(
        add,
        state,
        "native S2I takes string s returns integer",
        |vm, state, args| { Ok(Some(Value::Integer(s2i(string(&args[0]).unwrap_or(""))))) }
    );
    // R2S and R2SW as SPEC-0004 describes them.
    native!(
        add,
        state,
        "native R2S takes real r returns string",
        |vm, state, args| { Ok(Some(Value::string(&ow3_math::r2sw(real(&args[0]), 0, 3)))) }
    );
    native!(
        add,
        state,
        "native R2SW takes real r, integer width, integer precision returns string",
        |vm, state, args| {
            let text = ow3_math::r2sw(real(&args[0]), integer(&args[1]), integer(&args[2]));
            Ok(Some(Value::string(&text)))
        }
    );
    native!(
        add,
        state,
        "native StringLength takes string s returns integer",
        |vm, state, args| {
            Ok(Some(Value::Integer(
                string(&args[0]).map_or(0, str::len) as i32
            )))
        }
    );
    // PROVISIONAL: byte positions, clamped to the string; an end before the
    // start gives an empty string.
    native!(
        add,
        state,
        "native SubString takes string source, integer start, integer end returns string",
        |vm, state, args| {
            let Some(source) = string(&args[0]) else {
                return Ok(Some(Value::String(None)));
            };
            let clamp = |i: i32| (i.max(0) as usize).min(source.len());
            let (start, end) = (clamp(integer(&args[1])), clamp(integer(&args[2])));
            let bytes = source.as_bytes().get(start..end.max(start)).unwrap_or(&[]);
            Ok(Some(Value::string(&String::from_utf8_lossy(bytes))))
        }
    );
    // PROVISIONAL: ASCII letters only.
    native!(
        add,
        state,
        "native StringCase takes string source, boolean upper returns string",
        |vm, state, args| {
            let Some(source) = string(&args[0]) else {
                return Ok(Some(Value::String(None)));
            };
            Ok(Some(Value::string(&if args[1] == Value::Boolean(true) {
                source.to_ascii_uppercase()
            } else {
                source.to_ascii_lowercase()
            })))
        }
    );

    native!(
        add,
        state,
        "native GetRandomInt takes integer lowBound, integer highBound returns integer",
        |vm, state, args| {
            let value = state
                .borrow_mut()
                .random
                .integer(integer(&args[0]), integer(&args[1]));
            Ok(Some(Value::Integer(value)))
        }
    );
    native!(
        add,
        state,
        "native GetRandomReal takes real lowBound, real highBound returns real",
        |vm, state, args| {
            let value = state
                .borrow_mut()
                .random
                .real(real(&args[0]), real(&args[1]));
            Ok(Some(Value::Real(value)))
        }
    );
    native!(
        add,
        state,
        "native SetRandomSeed takes integer seed returns nothing",
        |vm, state, args| {
            state.borrow_mut().random = Random::new(integer(&args[0]) as u32 as u64);
            Ok(None)
        }
    );

    enumerate::register(&mut add, state);
    players::register(&mut add, state);
    widgets::register(&mut add, state);
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

/// PROVISIONAL: a deterministic stand-in for the game's random numbers
/// (SplitMix64), so that scripts see varied values. The game's generator,
/// its seeding and how bounds are handled are not specified yet.
#[derive(Clone, Debug)]
pub struct Random {
    state: u64,
}

impl Random {
    pub fn new(seed: u64) -> Random {
        Random { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// An integer from `low` to `high`, both included, in either order.
    pub fn integer(&mut self, low: i32, high: i32) -> i32 {
        let (low, high) = (low.min(high), low.max(high));
        let span = (i64::from(high) - i64::from(low) + 1) as u64;
        (i64::from(low) + (self.next() % span) as i64) as i32
    }

    /// A real from `low` to `high` with engine arithmetic: `low` plus the
    /// span times a multiple of 2^-24 below 1.
    pub fn real(&mut self, low: u32, high: u32) -> u32 {
        let fraction = mul(i2r((self.next() >> 40) as i32), 0x3380_0000);
        add(low, mul(sub(high, low), fraction))
    }
}
