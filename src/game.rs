//! Deterministic 50 Hz rail-shooter simulation. Input is sampled independently at
//! display refresh rate, so PAL and NTSC use the same millisecond gesture windows.
use crate::assets::{self, AssetBank, Segment, Spawn, StageInfo};
use crate::audio::{AudioEvent, Effect, MusicState};
use crate::{emit_mesh, GeometrySink, Vec3, TERRAIN_CLEAR_GEOMETRY, TWO_SIDED};

pub const UP: u8 = 1;
pub const DOWN: u8 = 2;
pub const LEFT: u8 = 4;
pub const RIGHT: u8 = 8;
pub const FIRE: u8 = 16;
pub const MISSION_DISTANCE: u32 = 360_000;
/// Added scenic flight time between combat sections, at 50 simulation ticks/s.
pub const TRANSITION_TICKS: u16 = 400;
/// Fixed camera-space backdrop geometry; nearer scene geometry still occludes it.
pub(crate) const STATIC_SKY: u8 = 2;
const PLAYER_Z: i16 = 180;
const SPEED: i16 = 8;
const ENEMY_RAM_DEPTH: i16 = 32 * assets::ENEMY_SCALE;
const BOSS_TURRET_X: i16 = 65 * assets::ENEMY_SCALE;
const BOSS_TURRET_Y: i16 = 15 * assets::ENEMY_SCALE;
const BOSS_TURRET_Z: i16 = -35 * assets::ENEMY_SCALE;
const BOSS_CORE_Z: i16 = -100 * assets::ENEMY_SCALE;
const BOSS_TURRET_RADIUS: i16 = 23 * assets::ENEMY_SCALE;
const BOSS_CORE_RADIUS: i16 = 30 * assets::ENEMY_SCALE;
const BOSS_HIT_DEPTH: i16 = 48 * assets::ENEMY_SCALE;
const SHIELD_MAGNET_RANGE: i16 = 64;
const SHIELD_MAGNET_DEPTH: i16 = 240;
const SHIELD_MAGNET_SPEED: i16 = 2;
pub const SECTION_TICKS: u16 = 3750;
pub const MISSION_TICKS: u16 = SECTION_TICKS * 12;
const BOSS_START_TICKS: u16 = 1000;
const ATTRACT_TICKS: u16 = 750;
const ACTORS: usize = 12;
const SHOTS: usize = 24;
const AUDIO_CAPACITY: usize = 8;
const DOUBLE_TAP_WINDOW_MS: u32 = 500;

mod enemies;
mod math;
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
    pub ms: u32,
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hud {
    pub mode: Mode,
    pub shield: u8,
    pub bombs: u8,
    pub lives: u8,
    pub charge: u8,
    pub score: u32,
    pub distance: u32,
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
    now: u32,
    released_at: u32,
    fire_at: u32,
    left_at: u32,
    right_at: u32,
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
    // The reticle needs charge while GEOMETRY is loaded. Sampling performs the
    // timer arithmetic in SIM and leaves this byte available in every phase.
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
        } else if elapsed >= 2000 {
            255
        } else {
            ((elapsed as u16 - 1000) * 51 / 200) as u8
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
        // Charge is measured before storing the newly held fire button.
        if pressed & FIRE != 0 {
            self.shot_charge = self.computed_charge();
            self.shot_pending = true;
            self.start_pending = true;
            if self.fire_armed && input.ms.wrapping_sub(self.fire_at) <= DOUBLE_TAP_WINDOW_MS {
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
                if self.left_armed && input.ms.wrapping_sub(self.left_at) <= DOUBLE_TAP_WINDOW_MS {
                    self.roll_pending = -1;
                    self.left_armed = false;
                } else {
                    self.left_at = input.ms;
                    self.left_armed = true;
                }
            }
            if pressed & RIGHT != 0 {
                if self.right_armed && input.ms.wrapping_sub(self.right_at) <= DOUBLE_TAP_WINDOW_MS
                {
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
    // One rail tick is eight world units. This keeps all simulation distance
    // arithmetic in 16 bits while representing the complete 360000-unit route.
    rail_ticks: u16,
    stage_id: u8,
    stage_info: StageInfo,
    transition_ticks: u16,
    score: u32,
    ticks: u16,
    menu_ticks: u16,
    menu_activity: bool,
    audio_head: u8,
    audio_len: u8,
    spawn_cursor: u8,
    spawn_segment: u8,
    shield: u8,
    bombs: u8,
    bomb_kills: u8,
    lives: u8,
    shot_cooldown: u8,
    invulnerability: u8,
    hit_flash_ticks: u8,
    roll_ticks: u8,
    roll_cooldown: u8,
    roll_direction: i8,
    respawn_ticks: u8,
    bank: i8,
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
}

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
            rail_ticks: 0,
            stage_id: 0,
            stage_info: StageInfo::FIRST,
            transition_ticks: 0,
            score: 0,
            ticks: 0,
            menu_ticks: 0,
            menu_activity: false,
            actors: [Actor::EMPTY; ACTORS],
            shots: [Shot::EMPTY; SHOTS],
            pickups: [Pickup::EMPTY; 4],
            explosions: [Explosion::EMPTY; 6],
            audio: [AudioEvent {
                effect: Effect::Fire,
                pan: 0,
            }; AUDIO_CAPACITY],
            audio_head: 0,
            audio_len: 0,
            spawn_cursor: 0,
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
            bank: 0,
            boss_z: 1800,
            boss_left: 20,
            boss_right: 20,
            boss_core: 60,
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
        self.controls.sample(input);
    }
    /// Host inspection starts at a useful mission location without changing the
    /// production binary. Late encounter records are aged on the following tick.
    #[cfg(feature = "host-tools")]
    pub fn preview_at(section: u8, local_distance: u16) -> Self {
        let mut game = Self::new();
        game.reset_run();
        game.stage_id = section.min(assets::SEGMENT_COUNT - 1);
        game.stage_info = assets::preview_stage_info(game.stage_id);
        game.rail_ticks = game.stage_id as u16 * SECTION_TICKS + local_distance.min(29_999) / 8;
        game.spawn_segment = game.segment_index();
        game.invulnerability = 0;
        if game.stage_info.boss_kind != 0 && local_distance / 8 >= BOSS_START_TICKS {
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
        game.reset_scene(Mode::Transition, (section as u16 + 1) * SECTION_TICKS);
        game.stage_id = section;
        game.stage_info = assets::preview_stage_info(section);
        if elapsed_ticks >= TRANSITION_TICKS {
            game.mode = Mode::Playing;
            game.stage_id += 1;
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
        game.reset_scene(Mode::Victory, MISSION_TICKS);
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
    /// Continuous world position including completed bridges. The attract tour
    /// retains its original route and timing.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
    pub fn scenery_ticks(&self) -> u16 {
        if matches!(self.mode, Mode::Title | Mode::Attract) {
            self.rail_ticks
        } else {
            self.rail_ticks + self.segment_index() as u16 * TRANSITION_TICKS + self.transition_ticks
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
    pub fn scenery_camera_y(&self) -> i16 {
        chase_camera_y(self.mode, self.player.y)
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
            shield: self.shield,
            bombs: self.bombs,
            lives: self.lives,
            charge: self.controls.charge(),
            score: self.score,
            distance: self.rail_ticks as u32 * 8,
            section: self.segment_index(),
            roll_ticks: self.roll_ticks,
            boss_shield: if self.boss_started {
                self.boss_left + self.boss_right + self.boss_core
            } else {
                0
            },
            boss_kind: self.stage_info.boss_kind,
            victory_ticks: self.victory_ticks(),
        }
    }
    #[inline(never)]
    pub fn music_state(&self) -> MusicState {
        match self.mode {
            Mode::Title => MusicState::Title,
            Mode::Victory => MusicState::Victory,
            Mode::GameOver => MusicState::Defeat,
            _ => MusicState::from_cue(if self.boss_started {
                9 + self.stage_info.boss_kind
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
        let e = self.audio[self.audio_head as usize];
        self.audio_head = (self.audio_head + 1) % AUDIO_CAPACITY as u8;
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
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn sound(&mut self, effect: Effect, x: i16) {
        // Keep the live previews audible as music only.
        if self.mode == Mode::Attract {
            return;
        }
        let event = AudioEvent {
            effect,
            pan: x.clamp(-127, 127) as i8,
        };
        // Reserve an event for urgent player cues even when a full formation fires.
        if self.audio_len as usize == AUDIO_CAPACITY {
            if matches!(
                effect,
                Effect::Death | Effect::Bomb | Effect::Hit | Effect::MissileLaunch
            ) {
                let last =
                    (self.audio_head as usize + self.audio_len as usize - 1) % AUDIO_CAPACITY;
                self.audio[last] = event;
            }
            return;
        }
        let i = (self.audio_head as usize + self.audio_len as usize) % AUDIO_CAPACITY;
        self.audio[i] = event;
        self.audio_len += 1;
    }
    #[inline(always)]
    fn reset_run(&mut self) {
        self.reset_scene(Mode::Playing, 0);
        self.invulnerability = 50;
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.terminal")]
    fn reset_scene(&mut self, mode: Mode, rail_ticks: u16) {
        let bits = self.controls.bits;
        let now = self.controls.now;
        self.reset_in_place();
        self.presentation_event = PresentationEvent::Reset;
        self.mode = mode;
        self.rail_ticks = rail_ticks;
        self.stage_id = (rail_ticks / SECTION_TICKS).min((assets::SEGMENT_COUNT - 1) as u16) as u8;
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
        self.boss_left = 20;
        self.boss_right = 20;
        self.boss_core = 60;
        self.stage_info = StageInfo::FIRST;
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
            let boundary = (self.stage_id as u16 + 1) * SECTION_TICKS;
            let next = (self.rail_ticks + 1).min(boundary);
            if next == boundary
                && self.stage_id + 1 < assets::SEGMENT_COUNT
                && (self.stage_info.boss_kind == 0 || self.boss_defeated)
            {
                // Retire outgoing hazards before handling this tick's live
                // controls. Player projectiles and gestures remain intact.
                self.rail_ticks = next;
                self.mode = Mode::Transition;
                self.transition_ticks = 0;
                if self.stage_info.boss_kind != 0 {
                    self.shield = 100;
                    self.bombs = self.bombs.max(2);
                    self.lives = (self.lives + 1).min(3);
                }
                self.actors = [Actor::EMPTY; ACTORS];
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
        self.rail_ticks = (self.rail_ticks + 1).min((self.stage_id as u16 + 1) * SECTION_TICKS);
        let section = self.segment_index();
        if self.spawn_segment != section {
            self.spawn_segment = section;
            self.spawn_cursor = 0;
        }
        if segment.index() == section && bank.level() == section {
            let local = (self.rail_ticks - section as u16 * SECTION_TICKS) * 8;
            while self.spawn_cursor < segment.spawn_count() {
                let spawn = segment.spawn(self.spawn_cursor);
                if spawn.distance > local {
                    break;
                }
                self.spawn_cursor += 1;
                if self.stage_info.boss_kind == 0 || !self.boss_defeated {
                    self.spawn_actor(spawn, local - spawn.distance, bank);
                }
            }
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
        if self.mode == Mode::Playing && self.rail_ticks >= MISSION_TICKS && self.boss_defeated {
            self.mode = Mode::Victory;
            self.menu_ticks = 0;
            self.shots = [Shot::EMPTY; SHOTS];
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
                        self.reset_scene(Mode::Attract, 0);
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
                    self.reset_scene(Mode::Title, 0);
                    return;
                }
                self.menu_ticks += 1;
                if self.menu_ticks == ATTRACT_TICKS {
                    let next = match self.stage_id {
                        0 => 1,
                        1 => 6,
                        6 => 10,
                        _ => 12,
                    };
                    if next < assets::SEGMENT_COUNT {
                        self.reset_scene(Mode::Attract, next as u16 * SECTION_TICKS);
                    } else {
                        self.reset_scene(Mode::Title, 0);
                    }
                    return;
                }
                self.rail_ticks += 1;
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
                    #[cfg(any(test, feature = "host-tools"))]
                    {
                        self.stage_info = assets::preview_stage_info(self.stage_id);
                    }
                    self.spawn_segment = self.segment_index();
                    self.spawn_cursor = 0;
                    self.boss_started = false;
                    self.boss_defeated = false;
                    self.boss_z = 1800;
                    self.boss_left = 20;
                    self.boss_right = 20;
                    self.boss_core = 60;
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
            self.reset_run();
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
            self.sound(Effect::Roll, 0);
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
    #[inline(never)]
    fn move_player(&mut self) {
        let dx = ((self.controls.bits & RIGHT != 0) as i16
            - (self.controls.bits & LEFT != 0) as i16)
            * 3;
        let dy =
            ((self.controls.bits & UP != 0) as i16 - (self.controls.bits & DOWN != 0) as i16) * 3;
        // The first 300 ms of a roll adds a tapering sideways burst. Steering
        // remains live, and the ship keeps its new position when the burst ends.
        let dodge = if self.roll_ticks > 20 {
            6
        } else if self.roll_ticks > 15 {
            3
        } else if self.roll_ticks > 10 {
            1
        } else {
            0
        };
        self.player.x = (self.player.x + dx + self.roll_direction as i16 * dodge).clamp(-100, 100);
        self.player.y = (self.player.y + dy).clamp(-65, 65);
        let target = (-dx * 4) as i8;
        if self.bank < target {
            self.bank += 2;
        } else if self.bank > target {
            self.bank -= 2;
        }
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
        self.sound(
            if charge == 255 {
                Effect::MissileLaunch
            } else {
                Effect::Fire
            },
            0,
        );
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn add_shot(&mut self, shot: Shot) {
        if let Some(slot) = self.shots.iter_mut().find(|s| s.kind == 0) {
            *slot = shot;
        } else if shot.kind == 1 || shot.kind == 3 {
            if let Some(slot) = self.shots.iter_mut().find(|s| s.kind == 2) {
                *slot = shot;
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
        self.add_shot(Shot {
            pos: position,
            dx,
            dy,
            damage: 12,
            kind: 2,
        });
        if pattern == 2 {
            for spread in [-2i8, 2] {
                self.add_shot(Shot {
                    pos: position,
                    dx: (dx + spread).clamp(-8, 8),
                    dy,
                    damage: 12,
                    kind: 2,
                });
            }
        }
        self.sound(Effect::EnemyFire, position.x);
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn spawn_actor(&mut self, spawn: Spawn, late: u16, bank: &AssetBank<'_>) {
        // Handles host inspection and a delayed segment handoff without replaying
        // an entire section's expired encounters at once.
        if late >= 1600 {
            return;
        }
        let z = 1800 - late as i16;
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
        let hp = if obstacle { 255 } else { spec.hp };
        if let Some(slot) = self.actors.iter_mut().find(|a| a.kind == 0) {
            *slot = Actor {
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
            let radius = if obstacle {
                36
            } else {
                bank.enemy(a.kind).ram_radius as i16
            };
            if self.mode == Mode::Playing
                && (a.pos.z - PLAYER_Z).abs() < if obstacle { 32 } else { ENEMY_RAM_DEPTH }
                && close_xy(actor_center_sim(&a, bank), self.player, radius)
            {
                a.kind = 0;
                self.damage_player(30);
                self.explode(a.pos, 8);
            } else if a.pos.z < 40 {
                a.kind = 0;
            }
            self.actors[i] = a;
        }
    }
    #[cfg(not(target_arch = "mos"))]
    fn update_boss(&mut self) {
        self.update_boss_for(self.stage_info.boss_kind);
    }
    #[inline(always)]
    fn update_boss_for(&mut self, kind: u8) {
        if kind == 0
            || self.rail_ticks - self.stage_id as u16 * SECTION_TICKS < BOSS_START_TICKS
            || self.boss_defeated
        {
            return;
        }
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
            let x = 14 * assets::ENEMY_SCALE;
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
                            y: BOSS_TURRET_Y + 18 * assets::ENEMY_SCALE,
                            z: self.boss_z + BOSS_TURRET_Z - 28 * assets::ENEMY_SCALE,
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
                self.steer_missile(&mut s, bank);
            }
            let old_z = s.pos.z;
            s.pos.x += s.dx as i16;
            s.pos.y += s.dy as i16;
            s.pos.z += match s.kind {
                1 => 48,
                3 => 32,
                _ => -18,
            };
            if s.pos.z > 2400 || s.pos.z < 32 || s.pos.x.abs() > 1000 || s.pos.y.abs() > 600 {
                s.kind = 0;
            }
            if s.kind == 1 || s.kind == 3 {
                for a in 0..ACTORS {
                    let target = &self.actors[a];
                    if target.kind == 0 {
                        continue;
                    }
                    let radius = if matches!(target.kind, assets::ROCK | assets::BUILDING) {
                        35
                    } else {
                        bank.enemy(target.kind).shot_radius as i16
                    };
                    if swept_z(old_z, s.pos.z, target.pos.z, radius)
                        && close_xy(s.pos, actor_center_sim(target, bank), radius)
                    {
                        let target_hp = target.hp;
                        let target_x = target.pos.x;
                        s.kind = 0;
                        if target_hp != 255 {
                            self.actors[a].hp = self.actors[a].hp.saturating_sub(s.damage);
                            if self.actors[a].hp == 0 {
                                self.kill_actor(a, bank);
                            } else {
                                self.sound(Effect::Hit, target_x);
                            }
                        } else {
                            self.explode(s.pos, 15);
                        }
                        break;
                    }
                }
                if s.kind != 0 && self.boss_started && !self.boss_defeated {
                    let mut hit = false;
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
                        *turret_hp = turret_hp.saturating_sub(s.damage);
                        hit = true;
                    } else if swept_z(old_z, s.pos.z, self.boss_z + BOSS_CORE_Z, BOSS_HIT_DEPTH)
                        && close_xy(s.pos, Vec3 { x: 0, y: 0, z: 0 }, BOSS_CORE_RADIUS)
                    {
                        if self.boss_left == 0 && self.boss_right == 0 {
                            self.boss_core = self.boss_core.saturating_sub(s.damage);
                        }
                        hit = true;
                    }
                    if hit {
                        s.kind = 0;
                        self.sound(Effect::Hit, s.pos.x);
                        self.check_boss_death();
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
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn missile_target(&self, position: Vec3, bank: &AssetBank<'_>) -> Option<(Vec3, i16)> {
        let mut best = None;
        let mut best_z = i16::MAX;
        let mut best_xy = i16::MAX;
        // Stable order breaks exact ties: actor slots, left turret, right
        // turret, exposed core. Reacquisition needs no persistent actor ID.
        for i in 0..ACTORS + 3 {
            let (target, closing) = if i < ACTORS {
                let actor = self.actors[i];
                let spec = bank.enemy(actor.kind);
                if actor.hp == 0 || spec.hp == 0 {
                    continue;
                }
                // Reacquisition scans every actor for every live missile.
                // Keep this tiny descriptor read in the hot loop: an outlined
                // transform spills its target/order state on the 6502.
                (
                    Vec3 {
                        y: actor.pos.y + spec.center_y as i16,
                        ..actor.pos
                    },
                    32 + actor.approach_speed as i16,
                )
            } else {
                if !self.boss_started || self.boss_defeated {
                    continue;
                }
                let (x, y, z) = match i - ACTORS {
                    0 if self.boss_left > 0 => (-BOSS_TURRET_X, BOSS_TURRET_Y, BOSS_TURRET_Z),
                    1 if self.boss_right > 0 => (BOSS_TURRET_X, BOSS_TURRET_Y, BOSS_TURRET_Z),
                    2 if self.boss_left == 0 && self.boss_right == 0 && self.boss_core > 0 => {
                        (0, 0, BOSS_CORE_Z)
                    }
                    _ => continue,
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
            let dz = target.z - position.z;
            let xy = (target.x - position.x).abs() + (target.y - position.y).abs();
            if dz > 0 && (dz < best_z || (dz == best_z && xy < best_xy)) {
                best = Some((target, closing));
                best_z = dz;
                best_xy = xy;
            }
        }
        best
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn steer_missile(&self, shot: &mut Shot, bank: &AssetBank<'_>) {
        let (dx, dy) = if let Some((target, closing)) = self.missile_target(shot.pos, bank) {
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
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn kill_actor(&mut self, index: usize, bank: &AssetBank<'_>) {
        let a = self.actors[index];
        self.actors[index].kind = 0;
        self.score += bank.enemy(a.kind).score as u32;
        self.kills = self.kills.wrapping_add(1);
        // Keep the five-kill cadence independent of the wrapping total and
        // consume the milestone even when the three-bomb inventory is full.
        self.bomb_kills += 1;
        if self.bomb_kills == 5 {
            self.bomb_kills = 0;
            self.bombs = (self.bombs + 1).min(3);
        }
        self.explode(a.pos, 7);
        self.sound(Effect::Death, a.pos.x);
        if a.flags & 1 != 0 {
            self.add_pickup(a.pos, 2);
        } else if self.kills % 4 == 0 {
            self.add_pickup(a.pos, 1);
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
            if self.actors[i].kind != 0 && self.actors[i].hp != 255 {
                self.actors[i].hp = self.actors[i].hp.saturating_sub(8);
                if self.actors[i].hp == 0 {
                    self.kill_actor(i, bank);
                }
            }
        }
        if self.boss_started && !self.boss_defeated {
            let open = self.boss_left == 0 && self.boss_right == 0;
            self.boss_left = self.boss_left.saturating_sub(8);
            self.boss_right = self.boss_right.saturating_sub(8);
            if open {
                self.boss_core = self.boss_core.saturating_sub(8);
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
            self.score += 5000;
            self.sound(Effect::Death, 0);
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
        self.shield = self.shield.saturating_sub(amount);
        self.invulnerability = 35;
        self.hit_flash_ticks = 35;
        self.sound(Effect::Hit, 0);
        if self.shield == 0 {
            self.lives = self.lives.saturating_sub(1);
            self.explode(self.player, 7);
            self.sound(Effect::Death, 0);
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
        self.rail_ticks = self.segment_index() as u16 * SECTION_TICKS;
        self.transition_ticks = 0;
        self.spawn_segment = self.segment_index();
        self.spawn_cursor = 0;
        self.actors = [Actor::EMPTY; ACTORS];
        self.shots = [Shot::EMPTY; SHOTS];
        self.pickups = [Pickup::EMPTY; 4];
        self.explosions = [Explosion::EMPTY; 6];
        self.player = Vec3 {
            x: 0,
            y: 0,
            z: PLAYER_Z,
        };
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
        self.boss_left = 20;
        self.boss_right = 20;
        self.boss_core = 60;
        self.controls.fire_armed = false;
        self.controls.left_armed = false;
        self.controls.right_armed = false;
        self.controls.released_at = self.controls.now;
        self.controls.cached_charge = 0;
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn add_pickup(&mut self, pos: Vec3, kind: u8) {
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
                if p.kind == 1
                    && depth as u16 <= SHIELD_MAGNET_DEPTH as u16
                    && close_xy(p.pos, self.player, SHIELD_MAGNET_RANGE)
                {
                    // Move the visible shield gently toward the ship, leaving the
                    // collection hitbox unchanged and allowing steering to outrun it.
                    p.pos.x = magnet_coordinate(p.pos.x, self.player.x);
                    p.pos.y = magnet_coordinate(p.pos.y, self.player.y);
                }
                if depth.wrapping_add(24) as u16 <= 48 && close_xy(p.pos, self.player, 28) {
                    let kind = p.kind;
                    p.kind = 0;
                    if kind == 1 {
                        self.shield = self.shield.saturating_add(30).min(100);
                    } else {
                        self.bombs = (self.bombs + 1).min(3);
                    }
                    self.score += 50;
                    self.sound(Effect::Pickup, 0);
                    continue;
                }
            }
            if p.pos.z < 40 {
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
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
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
    #[cfg(not(target_arch = "mos"))]
    pub fn render(
        &self,
        sink: &mut impl GeometrySink,
        bank: &AssetBank<'_>,
        segment: &Segment<'_>,
    ) {
        struct CountFaces(u16);
        impl GeometrySink for CountFaces {
            fn triangle(&mut self, _: Vec3, _: Vec3, _: Vec3, _: u8, _: u8) {
                self.0 += 1;
            }
        }
        let mut scenery = CountFaces(0);
        self.render_scenery(&mut scenery, segment);
        let foreground_limit = 174.min(233u16.saturating_sub(scenery.0));
        if self.mode == Mode::Victory {
            struct Fade<'a, S> {
                sink: &'a mut S,
                ticks: u16,
            }
            impl<S: GeometrySink> GeometrySink for Fade<'_, S> {
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
            self.render_foreground(&mut world, bank, foreground_limit);
            self.render_scenery(&mut world, segment);
            self.render_medal(sink, bank);
        } else {
            self.render_foreground(sink, bank, foreground_limit);
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
        crate::victory::emit(&mut budget, bank, self.menu_ticks, self.ticks);
    }
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
        // 233 raw faces leave room for near-plane splitting in the 256 commands.
        let mut budget = SceneBudget {
            sink,
            count: 0,
            limit: foreground_limit.min(174),
            // An elevated chase camera separates the nearby ship from targets
            // on its firing line. Keep the title's original showcase framing.
            // Follow a quarter of vertical movement so the lowest banked hull
            // stays above the cockpit while neutral targets remain exposed.
            camera_y: chase_camera_y(self.mode, self.player.y),
            color_override: 0,
            extra_flags: 0,
        };
        let sink = &mut budget;
        if matches!(
            self.mode,
            Mode::Playing | Mode::Transition | Mode::Title | Mode::Victory
        ) {
            let appearance = self.player_appearance();
            sink.extra_flags = TERRAIN_CLEAR_GEOMETRY;
            sink.color_override = (appearance >> 8) as u8;
            emit_mesh(
                sink,
                bank,
                assets::PLAYER,
                self.player,
                0,
                0,
                appearance as u8,
                256,
            );
            sink.color_override = 0;
            // A pair of flat exhaust triangles; deliberately no particles/textures.
            for x in [-11, 11] {
                let p = Vec3 {
                    x: self.player.x + x,
                    y: self.player.y - 3,
                    z: PLAYER_Z - 20,
                };
                tri(
                    sink,
                    Vec3 {
                        x: p.x - 3,
                        y: p.y,
                        z: p.z,
                    },
                    Vec3 {
                        x: p.x + 3,
                        y: p.y,
                        z: p.z,
                    },
                    Vec3 {
                        x: p.x,
                        y: p.y,
                        z: p.z
                            - 12
                            - (if self.mode == Mode::Victory {
                                self.ticks.wrapping_sub(self.menu_ticks)
                            } else {
                                self.ticks
                            } & 7) as i16,
                    },
                    8,
                );
            }
        }
        sink.extra_flags = 0;
        // World-space aiming reticle, held on the player's firing line.
        if matches!(self.mode, Mode::Playing | Mode::Transition) {
            let center = Vec3 {
                x: self.player.x,
                y: self.player.y,
                z: 800,
            };
            let color = if self.controls.charge() == 255 { 7 } else { 15 };
            for side in [-1, 1] {
                let x = center.x + side * 15;
                tri(
                    sink,
                    Vec3 {
                        x,
                        y: center.y - 7,
                        z: center.z,
                    },
                    Vec3 {
                        x: x + side * 2,
                        y: center.y,
                        z: center.z,
                    },
                    Vec3 {
                        x,
                        y: center.y + 7,
                        z: center.z,
                    },
                    color,
                );
            }
        }
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
                diamond(
                    sink,
                    s.pos,
                    if s.kind == 2 { 5 } else { 3 + s.damage as i16 },
                    if s.kind == 2 {
                        2
                    } else if s.damage > 1 {
                        7
                    } else {
                        3
                    },
                );
            }
        }
        for p in &self.pickups {
            if p.kind == 1 {
                shield_pickup(sink, p.pos);
            } else if p.kind != 0 {
                diamond(sink, p.pos, 13, 7);
            }
        }
        for e in &self.explosions {
            if e.ticks > 0 {
                diamond(sink, e.pos, 4 + (18 - e.ticks) as i16 * 3, e.color);
            }
        }
    }
    // Native scenic flights load only the shared GEOMETRY prefix. This tail
    // remains reachable exclusively while the complete combat overlay is loaded.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.geometry.combat")]
    fn render_combat(&self, sink: &mut SceneBudget<'_, impl GeometrySink>, bank: &AssetBank<'_>) {
        if self.boss_started && !self.boss_defeated {
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
                256,
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
                    256,
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
                    256,
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
                    22 * assets::ENEMY_SCALE,
                    if self.ticks & 8 == 0 { 7 } else { 2 },
                );
            }
        }
        self.render_effects(sink);
        for (i, a) in self.actors.iter().enumerate() {
            if a.kind != 0 && a.pos.z >= 80 {
                // Authored ground guns and their proxies are terrain-clear:
                // Lowlands fits the flight corridor; Fortress lies above its
                // low shoulders. Its first spawn follows the Canyon bridge.
                // Wall batteries, boss turrets and obstacles keep normal order.
                sink.extra_flags = if a.kind == assets::TURRET
                    && matches!(self.stage_info.enemy_profile, 0 | 2 | 3)
                {
                    TERRAIN_CLEAR_GEOMETRY
                } else {
                    0
                };
                let reserve = (ACTORS - i - 1) as u16 * 2;
                if a.pos.z > 1200
                    || sink.count + bank.face_count(a.kind) as u16 + reserve > sink.limit
                {
                    diamond(
                        sink,
                        actor_center(*a, bank),
                        if a.kind == assets::FIGHTER {
                            22 * assets::ENEMY_SCALE
                        } else if a.kind <= bank.enemy_count() {
                            18 * assets::ENEMY_SCALE
                        } else {
                            18
                        },
                        if a.hp == 255 { 12 } else { 2 },
                    );
                } else {
                    let roll = if bank.enemy(a.kind).render_mode == 1 {
                        (a.age as u8).wrapping_mul(2)
                    } else {
                        0
                    };
                    emit_mesh(sink, bank, a.kind, a.pos, 0, 0, roll, 256);
                }
                sink.extra_flags = 0;
            }
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
    pub fn render_scenery(
        &self,
        #[cfg(target_arch = "mos")] sink: &mut crate::render::SceneryStage<'_>,
        #[cfg(not(target_arch = "mos"))] sink: &mut impl GeometrySink,
        segment: &Segment<'_>,
    ) {
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
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
    fn player_appearance(&self) -> u16 {
        let roll = if self.roll_ticks > 0 {
            ((25 - self.roll_ticks) as i16 * 256 / 25 * self.roll_direction as i16) as u8
        } else {
            self.bank as u8
        };
        // 200 ms phases remain distinct at the current rendering cadence.
        let white = self.mode == Mode::Playing
            && (self.hit_flash_ticks > 25
                || (self.hit_flash_ticks > 5 && self.hit_flash_ticks <= 15));
        roll as u16 | ((white as u16) << 8)
    }
    #[cfg(test)]
    fn terrain(&self, sink: &mut impl GeometrySink, segment: &Segment<'_>) {
        crate::scenery::emit(
            sink,
            segment,
            self.rail_ticks,
            self.segment_index(),
            None,
            0,
            true,
        );
    }
}

struct SceneBudget<'a, S: GeometrySink> {
    sink: &'a mut S,
    count: u16,
    limit: u16,
    camera_y: i16,
    color_override: u8,
    extra_flags: u8,
}
impl<S: GeometrySink> GeometrySink for SceneBudget<'_, S> {
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
    fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
        if self.count < self.limit {
            self.count += 1;
            let camera_y = if flags & STATIC_SKY != 0 {
                0
            } else {
                self.camera_y
            };
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
        }
    }
}
// Keep this small, shared transform resident so MOS does not duplicate its
// saturating arithmetic and value marshaling three times in the geometry phase.
#[inline(never)]
fn camera_point(p: Vec3, camera_y: i16) -> Vec3 {
    Vec3 {
        y: p.y.saturating_sub(camera_y),
        ..p
    }
}
#[inline(never)]
fn chase_camera_y(mode: Mode, player_y: i16) -> i16 {
    if mode == Mode::Title {
        0
    } else {
        40 + player_y / 4
    }
}
#[inline(always)]
fn actor_center_sim(actor: &Actor, bank: &AssetBank<'_>) -> Vec3 {
    Vec3 {
        x: actor.pos.x,
        y: actor_center_y(actor.kind, actor.pos.y, bank),
        z: actor.pos.z,
    }
}
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.sim_aux")]
fn actor_center_y(kind: u8, y: i16, bank: &AssetBank<'_>) -> i16 {
    y + match kind {
        assets::ROCK => 20,
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
                assets::ROCK => 20,
                assets::BUILDING => 30,
                _ => bank.enemy(actor.kind).center_y as i16,
            },
    }
}
// Shared collision bounds stay resident to leave room for the shield magnet
// and benchmark instrumentation in the composed SIM overlay.
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
    if delta > SHIELD_MAGNET_SPEED as i8 {
        current + SHIELD_MAGNET_SPEED
    } else if delta < -SHIELD_MAGNET_SPEED as i8 {
        current - SHIELD_MAGNET_SPEED
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
#[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
fn missile_velocity(error: i16, closing: i16, dz: i16) -> i8 {
    // Clamp before multiplying so all guidance arithmetic stays signed 16-bit.
    ((error.clamp(-512, 512) * closing) / dz).clamp(-12, 12) as i8
}
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
fn tri(sink: &mut impl GeometrySink, a: Vec3, b: Vec3, c: Vec3, color: u8) {
    sink.triangle(a, b, c, color, TWO_SIDED);
}
// Each byte selects x from its low two bits and y from its next two bits.
// Keep the original face order: equal-depth sorting preserves the shield rim
// before its inset, and partial scene budgets retain the same triangle prefix.
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry.data")]
static DIAMOND_FACES: [[u8; 3]; 2] = [[4, 9, 6], [4, 6, 1]];
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry.data")]
static SHIELD_FACES: [[u8; 3]; 3] = [[8, 10, 6], [8, 6, 4], [4, 6, 1]];

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
fn flat_shape(sink: &mut impl GeometrySink, p: Vec3, r: i16, color: u8, shield: bool) {
    // The spare center entry makes every masked selector a valid array index.
    let x = [p.x - r, p.x, p.x + r, p.x];
    let y = [p.y - r, p.y, p.y + r, p.y];
    let faces: &[[u8; 3]] = if shield {
        &SHIELD_FACES
    } else {
        &DIAMOND_FACES
    };
    for face in faces {
        let mut points = [p; 3];
        for corner in 0..3 {
            let selector = face[corner];
            points[corner].x = x[(selector & 3) as usize];
            points[corner].y = y[((selector >> 2) & 3) as usize];
        }
        tri(sink, points[0], points[1], points[2], color);
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
fn diamond(sink: &mut impl GeometrySink, p: Vec3, r: i16, color: u8) {
    flat_shape(sink, p, r, color, false);
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
fn shield_pickup(sink: &mut impl GeometrySink, p: Vec3) {
    // Paint the pale rim before the green inset. Equal depths retain this
    // order, keeping the camera-facing shield to six triangles in total.
    // Use green for subpixel rims: a distant gray edge can otherwise
    // displace the green face when terrain competes for cell colors.
    let rim = if p.z > 600 { 5 } else { 15 };
    flat_shape(sink, p, 13, rim, true);
    flat_shape(sink, p, 10, 5, true);
}

#[cfg(test)]
mod tests;
