//! Deterministic 50 Hz rail-shooter simulation. Input is sampled independently at
//! display refresh rate, so PAL and NTSC use the same millisecond gesture windows.
use crate::assets::{self, AssetBank, Segment, Spawn, StageInfo};
use crate::audio::{AudioEvent, Effect, MusicState};
use crate::{emit_mesh, GeometrySink, Vec3, TERRAIN_CLEAR_GEOMETRY};

pub const UP: u8 = 1;
pub const DOWN: u8 = 2;
pub const LEFT: u8 = 4;
pub const RIGHT: u8 = 8;
pub const FIRE: u8 = 16;
pub const MISSION_DISTANCE: u32 = assets::SEGMENT_DISTANCE * assets::SEGMENT_COUNT as u32;
/// Added scenic flight time between combat sections, at 50 simulation ticks/s.
pub const TRANSITION_TICKS: u16 = 400;
/// Fixed camera-space backdrop geometry; nearer scene geometry still occludes it.
pub(crate) const STATIC_SKY: u8 = 2;
const PLAYER_Z: i16 = 180;
const SPEED: i16 = 8;
/// Nearest whole-unit radius to a three-quarter-size hostile projectile.
const ENEMY_SHOT_RADIUS: i16 = 4;
/// Actors fly past the camera instead of vanishing: they stay alive and drawn
/// until the deepest actor mesh (72 units, checked by the asset tests) is
/// entirely behind the near plane, where the renderer rejects every face.
pub const ACTOR_RETIRE_Z: i16 = crate::render::NEAR - 72;
const ENEMY_RAM_DEPTH: i16 = assets::enemy_scaled(32);
const BOSS_TURRET_X: i16 = 65 * assets::BOSS_SCALE;
const BOSS_TURRET_Y: i16 = 15 * assets::BOSS_SCALE;
const BOSS_TURRET_Z: i16 = -35 * assets::BOSS_SCALE;
const BOSS_CORE_Z: i16 = -100 * assets::BOSS_SCALE;
const BOSS_TURRET_RADIUS: i16 = 23 * assets::BOSS_SCALE;
const BOSS_CORE_RADIUS: i16 = 30 * assets::BOSS_SCALE;
const BOSS_HIT_DEPTH: i16 = 48 * assets::BOSS_SCALE;
const PICKUP_MAGNET_RANGE: i16 = 64;
const PICKUP_MAGNET_DEPTH: i16 = 240;
const PICKUP_MAGNET_SPEED: i16 = 2;
const BOSS_SIDE_HP: u8 = 40;
const BOSS_CORE_HP: u8 = 120;
pub const SECTION_TICKS: u16 = 3750;
pub const MISSION_TICKS: u32 = SECTION_TICKS as u32 * assets::SEGMENT_COUNT as u32;
const BOSS_START_TICKS: u16 = 1000;
const SPACE_BOSS_START_TICKS: u16 = 2500;
const ATTRACT_TICKS: u16 = 750;
const ACTORS: usize = 12;
/// Repeat ordinary encounters after 3.375 seconds without enlarging the cache.
const ENEMY_REPEAT_DISTANCE: u16 = 1350;
const ENEMY_HIT_FLASH_TICKS: u8 = 10;
const ENEMY_HIT_FLASH_PENDING: u8 = ENEMY_HIT_FLASH_TICKS * 2 + 1;
const SHOTS: usize = 24;
const MISSILE_SEED: u8 = 0xa5;
const AUDIO_CAPACITY: usize = 8;
const DOUBLE_TAP_WINDOW_MS: u16 = 500;
/// Fire released this long charges a homing missile; the remembered release
/// instant is held within this window of the present.
const CHARGE_FULL_MS: u16 = 2000;

// Per-level switches compile into three bytes. Shadow ink is selected separately
// by shadow_color so the Space Carrier can contrast with its gray deck.
#[cfg_attr(target_arch = "mos", link_section = ".tables.shadow_levels")]
static SHADOW_LEVELS: [u8; 3] = {
    #[rustfmt::skip]
    const LEVELS: [bool; assets::SEGMENT_COUNT as usize] = [
        true,  // Lowland Approach
        true,  // Crystal Cave
        true,  // Iron Canyon
        true,  // Canyon Gate
        true,  // Ashen Wastes
        true,  // Foundry Belt
        true,  // Fortress Run
        true,  // Citadel Core
        true,  // Storm Front
        true,  // Meteor Reach
        true,  // Carrier Coast
        true,  // Carrier Assault
        false, // Asteroid Belt
        false, // Shattered Moon
        false, // Ship Graveyard
        false, // Orbital Shipyard
        true,  // Space Carrier
        false, // Jump Gate
    ];
    let mut packed = [0; 3];
    let mut stage = 0;
    while stage < LEVELS.len() {
        if LEVELS[stage] {
            packed[stage / 8] |= 1 << (stage & 7);
        }
        stage += 1;
    }
    packed
};

mod enemies;
pub mod flight;
mod math;
use flight::Flight;
use math::swept_z;

#[cfg(target_arch = "mos")]
extern "C" {
    fn terminal_step(game: *mut (), bank: *const ());
    fn encounter_step(game: *mut (), bank: *const ());
}

/// Terminal simulation replaces the level-specific enemy tail on the C64.
#[no_mangle]
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.terminal.entry")]
pub unsafe extern "C" fn game_terminal(game: *mut (), bank: *const ()) {
    (*(game as *mut Game)).tick_nonplaying(&*bank.cast::<AssetBank<'_>>());
}

#[derive(Clone, Copy, Default, Debug)]
pub struct InputSample {
    pub bits: u8,
    /// Rising edges accumulated since the previous sample, if any.
    pub pressed: u8,
    /// A 16-bit millisecond clock, wrapping every 65.5 s: every instant the
    /// simulation remembers is kept within two seconds of the present.
    pub ms: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Title,
    Playing,
    Respawning,
    Victory,
    GameOver,
    Attract,
    Transition,
}

/// Latest effect request since presentation last sampled the simulation.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentationEvent {
    None,
    Bomb,
    Reset,
    /// The player took damage: a short shake with a red border flash.
    /// A pending bomb outranks it; a reset replaces either.
    Hit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hud {
    pub mode: Mode,
    pub story_mode: bool,
    pub shield: u8,
    pub bombs: u8,
    pub lives: u8,
    pub charge: u8,
    pub score: u32,
    /// Distance into the current section, 0..=30,000 world units.
    pub distance: u16,
    pub section: u8,
    pub roll_ticks: u8,
    /// Combined remaining boss health, normalized to 0..100.
    pub boss_shield: u8,
    pub boss_kind: u8,
    /// Elapsed victory-reveal ticks, capped at the settled results screen.
    pub victory_ticks: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
pub(crate) struct Actor {
    pos: Vec3,
    origin_x: i16,
    origin_y: i16,
    age: u16,
    kind: u8,
    hp: u8,
    flags: u8,
    approach_speed: u8,
}
impl Actor {
    #[inline(always)]
    fn small_debris(&self) -> bool {
        // Checked encounter pages permit this bit only on orbital ROCKs.
        self.flags & assets::SMALL_DEBRIS != 0
    }
    const EMPTY: Self = Self {
        pos: Vec3 { x: 0, y: 0, z: 0 },
        origin_x: 0,
        origin_y: 0,
        age: 0,
        kind: 0,
        hp: 0,
        flags: 0,
        approach_speed: 0,
    };
}
#[derive(Clone, Copy)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Shot {
    pos: Vec3,
    dx: i8,
    dy: i8,
    damage: u8,
    kind: u8,
}
impl Shot {
    const EMPTY: Self = Self {
        pos: Vec3 { x: 0, y: 0, z: 0 },
        dx: 0,
        dy: 0,
        damage: 0,
        kind: 0,
    };
}
#[derive(Clone, Copy)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Pickup {
    pos: Vec3,
    kind: u8,
}
impl Pickup {
    const EMPTY: Self = Self {
        pos: Vec3 { x: 0, y: 0, z: 0 },
        kind: 0,
    };
}
#[derive(Clone, Copy)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Explosion {
    pos: Vec3,
    ticks: u8,
    color: u8,
}
impl Explosion {
    const EMPTY: Self = Self {
        pos: Vec3 { x: 0, y: 0, z: 0 },
        ticks: 0,
        color: 0,
    };
}

#[derive(Clone, Copy)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Controls {
    bits: u8,
    // Sixteen-bit milliseconds: the windows below are all under two seconds
    // and `sample` keeps every remembered instant within its window.
    now: u16,
    released_at: u16,
    fire_at: u16,
    left_at: u16,
    right_at: u16,
    fire_armed: bool,
    left_armed: bool,
    right_armed: bool,
    cached_charge: u8,
    shot_pending: bool,
    shot_charge: u8,
    bomb_pending: bool,
    roll_pending: i8,
    start_pending: bool,
}
impl Controls {
    const NEW: Self = Self {
        bits: 0,
        now: 0,
        released_at: 0,
        fire_at: 0,
        left_at: 0,
        right_at: 0,
        fire_armed: false,
        left_armed: false,
        right_armed: false,
        cached_charge: 0,
        shot_pending: false,
        shot_charge: 0,
        bomb_pending: false,
        roll_pending: 0,
        start_pending: false,
    };
    // The HUD reads charge outside SIM. Sampling performs the timer arithmetic
    // in SIM and leaves this byte available in every phase.
    #[inline(always)]
    fn charge(&self) -> u8 {
        self.cached_charge
    }
    #[inline(never)]
    fn computed_charge(&self) -> u8 {
        if self.bits & FIRE != 0 {
            return 0;
        }
        let elapsed = self.now.wrapping_sub(self.released_at);
        if elapsed <= 1000 {
            0
        } else if elapsed >= CHARGE_FULL_MS {
            255
        } else {
            ((elapsed - 1000) * 51 / 200) as u8
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn sample(&mut self, input: InputSample) {
        let old = self.bits;
        let pressed = input.pressed | (input.bits & !old);
        self.now = input.ms;
        if old & FIRE != 0 && input.bits & FIRE == 0 {
            self.released_at = input.ms;
        }
        // The clock wraps every 65.5 s. Samples arrive every frame (also
        // while paused), so holding each remembered instant within its window
        // keeps a stale one from ever looking recent again: a saturated
        // charge stays saturated and an expired tap is disarmed.
        if input.ms.wrapping_sub(self.released_at) > CHARGE_FULL_MS {
            self.released_at = input.ms.wrapping_sub(CHARGE_FULL_MS);
        }
        if input.ms.wrapping_sub(self.fire_at) > DOUBLE_TAP_WINDOW_MS {
            self.fire_armed = false;
        }
        if input.ms.wrapping_sub(self.left_at) > DOUBLE_TAP_WINDOW_MS {
            self.left_armed = false;
        }
        if input.ms.wrapping_sub(self.right_at) > DOUBLE_TAP_WINDOW_MS {
            self.right_armed = false;
        }
        // Charge is measured before storing the newly held fire button.
        if pressed & FIRE != 0 {
            self.shot_charge = self.computed_charge();
            self.shot_pending = true;
            self.start_pending = true;
            if self.fire_armed {
                self.bomb_pending = true;
                self.fire_armed = false;
            } else {
                self.fire_at = input.ms;
                self.fire_armed = true;
            }
        }
        if input.bits & (LEFT | RIGHT) == LEFT | RIGHT {
            self.left_armed = false;
            self.right_armed = false;
        } else {
            if pressed & LEFT != 0 {
                if self.left_armed {
                    self.roll_pending = -1;
                    self.left_armed = false;
                } else {
                    self.left_at = input.ms;
                    self.left_armed = true;
                }
            }
            if pressed & RIGHT != 0 {
                if self.right_armed {
                    self.roll_pending = 1;
                    self.right_armed = false;
                } else {
                    self.right_at = input.ms;
                    self.right_armed = true;
                }
            }
        }
        self.bits = input.bits;
        self.cached_charge = self.computed_charge();
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn clear_pending(&mut self) {
        self.shot_pending = false;
        self.bomb_pending = false;
        self.roll_pending = 0;
        self.start_pending = false;
    }
}

/// All simulation state is resident; no heap, filesystem, or REU access occurs
/// here. Asset records and meshes are supplied by the platform's prefetched views.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
#[repr(C)]
pub struct Game {
    controls: Controls,
    mode: Mode,
    presentation_event: PresentationEvent,
    player: Vec3,
    /// Fractional position, velocity and attitude of the ship (flight.rs).
    flight: Flight,
    // One tick is eight world units within this checkpoint. Campaign duration
    // may exceed 65535 ticks without widening the hot simulation arithmetic.
    stage_ticks: u16,
    stage_id: u8,
    stage_info: StageInfo,
    transition_ticks: u16,
    score: u32,
    ticks: u16,
    menu_ticks: u16,
    menu_activity: bool,
    /// Menu track selection survives scene resets; victory selects Galactic.
    menu_music_galactic: bool,
    /// Title selection persists until the program restarts.
    story_mode: bool,
    audio_head: u8,
    audio_len: u8,
    spawn_cursor: u8,
    repeat_spawn_cursor: u8,
    encounter_seed: u8,
    spawn_segment: u8,
    shield: u8,
    bombs: u8,
    // Low nibble: kills toward a bomb; high nibble: pickup opportunities (0..2).
    bomb_kills: u8,
    lives: u8,
    shot_cooldown: u8,
    invulnerability: u8,
    hit_flash_ticks: u8,
    roll_ticks: u8,
    roll_cooldown: u8,
    roll_direction: i8,
    respawn_ticks: u8,
    boss_z: i16,
    boss_left: u8,
    boss_right: u8,
    boss_core: u8,
    boss_started: bool,
    boss_defeated: bool,
    kills: u16,
    actors: [Actor; ACTORS],
    shots: [Shot; SHOTS],
    pickups: [Pickup; 4],
    explosions: [Explosion; 6],
    audio: [AudioEvent; AUDIO_CAPACITY],
    // Two four-bit locks per byte: 0 none, 1..12 actors, 13..15 boss parts.
    missile_targets: [u8; SHOTS / 2],
    missile_rng: u8,
    // Separate feedback state preserves the native 16-byte Actor ABI:
    // 0 ready, 1..10 normal recovery, 11..20 white, 21 awaiting its first frame.
    actor_hit_flash_ticks: [u8; ACTORS],
    boss_hit_flash_ticks: u8,
    damage_feedback_tick: u16,
}

#[cfg(target_arch = "mos")]
const _: () = assert!(core::mem::size_of::<Game>() <= 640);

impl Default for Game {
    fn default() -> Self {
        Self::new()
    }
}
impl Game {
    pub const fn new() -> Self {
        Self {
            controls: Controls::NEW,
            mode: Mode::Title,
            presentation_event: PresentationEvent::None,
            player: Vec3 {
                x: 0,
                y: 0,
                z: PLAYER_Z,
            },
            flight: Flight::ZERO,
            stage_ticks: 0,
            stage_id: 0,
            stage_info: StageInfo::FIRST,
            transition_ticks: 0,
            score: 0,
            ticks: 0,
            menu_ticks: 0,
            menu_activity: false,
            menu_music_galactic: false,
            story_mode: false,
            actors: [Actor::EMPTY; ACTORS],
            shots: [Shot::EMPTY; SHOTS],
            missile_targets: [0; SHOTS / 2],
            missile_rng: MISSILE_SEED,
            actor_hit_flash_ticks: [0; ACTORS],
            boss_hit_flash_ticks: 0,
            damage_feedback_tick: 0,
            pickups: [Pickup::EMPTY; 4],
            explosions: [Explosion::EMPTY; 6],
            audio: [AudioEvent {
                effect: Effect::Fire,
                pan: 0,
            }; AUDIO_CAPACITY],
            audio_head: 0,
            audio_len: 0,
            spawn_cursor: 0,
            repeat_spawn_cursor: 0,
            encounter_seed: 0,
            spawn_segment: 0,
            shield: 100,
            bombs: 3,
            bomb_kills: 0,
            lives: 3,
            shot_cooldown: 0,
            invulnerability: 0,
            hit_flash_ticks: 0,
            roll_ticks: 0,
            roll_cooldown: 0,
            roll_direction: 0,
            respawn_ticks: 0,
            boss_z: 1800,
            boss_left: BOSS_SIDE_HP,
            boss_right: BOSS_SIDE_HP,
            boss_core: BOSS_CORE_HP,
            boss_started: false,
            boss_defeated: false,
            kills: 0,
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    pub fn sample_input(&mut self, input: InputSample) {
        if matches!(self.mode, Mode::Title | Mode::Attract)
            && (input.bits | input.pressed) & (UP | DOWN | LEFT | RIGHT | FIRE) != 0
        {
            // Keep short pulses until the next simulation tick, even if the
            // controller is released again between display-rate samples.
            self.menu_activity = true;
        }
        let before = self.controls.charge();
        self.controls.sample(input);
        if before < 255 && self.controls.charge() == 255
            && matches!(self.mode, Mode::Playing | Mode::Transition) {
            self.sound(Effect::MissileReady, 0);
        }
    }
    /// Keep held controls and charge timing current while paused, discarding
    /// actions and tap history so gestures cannot cross the pause boundary.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    pub fn sample_paused_input(&mut self, bits: u8, ms: u16) {
        self.controls.sample(InputSample {
            bits,
            pressed: 0,
            ms,
        });
        self.controls.clear_pending();
        self.controls.fire_armed = false;
        self.controls.left_armed = false;
        self.controls.right_armed = false;
    }
    /// Host inspection starts at a useful mission location without changing the
    /// production binary. Late encounter records are aged on the following tick.
    #[cfg(feature = "host-tools")]
    pub fn preview_at(section: u8, local_distance: u16) -> Self {
        let mut game = Self::new();
        game.reset_run();
        game.stage_id = section.min(assets::SEGMENT_COUNT - 1);
        game.stage_info = assets::preview_stage_info(game.stage_id);
        game.stage_ticks = local_distance.min(29_999) / 8;
        game.spawn_segment = game.segment_index();
        game.invulnerability = 0;
        if game.stage_info.boss_kind != 0 && local_distance / 8 >= game.boss_start_ticks() {
            game.boss_started = true;
            game.boss_z = 680;
        }
        game
    }
    /// Inspect a scenic bridge without replaying the preceding combat section.
    #[cfg(feature = "host-tools")]
    pub fn preview_transition(section: u8, elapsed_ticks: u16) -> Self {
        let section = section.min(assets::SEGMENT_COUNT - 2);
        let mut game = Self::new();
        game.reset_scene(Mode::Transition, section, SECTION_TICKS);
        game.stage_id = section;
        game.stage_info = assets::preview_stage_info(section);
        if elapsed_ticks >= TRANSITION_TICKS {
            game.mode = Mode::Playing;
            game.stage_id += 1;
            game.stage_ticks = 0;
            game.stage_info = assets::preview_stage_info(game.stage_id);
            game.spawn_segment = game.segment_index();
        } else {
            game.transition_ticks = elapsed_ticks;
        }
        game
    }
    /// Inspect the final carrier scene, medal reveal, and settled victory sway.
    #[cfg(feature = "host-tools")]
    pub fn preview_victory(elapsed_ticks: u16) -> Self {
        let mut game = Self::new();
        game.reset_scene(Mode::Victory, assets::SEGMENT_COUNT - 1, SECTION_TICKS);
        game.stage_id = assets::SEGMENT_COUNT - 1;
        game.stage_info = assets::preview_stage_info(game.stage_id);
        game.menu_ticks = elapsed_ticks.min(crate::victory::REVEAL_TICKS);
        game.ticks = elapsed_ticks;
        game.score = 24_000;
        game.shield = 76;
        game.bombs = 1;
        game.lives = 2;
        game.boss_started = true;
        game.boss_defeated = true;
        game.boss_z = 680;
        game.boss_left = 0;
        game.boss_right = 0;
        game.boss_core = 0;
        game
    }
    /// Select terminal simulation independently of the active scene/model bank.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim_aux")]
    pub fn simulation_profile(&self) -> u8 {
        if self.mode == Mode::Playing
            || (self.mode == Mode::Attract
                && self.menu_ticks < ATTRACT_TICKS - 1
                && !self.menu_activity
                && self.controls.bits == 0)
        {
            self.stage_info.simulation_profile()
        } else {
            7
        }
    }
    #[inline(always)]
    pub fn mode(&self) -> Mode {
        self.mode
    }
    #[inline(always)]
    pub fn victory_ticks(&self) -> u16 {
        if self.mode == Mode::Victory {
            self.menu_ticks
        } else {
            0
        }
    }
    #[inline(always)]
    pub fn animation_ticks(&self) -> u16 {
        self.ticks
    }
    #[inline(never)]
    pub fn segment_index(&self) -> u8 {
        self.stage_id
    }
    #[inline(always)]
    pub fn stage_info(&self) -> StageInfo {
        self.stage_info
    }
    /// One authored flat plane per enabled level, expressed in camera space.
    /// Orbital crossings and the ascent move their scenery away from this plane.
    #[inline(always)]
    pub fn shadow_ground_y(&self) -> Option<i16> {
        if SHADOW_LEVELS[(self.stage_id / 8) as usize] & (1 << (self.stage_id & 7)) == 0
            || (self.mode == Mode::Transition
                && (self.stage_info.bridge_profile == 3 || self.stage_info.theme >= 10))
            || (self.mode == Mode::Victory && self.menu_ticks >= crate::victory::FADE_TICKS)
        {
            None
        } else {
            Some(-105 - scene_camera_y(self.mode, self.player.y, self.ticks))
        }
    }
    /// Black separates shadows from the Space Carrier's gray flight deck.
    #[inline(always)]
    pub fn shadow_color(&self) -> u8 {
        if self.stage_id == 16 {
            0
        } else {
            crate::shadow::SHADOW_COLOR
        }
    }
    /// Bind only a validated page belonging to the current checkpoint. Native
    /// callers do this before selecting the model bank and simulation image.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    pub fn bind_stage(&mut self, segment: &Segment<'_>) {
        if segment.index() == self.stage_id {
            self.stage_info = segment.stage_info();
        }
    }
    /// Outgoing section and elapsed ticks; the incoming checkpoint opens on exit.
    #[inline(never)]
    pub fn transition_progress(&self) -> Option<(u8, u16)> {
        if self.mode == Mode::Transition {
            Some((self.segment_index(), self.transition_ticks))
        } else {
            None
        }
    }
    /// Local scenery clock. The scenery overlay supplies the repeating world
    /// phase, avoiding an overflowing absolute campaign clock.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
    pub fn scenery_ticks(&self) -> u16 {
        self.stage_ticks + self.transition_ticks
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
    pub fn scenery_camera_y(&self) -> i16 {
        scene_camera_y(self.mode, self.player.y, self.ticks)
    }
    /// Sky color follows the active mission section in every camera mode.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
    pub fn sky_color(&self) -> u8 {
        let color = crate::scenery::sky_color(
            self.stage_info.theme,
            self.transition_progress()
                .map(|(_, elapsed)| (self.stage_info.next_theme, elapsed)),
        );
        #[cfg(not(target_arch = "mos"))]
        if self.mode == Mode::Victory {
            return crate::victory::fade_color(color, self.menu_ticks);
        }
        color
    }
    #[inline(never)]
    pub fn hud(&self) -> Hud {
        Hud {
            mode: self.mode,
            story_mode: self.story_mode,
            shield: self.shield,
            bombs: self.bombs,
            lives: self.lives,
            charge: self.controls.charge(),
            score: self.score,
            distance: self.stage_ticks * 8,
            section: self.segment_index(),
            roll_ticks: self.roll_ticks,
            boss_shield: if self.boss_started {
                (self.boss_left + self.boss_right + self.boss_core + 1) >> 1
            } else {
                0
            },
            boss_kind: self.stage_info.boss_kind,
            victory_ticks: self.victory_ticks(),
        }
    }
    /// Switch menu tracks without affecting the scene or its inactivity timer.
    /// Inlined so the native PIXELS keyboard scan needs no other overlay.
    #[inline(always)]
    pub fn toggle_menu_music(&mut self) {
        if matches!(self.mode, Mode::Title | Mode::Attract) {
            self.menu_music_galactic = !self.menu_music_galactic;
        }
    }
    /// Change the next run's life allowance from the title screen only.
    /// Inlined into the native PIXELS keyboard scan without an overlay call.
    #[inline(always)]
    pub fn toggle_story_mode(&mut self) {
        if self.mode == Mode::Title {
            self.story_mode = !self.story_mode;
            self.menu_activity = true;
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    pub fn music_state(&self) -> MusicState {
        match self.mode {
            // The title and complete attract tour share one uninterrupted track.
            Mode::Title | Mode::Attract => {
                if self.menu_music_galactic {
                    MusicState::Victory
                } else {
                    MusicState::Title
                }
            }
            // Keep the final battle music through the fade, then start the
            // reward track with the first visible medal frame.
            Mode::Victory if self.menu_ticks >= crate::victory::FADE_TICKS => MusicState::Victory,
            Mode::GameOver => MusicState::Defeat,
            Mode::Transition if self.stage_info.bridge_profile == 3 => {
                MusicState::from_cue(19).unwrap()
            }
            _ => MusicState::from_cue(if self.boss_started {
                match self.stage_info.boss_kind {
                    4 => 12,
                    5 => 20,
                    kind => 9 + kind,
                }
            } else {
                self.stage_info.music_cue
            })
            .unwrap_or(MusicState::Lowlands),
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    pub fn pop_audio(&mut self) -> Option<AudioEvent> {
        if self.audio_len == 0 {
            return None;
        }
        let head = self.audio_head & (AUDIO_CAPACITY as u8 - 1);
        let e = self.audio[head as usize];
        self.audio_head = (head + 1) & (AUDIO_CAPACITY as u8 - 1);
        self.audio_len -= 1;
        Some(e)
    }
    /// Drain the latest request after simulation catch-up. A later reset cancels
    /// an earlier bomb, while a bomb after a reset starts a fresh effect.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    pub fn take_presentation_event(&mut self) -> PresentationEvent {
        let event = self.presentation_event;
        self.presentation_event = PresentationEvent::None;
        event
    }
    // Resident queue writer: shared by SIM and its encounter/terminal tails,
    // leaving room for damage feedback in the composed simulation overlay.
    #[inline(never)]
    fn sound(&mut self, effect: Effect, pan: i8) {
        // Keep the live previews audible as music only.
        if self.mode == Mode::Attract {
            return;
        }
        let event = AudioEvent { effect, pan };
        crate::audio::queue_events(&mut self.audio, self.audio_head, &mut self.audio_len, event);
    }
    /// Sample the source's screen side at onset; the IRQ keeps its assigned chip.
    #[inline(always)]
    fn sound_at(&mut self, effect: Effect, x: i16, z: i16) {
        self.sound(effect, crate::audio::spatial_pan(x, z));
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn sound_player(&mut self, effect: Effect) {
        // The ship and its muzzle (Z=180/198) share the same integer-X
        // center band, [-9,9]. Player movement changes X/Y only.
        let x = self.player.x;
        let pan = if x.wrapping_add(9) as u16 <= 18 { 0 }
                  else if x < 0 { -100 } else { 100 };
        self.sound(effect, pan);
    }
    #[inline(always)]
    fn reset_run(&mut self) {
        // Launch timing varies on hardware; keep the seed fixed through deaths
        // and stage changes so checkpoint retries retain their enemy layout.
        let clock = self.controls.now;
        // An odd increment cannot be zero, even when the clock repeats.
        let seed = self
            .encounter_seed
            .wrapping_add((clock as u8 ^ (clock >> 8) as u8) | 1);
        self.reset_scene(Mode::Playing, 0, 0);
        self.encounter_seed = seed;
        self.invulnerability = 50;
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.terminal")]
    fn reset_scene(&mut self, mode: Mode, stage_id: u8, stage_ticks: u16) {
        let bits = self.controls.bits;
        let now = self.controls.now;
        let menu_music_galactic = self.menu_music_galactic;
        let story_mode = self.story_mode;
        let encounter_seed = self.encounter_seed;
        self.reset_in_place();
        self.menu_music_galactic = menu_music_galactic;
        self.story_mode = story_mode;
        self.lives = if story_mode { 99 } else { 3 };
        self.encounter_seed = encounter_seed;
        self.presentation_event = PresentationEvent::Reset;
        self.mode = mode;
        self.stage_ticks = stage_ticks;
        self.stage_id = stage_id;
        #[cfg(any(test, feature = "host-tools"))]
        {
            self.stage_info = assets::preview_stage_info(self.stage_id);
        }
        self.spawn_segment = self.segment_index();
        self.controls.bits = bits;
        self.controls.now = now;
        self.controls.released_at = now;
    }
    #[inline(always)]
    fn reset_in_place(&mut self) {
        // A bulk clear is much smaller on the 6510 than constructing and copying
        // the complete value. Every field is an integer, bool, or fieldless enum.
        // Restore every enum using raw writes before accessing the value again;
        // this does not depend on an enum's unspecified physical representation.
        unsafe {
            let game = self as *mut Self;
            core::ptr::write_bytes(game.cast::<u8>(), 0, core::mem::size_of::<Self>());
            core::ptr::addr_of_mut!((*game).mode).write(Mode::Title);
            core::ptr::addr_of_mut!((*game).presentation_event).write(PresentationEvent::None);
            let audio = core::ptr::addr_of_mut!((*game).audio).cast::<AudioEvent>();
            for index in 0..AUDIO_CAPACITY {
                audio.add(index).write(AudioEvent {
                    effect: Effect::Fire,
                    pan: 0,
                });
            }
        }
        self.player.z = PLAYER_Z;
        self.shield = 100;
        self.bombs = 3;
        self.lives = 3;
        self.boss_z = 1800;
        self.boss_left = BOSS_SIDE_HP;
        self.boss_right = BOSS_SIDE_HP;
        self.boss_core = BOSS_CORE_HP;
        self.stage_info = StageInfo::FIRST;
        self.missile_rng = MISSILE_SEED;
    }
    /// Prepare damage feedback once after simulation catch-up, before drawing
    /// a frame. Render methods remain read-only so repeated captures agree.
    /// Each phase gets ten 20 ms ticks from its first rendered frame; catch-up
    /// may extend a phase, but cannot skip white or the normal-color recovery.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
    pub fn prepare_damage_feedback_frame(&mut self) {
        let elapsed = self
            .ticks
            .wrapping_sub(self.damage_feedback_tick)
            .min(ENEMY_HIT_FLASH_TICKS as u16) as u8;
        self.damage_feedback_tick = self.ticks;
        for flash in &mut self.actor_hit_flash_ticks {
            advance_enemy_hit_flash(flash, elapsed);
        }
        advance_enemy_hit_flash(&mut self.boss_hit_flash_ticks, elapsed);
    }
    #[inline(never)]
    fn damage_actor(&mut self, index: usize, damage: u8) -> u8 {
        let previous = self.actors[index].hp;
        let hp = previous.saturating_sub(damage);
        self.actors[index].hp = hp;
        if hp < previous && self.actor_hit_flash_ticks[index] == 0 {
            self.actor_hit_flash_ticks[index] = ENEMY_HIT_FLASH_PENDING;
        }
        hp
    }
    /// One 20 ms simulation step. Feed every captured input sample first.
    /// `bank` must belong to `segment_index()` before this step. If the step
    /// changes stages, select the new bank and call `bind_stage` with its checked
    /// segment before querying stage metadata, rendering, or taking another step.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    pub fn tick(&mut self, segment: &Segment<'_>, bank: &AssetBank<'_>) {
        self.bind_stage(segment);
        self.ticks = self.ticks.wrapping_add(1);
        if self.simulation_profile() == 7 {
            #[cfg(target_arch = "mos")]
            unsafe {
                terminal_step(
                    (self as *mut Self).cast(),
                    (bank as *const AssetBank<'_>).cast(),
                );
            }
            #[cfg(not(target_arch = "mos"))]
            self.tick_nonplaying(bank);
            return;
        }
        if self.mode == Mode::Playing {
            let boundary = SECTION_TICKS;
            let next = (self.stage_ticks + 1).min(boundary);
            if next == boundary
                && self.stage_id + 1 < assets::SEGMENT_COUNT
                && (self.stage_info.boss_kind == 0 || self.boss_defeated)
            {
                // Retire outgoing hazards before handling this tick's live
                // controls. Player projectiles and gestures remain intact.
                self.stage_ticks = next;
                self.mode = Mode::Transition;
                self.transition_ticks = 0;
                if self.stage_info.boss_kind != 0 {
                    let cap = if self.story_mode { 99 } else { 3 };
                    // Run initialization and this cap keep lives at most 99.
                    self.lives = (self.lives + 1).min(cap);
                }
                self.actors = [Actor::EMPTY; ACTORS];
                self.actor_hit_flash_ticks = [0; ACTORS];
                self.boss_hit_flash_ticks = 0;
                self.damage_feedback_tick = self.ticks;
                self.missile_targets = [0; SHOTS / 2];
                self.pickups = [Pickup::EMPTY; 4];
                for shot in &mut self.shots {
                    if shot.kind == 2 {
                        *shot = Shot::EMPTY;
                    }
                }
                self.update_player_controls(bank);
                self.controls.clear_pending();
                self.update_shots(bank);
                self.update_explosions();
                return;
            }
        } else {
            self.menu_ticks += 1;
        }
        if self.mode == Mode::Playing {
            self.update_player_controls(bank);
        }
        self.controls.clear_pending();
        self.stage_ticks = (self.stage_ticks + 1).min(SECTION_TICKS);
        let section = self.segment_index();
        if self.spawn_segment != section {
            self.spawn_segment = section;
            self.spawn_cursor = 0;
            self.repeat_spawn_cursor = 0;
        }
        if segment.index() == section && bank.level() == section {
            self.spawn_encounters(segment, bank);
        }
        #[cfg(target_arch = "mos")]
        unsafe {
            encounter_step(
                (self as *mut Self).cast(),
                (bank as *const AssetBank<'_>).cast(),
            );
        }
        #[cfg(not(target_arch = "mos"))]
        {
            self.update_actors(bank);
            self.update_boss();
        }
        self.update_shots(bank);
        self.update_pickups();
        self.update_explosions();
        if self.mode == Mode::Playing
            && self.stage_id == assets::SEGMENT_COUNT - 1
            && self.stage_ticks == SECTION_TICKS
            && self.boss_defeated
        {
            self.mode = Mode::Victory;
            self.menu_music_galactic = true;
            self.menu_ticks = 0;
            self.shots = [Shot::EMPTY; SHOTS];
            self.missile_targets = [0; SHOTS / 2];
            self.sound(Effect::Pickup, 0);
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.terminal")]
    fn tick_nonplaying(&mut self, bank: &AssetBank<'_>) {
        match self.mode {
            Mode::Title => {
                if self.controls.start_pending {
                    self.reset_run();
                } else if self.menu_activity || self.controls.bits != 0 {
                    self.menu_ticks = 0;
                } else {
                    self.menu_ticks += 1;
                    if self.menu_ticks == ATTRACT_TICKS {
                        self.reset_scene(Mode::Attract, 0, 0);
                    }
                }
                self.menu_activity = false;
                self.controls.clear_pending();
                return;
            }
            Mode::Attract => {
                if self.menu_activity || self.controls.bits != 0 {
                    // Consume the wake-up gesture. A held button remains held
                    // on the title, so only a fresh fire edge starts a run.
                    self.reset_scene(Mode::Title, 0, 0);
                    return;
                }
                self.menu_ticks += 1;
                if self.menu_ticks == ATTRACT_TICKS {
                    let next = match self.stage_id {
                        0 => 1,
                        1 => 6,
                        6 => 10,
                        10 => 12,
                        12 => 14,
                        14 => 16,
                        _ => assets::SEGMENT_COUNT,
                    };
                    if next < assets::SEGMENT_COUNT {
                        self.reset_scene(Mode::Attract, next, 0);
                    } else {
                        self.reset_scene(Mode::Title, 0, 0);
                    }
                    return;
                }
                self.stage_ticks += 1;
            }
            Mode::Victory | Mode::GameOver => {
                self.tick_terminal();
            }
            Mode::Respawning => {
                self.update_explosions();
                self.respawn_ticks = self.respawn_ticks.saturating_sub(1);
                if self.respawn_ticks == 0 {
                    self.respawn();
                }
                self.controls.clear_pending();
                return;
            }
            Mode::Transition => {
                self.update_player_controls(bank);
                self.controls.clear_pending();
                self.update_shots(bank);
                self.update_explosions();
                self.transition_ticks += 1;
                if self.transition_ticks == TRANSITION_TICKS {
                    self.transition_ticks = 0;
                    self.mode = Mode::Playing;
                    self.stage_id += 1;
                    self.stage_ticks = 0;
                    #[cfg(any(test, feature = "host-tools"))]
                    {
                        self.stage_info = assets::preview_stage_info(self.stage_id);
                    }
                    self.spawn_segment = self.segment_index();
                    self.spawn_cursor = 0;
                    self.repeat_spawn_cursor = 0;
                    self.boss_started = false;
                    self.boss_hit_flash_ticks = 0;
                    self.damage_feedback_tick = self.ticks;
                    self.boss_defeated = false;
                    self.boss_z = 1800;
                    self.boss_left = BOSS_SIDE_HP;
                    self.boss_right = BOSS_SIDE_HP;
                    self.boss_core = BOSS_CORE_HP;
                }
                return;
            }
            Mode::Playing => unreachable!(),
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.terminal")]
    fn tick_terminal(&mut self) {
        if self.mode == Mode::Victory && self.menu_ticks < crate::victory::REVEAL_TICKS {
            // Consume every reveal gesture, including the tick that reaches the
            // result screen. A held fire button cannot skip or restart it.
            self.menu_ticks += 1;
        } else if self.controls.start_pending {
            if self.mode == Mode::Victory {
                self.reset_scene(Mode::Title, 0, 0);
            } else {
                self.reset_run();
            }
        }
        self.controls.clear_pending();
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn update_player_controls(&mut self, bank: &AssetBank<'_>) {
        self.invulnerability = self.invulnerability.saturating_sub(1);
        self.hit_flash_ticks = self.hit_flash_ticks.saturating_sub(1);
        self.roll_ticks = self.roll_ticks.saturating_sub(1);
        self.roll_cooldown = self.roll_cooldown.saturating_sub(1);
        self.shot_cooldown = self.shot_cooldown.saturating_sub(1);
        if self.controls.roll_pending != 0 && self.roll_cooldown == 0 {
            self.roll_direction = self.controls.roll_pending;
            self.roll_ticks = 25;
            self.roll_cooldown = 50;
            self.sound_player(Effect::Roll);
        }
        self.move_player();
        if self.controls.shot_pending {
            self.fire_player(self.controls.shot_charge);
            self.shot_cooldown = 5;
        } else if self.controls.bits & FIRE != 0 && self.shot_cooldown == 0 {
            self.fire_player(0);
            self.shot_cooldown = 5;
        }
        if self.controls.bomb_pending && self.bombs > 0 {
            self.bomb(bank);
        }
    }
    /// The assisted flight model (flight.rs): the stick commands a velocity,
    /// the barrel roll adds a sideways push while it spins, and the hull banks
    /// and pitches with the manoeuvre. Resident: called from the SIM image.
    #[inline(never)]
    fn move_player(&mut self) {
        let push = if self.roll_ticks > 0 {
            self.roll_direction
        } else {
            0
        };
        flight::step(&mut self.player, &mut self.flight, self.controls.bits, push);
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn fire_player(&mut self, charge: u8) {
        let damage = 1 + (charge as u16 * 3 / 255) as u8;
        self.add_shot(Shot {
            pos: Vec3 {
                x: self.player.x,
                y: self.player.y,
                z: PLAYER_Z + 18,
            },
            dx: 0,
            dy: 0,
            damage,
            kind: if charge == 255 { 3 } else { 1 },
        });
        self.sound_player(
            if charge == 255 {
                Effect::MissileLaunch
            } else {
                Effect::Fire
            },
        );
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn add_shot(&mut self, shot: Shot) {
        if let Some(slot) = self.shots.iter().position(|s| s.kind == 0) {
            self.shots[slot] = shot;
            self.set_missile_lock(slot, 0);
        } else if shot.kind == 1 || shot.kind == 3 {
            if let Some(slot) = self.shots.iter().position(|s| s.kind == 2) {
                self.shots[slot] = shot;
                self.set_missile_lock(slot, 0);
            } else if shot.kind == 3 {
                // A fully charged shot must not disappear in a saturated pool.
                // Prefer replacing an ordinary laser nearest its range limit;
                // only an artificial all-missile pool replaces another missile.
                let kind = if self.shots.iter().any(|s| s.kind == 1) {
                    1
                } else {
                    3
                };
                let mut farthest = 0;
                let mut z = i16::MIN;
                for (i, s) in self.shots.iter().enumerate() {
                    if s.kind == kind && s.pos.z > z {
                        farthest = i;
                        z = s.pos.z;
                    }
                }
                self.shots[farthest] = shot;
                self.set_missile_lock(farthest, 0);
            }
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn enemy_shot(&mut self, position: Vec3) {
        self.enemy_attack(position, 1);
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim_aux")]
    fn enemy_attack(&mut self, position: Vec3, pattern: u8) {
        let dz = (position.z - PLAYER_Z).max(32);
        let dx = enemy_velocity(self.player.x, position.x, dz);
        let dy = enemy_velocity(self.player.y, position.y, dz);
        // Enemy shots only use empty slots. Fill those slots in one pass,
        // retaining center/left/right priority when a fan only partly fits.
        let mut remaining = if pattern == 2 { 3u8 } else { 1 };
        let mut spread = 0i8;
        for index in 0u8..SHOTS as u8 {
            if self.shots[index as usize].kind != 0 {
                continue;
            }
            self.set_missile_lock(index as usize, 0);
            self.shots[index as usize] = Shot {
                pos: position,
                dx: (dx + spread).clamp(-8, 8),
                dy,
                damage: 6,
                kind: 2,
            };
            remaining -= 1;
            if remaining == 0 {
                break;
            }
            spread = if remaining == 2 { -2 } else { 2 };
        }
        self.sound_at(Effect::EnemyFire, position.x, position.z);
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn spawn_encounters(&mut self, segment: &Segment<'_>, bank: &AssetBank<'_>) {
        let local = self.stage_ticks * 8;
        loop {
            let original = segment.spawn_distance(self.spawn_cursor, 0);
            let repeat = segment.spawn_distance(self.repeat_spawn_cursor, ENEMY_REPEAT_DISTANCE);
            // Original records win ties, including a delayed segment handoff.
            let repeated = repeat < original;
            let distance = if repeated { repeat } else { original };
            if distance > local {
                break;
            }
            let token = if repeated {
                let index = self.repeat_spawn_cursor;
                self.repeat_spawn_cursor += 1;
                index | 0x80
            } else {
                let index = self.spawn_cursor;
                self.spawn_cursor += 1;
                index
            };
            let spawn = segment.spawn(token & 0x7f);
            if self.stage_info.boss_kind == 0 || !self.boss_defeated {
                let enemy = matches!(spawn.kind, assets::FIGHTER | assets::DIVER | assets::TURRET);
                if enemy || !repeated {
                    self.spawn_encounter(spawn, local - distance, bank, token);
                }
            }
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn spawn_encounter(&mut self, mut spawn: Spawn, late: u16, bank: &AssetBank<'_>, token: u8) {
        let mut z = 1800;
        if matches!(spawn.kind, assets::FIGHTER | assets::DIVER | assets::TURRET) {
            // A local stream makes layouts independent of kills, rendering,
            // missile targeting, or encounters skipped during a late handoff.
            let mut random =
                self.encounter_seed ^ self.stage_id.rotate_left(3) ^ token.rotate_left(1);
            random = placement_random(random);
            // Flying formations include close side passes around the lens.
            // Ground units retain their mounted positions and original limits.
            let lateral_limit = if spawn.kind == assets::TURRET { 115 } else { 180 };
            spawn.x = spawn
                .x
                .saturating_add(i16::from(random & 31) - 16)
                .clamp(-lateral_limit, lateral_limit);
            random = placement_random(random);
            if spawn.kind != assets::TURRET {
                spawn.y = spawn
                    .y
                    .saturating_add(i16::from(random & 31) - 16)
                    .clamp(-55, 90);
            }
            random = placement_random(random);
            z += i16::from(random & 127) - 64;
        }
        self.spawn_actor_at(spawn, late, bank, z);
    }
    #[cfg(test)]
    fn spawn_actor(&mut self, spawn: Spawn, late: u16, bank: &AssetBank<'_>) {
        self.spawn_actor_at(spawn, late, bank, 1800);
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn spawn_actor_at(&mut self, spawn: Spawn, late: u16, bank: &AssetBank<'_>, z: i16) {
        // Handles host inspection and a delayed segment handoff without replaying
        // an entire section's expired encounters at once.
        if late >= 1600 {
            return;
        }
        let z = z - late as i16;
        if spawn.kind == assets::CRYSTAL {
            self.add_pickup(
                Vec3 {
                    x: spawn.x,
                    y: spawn.y,
                    z,
                },
                if spawn.flags & 1 != 0 { 2 } else { 1 },
            );
            return;
        }
        if !matches!(
            spawn.kind,
            assets::FIGHTER | assets::DIVER | assets::TURRET | assets::BUILDING | assets::ROCK
        ) {
            return;
        }
        let spec = bank.enemy(spawn.kind);
        let obstacle = matches!(spawn.kind, assets::BUILDING | assets::ROCK);
        if !obstacle && spec.hp == 0 {
            return;
        }
        let hp = if spawn.flags & assets::SMALL_DEBRIS != 0 {
            2
        } else if obstacle {
            255
        } else {
            spec.hp
        };
        if let Some(slot) = self.actors.iter().position(|a| a.kind == 0) {
            // A reused slot is a new target, never an inherited lock.
            let target = slot as u8 + 1;
            for locks in &mut self.missile_targets {
                if *locks & 15 == target {
                    *locks &= 0xf0;
                }
                if *locks >> 4 == target {
                    *locks &= 0x0f;
                }
            }
            self.actors[slot] = Actor {
                pos: Vec3 {
                    x: spawn.x,
                    y: spawn.y,
                    z,
                },
                origin_x: spawn.x,
                origin_y: spawn.y,
                age: late / 8,
                kind: spawn.kind,
                hp,
                flags: spawn.flags,
                approach_speed: if obstacle { 8 } else { spec.speed },
            };
            self.actor_hit_flash_ticks[slot] = 0;
        }
    }
    #[cfg(not(target_arch = "mos"))]
    fn update_actors(&mut self, bank: &AssetBank<'_>) {
        let step = match self.stage_info.enemy_profile {
            0 => enemies::enemy_lowlands,
            1 => enemies::enemy_canyon,
            2 => enemies::enemy_fortress,
            _ => enemies::enemy_carrier,
        };
        self.update_actors_with(bank, step);
    }
    #[inline(always)]
    fn update_actors_with(
        &mut self,
        bank: &AssetBank<'_>,
        step: unsafe extern "C" fn(*mut Actor, *const Vec3, u8) -> u8,
    ) {
        for i in 0..ACTORS {
            let mut a = self.actors[i];
            if a.kind == 0 {
                continue;
            }
            a.age = a.age.wrapping_add(1);
            let obstacle = matches!(a.kind, assets::BUILDING | assets::ROCK);
            let old_z = a.pos.z;
            let attack = if obstacle {
                a.pos.z -= SPEED;
                0
            } else {
                unsafe { step(&mut a, &self.player, i as u8) }
            };
            a.approach_speed = (old_z - a.pos.z).clamp(0, 31) as u8;
            if attack != 0 {
                self.enemy_attack(actor_center_sim(&a, bank), attack);
            }
            let radius = if a.small_debris() {
                18
            } else if obstacle {
                36
            } else {
                bank.enemy(a.kind).ram_radius as i16
            };
            if self.mode == Mode::Playing
                && (a.pos.z - PLAYER_Z).abs()
                    < if a.small_debris() {
                        16
                    } else if obstacle {
                        32
                    } else {
                        ENEMY_RAM_DEPTH
                    }
                && close_xy(actor_center_sim(&a, bank), self.player, radius)
            {
                a.kind = 0;
                self.damage_player(15);
                self.explode(a.pos, 8);
            } else if a.pos.z < ACTOR_RETIRE_Z {
                a.kind = 0;
            }
            self.actors[i] = a;
        }
    }
    #[cfg(not(target_arch = "mos"))]
    fn update_boss(&mut self) {
        self.update_boss_for(match self.stage_info.boss_kind {
            4 => 3,
            5 => 2,
            kind => kind,
        });
    }
    #[inline(always)]
    fn boss_start_ticks(&self) -> u16 {
        if self.stage_info.boss_kind >= 4 {
            SPACE_BOSS_START_TICKS
        } else {
            BOSS_START_TICKS
        }
    }
    #[inline(always)]
    fn update_boss_for(&mut self, kind: u8) {
        if kind == 0 || self.stage_ticks < self.boss_start_ticks() || self.boss_defeated {
            return;
        }
        if !self.boss_started { self.sound(Effect::BossArrival, 0); }
        self.boss_started = true;
        self.boss_z = (self.boss_z - 8).max(680);
        if self.boss_z > 900 {
            return;
        }
        let exposed = self.boss_left == 0 && self.boss_right == 0;
        let rate = if kind == 1 {
            50
        } else if kind == 2 {
            45
        } else if exposed {
            25
        } else {
            40
        };
        let pulse = enemies::boss_pulse(self.ticks, rate);
        if pulse == 0 {
            return;
        }
        let side = pulse == 2;
        if exposed {
            let x = 14 * assets::BOSS_SCALE;
            self.enemy_shot(Vec3 {
                x: if side { x } else { -x },
                y: 0,
                z: self.boss_z + BOSS_CORE_Z,
            });
            if kind == 1 {
                self.enemy_shot(Vec3 {
                    x: if side { -x } else { x },
                    y: 0,
                    z: self.boss_z + BOSS_CORE_Z,
                });
            }
        } else {
            for right in [false, true] {
                let alive = if right {
                    self.boss_right
                } else {
                    self.boss_left
                } != 0;
                if alive && (kind == 3 || right == side) {
                    self.enemy_attack(
                        Vec3 {
                            x: if right { BOSS_TURRET_X } else { -BOSS_TURRET_X },
                            y: BOSS_TURRET_Y + 18 * assets::BOSS_SCALE,
                            z: self.boss_z + BOSS_TURRET_Z - 28 * assets::BOSS_SCALE,
                        },
                        if kind == 2 { 2 } else { 1 },
                    );
                }
            }
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn update_shots(&mut self, bank: &AssetBank<'_>) {
        for i in 0..SHOTS {
            let mut s = self.shots[i];
            if s.kind == 0 {
                continue;
            }
            if s.kind == 3 {
                let mut target = self.missile_lock(i);
                self.steer_missile(&mut s, &mut target, bank);
                self.set_missile_lock(i, target);
            }
            let old_z = s.pos.z;
            s.pos.x += s.dx as i16;
            s.pos.y += s.dy as i16;
            s.pos.z += match s.kind {
                1 => 48,
                3 => 32,
                _ => -18,
            };
            if s.pos.z > 2400
                || s.pos.z < crate::render::NEAR
                || s.pos.x.abs() > 1000
                || s.pos.y.abs() > 600
            {
                s.kind = 0;
            }
            if s.kind == 1 || s.kind == 3 {
                if let Some(a) = self.shot_target(old_z, &s.pos, bank) {
                    let missile = s.kind == 3;
                    s.kind = 0;
                    let destroyed = if self.actors[a].hp == 255 {
                        self.explode(s.pos, 15);
                        false
                    } else {
                        self.damage_actor(a, s.damage) == 0
                    };
                    if destroyed {
                        self.kill_actor(a, bank);
                    } else {
                        let pos = self.actors[a].pos;
                        self.sound_at(if missile { Effect::MissileImpact } else { Effect::EnemyHit }, pos.x, pos.z);
                    }
                }
                if s.kind != 0 && self.boss_started && !self.boss_defeated {
                    let mut hit = false;
                    let mut part_destroyed = false;
                    let turret_depth =
                        swept_z(old_z, s.pos.z, self.boss_z + BOSS_TURRET_Z, BOSS_HIT_DEPTH);
                    // Turret X ranges [-176,-84] and [84,176] are disjoint,
                    // so the shot's sign selects the only side it can hit.
                    let (turret_x, turret_hp) = if s.pos.x < 0 {
                        (-BOSS_TURRET_X, &mut self.boss_left)
                    } else {
                        (BOSS_TURRET_X, &mut self.boss_right)
                    };
                    if *turret_hp > 0
                        && turret_depth
                        && close_xy(
                            s.pos,
                            Vec3 {
                                x: turret_x,
                                y: BOSS_TURRET_Y,
                                z: 0,
                            },
                            BOSS_TURRET_RADIUS,
                        )
                    {
                        damage_boss_part(turret_hp, s.damage, &mut self.boss_hit_flash_ticks);
                        part_destroyed = *turret_hp == 0;
                        hit = true;
                    } else if swept_z(old_z, s.pos.z, self.boss_z + BOSS_CORE_Z, BOSS_HIT_DEPTH)
                        && close_xy(s.pos, Vec3 { x: 0, y: 0, z: 0 }, BOSS_CORE_RADIUS)
                    {
                        if self.boss_left == 0 && self.boss_right == 0 {
                            damage_boss_part(
                                &mut self.boss_core,
                                s.damage,
                                &mut self.boss_hit_flash_ticks,
                            );
                        }
                        hit = true;
                    }
                    if hit {
                        let missile = s.kind == 3;
                        s.kind = 0;
                        self.check_boss_death();
                        if !self.boss_defeated {
                            self.sound_at(if part_destroyed { Effect::EnemyExplosion }
                                else if missile { Effect::MissileImpact } else { Effect::BossHit }, s.pos.x, s.pos.z);
                        }
                    }
                }
            } else if self.mode == Mode::Playing
                && s.kind == 2
                && swept_z(old_z, s.pos.z, PLAYER_Z, 10)
                && close_xy(s.pos, self.player, 12)
            {
                s.kind = 0;
                if self.roll_ticks == 0 {
                    self.damage_player(s.damage);
                } else {
                    self.explode(s.pos, 3);
                }
            }
            self.shots[i] = s;
            if s.kind == 0 {
                self.set_missile_lock(i, 0);
            }
        }
    }
    /// The first actor slot, in slot order, that a player shot or missile at
    /// `pos` hits after moving from depth `old_z`.
    #[inline(always)]
    fn shot_target(&self, old_z: i16, pos: &Vec3, bank: &AssetBank<'_>) -> Option<usize> {
        #[cfg(target_arch = "mos")]
        {
            // The native body is `fast_shot_scan` (target-c64/asm/shots.S),
            // compared against a transcription of the loop below by
            // tools/test-shots-kernel.py.
            extern "C" {
                fn fast_shot_scan(
                    old_z: i16,
                    pos: *const Vec3,
                    actors: *const Actor,
                    specs: *const u8,
                    enemy_count: u8,
                ) -> u8;
            }
            // SAFETY: the kernel reads the position, the twelve actor slots
            // and the bank's descriptors through these pointers and writes
            // nothing.
            let slot = unsafe {
                fast_shot_scan(
                    old_z,
                    pos,
                    self.actors.as_ptr(),
                    bank.enemy_specs(),
                    bank.enemy_count(),
                )
            };
            if (slot as usize) < ACTORS {
                Some(slot as usize)
            } else {
                None
            }
        }
        #[cfg(not(target_arch = "mos"))]
        {
            for a in 0..ACTORS {
                let target = &self.actors[a];
                if target.kind == 0 {
                    continue;
                }
                let radius = if target.small_debris() {
                    18
                } else if matches!(target.kind, assets::ROCK | assets::BUILDING) {
                    35
                } else {
                    bank.enemy(target.kind).shot_radius as i16
                };
                if swept_z(old_z, pos.z, target.pos.z, radius)
                    && close_xy(*pos, actor_center_sim(target, bank), radius)
                {
                    return Some(a);
                }
            }
            None
        }
    }
    #[inline(always)]
    fn missile_lock(&self, index: usize) -> u8 {
        let locks = self.missile_targets[index / 2];
        if index & 1 == 0 {
            locks & 15
        } else {
            locks >> 4
        }
    }
    // Resident: shared by SIM and its attack helper, leaving room for the
    // selected life cap in every composed simulation profile.
    #[inline(never)]
    fn set_missile_lock(&mut self, index: usize, target: u8) {
        let locks = &mut self.missile_targets[index / 2];
        if index & 1 == 0 {
            *locks = (*locks & 0xf0) | target;
        } else {
            *locks = (*locks & 0x0f) | (target << 4);
        }
    }
    #[cfg(not(target_arch = "mos"))]
    fn missile_candidate(
        &self,
        position: Vec3,
        id: u8,
        bank: &AssetBank<'_>,
    ) -> Option<(Vec3, i16)> {
        let (target, closing) = if (1..=ACTORS as u8).contains(&id) {
            let actor = self.actors[id as usize - 1];
            let spec = bank.enemy(actor.kind);
            if actor.kind == 0 || actor.hp == 0 || (spec.hp == 0 && !actor.small_debris()) {
                return None;
            }
            (
                Vec3 {
                    y: actor.pos.y
                        + if actor.small_debris() {
                            10
                        } else {
                            spec.center_y as i16
                        },
                    ..actor.pos
                },
                32 + actor.approach_speed as i16,
            )
        } else {
            if !self.boss_started || self.boss_defeated {
                return None;
            }
            let (x, y, z) = match id {
                13 if self.boss_left > 0 => (-BOSS_TURRET_X, BOSS_TURRET_Y, BOSS_TURRET_Z),
                14 if self.boss_right > 0 => (BOSS_TURRET_X, BOSS_TURRET_Y, BOSS_TURRET_Z),
                15 if self.boss_left == 0 && self.boss_right == 0 && self.boss_core > 0 => {
                    (0, 0, BOSS_CORE_Z)
                }
                _ => return None,
            };
            (
                Vec3 {
                    x,
                    y,
                    z: self.boss_z + z,
                },
                if self.boss_z > 680 { 40 } else { 32 },
            )
        };
        if target.z - position.z > 0 {
            Some((target, closing))
        } else {
            None
        }
    }
    #[inline(never)]
    #[cfg(not(target_arch = "mos"))]
    fn missile_target(
        &mut self,
        position: Vec3,
        target: &mut u8,
        bank: &AssetBank<'_>,
    ) -> Option<(Vec3, i16)> {
        if let Some(candidate) = self.missile_candidate(position, *target, bank) {
            return Some(candidate);
        }
        *target = 0;
        let mut count = 0;
        for id in 1..=15 {
            count += u8::from(self.missile_candidate(position, id, bank).is_some());
        }
        if count == 0 {
            return None;
        }
        let mut rank = 0;
        if count > 1 {
            if self.missile_rng == 0 {
                self.missile_rng = MISSILE_SEED;
            }
            loop {
                self.missile_rng =
                    (self.missile_rng >> 1) ^ if self.missile_rng & 1 != 0 { 0xb8 } else { 0 };
                // Each nonzero high nibble appears 16 times per LFSR cycle.
                // Reject zero and out-of-range values instead of modulo bias.
                let draw = self.missile_rng >> 4;
                if draw != 0 && draw <= count {
                    rank = draw - 1;
                    break;
                }
            }
        }
        for id in 1..=15 {
            if let Some(candidate) = self.missile_candidate(position, id, bank) {
                if rank == 0 {
                    *target = id;
                    return Some(candidate);
                }
                rank -= 1;
            }
        }
        None
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn steer_missile(&mut self, shot: &mut Shot, target: &mut u8, bank: &AssetBank<'_>) {
        #[cfg(target_arch = "mos")]
        {
            // The native body is `fast_steer_missile` (target-c64/asm/
            // missile.S): the search below and `missile_velocity`, compared
            // against a transcription of them by tools/test-missile-kernel.py.
            #[repr(C)]
            struct SteerArgs {
                actors: *const Actor,
                specs: *const u8,
                shot: Vec3,
                boss_z: i16,
                enemy_count: u8,
                boss_active: u8,
                boss_left: u8,
                boss_right: u8,
                boss_core: u8,
                dx: i8,
                dy: i8,
                target: u8,
                rng: u8,
            }
            const _: [(); 21] = [(); core::mem::size_of::<SteerArgs>()];
            extern "C" {
                fn fast_steer_missile(args: *mut SteerArgs);
            }
            let mut args = SteerArgs {
                actors: self.actors.as_ptr(),
                specs: bank.enemy_specs(),
                shot: shot.pos,
                boss_z: self.boss_z,
                enemy_count: bank.enemy_count(),
                boss_active: (self.boss_started && !self.boss_defeated) as u8,
                boss_left: self.boss_left,
                boss_right: self.boss_right,
                boss_core: self.boss_core,
                dx: shot.dx,
                dy: shot.dy,
                target: *target,
                rng: self.missile_rng,
            };
            // SAFETY: the kernel reads the twelve actor slots and the bank's
            // descriptors through the pointers above and writes only dx/dy,
            // the target lock and random state in this argument record.
            unsafe { fast_steer_missile(&mut args) };
            shot.dx = args.dx;
            shot.dy = args.dy;
            *target = args.target;
            self.missile_rng = args.rng;
        }
        #[cfg(not(target_arch = "mos"))]
        {
            let (dx, dy) =
                if let Some((target, closing)) = self.missile_target(shot.pos, target, bank) {
                    let dz = (target.z - shot.pos.z).max(closing);
                    (
                        missile_velocity(target.x - shot.pos.x, closing, dz),
                        missile_velocity(target.y - shot.pos.y, closing, dz),
                    )
                } else {
                    (0, 0)
                };
            shot.dx += (dx - shot.dx).clamp(-1, 1);
            shot.dy += (dy - shot.dy).clamp(-1, 1);
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn kill_actor(&mut self, index: usize, bank: &AssetBank<'_>) {
        let a = self.actors[index];
        self.actors[index].kind = 0;
        self.explode(a.pos, 7);
        self.sound_at(Effect::EnemyExplosion, a.pos.x, a.pos.z);
        if a.kind == assets::ROCK {
            self.score += 25;
            return;
        } else {
            self.score += bank.enemy(a.kind).score as u32;
            self.kills = self.kills.wrapping_add(1);
            // Keep the 15-kill cadence independent of the wrapping total and
            // consume the milestone even when the three-bomb inventory is full.
            self.bomb_kills += 1;
            if self.bomb_kills & 15 == 15 {
                self.bomb_kills &= 0xf0;
                self.bombs = (self.bombs + 1).min(3);
            }
            if a.flags & 1 != 0 {
                self.add_pickup(a.pos, 2);
            } else if self.kills % 8 == 0 {
                self.add_pickup(a.pos, 1);
            }
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn bomb(&mut self, bank: &AssetBank<'_>) {
        self.bombs -= 1;
        self.presentation_event = PresentationEvent::Bomb;
        self.sound(Effect::Bomb, 0);
        for s in &mut self.shots {
            if s.kind == 2 {
                s.kind = 0;
            }
        }
        for i in 0..ACTORS {
            if self.actors[i].kind == assets::ROCK && self.stage_info.theme >= 10 {
                self.kill_actor(i, bank);
            } else if self.actors[i].kind != 0 && self.actors[i].hp != 255 {
                if self.damage_actor(i, 8) == 0 {
                    self.kill_actor(i, bank);
                }
            }
        }
        if self.boss_started && !self.boss_defeated {
            let open = self.boss_left == 0 && self.boss_right == 0;
            damage_boss_part(&mut self.boss_left, 8, &mut self.boss_hit_flash_ticks);
            damage_boss_part(&mut self.boss_right, 8, &mut self.boss_hit_flash_ticks);
            if open {
                damage_boss_part(&mut self.boss_core, 8, &mut self.boss_hit_flash_ticks);
            }
            self.check_boss_death();
        }
        self.explode(Vec3 { x: 0, y: 0, z: 400 }, 1);
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn check_boss_death(&mut self) {
        if self.boss_core == 0 && !self.boss_defeated {
            self.boss_defeated = true;
            self.shield = 100;
            self.bombs = 3;
            self.score += 5000;
            self.sound(Effect::BossExplosion, 0);
            for x in [-80, -40, 0, 40, 80] {
                self.explode(
                    Vec3 {
                        x,
                        y: 0,
                        z: self.boss_z,
                    },
                    7,
                );
            }
            for s in &mut self.shots {
                if s.kind == 2 {
                    s.kind = 0;
                }
            }
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn damage_player(&mut self, amount: u8) {
        if self.invulnerability > 0 || self.mode != Mode::Playing {
            return;
        }
        let previous_shield = self.shield;
        self.shield = self.shield.saturating_sub(amount);
        self.invulnerability = 35;
        self.hit_flash_ticks = 35;
        if self.presentation_event != PresentationEvent::Bomb {
            self.presentation_event = PresentationEvent::Hit;
        }
        if self.shield != 0 {
            self.sound_player(if previous_shield > 25 && self.shield <= 25 { Effect::LowShield }
                       else { Effect::PlayerHit });
        }
        if self.shield == 0 {
            self.lives = self.lives.saturating_sub(1);
            self.explode(self.player, 7);
            self.sound(Effect::PlayerDeath, 0);
            if self.lives == 0 {
                self.mode = Mode::GameOver;
            } else {
                self.mode = Mode::Respawning;
                self.respawn_ticks = 75;
            }
            // A held fire button cannot dismiss the result or trigger a restart.
            self.controls.clear_pending();
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.terminal")]
    fn respawn(&mut self) {
        self.presentation_event = PresentationEvent::Reset;
        self.stage_ticks = 0;
        self.transition_ticks = 0;
        self.spawn_segment = self.segment_index();
        self.spawn_cursor = 0;
        self.repeat_spawn_cursor = 0;
        self.actors = [Actor::EMPTY; ACTORS];
        self.actor_hit_flash_ticks = [0; ACTORS];
        self.boss_hit_flash_ticks = 0;
        self.damage_feedback_tick = self.ticks;
        self.shots = [Shot::EMPTY; SHOTS];
        self.missile_targets = [0; SHOTS / 2];
        self.pickups = [Pickup::EMPTY; 4];
        self.explosions = [Explosion::EMPTY; 6];
        self.player = Vec3 {
            x: 0,
            y: 0,
            z: PLAYER_Z,
        };
        self.flight = Flight::ZERO;
        self.shield = 100;
        self.invulnerability = 100;
        self.hit_flash_ticks = 0;
        self.roll_ticks = 0;
        self.roll_cooldown = 0;
        self.shot_cooldown = 0;
        self.bombs = self.bombs.max(1);
        self.mode = Mode::Playing;
        self.boss_started = false;
        self.boss_defeated = false;
        self.boss_z = 1800;
        self.boss_left = BOSS_SIDE_HP;
        self.boss_right = BOSS_SIDE_HP;
        self.boss_core = BOSS_CORE_HP;
        self.controls.fire_armed = false;
        self.controls.left_armed = false;
        self.controls.right_armed = false;
        self.controls.released_at = self.controls.now;
        self.controls.cached_charge = 0;
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn add_pickup(&mut self, pos: Vec3, kind: u8) {
        // Admit every third bomb from either scripted spawns or enemy drops.
        if kind == 2 {
            self.bomb_kills += 16;
            if self.bomb_kills < 48 {
                return;
            }
            self.bomb_kills &= 15;
        }
        if let Some(p) = self.pickups.iter_mut().find(|p| p.kind == 0) {
            *p = Pickup { pos, kind };
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn update_pickups(&mut self) {
        for i in 0..self.pickups.len() {
            let p = &mut self.pickups[i];
            if p.kind == 0 {
                continue;
            }
            p.pos.z -= SPEED as i16;
            if self.mode == Mode::Playing {
                let depth = p.pos.z.wrapping_sub(PLAYER_Z);
                if depth as u16 <= PICKUP_MAGNET_DEPTH as u16
                    && close_xy(p.pos, self.player, PICKUP_MAGNET_RANGE)
                {
                    // Move the visible pickup gently toward the ship, leaving the
                    // collection hitbox unchanged and allowing steering to outrun it.
                    p.pos.x = magnet_coordinate(p.pos.x, self.player.x);
                    p.pos.y = magnet_coordinate(p.pos.y, self.player.y);
                }
                if depth.wrapping_add(24) as u16 <= 48 && close_xy(p.pos, self.player, 28) {
                    let kind = p.kind;
                    let (x, z) = (p.pos.x, p.pos.z);
                    p.kind = 0;
                    if kind == 1 {
                        self.shield = self.shield.saturating_add(60).min(100);
                    } else {
                        self.bombs = (self.bombs + 1).min(3);
                    }
                    self.score += 50;
                    self.sound_at(Effect::Pickup, x, z);
                    continue;
                }
            }
            // Camera-facing pickups are flat: once behind the near plane they
            // cannot be drawn, so the slot is released there.
            if p.pos.z < crate::render::NEAR {
                p.kind = 0;
            }
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn explode(&mut self, pos: Vec3, color: u8) {
        let index = self
            .explosions
            .iter()
            .position(|e| e.ticks == 0)
            .unwrap_or((self.ticks as usize) % 6);
        self.explosions[index] = Explosion {
            pos,
            ticks: 18,
            color,
        };
    }
    #[inline(never)]
    fn update_explosions(&mut self) {
        for e in &mut self.explosions {
            if e.ticks > 0 {
                e.ticks -= 1;
                e.pos.z -= SPEED as i16;
            }
        }
    }
    /// Emit only constant-color triangles. The host and C64 use the same scene.
    /// Host reference ordering: foreground commands precede scenery at equal depth.
    /// Call `prepare_damage_feedback_frame` after catch-up before rendering.
    #[cfg(not(target_arch = "mos"))]
    pub fn render(
        &self,
        sink: &mut impl GeometrySink,
        bank: &AssetBank<'_>,
        segment: &Segment<'_>,
    ) {
        if self.mode == Mode::Victory {
            struct Fade<'a, S> {
                sink: &'a mut S,
                ticks: u16,
            }
            impl<S: GeometrySink> GeometrySink for Fade<'_, S> {
                fn checkpoint(&self) -> Option<crate::render::Checkpoint> {
                    self.sink.checkpoint()
                }
                fn rollback(&mut self, checkpoint: crate::render::Checkpoint) {
                    self.sink.rollback(checkpoint);
                }
                fn scenery_last(&mut self, prefix: u16) {
                    self.sink.scenery_last(prefix);
                }
                fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
                    self.sink.triangle(
                        a,
                        b,
                        c,
                        crate::victory::fade_color(color, self.ticks),
                        flags,
                    );
                }
            }
            let mut world = Fade {
                sink,
                ticks: self.menu_ticks,
            };
            self.render_world(&mut world, bank, segment);
            self.render_medal(sink, bank);
        } else {
            self.render_world(sink, bank, segment);
        }
    }
    /// Complete host scene with optional downward shadows. Scratch belongs to
    /// the caller and may be reused or released as soon as this method returns.
    #[cfg(not(target_arch = "mos"))]
    pub fn render_with_shadows(
        &self,
        renderer: &mut crate::render::Renderer,
        frame: &mut crate::shadow::ShadowFrame,
        bank: &AssetBank<'_>,
        segment: &Segment<'_>,
    ) {
        struct Scenery<'a> {
            renderer: &'a mut crate::render::Renderer,
            frame: &'a mut crate::shadow::ShadowFrame,
        }
        impl GeometrySink for Scenery<'_> {
            fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
                self.renderer.triangle(a, b, c, color, flags);
            }
            fn shadow_enabled(&self) -> bool {
                true
            }
            fn shadow_caster(&mut self, caster: crate::shadow::Caster) {
                self.frame.push_caster(caster);
            }
        }
        frame.reset();
        let Some(ground_y) = self.shadow_ground_y() else {
            self.render(renderer, bank, segment);
            frame.finish_without_shadows(renderer);
            return;
        };
        self.render_scenery(&mut Scenery { renderer, frame }, segment);
        let prefix = renderer.triangle_count;
        self.render_foreground(renderer, bank, 174.min(renderer.remaining_commands()));
        renderer.scenery_last(prefix);
        if self.mode == Mode::Victory {
            renderer.fade_scene(self.menu_ticks);
            self.render_medal(renderer, bank);
        }
        self.collect_shadow_casters(frame, bank);
        frame.render(renderer, ground_y, self.shadow_color());
    }

    /// Collect only value descriptors: the shadow overlay never calls geometry
    /// or scenery code after their execution windows have been replaced.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.shadow")]
    pub fn collect_shadow_casters(
        &self,
        frame: &mut crate::shadow::ShadowFrame,
        bank: &AssetBank<'_>,
    ) {
        use crate::shadow::Caster;
        if self.shadow_ground_y().is_none() {
            return;
        }
        let camera_y = scene_camera_y(self.mode, self.player.y, self.ticks);
        // Insert important gameplay objects ahead of the already-collected decorations.
        let mut priority = 0u8;
        if matches!(
            self.mode,
            Mode::Playing | Mode::Transition | Mode::Title | Mode::Victory
        ) {
            priority += u8::from(frame.insert_caster(
                priority,
                shadow_model(bank, assets::PLAYER, self.player, false, camera_y),
            ));
        }
        if self.mode == Mode::Victory {
            return;
        }
        if self.mode != Mode::Transition {
            if self.boss_started && !self.boss_defeated {
                priority += u8::from(frame.insert_caster(
                    priority,
                    shadow_model(
                        bank,
                        assets::CARRIER,
                        Vec3 {
                            x: 0,
                            y: 0,
                            z: self.boss_z,
                        },
                        false,
                        camera_y,
                    ),
                ));
            }
            for actor in &self.actors {
                if actor.kind != 0
                    && actor.pos.z >= ACTOR_RETIRE_Z
                    && (matches!(actor.kind, assets::FIGHTER | assets::DIVER)
                        || (self.stage_info.theme >= 10 && actor.kind == assets::ROCK))
                {
                    let position = if actor.kind == assets::ROCK {
                        actor_center(*actor, bank)
                    } else {
                        actor.pos
                    };
                    priority += u8::from(frame.insert_caster(
                        priority,
                        shadow_model(bank, actor.kind, position, actor.small_debris(), camera_y),
                    ));
                }
            }
        }
        for pickup in &self.pickups {
            if pickup.kind != 0 {
                let (half_x, half_z, shape) = if pickup.kind == 1 {
                    (13, 13, 3)
                } else {
                    (16, 12, 4)
                };
                let caster = Caster {
                    center: Vec3 {
                        y: pickup.pos.y - camera_y,
                        ..pickup.pos
                    },
                    half_x,
                    half_z,
                    shape,
                };
                frame.insert_caster(frame.caster_count(), caster);
            }
        }
        for shot in &self.shots {
            if shot.kind != 0 {
                let radius = if shot.kind == 2 {
                    ENEMY_SHOT_RADIUS
                } else {
                    3 + shot.damage as i16
                };
                let caster = Caster {
                    center: Vec3 {
                        y: shot.pos.y - camera_y,
                        ..shot.pos
                    },
                    half_x: radius,
                    half_z: radius,
                    shape: 3,
                };
                frame.insert_caster(frame.caster_count(), caster);
            }
        }
    }
    /// Runtime sinks budget projected commands. Plain captures retain authoring
    /// order and are deliberately not used to predict projection capacity.
    #[cfg(not(target_arch = "mos"))]
    fn render_world(
        &self,
        sink: &mut impl GeometrySink,
        bank: &AssetBank<'_>,
        segment: &Segment<'_>,
    ) {
        if sink.checkpoint().is_some() {
            self.render_scenery(sink, segment);
            let prefix = sink.checkpoint().unwrap().triangle_count;
            self.render_foreground(sink, bank, 174.min(256 - prefix));
            sink.scenery_last(prefix);
        } else {
            self.render_foreground(sink, bank, 174);
            self.render_scenery(sink, segment);
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.victory")]
    pub fn render_medal(&self, sink: &mut impl GeometrySink, bank: &AssetBank<'_>) {
        // Match the ship's sink type so native meshes share one bounded
        // transform/vertex-cache implementation in the geometry prefix.
        let mut budget = SceneBudget {
            sink,
            count: 0,
            limit: 174,
            camera_y: 0,
            color_override: 0,
            extra_flags: 0,
        };
        let checkpoint = budget.begin_group();
        crate::victory::emit(&mut budget, bank, self.menu_ticks, self.ticks);
        budget.finish_group(checkpoint);
    }
    /// Draw prepared foreground state without advancing presentation timers.
    /// Call `prepare_damage_feedback_frame` once before drawing a new frame.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
    pub fn render_foreground(
        &self,
        sink: &mut impl GeometrySink,
        bank: &AssetBank<'_>,
        foreground_limit: u16,
    ) {
        if self.mode == Mode::Victory && self.menu_ticks >= crate::victory::FADE_TICKS {
            return;
        }
        // Prioritize player, boss, projectiles and pickups before distant detail.
        // The limit counts accepted commands, including near-plane splits.
        let mut budget = SceneBudget {
            sink,
            count: 0,
            limit: foreground_limit.min(174),
            // An elevated chase camera separates the nearby ship from targets
            // on its firing line. Menus float around their authored framing.
            // Follow a quarter of vertical movement so the lowest banked hull
            // stays above the cockpit while neutral targets remain exposed.
            camera_y: scene_camera_y(self.mode, self.player.y, self.ticks),
            color_override: 0,
            extra_flags: 0,
        };
        let sink = &mut budget;
        if matches!(
            self.mode,
            Mode::Playing | Mode::Transition | Mode::Title | Mode::Victory
        ) {
            let checkpoint = sink.begin_group();
            let appearance = self.player_appearance();
            sink.extra_flags = TERRAIN_CLEAR_GEOMETRY;
            sink.color_override = (appearance >> 8) as u8;
            emit_mesh(
                sink,
                bank,
                assets::PLAYER,
                self.player,
                0,
                self.player_pitch(),
                appearance as u8,
                false,
            );
            sink.color_override = 0;
            // Each flat exhaust has an explicit underside for high climbs.
            for x in [-11, 11] {
                let p = Vec3 {
                    x: self.player.x + x,
                    y: self.player.y - 3,
                    z: PLAYER_Z - 20,
                };
                let left = Vec3 { x: p.x - 3, ..p };
                let right = Vec3 { x: p.x + 3, ..p };
                let tip = Vec3 {
                    z: p.z
                        - 12
                        - (if self.mode == Mode::Victory {
                            self.ticks.wrapping_sub(self.menu_ticks)
                        } else {
                            self.ticks
                        } & 7) as i16,
                    ..p
                };
                tri(sink, left, right, tip, 8);
                tri(sink, right, left, tip, 8);
            }
            sink.finish_group(checkpoint);
        }
        sink.extra_flags = 0;
        if self.mode == Mode::Victory {
            return;
        } else if self.mode == Mode::Transition {
            self.render_effects(sink);
        } else {
            self.render_combat(sink, bank);
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
    fn render_effects(&self, sink: &mut SceneBudget<'_, impl GeometrySink>) {
        for s in &self.shots {
            if s.kind != 0 {
                let checkpoint = sink.begin_group();
                diamond(
                    sink,
                    s.pos,
                    if s.kind == 2 {
                        ENEMY_SHOT_RADIUS
                    } else {
                        3 + s.damage as i16
                    },
                    if s.kind == 2 {
                        7
                    } else if s.damage > 1 {
                        7
                    } else {
                        3
                    },
                );
                sink.finish_group(checkpoint);
            }
        }
        for p in &self.pickups {
            let checkpoint = sink.begin_group();
            if p.kind == 1 {
                shield_pickup(sink, p.pos);
            } else if p.kind != 0 {
                bomb_pickup(sink, p.pos);
            }
            sink.finish_group(checkpoint);
        }
        for e in &self.explosions {
            if e.ticks > 0 {
                let checkpoint = sink.begin_group();
                diamond(sink, e.pos, 4 + (18 - e.ticks) as i16 * 3, e.color);
                sink.finish_group(checkpoint);
            }
        }
    }
    // Native scenic flights load only the shared GEOMETRY prefix. This tail
    // remains reachable exclusively while the complete combat overlay is loaded.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.geometry.combat")]
    fn render_combat(&self, sink: &mut SceneBudget<'_, impl GeometrySink>, bank: &AssetBank<'_>) {
        if self.boss_started && !self.boss_defeated {
            let checkpoint = sink.begin_group();
            let previous_flags = sink.extra_flags;
            // The Dreadnought stays above the flight deck. Protect the whole
            // assembly from long deck triangles and the final ground shadows.
            if self.stage_info.boss_kind == 4 {
                sink.extra_flags |= TERRAIN_CLEAR_GEOMETRY;
            }
            sink.color_override = u8::from(self.boss_hit_flash_ticks > ENEMY_HIT_FLASH_TICKS);
            emit_mesh(
                sink,
                bank,
                assets::CARRIER,
                Vec3 {
                    x: 0,
                    y: 0,
                    z: self.boss_z,
                },
                0,
                0,
                0,
                false,
            );
            if self.boss_left > 0 {
                emit_mesh(
                    sink,
                    bank,
                    assets::TURRET,
                    Vec3 {
                        x: -BOSS_TURRET_X,
                        y: BOSS_TURRET_Y,
                        z: self.boss_z + BOSS_TURRET_Z,
                    },
                    0,
                    0,
                    0,
                    false,
                );
            }
            if self.boss_right > 0 {
                emit_mesh(
                    sink,
                    bank,
                    assets::TURRET,
                    Vec3 {
                        x: BOSS_TURRET_X,
                        y: BOSS_TURRET_Y,
                        z: self.boss_z + BOSS_TURRET_Z,
                    },
                    0,
                    0,
                    0,
                    false,
                );
            }
            if self.boss_left == 0 && self.boss_right == 0 {
                diamond(
                    sink,
                    Vec3 {
                        x: 0,
                        y: 0,
                        z: self.boss_z + BOSS_CORE_Z,
                    },
                    22 * assets::BOSS_SCALE,
                    if self.ticks & 8 == 0 { 7 } else { 2 },
                );
            }
            sink.color_override = 0;
            sink.extra_flags = previous_flags;
            sink.finish_group(checkpoint);
        }
        self.render_effects(sink);
        for (i, a) in self.actors.iter().enumerate() {
            if a.kind != 0 && a.pos.z >= ACTOR_RETIRE_Z {
                let reserve = self.actors[i + 1..]
                    .iter()
                    .filter(|actor| actor.kind != 0 && actor.pos.z >= ACTOR_RETIRE_Z)
                    .count() as u16
                    * 2;
                let limit = sink.limit;
                sink.color_override =
                    u8::from(self.actor_hit_flash_ticks[i] > ENEMY_HIT_FLASH_TICKS);
                // Ground hazards and mounted enemies stay visible over terrain,
                // including their shallow embedded bases and diamond proxies.
                // Orbital hazards and boss-mounted units keep normal ordering.
                sink.extra_flags = if self.stage_info.theme < 10
                    && (matches!(a.kind, assets::ROCK | assets::BUILDING)
                        || (a.kind == assets::TURRET && self.stage_info.boss_kind == 0))
                {
                    TERRAIN_CLEAR_GEOMETRY
                } else {
                    0
                };
                sink.limit = limit.saturating_sub(reserve);
                let mut proxy = a.pos.z > 1200
                    || (sink.sink.checkpoint().is_none()
                        && sink.count + bank.face_count(a.kind) as u16 > sink.limit);
                if !proxy {
                    let checkpoint = sink.begin_group();
                    let roll = if bank.enemy(a.kind).render_mode == 1 {
                        (a.age as u8).wrapping_mul(2)
                    } else {
                        0
                    };
                    // Rotate space rocks around their scaled center, preserving
                    // collision alignment while keeping terrestrial meshes still.
                    let space_rock = self.stage_info.theme >= 10 && a.kind == assets::ROCK;
                    let pitch = if space_rock { (a.age as u8) >> 1 } else { 0 };
                    let position = if space_rock {
                        actor_center(*a, bank)
                    } else {
                        a.pos
                    };
                    emit_mesh(
                        sink,
                        bank,
                        a.kind,
                        position,
                        0,
                        pitch,
                        roll,
                        a.small_debris(),
                    );
                    proxy = !sink.finish_group(checkpoint);
                }
                if proxy {
                    let checkpoint = sink.begin_group();
                    if self.stage_info.theme == 4
                        && matches!(a.kind, assets::FIGHTER | assets::DIVER)
                    {
                        cave_flyer_proxy(sink, bank, a.kind, a.pos);
                    } else {
                        actor_proxy(
                            sink,
                            bank,
                            a.kind,
                            actor_center(*a, bank),
                            if a.kind == assets::FIGHTER {
                                assets::enemy_scaled(22)
                            } else if a.kind <= bank.enemy_count() {
                                assets::enemy_scaled(18)
                            } else if a.small_debris() {
                                9
                            } else {
                                18
                            },
                        );
                    }
                    sink.finish_group(checkpoint);
                }
                sink.limit = limit;
                sink.extra_flags = 0;
                sink.color_override = 0;
            }
        }
    }
    /// Stage unprojected scenery exactly as the native frame does. Backdrop
    /// records carry STATIC_SKY; `Renderer::project_scenery` applies the sky
    /// parallax when it replays them, so staging has no camera state.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
    pub fn stage_scenery(&self, sink: &mut crate::scenery::SceneSink<'_>, segment: &Segment<'_>) {
        if self.mode == Mode::Victory && self.menu_ticks >= crate::victory::FADE_TICKS {
            return;
        }
        crate::scenery::emit(
            sink,
            segment,
            self.scenery_ticks(),
            self.segment_index(),
            self.transition_progress(),
            self.scenery_camera_y(),
            matches!(self.mode, Mode::Title | Mode::Attract),
        );
    }
    /// Host reference: scenery as it reaches the projector, sky parallax
    /// included, in staging order.
    #[cfg(not(target_arch = "mos"))]
    pub fn render_scenery(&self, sink: &mut impl GeometrySink, segment: &Segment<'_>) {
        struct SkyDrift<'a, S: ?Sized> {
            sink: &'a mut S,
            parallax: [i16; 2],
        }
        impl<S: GeometrySink + ?Sized> GeometrySink for SkyDrift<'_, S> {
            fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
                let mut points = [a, b, c];
                crate::render::drift_sky(&mut points, flags, self.parallax[0], self.parallax[1]);
                self.sink
                    .triangle(points[0], points[1], points[2], color, flags);
            }
            fn shadow_enabled(&self) -> bool {
                self.sink.shadow_enabled()
            }
            fn shadow_caster(&mut self, caster: crate::shadow::Caster) {
                self.sink.shadow_caster(caster);
            }
        }
        let mut drifted = SkyDrift {
            sink,
            parallax: self.sky_parallax(),
        };
        self.stage_scenery(&mut drifted, segment);
    }
    /// A slight camera pan toward the ship: the far backdrop drifts opposite
    /// to its sideways movement and dips as it climbs, nearer clouds more than
    /// the horizon. Title and map tours drift independently of the ship. The
    /// arithmetic shift is exact enough and far cheaper than a signed divide;
    /// outlined because inlining it bloats the GEOMETRY frame driver.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
    pub fn sky_parallax(&self) -> [i16; 2] {
        if matches!(self.mode, Mode::Title | Mode::Attract) {
            // One horizontal cycle takes 512 ticks (10.24 s); the smaller
            // vertical float takes 1024 ticks (20.48 s), on both PAL and NTSC.
            [
                crate::math::sin8((self.ticks >> 1) as u8) >> 2,
                crate::math::sin8((self.ticks >> 2) as u8) >> 5,
            ]
        } else {
            [self.player.x, self.player.y >> 1]
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
    fn player_appearance(&self) -> u16 {
        // The barrel roll's linear spin rides on the coordinated bank, so the
        // end of the spin (a full turn) is continuous with the banked hull.
        // Negative angles lower the right wing (clockwise from the cockpit),
        // the bank's sense for a right turn, so a right roll spins negative.
        let bank = (self.flight.bank >> 8) as u8;
        let roll = if self.roll_ticks > 0 {
            ((25 - self.roll_ticks) as i16 * 256 / 25 * -(self.roll_direction as i16)) as u8
        } else {
            0
        }
        .wrapping_add(bank);
        // 200 ms phases remain distinct at the current rendering cadence.
        let white = self.mode == Mode::Playing
            && (self.hit_flash_ticks > 25
                || (self.hit_flash_ticks > 5 && self.hit_flash_ticks <= 15));
        roll as u16 | ((white as u16) << 8)
    }
    /// The flight model's pitch (nose up while climbing) plus the authored
    /// ascent of the orbital bridge.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
    fn player_pitch(&self) -> u8 {
        let flight = (self.flight.pitch >> 8) as u8;
        if self.mode != Mode::Transition || self.stage_info.bridge_profile != 3 {
            return flight;
        }
        let tick = self.transition_ticks;
        let amount = if tick < 100 {
            tick / 5
        } else if tick < 300 {
            20
        } else {
            (400 - tick) / 5
        };
        flight.wrapping_sub(amount as u8)
    }
    #[cfg(test)]
    fn terrain(&self, sink: &mut impl GeometrySink, segment: &Segment<'_>) {
        crate::scenery::emit(
            sink,
            segment,
            self.stage_ticks,
            self.segment_index(),
            None,
            0,
            true,
        );
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.shadow")]
fn shadow_model(
    bank: &AssetBank<'_>,
    id: u8,
    position: Vec3,
    half: bool,
    camera_y: i16,
) -> crate::shadow::Caster {
    let spec = bank.shadow_spec(id);
    let divisor = if half { 2 } else { 1 };
    crate::shadow::Caster {
        center: Vec3 {
            x: position.x + spec.offset_x / divisor,
            y: position.y - camera_y,
            z: position.z + spec.offset_z / divisor,
        },
        half_x: spec.half_x as i16 / divisor,
        half_z: spec.half_z as i16 / divisor,
        shape: spec.family | (spec.heading << 4),
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".sfx_helpers")]
fn placement_random(value: u8) -> u8 {
    (value >> 1) ^ if value & 1 != 0 { 0xb8 } else { 0 }
}

// Resident: used by projectile and bomb damage without duplicating the health
// comparison and timer update in the crowded SIM overlay.
#[inline(never)]
fn damage_boss_part(hp: &mut u8, damage: u8, flash: &mut u8) {
    let previous = *hp;
    *hp = hp.saturating_sub(damage);
    if *hp < previous && *flash == 0 {
        *flash = ENEMY_HIT_FLASH_PENDING;
    }
}

#[inline(always)]
fn advance_enemy_hit_flash(flash: &mut u8, elapsed: u8) {
    *flash = if *flash == ENEMY_HIT_FLASH_PENDING {
        ENEMY_HIT_FLASH_TICKS * 2
    } else if *flash > ENEMY_HIT_FLASH_TICKS {
        flash.saturating_sub(elapsed).max(ENEMY_HIT_FLASH_TICKS)
    } else {
        flash.saturating_sub(elapsed)
    };
}

struct SceneBudget<'a, S: GeometrySink> {
    sink: &'a mut S,
    count: u16,
    limit: u16,
    camera_y: i16,
    color_override: u8,
    extra_flags: u8,
}
impl<S: GeometrySink> SceneBudget<'_, S> {
    #[inline(always)]
    fn begin_group(&self) -> (u16, Option<crate::render::Checkpoint>) {
        (self.count, self.sink.checkpoint())
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
    fn finish_group(
        &mut self,
        (count, checkpoint): (u16, Option<crate::render::Checkpoint>),
    ) -> bool {
        if let Some(checkpoint) = checkpoint {
            if self.count > self.limit
                || self.sink.checkpoint().unwrap().dropped != checkpoint.dropped
            {
                self.sink.rollback(checkpoint);
                self.count = count;
                return false;
            }
        }
        true
    }
}
impl<S: GeometrySink> GeometrySink for SceneBudget<'_, S> {
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
    fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
        let before = self.sink.checkpoint();
        if before.is_some() || self.count < self.limit {
            // Scenery (and so every STATIC_SKY face) is staged elsewhere; all
            // foreground geometry follows the chase camera.
            let camera_y = self.camera_y;
            self.sink.triangle(
                camera_point(a, camera_y),
                camera_point(b, camera_y),
                camera_point(c, camera_y),
                if self.color_override != 0 {
                    self.color_override
                } else {
                    color
                },
                flags | self.extra_flags,
            );
            self.count += before.map_or(1, |before| {
                self.sink.checkpoint().unwrap().triangle_count - before.triangle_count
            });
        }
    }
    /// The asm mesh path: `fast_emit_mesh` transforms the instance once into
    /// the vertex records at $E000 and emits its unclipped faces straight into
    /// the resident renderer, applying this budget, camera and overrides.
    /// Faces that straddle the near plane come back one at a time and take
    /// the inner sink's clipping path, exactly as the reference does.
    #[cfg(target_arch = "mos")]
    #[inline(never)]
    #[link_section = ".phase.geometry"]
    fn mesh(
        &mut self,
        bank: &AssetBank<'_>,
        id: u8,
        position: Vec3,
        yaw: u8,
        pitch: u8,
        roll: u8,
        half: bool,
    ) {
        #[repr(C)]
        struct MeshArgs {
            vertices: *const u8,
            faces: *const u8,
            matrix: *const [[i16; 3]; 3],
            position: Vec3,
            camera_y: i16,
            vertex_count: u8,
            face_count: u8,
            shift: u8,
            flags: u8,
            color: u8,
            face_color: u8,
            indices: [u8; 3],
        }
        extern "C" {
            fn fast_emit_mesh(args: *mut MeshArgs, start: u8) -> u8;
        }
        const _: [(); 23] = [(); core::mem::size_of::<MeshArgs>()];
        const RECORDS: usize = 0xE000;
        let before = self
            .sink
            .checkpoint()
            .expect("native mesh needs a projected sink");
        let rotation = crate::math::RotationMatrix::new(yaw, pitch, roll);
        let mesh = crate::asteroid::mesh_data(bank, id, position.z, half);
        let mut args = MeshArgs {
            vertices: mesh.vertices,
            faces: mesh.faces,
            matrix: &rotation.m,
            position,
            camera_y: self.camera_y,
            vertex_count: mesh.vertex_count,
            face_count: mesh.face_count,
            shift: mesh.shift,
            flags: self.extra_flags,
            color: self.color_override,
            face_color: 0,
            indices: [0; 3],
        };
        let mut face = 0;
        loop {
            // SAFETY: the kernel reads this validated bank's mesh, writes the
            // idle raster-tail region and the single resident renderer, and
            // returns either face_count or the index of a face to clip.
            face = unsafe { fast_emit_mesh(&mut args, face) };
            if face >= args.face_count {
                break;
            }
            let corner = |index: u8| -> Vec3 {
                // SAFETY: the kernel stored every vertex's camera-space point
                // at the start of its 16-byte record.
                unsafe { ((RECORDS + index as usize * 16) as *const Vec3).read() }
            };
            self.sink.triangle(
                corner(args.indices[0]),
                corner(args.indices[1]),
                corner(args.indices[2]),
                args.face_color,
                args.flags,
            );
            face += 1;
        }
        self.count += self.sink.checkpoint().unwrap().triangle_count - before.triangle_count;
    }
}
// Only foreground GEOMETRY uses this transform. Keep one outlined copy in its
// overlay so the expanded campaign leaves resident interrupt code room. Mesh
// translation already saturates; the bounded camera cannot overflow i16.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
fn camera_point(p: Vec3, camera_y: i16) -> Vec3 {
    Vec3 {
        y: p.y - camera_y,
        ..p
    }
}
#[inline(always)]
// Each rendering overlay keeps its own small camera calculation.
fn scene_camera_y(mode: Mode, player_y: i16, ticks: u16) -> i16 {
    if matches!(mode, Mode::Title | Mode::Attract) {
        let base = if mode == Mode::Title { 0 } else { 40 };
        base + (crate::math::sin8((ticks >> 2) as u8) >> 6)
    } else {
        40 + player_y / 4
    }
}
#[inline(always)]
fn actor_center_sim(actor: &Actor, bank: &AssetBank<'_>) -> Vec3 {
    Vec3 {
        x: actor.pos.x,
        y: actor_center_y(actor.kind, actor.pos.y, actor.flags, bank),
        z: actor.pos.z,
    }
}
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.sim_aux")]
fn actor_center_y(kind: u8, y: i16, flags: u8, bank: &AssetBank<'_>) -> i16 {
    y + match kind {
        assets::ROCK => {
            if flags & assets::SMALL_DEBRIS != 0 {
                10
            } else {
                20
            }
        }
        assets::BUILDING => 30,
        _ => bank.enemy(kind).center_y as i16,
    }
}
#[inline(always)]
fn actor_center(actor: Actor, bank: &AssetBank<'_>) -> Vec3 {
    Vec3 {
        x: actor.pos.x,
        z: actor.pos.z,
        y: actor.pos.y
            + match actor.kind {
                assets::ROCK => {
                    if actor.small_debris() {
                        10
                    } else {
                        20
                    }
                }
                assets::BUILDING => 30,
                _ => bank.enemy(actor.kind).center_y as i16,
            },
    }
}
// Collision and control helpers belong to every SIM profile. The small audio
// queue drain stays resident to balance the tight SIM and resident allocations.
#[inline(never)]
fn close_xy(a: Vec3, b: Vec3, radius: i16) -> bool {
    (a.x - b.x).abs() <= radius && (a.y - b.y).abs() <= radius
}
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.sim_aux")]
fn magnet_coordinate(current: i16, target: i16) -> i16 {
    // The caller has proved both axes are within 64 units. Narrow only that
    // bounded difference, keeping the coordinates and result fully signed.
    let delta = (target - current) as i8;
    if delta > PICKUP_MAGNET_SPEED as i8 {
        current + PICKUP_MAGNET_SPEED
    } else if delta < -PICKUP_MAGNET_SPEED as i8 {
        current - PICKUP_MAGNET_SPEED
    } else {
        target
    }
}
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.sim_aux")]
fn enemy_velocity(player: i16, origin: i16, dz: i16) -> i8 {
    ((player.saturating_sub(origin).clamp(-512, 512) * 18) / dz).clamp(-8, 8) as i8
}
#[inline(never)]
#[cfg(not(target_arch = "mos"))]
fn missile_velocity(error: i16, closing: i16, dz: i16) -> i8 {
    // Clamp before multiplying so all guidance arithmetic stays signed 16-bit.
    // The product wraps and the division truncates toward zero, exactly as
    // the native kernel reproduces them.
    ((error.clamp(-512, 512).wrapping_mul(closing)) / dz).clamp(-12, 12) as i8
}
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
fn tri(sink: &mut impl GeometrySink, a: Vec3, b: Vec3, c: Vec3, color: u8) {
    sink.triangle(a, b, c, color, 0);
}
// Each byte selects x from its low two bits and y from its next two bits.
// Keep the original face order: equal-depth sorting preserves the shield rim
// before its inset, and partial scene budgets retain the same triangle prefix.
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry.data")]
static DIAMOND_FACES: [[u8; 3]; 2] = [[4, 9, 6], [4, 6, 1]];
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry.data")]
static SHIELD_FACES: [[u8; 3]; 3] = [[8, 10, 6], [8, 6, 4], [4, 6, 1]];
// The bomb pickup is the HUD's bomb icon: a solid rectangle split along one
// diagonal, wound like the diamond.
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry.data")]
static BOMB_FACES: [[u8; 3]; 2] = [[8, 10, 2], [8, 2, 0]];

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
fn flat_shape(
    sink: &mut impl GeometrySink,
    p: Vec3,
    rx: i16,
    ry: i16,
    mut colors: u8,
    faces: &[[u8; 3]],
) {
    // Alternate the packed palette nibbles between facets. Uniform shapes
    // repeat their color in both nibbles; actor proxies retain two hull shades.
    // The spare center entry makes every masked selector a valid array index.
    let x = [p.x - rx, p.x, p.x + rx, p.x];
    let y = [p.y - ry, p.y, p.y + ry, p.y];
    for face in faces {
        let mut points = [p; 3];
        for corner in 0..3 {
            let selector = face[corner];
            points[corner].x = x[(selector & 3) as usize];
            points[corner].y = y[((selector >> 2) & 3) as usize];
        }
        tri(sink, points[0], points[1], points[2], colors & 15);
        colors = colors.rotate_right(4);
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
fn diamond(sink: &mut impl GeometrySink, p: Vec3, r: i16, color: u8) {
    flat_shape(sink, p, r, r, color | color << 4, &DIAMOND_FACES);
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry.combat")]
fn cave_flyer_proxy(sink: &mut impl GeometrySink, bank: &AssetBank<'_>, kind: u8, p: Vec3) {
    // Both cave flyers store left wing, right wing, crown and keel at 1..=4.
    // Flatten their front silhouette to two facets, retaining the skimmer's
    // broad cyan wings and the Needle's narrower purple hull. Read the baked
    // model so its scale and palette remain consistent at the LOD boundary.
    // Two triangles also fit the per-actor reservation during budget fallback.
    let mut points = [p; 4];
    for (index, point) in points.iter_mut().enumerate() {
        let vertex = bank.vertex(kind, index as u8 + 1);
        point.x += vertex.x;
        point.y += vertex.y;
    }
    tri(sink, points[0], points[2], points[3], bank.face(kind, 0).1);
    tri(sink, points[3], points[2], points[1], bank.face(kind, 2).1);
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry.combat")]
fn actor_proxy(
    sink: &mut impl GeometrySink,
    bank: &AssetBank<'_>,
    kind: u8,
    p: Vec3,
    radius: i16,
) {
    // Most models lead with their broad front facets. These three families
    // instead lead with accent panels; use their visible hull colors so the
    // distance and budget proxies retain the detailed model's appearance.
    // Low nibble selects the first facet; every second index is nonzero.
    // One bounded color lookup loop keeps the native combat overlay compact.
    let mut faces = match (bank.level(), kind) {
        (2 | 3, assets::DIVER) => 0x24,
        (10 | 11, assets::FIGHTER) => 0x19,
        (14, assets::DIVER) => 0x23,
        _ => 0x10,
    };
    let mut colors = 0u8;
    while faces != 0 {
        colors = colors.rotate_right(4) | bank.face(kind, faces & 15).1 << 4;
        faces >>= 4;
    }
    flat_shape(sink, p, radius, radius, colors, &DIAMOND_FACES);
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
fn shield_pickup(sink: &mut impl GeometrySink, p: Vec3) {
    // Paint the pale rim before the green inset. Equal depths retain this
    // order, keeping the camera-facing shield to six triangles in total.
    // Use green for subpixel rims: a distant gray edge can otherwise
    // displace the green face when terrain competes for cell colors.
    let rim = if p.z > 600 { 5 } else { 15 };
    flat_shape(sink, p, 13, 13, rim | rim << 4, &SHIELD_FACES);
    flat_shape(sink, p, 10, 10, 0x55, &SHIELD_FACES);
}

/// The HUD's bomb icon as a camera-facing 4:3 rectangle with the shield
/// pickup's pale rim: rim first, then the purple inset at equal depth.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
fn bomb_pickup(sink: &mut impl GeometrySink, p: Vec3) {
    // Keep at least one projected pixel of pale rim on purple skies. The
    // projection scales are 80 horizontally and 133 vertically; these cheap
    // depth shifts retain the original three-unit margin on nearby pickups.
    let margin_x = ((p.z >> 6) + 1).max(3);
    let margin_y = ((p.z >> 7) + 1).max(3);
    flat_shape(sink, p, 13 + margin_x, 9 + margin_y, 0xff, &BOMB_FACES);
    flat_shape(sink, p, 13, 9, 0x44, &BOMB_FACES);
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod story_tests;

#[cfg(test)]
mod balance_tests;

#[cfg(test)]
mod damage_flash_tests;

#[cfg(test)]
mod culling_budget_tests;

#[cfg(all(test, feature = "host-tools"))]
mod lod_tests;

#[cfg(all(test, feature = "host-tools"))]
mod shadow_tests;

#[cfg(all(test, feature = "host-tools"))]
mod camera_clearance_tests;

#[cfg(all(test, feature = "host-tools"))]
mod flyby_capture_tests;
