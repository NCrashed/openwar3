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
use crate::random::Random;
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

mod boards;
pub use boards::Button;
mod attack;
mod enumerate;
mod facing;
pub mod players;
mod widgets;

/// The first handle id (SPEC-0006).
const FIRST_HANDLE: u32 = 0x0010_0000;
/// PROVISIONAL: ids the game takes before the players, so that `Player(0)`
/// is 0x100008 as in SPEC-0006; the 16 players follow in order.
const RESERVED_HANDLES: u32 = 8;
/// `GAME_STATE_TIME_OF_DAY`, a float game state.
const GAME_STATE_TIME_OF_DAY: i32 = 2;

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
    Leaderboard(boards::Leaderboard),
    Multiboard(boards::Multiboard),
    MultiboardItem {
        board: u32,
        row: i32,
        column: i32,
    },
    Dialog(boards::Dialog),
    Button(boards::Button),
    /// An object of a kind the simulation does not model yet.
    Opaque,
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
    /// Hit points as a bit pattern (SPEC-0015).
    life: u32,
}

/// PROVISIONAL: a placeholder unit with what the enumerations need, until
/// units are specified.
#[derive(Clone, Debug)]
pub struct Unit {
    owner: i32,
    type_id: i32,
    x: u32,
    y: u32,
    /// In radians (SPEC-0013, see [`facing`]).
    facing: u32,
    /// A turn started by `SetUnitFacing`.
    turn: Option<facing::Turn>,
    /// The attack the unit is making (SPEC-0016).
    attack: Option<attack::Attacking>,
    /// `orientInterp` of the type.
    orient_interp: u8,
    life: Regenerating,
    mana: Regenerating,
    alive: bool,
    hidden: bool,
    user_data: i32,
    /// The tick the unit died at.
    died_at: Option<u64>,
    /// Gold of a mine.
    resources: i32,
}

/// A unit as [`Game::units`] shows it, for drawing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnitView {
    pub handle: u32,
    pub type_id: i32,
    pub owner: i32,
    pub x: f32,
    pub y: f32,
    /// In radians, counterclockwise from east.
    pub facing: f32,
    pub alive: bool,
    pub hidden: bool,
    /// The tick of its death.
    pub died_at: Option<u64>,
    /// The attack it is making.
    pub attack: Option<AttackView>,
    /// Life and mana now, and their maxima.
    pub life: f32,
    pub max_life: f32,
    pub mana: f32,
    pub max_mana: f32,
}

/// An attack in progress, in ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttackView {
    pub target: u32,
    /// The tick of the next hit.
    pub next_hit: u64,
    /// Ticks between hits, and from the start of a swing to its hit.
    pub cooldown: u64,
    pub damage_point: u64,
}

/// A regenerating value (life or mana) as SPEC-0013 computes it: the
/// value set last, the game time elapsed since then and the rate per
/// second; the current value is `add(base, mul(rate, elapsed))`, at most
/// `max`. All bit patterns of reals.
///
/// The elapsed time grows by `dt` every tick. A hero's regeneration
/// starts 0.0001 s after it is created, so its first step adds
/// `sub(dt, 0.0001)`. This reproduces the footman and paladin life
/// samples bit for bit; PROVISIONAL: the paladin mana samples differ by
/// one unit of the last place at 3 of 60 ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Regenerating {
    base: u32,
    elapsed: u32,
    rate: u32,
    max: u32,
    /// The start is delayed by the shortest timer timeout.
    delayed: bool,
}

impl Regenerating {
    fn full(max: u32, rate: u32) -> Regenerating {
        Regenerating {
            base: max,
            elapsed: 0,
            rate,
            max,
            delayed: false,
        }
    }

    /// The current value.
    pub fn value(&self) -> u32 {
        let value = if self.rate == 0 || self.elapsed == 0 {
            self.base
        } else {
            add(self.base, mul(self.rate, self.elapsed))
        };
        if f32::from_bits(value) > f32::from_bits(self.max) {
            self.max
        } else {
            value
        }
    }

    /// One tick of game time.
    fn advance(&mut self) {
        if self.rate == 0 {
            return;
        }
        let step = if std::mem::take(&mut self.delayed) {
            sub(crate::timers::TICK, crate::timers::MIN_TIMEOUT)
        } else {
            crate::timers::TICK
        };
        if self.value() != self.max {
            self.elapsed = add(self.elapsed, step);
        }
    }

    /// Sets the value, limited to `0..=max`.
    fn set(&mut self, value: u32) {
        let v = f32::from_bits(value);
        self.base = if v > f32::from_bits(self.max) {
            self.max
        } else if v < 0.0 {
            0
        } else {
            value
        };
        self.elapsed = 0;
    }
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
    killing_unit: Option<u32>,
    triggering_player: Option<u32>,
    triggering_widget: Option<u32>,
    clicked_button: Option<u32>,
    clicked_dialog: Option<u32>,
    /// `GetEventDamage` and `GetEventDamageSource` (SPEC-0015).
    event_damage: Option<u32>,
    damage_source: Option<u32>,
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
    /// Script random numbers (SPEC-0007). PROVISIONAL: the state at the
    /// start is that of `SetRandomSeed(0)`.
    pub random: Random,
    /// The game generators of SPEC-0007, reseeded with the script's.
    pub game_random: Vec<Random>,
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
    /// Triggers registered for dialogs and their buttons.
    dialog_events: Vec<boards::DialogEvent>,
    /// The leaderboard shown to each player.
    player_leaderboards: BTreeMap<i32, Option<u32>>,
    /// Unit, destructable and item types of the map.
    pub data: ObjectData,
    /// Simulation ticks so far.
    pub ticks: u64,
    /// Units turning, in the order of the calls.
    turning: Vec<u32>,
    /// Units attacking, in the order their attacks began.
    attacking: Vec<u32>,
    /// Interface strings for `GetLocalizedString` (SPEC-0010).
    localized: BTreeMap<String, String>,
    /// The whole map, as bit patterns of reals: min x, min y, max x, max y.
    world_bounds: [u32; 4],
    /// Ground heights for `GetLocationZ` (SPEC-0014).
    heights: crate::heights::Heights,
    /// The terrain and unit models of `SetDayNightModels` (SPEC-0017), as
    /// the script names them.
    pub day_night_models: Option<(String, String)>,
    /// Damage constants (SPEC-0015).
    pub combat: crate::combat::Combat,
    /// Camera bounds as bit patterns: min x, min y, max x, max y.
    camera_bounds: [u32; 4],
    next_handle: u32,
    /// Texts shown to the local player, in order.
    pub output: Vec<String>,
    /// Lines given to `Preload`, in order.
    pub preload: Vec<String>,
    /// Errors that ended threads. The game ends such threads silently.
    pub errors: Vec<RuntimeError>,
    context: Context,
    sleep_time: u64,
    /// `config` is running, and it called a native that returns a string.
    in_config: bool,
    config_called_string_native: bool,
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
            random: Random::seeded(0),
            game_random: Random::reseed(0).1,
            units: Vec::new(),
            destructables: Vec::new(),
            items: Vec::new(),
            events: Vec::new(),
            dialog_events: Vec::new(),
            player_leaderboards: BTreeMap::new(),
            data: ObjectData::default(),
            ticks: 0,
            turning: Vec::new(),
            attacking: Vec::new(),
            localized: BTreeMap::new(),
            world_bounds: [0; 4],
            heights: crate::heights::Heights::default(),
            combat: crate::combat::Combat::default(),
            day_night_models: None,
            camera_bounds: [0; 4],
            conditions: HashMap::new(),
            next_handle: FIRST_HANDLE + RESERVED_HANDLES,
            output: Vec::new(),
            preload: Vec::new(),
            errors: Vec::new(),
            context: Context::default(),
            sleep_time: 0,
            in_config: false,
            config_called_string_native: false,
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
        state.heights = map.heights.clone();
        state.combat = map.combat.clone();
        state.camera_bounds = bounds_of(&map.info.camera_bounds.map(f32::to_bits));
        state.data = map.data.clone();
        state.localized = map.localized.clone();
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

    /// Starts the map as the game does (SPEC-0008): `config`, then the
    /// slots are filled, then `main`. A map whose `config` calls a native
    /// that returns a string does not start.
    pub fn start(&mut self) -> Result<(), StartError> {
        self.state.borrow_mut().in_config = true;
        self.run("config");
        let failed = {
            let mut state = self.state.borrow_mut();
            state.in_config = false;
            state.config_called_string_native
        };
        if failed {
            return Err(StartError(
                "config called a native that returns a string".to_owned(),
            ));
        }
        self.state.borrow_mut().setup.fill();
        self.run("main");
        Ok(())
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
        {
            let mut state = self.state.borrow_mut();
            let state = &mut *state;
            state.ticks += 1;
            let tick = state.ticks;
            // Life and mana regenerate every tick (SPEC-0013).
            for h in &state.units {
                if let Some(Object::Unit(unit)) = state.objects.get_mut(h)
                    && unit.alive
                {
                    unit.life.advance();
                    unit.mana.advance();
                }
            }
            state.turning.retain(|h| {
                let Some(Object::Unit(unit)) = state.objects.get_mut(h) else {
                    return false;
                };
                let Some(turn) = unit.turn.as_mut() else {
                    return false;
                };
                let (facing, going) = turn.advance(unit.facing, tick);
                unit.facing = facing;
                if !going {
                    unit.turn = None;
                }
                going
            });
        }
        let mut tick = self.state.borrow_mut().timers.begin_tick();
        loop {
            let Some(fired) = self.state.borrow_mut().timers.next_due(&mut tick) else {
                break;
            };
            self.fire(fired);
            self.park();
        }
        self.state.borrow_mut().timers.end_tick(tick);
        attack::hits(&mut self.vm, &self.state);
        self.park();
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
        // PROVISIONAL: triggers registered for the expiry run after the
        // callback, in the order they were registered.
        let triggers: Vec<u32> = self
            .state
            .borrow()
            .events
            .iter()
            .filter_map(|r| match *r {
                widgets::Registration::TimerExpire { trigger, timer } if timer == handle => {
                    Some(trigger)
                }
                _ => None,
            })
            .collect();
        for trigger in triggers {
            let context = Context {
                expired_timer: Some(handle),
                ..Context::default()
            };
            fire_trigger(&mut self.vm, &self.state, trigger, context);
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
    /// The units that exist, in the order they were created, as a viewer
    /// sees them.
    pub fn units(&self) -> Vec<UnitView> {
        let state = self.state.borrow();
        state
            .units
            .iter()
            .filter_map(|&handle| match state.objects.get(&handle) {
                Some(Object::Unit(u)) => Some(UnitView {
                    handle,
                    type_id: u.type_id,
                    owner: u.owner,
                    x: f32::from_bits(u.x),
                    y: f32::from_bits(u.y),
                    facing: f32::from_bits(u.facing),
                    alive: u.alive,
                    hidden: u.hidden,
                    died_at: u.died_at,
                    attack: u.attack.map(|a| a.view(&state, u.type_id)),
                    life: if u.alive {
                        f32::from_bits(u.life.value())
                    } else {
                        0.0
                    },
                    max_life: f32::from_bits(u.life.max),
                    mana: f32::from_bits(u.mana.value()),
                    max_mana: f32::from_bits(u.mana.max),
                }),
                _ => None,
            })
            .collect()
    }

    /// Game ticks so far (0.005 s of game time each).
    pub fn ticks(&self) -> u64 {
        self.state.borrow().ticks
    }

    /// The time of day in hours (`GAME_STATE_TIME_OF_DAY`).
    ///
    /// PROVISIONAL: the day does not run yet; without a value from the
    /// script it is noon.
    pub fn time_of_day(&self) -> f32 {
        let state = self.state.borrow();
        state
            .setup
            .float_game_states
            .get(&GAME_STATE_TIME_OF_DAY)
            .map_or(12.0, |&bits| f32::from_bits(bits))
    }

    /// The terrain and unit models the script chose for the light.
    pub fn day_night_models(&self) -> Option<(String, String)> {
        self.state.borrow().day_night_models.clone()
    }

    /// The ground height at (`x`, `y`) as `GetLocationZ` gives it.
    pub fn ground_z(&self, x: f32, y: f32) -> f32 {
        f32::from_bits(self.state.borrow().heights.at(x.to_bits(), y.to_bits()))
    }

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
        // SPEC-0008: a native returning a string, called in `config`, keeps
        // the map from starting.
        let f = if declaration.trim_end().ends_with("returns string") {
            players::noting_config(f, shared.clone())
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

    // Camera bounds, from which Blizzard.j derives the playable area.
    // PROVISIONAL: the bounds are the smallest and largest coordinates of
    // the four points given to `SetCameraBounds` (before that, of the
    // eight values in `war3map.w3i`), and every camera margin is 0, so
    // the playable area is the bounds the map's `main` passes.
    native!(
        add,
        state,
        "native SetCameraBounds takes real x1, real y1, real x2, real y2, real x3, real y3, real x4, real y4 returns nothing",
        |vm, state, args| {
            let values: Vec<u32> = args.iter().map(real).collect();
            state.borrow_mut().camera_bounds = bounds_of(&values);
            Ok(None)
        }
    );
    for (declaration, index) in [
        (
            "constant native GetCameraBoundMinX takes nothing returns real",
            0,
        ),
        (
            "constant native GetCameraBoundMinY takes nothing returns real",
            1,
        ),
        (
            "constant native GetCameraBoundMaxX takes nothing returns real",
            2,
        ),
        (
            "constant native GetCameraBoundMaxY takes nothing returns real",
            3,
        ),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            Ok(Some(Value::Real(state.borrow().camera_bounds[index])))
        });
    }
    native!(
        add,
        state,
        "native GetCameraMargin takes integer whichMargin returns real",
        |vm, state, args| { Ok(Some(Value::Real(0))) }
    );

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
    // SPEC-0005: byte positions; an end before the start or past the end
    // reads to the end, and a start at or past the end gives "". A
    // negative start reads memory before the string in the game; it is
    // taken as 0. A cut inside a multi-byte character gives half of it in
    // the game; here such bytes become U+FFFD, since strings are UTF-8.
    native!(
        add,
        state,
        "native SubString takes string source, integer start, integer end returns string",
        |vm, state, args| {
            let bytes = string(&args[0]).unwrap_or("").as_bytes();
            let n = bytes.len();
            let start = integer(&args[1]).max(0) as usize;
            if start >= n {
                return Ok(Some(Value::string("")));
            }
            let end = integer(&args[2]);
            let end = if end < start as i32 {
                n
            } else {
                (end as usize).min(n)
            };
            Ok(Some(Value::string(&String::from_utf8_lossy(
                &bytes[start..end],
            ))))
        }
    );
    // SPEC-0005: only ASCII letters change.
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
    // The light of the day (SPEC-0017).
    native!(
        add,
        state,
        "native SetDayNightModels takes string terrainDNCFile, string unitDNCFile returns nothing",
        |vm, state, args| {
            let terrain = string(&args[0]).unwrap_or_default().to_owned();
            let unit = string(&args[1]).unwrap_or_default().to_owned();
            state.borrow_mut().day_night_models = Some((terrain, unit));
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native SetRandomSeed takes integer seed returns nothing",
        |vm, state, args| {
            let (script, game) = Random::reseed(integer(&args[0]) as u32);
            let mut state = state.borrow_mut();
            state.random = script;
            state.game_random = game;
            Ok(None)
        }
    );

    // Math natives (SPEC-0011), bit for bit.
    type Unary = fn(u32) -> u32;
    let unary: [(&str, Unary); 9] = [
        (
            "native Deg2Rad takes real degrees returns real",
            ow3_math::natives::deg2rad,
        ),
        (
            "native Rad2Deg takes real radians returns real",
            ow3_math::natives::rad2deg,
        ),
        (
            "native Sin takes real radians returns real",
            ow3_math::natives::sin,
        ),
        (
            "native Cos takes real radians returns real",
            ow3_math::natives::cos,
        ),
        (
            "native Tan takes real radians returns real",
            ow3_math::natives::tan,
        ),
        (
            "native Asin takes real y returns real",
            ow3_math::natives::asin,
        ),
        (
            "native Acos takes real x returns real",
            ow3_math::natives::acos,
        ),
        (
            "native Atan takes real x returns real",
            ow3_math::natives::atan,
        ),
        (
            "native SquareRoot takes real x returns real",
            ow3_math::natives::square_root,
        ),
    ];
    for (declaration, f) in unary {
        native!(add, state, declaration, |vm, state, args| {
            Ok(Some(Value::Real(f(real(&args[0])))))
        });
    }
    native!(
        add,
        state,
        "native Atan2 takes real y, real x returns real",
        |vm, state, args| {
            Ok(Some(Value::Real(ow3_math::natives::atan2(
                real(&args[0]),
                real(&args[1]),
            ))))
        }
    );
    native!(
        add,
        state,
        "native Pow takes real x, real power returns real",
        |vm, state, args| {
            Ok(Some(Value::Real(ow3_math::natives::pow(
                real(&args[0]),
                real(&args[1]),
            ))))
        }
    );

    enumerate::register(&mut add, state);
    players::register(&mut add, state);
    widgets::register(&mut add, state);
    attack::register(&mut add, state);
    boards::register(&mut add, state);

    // PROVISIONAL: natives that make an object the simulation does not
    // model yet (sounds, dialogs, quests, boards, effects and the like)
    // return a new handle instead of null, so that scripts that test the
    // result against null behave as in the game.
    let constructor = |name: &str| {
        name.contains("Create")
            || name.starts_with("Add")
            || name.starts_with("Init")
            || name.starts_with("DialogAdd")
    };
    for function in &program.functions {
        if !function.native
            || !matches!(function.returns, Some(Type::Handle(_)))
            || !constructor(&function.name)
            || natives.contains(&function.name)
        {
            continue;
        }
        let parameters: Vec<String> = function
            .parameters
            .iter()
            .enumerate()
            .map(|(i, &t)| format!("{} a{i}", program.type_name(t)))
            .collect();
        let declaration = format!(
            "native {} takes {} returns {}",
            function.name,
            if parameters.is_empty() {
                "nothing".to_owned()
            } else {
                parameters.join(", ")
            },
            program.type_name(function.returns.expect("checked")),
        );
        let state = state.clone();
        natives.register(
            &declaration,
            Rc::new(move |_: &mut Vm, _: &[Value]| {
                Ok(Some(Value::Handle(Some(
                    state.borrow_mut().create(Object::Opaque),
                ))))
            }),
        );
    }
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

/// Smallest and largest x and y of points given as `x1, y1, x2, y2, ...`
/// (bit patterns of reals): min x, min y, max x, max y.
fn bounds_of(values: &[u32]) -> [u32; 4] {
    let pick = |axis: usize, smaller: bool| {
        values
            .iter()
            .skip(axis)
            .step_by(2)
            .copied()
            .reduce(|a, b| {
                let b_wins = if smaller {
                    f32::from_bits(b) < f32::from_bits(a)
                } else {
                    f32::from_bits(b) > f32::from_bits(a)
                };
                if b_wins { b } else { a }
            })
            .unwrap_or(0)
    };
    [pick(0, true), pick(1, true), pick(0, false), pick(1, false)]
}

/// Why a map did not start.
#[derive(Debug)]
pub struct StartError(pub String);

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for StartError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// The measured regeneration of SPEC-0013: `(tick, footman life,
    /// paladin life, paladin mana)`.
    fn samples() -> Vec<[u32; 4]> {
        include_str!("../../../specs/SPEC-0013-regen-samples.tsv")
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .map(|l| {
                let mut f = l.split('\t');
                let tick = f.next().unwrap().parse().unwrap();
                let mut hex = || u32::from_str_radix(f.next().unwrap().trim(), 16).unwrap();
                [tick, hex(), hex(), hex()]
            })
            .collect()
    }

    #[test]
    fn footman_regeneration_matches_samples() {
        // SetUnitState(u, UNIT_STATE_LIFE, 100), 0.25 per second.
        let mut life = Regenerating::full(0x43D2_0000, 0x3E80_0000);
        life.set(0x42C8_0000);
        assert_eq!(life.value(), 0x42C8_0000);
        let samples = samples();
        assert_eq!(samples.len(), 60);
        for [n, footman, _, _] in samples {
            life.advance();
            assert_eq!(life.value(), footman, "tick {n}");
        }
    }

    /// A paladin (strength 22, intelligence 17) set to life 100 and mana
    /// 10 as it is created: its regeneration starts 0.0001 s late.
    #[test]
    fn paladin_regeneration_matches_samples() {
        let per_point = 0x3D4C_CCCD;
        let mut life = Regenerating::full(0x4422_8000, add(0x3E80_0000, mul(i2r(22), per_point)));
        let mut mana = Regenerating::full(0x437F_0000, mul(i2r(17), per_point));
        life.delayed = true;
        mana.delayed = true;
        life.set(0x42C8_0000);
        mana.set(0x4120_0000);
        let mut mana_misses = 0;
        for [n, _, paladin_life, paladin_mana] in samples() {
            life.advance();
            mana.advance();
            assert_eq!(life.value(), paladin_life, "tick {n}");
            // PROVISIONAL: one unit of the last place off at a few ticks.
            let off = mana.value().abs_diff(paladin_mana);
            assert!(off <= 1, "tick {n}");
            mana_misses += usize::from(off != 0);
        }
        assert!(mana_misses <= 3);
    }

    #[test]
    fn regenerating_limits() {
        let mut mana = Regenerating::full(0x4320_0000, 0x3F80_0000);
        mana.set(0xC2C8_0000);
        assert_eq!(mana.value(), 0);
        mana.set(0x4400_0000);
        assert_eq!(mana.value(), 0x4320_0000);
        mana.set(0x431F_0000);
        for _ in 0..1_000 {
            mana.advance();
        }
        assert_eq!(mana.value(), 0x4320_0000);
    }
}
