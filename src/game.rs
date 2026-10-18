//! Deterministic 50 Hz rail-shooter simulation. Input is sampled independently at
//! display refresh rate, so PAL and NTSC use the same millisecond gesture windows.
use crate::assets::{self, AssetBank, Segment, Spawn};
use crate::audio::{AudioEvent, Effect, MusicState};
use crate::{emit_mesh, GeometrySink, Vec3, TWO_SIDED};

pub const UP: u8 = 1;
pub const DOWN: u8 = 2;
pub const LEFT: u8 = 4;
pub const RIGHT: u8 = 8;
pub const FIRE: u8 = 16;
pub const MISSION_DISTANCE: u32 = 120_000;
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
const SECTION_TICKS: u16 = 3750;
const MISSION_TICKS: u16 = 15000;
const ATTRACT_TICKS: u16 = 750;
const ACTORS: usize = 12;
const SHOTS: usize = 24;
const AUDIO_CAPACITY: usize = 8;
const DOUBLE_TAP_WINDOW_MS: u32 = 500;

mod enemies;

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
}

#[repr(C)]
#[derive(Clone, Copy)]
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
        approach_speed: 8,
    };
}
#[derive(Clone, Copy)]
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
pub struct Game {
    controls: Controls,
    mode: Mode,
    presentation_event: PresentationEvent,
    player: Vec3,
    // One rail tick is eight world units. This keeps all simulation distance
    // arithmetic in 16 bits while representing the complete 120000-unit route.
    rail_ticks: u16,
    transition_ticks: u16,
    score: u32,
    ticks: u16,
    menu_ticks: u16,
    menu_activity: bool,
    actors: [Actor; ACTORS],
    shots: [Shot; SHOTS],
    pickups: [Pickup; 4],
    explosions: [Explosion; 6],
    audio: [AudioEvent; AUDIO_CAPACITY],
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
        game.rail_ticks = section.min(3) as u16 * SECTION_TICKS + local_distance.min(29_999) / 8;
        game.spawn_segment = game.segment_index();
        game.invulnerability = 0;
        if game.rail_ticks >= 11_400 {
            game.boss_started = true;
            game.boss_z = 680;
        }
        game
    }
    /// Inspect a scenic bridge without replaying the preceding combat section.
    #[cfg(feature = "host-tools")]
    pub fn preview_transition(section: u8, elapsed_ticks: u16) -> Self {
        let section = section.min(2);
        let mut game = Self::new();
        game.reset_scene(Mode::Transition, (section as u16 + 1) * SECTION_TICKS);
        if elapsed_ticks >= TRANSITION_TICKS {
            game.mode = Mode::Playing;
            game.spawn_segment = game.segment_index();
        } else {
            game.transition_ticks = elapsed_ticks;
        }
        game
    }
    #[inline(never)]
    pub fn segment_index(&self) -> u8 {
        let ticks = self
            .rail_ticks
            .saturating_sub(u16::from(self.mode == Mode::Transition));
        if ticks < SECTION_TICKS {
            0
        } else if ticks < SECTION_TICKS * 2 {
            1
        } else if ticks < SECTION_TICKS * 3 {
            2
        } else {
            3
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
        crate::scenery::sky_color(self.segment_index(), self.transition_progress())
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
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    pub fn music_state(&self) -> MusicState {
        match self.mode {
            Mode::Title => MusicState::Title,
            Mode::Victory => MusicState::Victory,
            Mode::GameOver => MusicState::Defeat,
            _ => match self.segment_index() {
                0 => MusicState::Lowlands,
                1 => MusicState::Canyon,
                2 => MusicState::Fortress,
                _ => MusicState::Carrier,
            },
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
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn reset_run(&mut self) {
        self.reset_scene(Mode::Playing, 0);
        self.invulnerability = 50;
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn reset_scene(&mut self, mode: Mode, rail_ticks: u16) {
        let bits = self.controls.bits;
        let now = self.controls.now;
        *self = Self::new();
        self.presentation_event = PresentationEvent::Reset;
        self.mode = mode;
        self.rail_ticks = rail_ticks;
        self.spawn_segment = self.segment_index();
        self.controls.bits = bits;
        self.controls.now = now;
        self.controls.released_at = now;
    }
    /// One 20 ms simulation step. Feed every captured input sample first.
    /// `bank` must belong to `segment_index()` before this step. If the step
    /// changes sections, select the new bank before the next step or rendering.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    pub fn tick(&mut self, segment: &Segment<'_>, bank: &AssetBank<'_>) {
        self.ticks = self.ticks.wrapping_add(1);
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
                    let next = self.segment_index() + 1;
                    if next < 4 {
                        self.reset_scene(Mode::Attract, next as u16 * SECTION_TICKS);
                    } else {
                        self.reset_scene(Mode::Title, 0);
                    }
                    return;
                }
            }
            Mode::Victory | Mode::GameOver => {
                if self.controls.start_pending {
                    self.reset_run();
                }
                self.controls.clear_pending();
                return;
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
                    self.spawn_segment = self.segment_index();
                    self.spawn_cursor = 0;
                }
                return;
            }
            Mode::Playing => {
                let next = self.rail_ticks + 1;
                if matches!(next, SECTION_TICKS | 7500 | 11250) {
                    // Retire outgoing hazards before handling this tick's live
                    // controls. Player projectiles and gestures remain intact.
                    self.rail_ticks = next;
                    self.mode = Mode::Transition;
                    self.transition_ticks = 0;
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
            }
        }
        if self.mode == Mode::Playing {
            self.update_player_controls(bank);
        }
        self.controls.clear_pending();
        self.rail_ticks = (self.rail_ticks + 1).min(MISSION_TICKS);
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
                if section != 3 || !self.boss_defeated {
                    self.spawn_actor(spawn, local - spawn.distance, bank);
                }
            }
        }
        self.update_actors(bank);
        self.update_boss();
        self.update_shots(bank);
        self.update_pickups();
        self.update_explosions();
        if self.mode == Mode::Playing && self.rail_ticks >= MISSION_TICKS && self.boss_defeated {
            self.mode = Mode::Victory;
            self.shots = [Shot::EMPTY; SHOTS];
            self.sound(Effect::Pickup, 0);
        }
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
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn enemy_attack(&mut self, position: Vec3, pattern: u8) {
        let dz = (position.z - PLAYER_Z).max(32);
        let dx = ((self.player.x.saturating_sub(position.x).clamp(-512, 512) * 18) / dz)
            .clamp(-8, 8) as i8;
        let dy = ((self.player.y.saturating_sub(position.y).clamp(-512, 512) * 18) / dz)
            .clamp(-8, 8) as i8;
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
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn update_actors(&mut self, bank: &AssetBank<'_>) {
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
                enemies::step(bank.level(), &mut a, &self.player, i as u8)
            };
            a.approach_speed = (old_z - a.pos.z).clamp(0, 31) as u8;
            if attack != 0 {
                self.enemy_attack(actor_center(a, bank), attack);
            }
            let radius = if obstacle {
                36
            } else {
                bank.enemy(a.kind).ram_radius as i16
            };
            if self.mode == Mode::Playing
                && (a.pos.z - PLAYER_Z).abs() < if obstacle { 32 } else { ENEMY_RAM_DEPTH }
                && close_xy(actor_center(a, bank), self.player, radius)
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
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn update_boss(&mut self) {
        if self.rail_ticks < 11_400 || self.boss_defeated {
            return;
        }
        self.boss_started = true;
        self.boss_z = (self.boss_z - 8).max(680);
        if self.boss_z > 900 {
            return;
        }
        let rate = if self.boss_left == 0 && self.boss_right == 0 {
            25
        } else {
            40
        };
        if self.ticks % rate == 0 {
            if self.boss_left > 0 {
                self.enemy_shot(Vec3 {
                    x: -BOSS_TURRET_X,
                    y: BOSS_TURRET_Y + 18 * assets::ENEMY_SCALE,
                    z: self.boss_z + BOSS_TURRET_Z - 28 * assets::ENEMY_SCALE,
                });
            }
            if self.boss_right > 0 {
                self.enemy_shot(Vec3 {
                    x: BOSS_TURRET_X,
                    y: BOSS_TURRET_Y + 18 * assets::ENEMY_SCALE,
                    z: self.boss_z + BOSS_TURRET_Z - 28 * assets::ENEMY_SCALE,
                });
            }
            if self.boss_left == 0 && self.boss_right == 0 {
                self.enemy_shot(Vec3 {
                    x: -14 * assets::ENEMY_SCALE,
                    y: 0,
                    z: self.boss_z + BOSS_CORE_Z,
                });
                self.enemy_shot(Vec3 {
                    x: 14 * assets::ENEMY_SCALE,
                    y: 0,
                    z: self.boss_z + BOSS_CORE_Z,
                });
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
                    let target = self.actors[a];
                    if target.kind == 0 {
                        continue;
                    }
                    let radius = if matches!(target.kind, assets::ROCK | assets::BUILDING) {
                        35
                    } else {
                        bank.enemy(target.kind).shot_radius as i16
                    };
                    if swept_z(old_z, s.pos.z, target.pos.z, radius)
                        && close_xy(s.pos, actor_center(target, bank), radius)
                    {
                        s.kind = 0;
                        if target.hp != 255 {
                            self.actors[a].hp = self.actors[a].hp.saturating_sub(s.damage);
                            if self.actors[a].hp == 0 {
                                self.kill_actor(a, bank);
                            } else {
                                self.sound(Effect::Hit, target.pos.x);
                            }
                        } else {
                            self.explode(s.pos, 15);
                        }
                        break;
                    }
                }
                if (s.kind == 1 || s.kind == 3) && self.boss_started && !self.boss_defeated {
                    let mut hit = false;
                    let turret_depth =
                        swept_z(old_z, s.pos.z, self.boss_z + BOSS_TURRET_Z, BOSS_HIT_DEPTH);
                    if self.boss_left > 0
                        && turret_depth
                        && close_xy(
                            s.pos,
                            Vec3 {
                                x: -BOSS_TURRET_X,
                                y: BOSS_TURRET_Y,
                                z: 0,
                            },
                            BOSS_TURRET_RADIUS,
                        )
                    {
                        self.boss_left = self.boss_left.saturating_sub(s.damage);
                        hit = true;
                    } else if self.boss_right > 0
                        && turret_depth
                        && close_xy(
                            s.pos,
                            Vec3 {
                                x: BOSS_TURRET_X,
                                y: BOSS_TURRET_Y,
                                z: 0,
                            },
                            BOSS_TURRET_RADIUS,
                        )
                    {
                        self.boss_right = self.boss_right.saturating_sub(s.damage);
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
                if actor.hp == 0 || bank.enemy(actor.kind).hp == 0 {
                    continue;
                }
                (actor_center(actor, bank), 32 + actor.approach_speed as i16)
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
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
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
        self.render_foreground(sink, bank, foreground_limit);
        self.render_scenery(sink, segment);
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
    pub fn render_foreground(
        &self,
        sink: &mut impl GeometrySink,
        bank: &AssetBank<'_>,
        foreground_limit: u16,
    ) {
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
        };
        let sink = &mut budget;
        if matches!(
            self.mode,
            Mode::Playing | Mode::Transition | Mode::Title | Mode::Victory
        ) {
            let appearance = self.player_appearance();
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
                        z: p.z - 12 - (self.ticks & 7) as i16,
                    },
                    8,
                );
            }
        }
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
        if self.mode == Mode::Transition {
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
            }
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
    pub fn render_scenery(&self, sink: &mut impl GeometrySink, segment: &Segment<'_>) {
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
                flags,
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
#[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
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
#[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
fn missile_velocity(error: i16, closing: i16, dz: i16) -> i8 {
    // Clamp before multiplying so all guidance arithmetic stays signed 16-bit.
    ((error.clamp(-512, 512) * closing) / dz).clamp(-12, 12) as i8
}
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
fn swept_z(from: i16, to: i16, target: i16, radius: i16) -> bool {
    from.min(to) <= target + radius && from.max(to) >= target - radius
}
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
fn tri(sink: &mut impl GeometrySink, a: Vec3, b: Vec3, c: Vec3, color: u8) {
    sink.triangle(a, b, c, color, TWO_SIDED);
}
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
fn quad(sink: &mut impl GeometrySink, a: Vec3, b: Vec3, c: Vec3, d: Vec3, color: u8) {
    tri(sink, a, b, c, color);
    tri(sink, a, c, d, color);
}
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
fn diamond(sink: &mut impl GeometrySink, p: Vec3, r: i16, color: u8) {
    quad(
        sink,
        Vec3 {
            x: p.x - r,
            y: p.y,
            z: p.z,
        },
        Vec3 {
            x: p.x,
            y: p.y + r,
            z: p.z,
        },
        Vec3 {
            x: p.x + r,
            y: p.y,
            z: p.z,
        },
        Vec3 {
            x: p.x,
            y: p.y - r,
            z: p.z,
        },
        color,
    );
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
fn shield_pickup(sink: &mut impl GeometrySink, p: Vec3) {
    // Paint the pale rim before the green inset. Equal depths retain this
    // order, keeping the camera-facing shield to six triangles in total.
    // Use green for subpixel rims: a distant gray edge can otherwise
    // displace the green face when terrain competes for cell colors.
    let rim = if p.z > 600 { 5 } else { 15 };
    for (r, color) in [(13, rim), (10, 5)] {
        let left = Vec3 {
            x: p.x - r,
            y: p.y,
            z: p.z,
        };
        let right = Vec3 { x: p.x + r, ..left };
        quad(
            sink,
            Vec3 { y: p.y + r, ..left },
            Vec3 {
                y: p.y + r,
                ..right
            },
            right,
            left,
            color,
        );
        tri(
            sink,
            left,
            right,
            Vec3 {
                x: p.x,
                y: p.y - r,
                z: p.z,
            },
            color,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Valid, immutable descriptor fixtures also keep no-default-feature tests
    // independent of the std-only authoring tools.
    const fn test_bank_bytes(level: u8) -> [u8; assets::COMMON_SIZE] {
        let mut data = [0; assets::COMMON_SIZE];
        data[0] = b'S';
        data[1] = b'M';
        data[2] = b'A';
        data[3] = b'2';
        data[4] = 8;
        data[5] = level;
        data[6] = if level == 3 { 2 } else { 3 };
        let health = [[3, 2, 6], [2, 3, 4], [4, 2, 6], [3, 2, 6]];
        let speeds = [[8, 12, 8], [10, 8, 8], [6, 8, 8], [8, 14, 8]];
        let mut kind = 0;
        while kind < 3 {
            let at = assets::ENEMY_SPECS_OFFSET + kind * 8;
            data[at] = health[level as usize][kind];
            data[at + 1] = if kind == 2 && (level == 0 || level == 2) {
                (18 * assets::ENEMY_SCALE - 8) as u8
            } else {
                0
            };
            data[at + 2] = (19 * assets::ENEMY_SCALE) as u8;
            data[at + 3] = (18 * assets::ENEMY_SCALE) as u8;
            data[at + 4] = if kind == 2 { 250 } else { 100 };
            data[at + 6] = speeds[level as usize][kind];
            data[at + 7] = if kind == 1 && level != 2 { 1 } else { 0 };
            kind += 1;
        }
        let mut mesh = 0;
        while mesh < 8 {
            let at = 16 + mesh * 8;
            data[at] = assets::MODEL_DATA_OFFSET as u8;
            data[at + 2] = 3;
            data[at + 3] = 1;
            data[at + 4] = (assets::MODEL_DATA_OFFSET + 18) as u8;
            mesh += 1;
        }
        data[assets::MODEL_DATA_OFFSET + 19] = 1;
        data[assets::MODEL_DATA_OFFSET + 20] = 2;
        data[assets::MODEL_DATA_OFFSET + 21] = 15;
        data
    }
    fn test_bank(level: u8) -> AssetBank<'static> {
        #[cfg(feature = "host-tools")]
        {
            static BANKS: std::sync::OnceLock<[[u8; assets::MODEL_SIZE]; 4]> =
                std::sync::OnceLock::new();
            let banks = BANKS.get_or_init(|| {
                core::array::from_fn(|level| enemies::authored::model_bank(level as u8))
            });
            AssetBank::new(&banks[level as usize]).unwrap()
        }
        #[cfg(not(feature = "host-tools"))]
        {
            static BANKS: [[u8; assets::COMMON_SIZE]; 4] = [
                test_bank_bytes(0),
                test_bank_bytes(1),
                test_bank_bytes(2),
                test_bank_bytes(3),
            ];
            AssetBank::new(&BANKS[level as usize]).unwrap()
        }
    }
    use crate::scenery::{
        backdrop_face, terrain_height, BACKDROP_CAMERA_Y, CLOUD_CENTERS, CLOUD_COLORS,
        CLOUD_OUTLINES, HORIZONS, HORIZON_COLORS, SKY_COLORS,
    };
    #[test]
    fn music_follows_area_and_results_without_boss_or_respawn_restarts() {
        let mut game = Game::new();
        assert_eq!(game.music_state(), MusicState::Title);
        for (index, cue) in [
            MusicState::Lowlands,
            MusicState::Canyon,
            MusicState::Fortress,
            MusicState::Carrier,
        ]
        .into_iter()
        .enumerate()
        {
            game.rail_ticks = index as u16 * 3750;
            for mode in [Mode::Playing, Mode::Attract, Mode::Respawning] {
                game.mode = mode;
                game.boss_started = false;
                assert_eq!(game.music_state(), cue);
                game.boss_started = true;
                assert_eq!(game.music_state(), cue);
            }
        }
        game.mode = Mode::Victory;
        assert_eq!(game.music_state(), MusicState::Victory);
        game.mode = Mode::GameOver;
        assert_eq!(game.music_state(), MusicState::Defeat);
        game.mode = Mode::Title;
        assert_eq!(game.music_state(), MusicState::Title);
    }

    fn empty_segment(index: u8) -> [u8; assets::SEGMENT_SIZE] {
        let mut bytes = [0; assets::SEGMENT_SIZE];
        bytes[..4].copy_from_slice(b"SMS1");
        bytes[4] = index;
        bytes[6] = 5;
        bytes
    }
    #[cfg(feature = "host-tools")]
    fn mission_segment(index: u8) -> [u8; assets::SEGMENT_SIZE] {
        enemies::authored::segment_bank(index)
    }
    #[cfg(not(feature = "host-tools"))]
    fn mission_segment(index: u8) -> [u8; assets::SEGMENT_SIZE] {
        let mut bytes = empty_segment(index);
        bytes[5] = index;
        let mut records = [Spawn::default(); 64];
        let mut count = 0;
        if index < 3 {
            for wave in 0..10u16 {
                for lane in 0..3i16 {
                    records[count] = Spawn {
                        distance: 1200 + wave * 2700 + lane as u16 * 100,
                        kind: if wave % 3 == 2 {
                            assets::DIVER
                        } else {
                            assets::FIGHTER
                        },
                        x: (lane - 1) * 80,
                        y: if wave % 2 == 0 { 35 } else { -25 },
                        flags: if wave % 4 == 0 && lane == 1 { 1 } else { 0 },
                    };
                    count += 1;
                }
                if wave % 2 == 1 {
                    records[count] = Spawn {
                        distance: 2400 + wave * 2700,
                        kind: assets::TURRET,
                        x: if wave % 4 == 1 { -95 } else { 95 },
                        y: -90,
                        flags: 0,
                    };
                    count += 1;
                }
            }
            for i in 0..6u16 {
                records[count] = Spawn {
                    distance: 2200 + i * 4300,
                    kind: if index == 2 {
                        assets::BUILDING
                    } else {
                        assets::ROCK
                    },
                    x: if i % 2 == 0 { -70 } else { 70 },
                    y: -110,
                    flags: 0,
                };
                count += 1;
            }
        }
        records[..count].sort_unstable_by_key(|s| s.distance);
        bytes[7] = count as u8;
        for (i, r) in records[..count].iter().enumerate() {
            let p = 16 + i * 8;
            bytes[p..p + 2].copy_from_slice(&r.distance.to_le_bytes());
            bytes[p + 2] = r.kind;
            bytes[p + 3] = r.flags;
            bytes[p + 4..p + 6].copy_from_slice(&r.x.to_le_bytes());
            bytes[p + 6..p + 8].copy_from_slice(&r.y.to_le_bytes());
        }
        bytes
    }
    fn running() -> Game {
        let mut game = Game::new();
        game.reset_run();
        game.invulnerability = 0;
        game
    }
    fn sample(game: &mut Game, bits: u8, ms: u32) {
        game.sample_input(InputSample {
            bits,
            pressed: 0,
            ms,
        });
    }
    fn queue_roll(game: &mut Game, direction: u8) {
        sample(game, direction, 100);
        sample(game, 0, 120);
        sample(game, direction, 200);
        sample(game, 0, 220);
    }
    #[test]
    fn scenic_bridges_add_exactly_eight_seconds_and_keep_the_outgoing_checkpoint() {
        for outgoing in 0..3 {
            let mut game = running();
            let boundary = (outgoing as u16 + 1) * SECTION_TICKS;
            game.rail_ticks = boundary - 1;
            game.spawn_segment = outgoing;
            game.spawn_cursor = 17;
            game.score = 1234;
            game.shield = 61;
            let before_scene = game.scenery_ticks();
            let cue = game.music_state();
            let bytes = empty_segment(outgoing);
            let segment = Segment::new(&bytes).unwrap();
            game.tick(&segment, &test_bank(game.segment_index()));
            for elapsed in 0..TRANSITION_TICKS {
                assert_eq!(game.transition_progress(), Some((outgoing, elapsed)));
                assert_eq!(game.hud().distance, boundary as u32 * 8);
                assert_eq!(game.segment_index(), outgoing);
                assert_eq!(game.music_state(), cue);
                assert_eq!(game.scenery_ticks(), before_scene + elapsed + 1);
                game.damage_player(100);
                assert_eq!(
                    (game.shield, game.score, game.lives, game.bombs),
                    (61, 1234, 3, 3)
                );
                game.tick(&segment, &test_bank(game.segment_index()));
            }
            assert_eq!(game.mode, Mode::Playing);
            assert_eq!(game.segment_index(), outgoing + 1);
            assert_eq!(game.rail_ticks, boundary);
            assert_eq!(game.scenery_ticks(), before_scene + TRANSITION_TICKS + 1);
            assert_eq!((game.spawn_segment, game.spawn_cursor), (outgoing + 1, 0));
            game.tick(&segment, &test_bank(game.segment_index())); // A stale outgoing view cannot spawn encounters.
            assert_eq!((game.rail_ticks, game.spawn_cursor), (boundary + 1, 0));
            let mut incoming = empty_segment(outgoing + 1);
            incoming[7] = 1;
            incoming[18] = assets::FIGHTER; // An encounter at local distance zero.
            let incoming = Segment::new(&incoming).unwrap();
            game.tick(&incoming, &test_bank(game.segment_index()));
            assert_eq!(game.spawn_cursor, 1);
            assert_eq!(
                game.actors.iter().filter(|actor| actor.kind != 0).count(),
                1
            );
            assert_eq!(
                game.actors[0].pos.z,
                1784 - test_bank(outgoing + 1).enemy(1).speed as i16
            );
            game.tick(&incoming, &test_bank(game.segment_index()));
            assert_eq!(
                game.actors.iter().filter(|actor| actor.kind != 0).count(),
                1
            );
        }
    }
    #[test]
    fn bridge_entry_removes_hostiles_before_live_bombs_and_keeps_player_projectiles() {
        let mut game = running();
        game.rail_ticks = SECTION_TICKS - 1;
        game.actors[0] = enemy(assets::FIGHTER, 0, 0, 180);
        game.shots[0] = Shot {
            pos: game.player,
            kind: 2,
            damage: 100,
            ..Shot::EMPTY
        };
        game.shots[1] = Shot {
            pos: Vec3 {
                x: 60,
                y: 10,
                z: 400,
            },
            kind: 3,
            damage: 4,
            ..Shot::EMPTY
        };
        game.pickups[0] = Pickup {
            pos: game.player,
            kind: 1,
        };
        game.controls.bomb_pending = true;
        let bytes = empty_segment(0);
        game.tick(
            &Segment::new(&bytes).unwrap(),
            &test_bank(game.segment_index()),
        );
        assert!(game.actors.iter().all(|actor| actor.kind == 0));
        assert!(game.shots.iter().all(|shot| shot.kind != 2));
        assert!(game.pickups.iter().all(|pickup| pickup.kind == 0));
        assert_eq!(game.shots[1].pos.z, 432);
        assert_eq!(game.shots[1].kind, 3);
        assert_eq!(
            game.missile_target(game.player, &test_bank(game.segment_index())),
            None
        );
        assert_eq!((game.shield, game.score, game.bombs), (100, 0, 2));
        assert!(game.explosions.iter().any(|explosion| explosion.ticks != 0));
        assert_eq!(game.take_presentation_event(), PresentationEvent::Bomb);
    }
    #[test]
    fn all_controls_and_double_taps_behave_identically_across_both_bridge_seams() {
        for entering in [true, false] {
            let mut game = running();
            if entering {
                game.rail_ticks = SECTION_TICKS - 2;
            } else {
                game.rail_ticks = SECTION_TICKS;
                game.mode = Mode::Transition;
                game.transition_ticks = TRANSITION_TICKS - 2;
            }
            let mut reference = running();
            reference.rail_ticks = 1000;
            let banks = [empty_segment(0), empty_segment(1)];
            for (tick, bits) in [
                LEFT | FIRE,
                0,
                LEFT | FIRE,
                UP | FIRE,
                RIGHT | FIRE,
                0,
                RIGHT,
                0,
            ]
            .into_iter()
            .enumerate()
            {
                // The initial charged shot and each two-tap gesture crosses a seam.
                let ms = 2200 + tick as u32 * 20;
                sample(&mut game, bits, ms);
                sample(&mut reference, bits, ms);
                game.tick(
                    &Segment::new(&banks[game.segment_index() as usize]).unwrap(),
                    &test_bank(game.segment_index()),
                );
                reference.tick(
                    &Segment::new(&banks[0]).unwrap(),
                    &test_bank(reference.segment_index()),
                );
                assert_eq!(game.player, reference.player);
                assert_eq!(game.bank, reference.bank);
                assert_eq!(game.roll_ticks, reference.roll_ticks);
                assert_eq!(game.bombs, reference.bombs);
                assert_eq!(
                    game.take_presentation_event(),
                    reference.take_presentation_event()
                );
                assert_eq!(game.controls.charge(), reference.controls.charge());
                for (actual, expected) in game.shots.iter().zip(&reference.shots) {
                    assert_eq!(
                        (actual.kind, actual.pos, actual.damage),
                        (expected.kind, expected.pos, expected.damage)
                    );
                }
                while let Some(expected) = reference.pop_audio() {
                    assert_eq!(game.pop_audio(), Some(expected));
                }
                assert!(game.pop_audio().is_none());
            }
            assert_eq!(game.bombs, 2);
            assert!(game.roll_ticks > 0);
        }
    }
    #[test]
    fn checkpoints_and_carrier_start_use_combat_distance_after_bridges() {
        let banks = [empty_segment(2), empty_segment(3)];
        let mut before = running();
        before.rail_ticks = SECTION_TICKS * 3 - 1;
        before.damage_player(100);
        before.respawn();
        assert_eq!(before.rail_ticks, SECTION_TICKS * 2);
        assert_eq!(
            before.scenery_ticks(),
            SECTION_TICKS * 2 + TRANSITION_TICKS * 2
        );
        let mut game = running();
        game.rail_ticks = SECTION_TICKS * 3 - 1;
        game.tick(
            &Segment::new(&banks[0]).unwrap(),
            &test_bank(game.segment_index()),
        );
        for _ in 0..TRANSITION_TICKS {
            game.tick(
                &Segment::new(&banks[0]).unwrap(),
                &test_bank(game.segment_index()),
            );
            assert!(!game.boss_started);
        }
        for _ in 0..149 {
            game.tick(
                &Segment::new(&banks[1]).unwrap(),
                &test_bank(game.segment_index()),
            );
            assert!(!game.boss_started);
        }
        game.tick(
            &Segment::new(&banks[1]).unwrap(),
            &test_bank(game.segment_index()),
        );
        assert!(game.boss_started);
        assert_eq!(game.rail_ticks, 11_400);
        game.damage_player(100);
        game.respawn();
        assert_eq!(game.rail_ticks, SECTION_TICKS * 3);
        assert_eq!(game.transition_progress(), None);
        assert_eq!(
            game.scenery_ticks(),
            SECTION_TICKS * 3 + TRANSITION_TICKS * 3
        );
        game.rail_ticks = MISSION_TICKS - 1;
        game.boss_defeated = true;
        game.tick(
            &Segment::new(&banks[1]).unwrap(),
            &test_bank(game.segment_index()),
        );
        assert_eq!(game.mode, Mode::Victory);
        assert_eq!(game.scenery_ticks(), MISSION_TICKS + 3 * TRANSITION_TICKS);
    }
    #[derive(Debug, PartialEq, Eq)]
    struct TerrainCapture {
        faces: [([Vec3; 3], u8); 54],
        count: usize,
    }
    impl GeometrySink for TerrainCapture {
        fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
            assert_eq!(flags & !STATIC_SKY, TWO_SIDED);
            assert!(
                self.count < self.faces.len(),
                "terrain exceeded its face reservation"
            );
            self.faces[self.count] = ([a, b, c], color);
            self.count += 1;
        }
    }
    fn terrain_scene(style: u8, ticks: u16) -> TerrainCapture {
        let mut bytes = empty_segment(style);
        bytes[5] = style;
        bytes[6] = [5, 9, 11, 6][style as usize];
        let segment = Segment::new(&bytes).unwrap();
        let mut game = Game::new();
        game.rail_ticks = ticks;
        let mut capture = TerrainCapture {
            faces: [([Vec3::default(); 3], 0); 54],
            count: 0,
        };
        game.terrain(&mut capture, &segment);
        capture
    }
    #[test]
    fn terrain_profile_is_bounded_repeatable_and_asymmetric() {
        for style in 0..4 {
            let (low, high) = match style {
                0 => (-105, 23),
                1 => (96, 160),
                _ => (-105, -41),
            };
            let mut seen_low = false;
            let mut seen_high = false;
            let mut different_sides = false;
            for row in 0..16 {
                let left = terrain_height(row, false, style);
                let right = terrain_height(row, true, style);
                assert!((low..=high).contains(&left));
                assert!((low..=high).contains(&right));
                assert_eq!(terrain_height(row + 16, false, style), left);
                assert_eq!(terrain_height(row + 7, false, style), right);
                seen_low |= left == low;
                seen_high |= left == high;
                different_sides |= left != right;
            }
            assert!(seen_low && seen_high && different_sides);
        }
    }
    #[test]
    fn terrain_keeps_the_flight_lane_flat_within_its_face_budget() {
        for style in 0..4 {
            for ticks in [0, 1, 63, 64, 1023, 3750, 14999] {
                let scene = terrain_scene(style, ticks);
                assert_eq!(scene, terrain_scene(style, ticks));
                assert_eq!(scene.count, if style == 1 || style == 2 { 54 } else { 38 });
                let surface = &scene.faces[..scene.count - 14];
                let mut raised = false;
                for (points, color) in surface {
                    assert!(*color < 16);
                    for point in points {
                        if point.x.abs() <= 145 {
                            let guide = style == 2 && matches!(point.x.abs(), 95 | 100);
                            assert_eq!(point.y, if guide { -103 } else { -105 });
                        } else {
                            raised |= point.y > -105;
                        }
                    }
                }
                assert!(raised, "each view must contain actual raised geometry");
                assert!(
                    surface.chunks_exact(2).any(|pair| pair[0].1 != pair[1].1),
                    "raised facets need distinct flat colors"
                );
            }
        }
    }
    #[test]
    fn adjacent_terrain_bands_share_exact_edges() {
        for style in 0..4 {
            for ticks in [0, 19, 63, 64, 511, 3750, 14999] {
                let scene = terrain_scene(style, ticks);
                let faces_per_band = (scene.count - 14) / 4;
                for band in 0..3 {
                    let left = &scene.faces[band * faces_per_band..(band + 1) * faces_per_band];
                    let right =
                        &scene.faces[(band + 1) * faces_per_band..(band + 2) * faces_per_band];
                    let shared_z = 40 + (band as i16 + 1) * 512 - ((ticks & 63) * 8) as i16;
                    for (from, to) in [(left, right), (right, left)] {
                        let mut shared = 0;
                        for point in from.iter().flat_map(|face| face.0.iter()) {
                            if point.z == shared_z {
                                assert!(to
                                    .iter()
                                    .flat_map(|face| face.0.iter())
                                    .any(|other| other == point));
                                shared += 1;
                            }
                        }
                        assert!(shared >= 4);
                    }
                }
            }
        }
    }
    #[test]
    fn terrain_scrolls_continuously_across_row_replacement() {
        for style in 0..4 {
            // Includes the old 20-tick reset and two complete 64-tick rows.
            for tick in 0..130 {
                let before = terrain_scene(style, tick);
                let after = terrain_scene(style, tick + 1);
                let per_band = (before.count - 14) / 4;
                let retired = if tick & 63 == 63 { per_band } else { 0 };
                for i in 0..before.count - 14 - retired {
                    let (old_points, old_color) = before.faces[i + retired];
                    let (new_points, new_color) = after.faces[i];
                    assert_eq!(
                        old_color, new_color,
                        "facet color must remain world anchored"
                    );
                    for corner in 0..3 {
                        let mut translated = old_points[corner];
                        translated.z -= SPEED;
                        assert_eq!(translated, new_points[corner], "style {style}, tick {tick}");
                    }
                }
            }
        }
    }
    #[test]
    fn clouds_remain_in_the_sky_behind_scenery_with_shared_facet_edges() {
        for theme in 0..4 {
            for face in 5..14 {
                let (points, color) = backdrop_face(theme, face);
                assert!(CLOUD_COLORS[theme as usize].contains(&color));
                for point in points {
                    assert!(
                        point.z > 2400,
                        "clouds must be behind gameplay and mountains"
                    );
                    let (_, screen_y) = crate::projection::project(point);
                    assert!(
                        (24..90).contains(&screen_y),
                        "cloud must stay above the horizon and below attract labels"
                    );
                }
            }
            for cloud in 0..3 {
                let first = backdrop_face(theme, 5 + cloud * 3).0;
                let second = backdrop_face(theme, 6 + cloud * 3).0;
                let third = backdrop_face(theme, 7 + cloud * 3).0;
                assert_eq!((first[0], first[2]), (second[0], second[1]));
                assert_eq!((second[0], second[2]), (third[0], third[1]));
            }
        }
    }
    #[test]
    fn backdrop_screen_geometry_is_fixed_within_each_section_across_camera_modes() {
        struct SkyCapture {
            faces: [([Vec3; 3], u8); 14],
            count: usize,
        }
        impl GeometrySink for SkyCapture {
            fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
                if flags & STATIC_SKY != 0 {
                    self.faces[self.count] = ([a, b, c], color);
                    self.count += 1;
                }
            }
        }
        let bytes = model_bank_for_tests();
        let bank = AssetBank::new(&bytes).unwrap();
        for mode in [
            Mode::Title,
            Mode::Attract,
            Mode::Playing,
            Mode::Respawning,
            Mode::Victory,
            Mode::GameOver,
        ] {
            for player_y in [-65, -1, 0, 1, 65] {
                for ticks in [0, 31, 32, 3749, 3750, 7499, 7500, 11249, 11250, 14999] {
                    let mut game = Game::new();
                    game.mode = mode;
                    game.player.y = player_y;
                    game.rail_ticks = ticks;
                    let theme = game.segment_index();
                    let expected = core::array::from_fn(|face| backdrop_face(theme, face as u8));
                    let segment_bytes = empty_segment(game.segment_index());
                    let segment = Segment::new(&segment_bytes).unwrap();
                    let mut capture = SkyCapture {
                        faces: [([Vec3::default(); 3], 0); 14],
                        count: 0,
                    };
                    game.render(&mut capture, &bank, &segment);
                    assert_eq!(capture.count, 14);
                    assert_eq!(capture.faces, expected);
                }
            }
        }
    }
    #[test]
    fn clouds_are_visible_and_nearer_geometry_occludes_them() {
        for theme in 0..4 {
            let mut renderer = crate::render::Renderer::new();
            let (cloud, color) = backdrop_face(theme, 8);
            let near = cloud.map(|p| Vec3 {
                x: p.x / 2,
                y: p.y / 2,
                z: p.z / 2,
            });
            // Submit the nearer triangle first to exercise the depth sort.
            renderer.triangle(near[0], near[1], near[2], 6, TWO_SIDED);
            renderer.triangle(cloud[0], cloud[1], cloud[2], color, TWO_SIDED);
            assert_eq!(renderer.triangle_count, 2);
            renderer.finish();
            let mut cloud_only = crate::render::Renderer::new();
            cloud_only.triangle(cloud[0], cloud[1], cloud[2], color, TWO_SIDED);
            let mut near_only = crate::render::Renderer::new();
            near_only.triangle(near[0], near[1], near[2], 6, TWO_SIDED);
            let mut overlap = 0;
            for band in 0..25 {
                let mut actual = [0; crate::render::BAND_PIXELS];
                let mut distant = [0; crate::render::BAND_PIXELS];
                let mut foreground = [0; crate::render::BAND_PIXELS];
                renderer.render_band(band, &mut actual);
                cloud_only.render_band(band, &mut distant);
                near_only.render_band(band, &mut foreground);
                for pixel in 0..actual.len() {
                    let expected = if foreground[pixel] == 6 {
                        6
                    } else {
                        distant[pixel]
                    };
                    assert_eq!(actual[pixel], expected);
                    if foreground[pixel] == 6 && distant[pixel] == color {
                        overlap += 1;
                    }
                }
            }
            assert!(overlap > 0, "nearer geometry must visibly cover the cloud");
        }
    }
    #[test]
    fn section_themes_are_distinct_compact_and_keep_all_backdrop_facets_cacheable() {
        let bytes = model_bank_for_tests();
        let bank = AssetBank::new(&bytes).unwrap();
        for theme in 0..4 {
            let mut game = running();
            game.rail_ticks = theme as u16 * SECTION_TICKS;
            assert_eq!(game.sky_color(), SKY_COLORS[theme as usize]);
            let colors = [
                game.sky_color(),
                CLOUD_COLORS[theme as usize][0],
                CLOUD_COLORS[theme as usize][1],
            ];
            assert!(colors.iter().all(|color| (1..16).contains(color)));
            assert_ne!(colors[0], colors[1]);
            assert_ne!(colors[0], colors[2]);
            assert_ne!(colors[1], colors[2]);
            for other in 0..theme {
                let horizon: [_; 5] =
                    core::array::from_fn(|face| backdrop_face(theme, face as u8).0);
                let clouds: [_; 9] =
                    core::array::from_fn(|face| backdrop_face(theme, 5 + face as u8).0);
                assert_ne!(
                    horizon,
                    core::array::from_fn(|face| backdrop_face(other, face as u8).0)
                );
                assert_ne!(
                    clouds,
                    core::array::from_fn(|face| backdrop_face(other, 5 + face as u8).0)
                );
            }
            let mut renderer = crate::render::Renderer::new();
            renderer.begin(game.sky_color());
            for face in 0..14 {
                let (points, color) = backdrop_face(theme, face);
                renderer.triangle(
                    points[0],
                    points[1],
                    points[2],
                    color,
                    TWO_SIDED | STATIC_SKY,
                );
            }
            assert_eq!(
                renderer.triangle_count, 14,
                "theme {theme} lost a projected backdrop facet"
            );
            assert_eq!(
                renderer.static_sky_bands(),
                0x01ff_ffff,
                "theme {theme} must accept every unobstructed backdrop band"
            );
            renderer.finish();
            for band in 0..25 {
                let mut pixels = [0; crate::render::BAND_PIXELS];
                renderer.render_band(band, &mut pixels);
                for cell in 0..40 {
                    let mut colors = 0u16;
                    for row in 0..8 {
                        for column in 0..4 {
                            colors |= 1 << pixels[row * 160 + cell * 4 + column];
                        }
                    }
                    assert!(
                        (colors & !1).count_ones() <= 3,
                        "cache palette overflow: theme {theme}, band {band}, cell {cell}"
                    );
                }
            }
            let mut segment_bytes = empty_segment(theme);
            segment_bytes[5] = theme;
            segment_bytes[6] = [5, 9, 11, 6][theme as usize];
            let segment = Segment::new(&segment_bytes).unwrap();
            renderer.begin(game.sky_color());
            game.render(&mut renderer, &bank, &segment);
            assert_eq!(renderer.dropped, 0);
            // At this opening camera position, canyon walls conservatively
            // cover every band; the other terrains leave upper sky reusable.
            assert_eq!(
                renderer.static_sky_bands() == 0,
                theme == 1,
                "theme {theme} has unexpected foreground coverage"
            );
        }
        assert!(
            core::mem::size_of_val(&SKY_COLORS)
                + core::mem::size_of_val(&HORIZON_COLORS)
                + core::mem::size_of_val(&CLOUD_COLORS)
                + core::mem::size_of_val(&HORIZONS)
                + core::mem::size_of_val(&CLOUD_CENTERS)
                + core::mem::size_of_val(&CLOUD_OUTLINES)
                <= 250
        );
    }
    #[test]
    fn lowered_horizons_overlap_terrain_at_every_scroll_phase_and_camera_extreme() {
        fn occupied_rows(renderer: &crate::render::Renderer, last: bool) -> [Option<usize>; 160] {
            let mut rows = [None; 160];
            for band in 0..25 {
                let mut pixels = [0; crate::render::BAND_PIXELS];
                renderer.render_band(band, &mut pixels);
                for (offset, color) in pixels.iter().enumerate() {
                    let column = offset % 160;
                    if *color != 0 && (last || rows[column].is_none()) {
                        rows[column] = Some(band as usize * 8 + offset / 160);
                    }
                }
            }
            rows
        }
        let mut worst = (0i16, 0, 0, 0, 0, 0, 0);
        for theme in 0..4 {
            for camera_y in [
                0,
                chase_camera_y(Mode::Playing, -65),
                40,
                chase_camera_y(Mode::Playing, 65),
            ] {
                let mut horizon = crate::render::Renderer::new();
                for face in 0..5 {
                    let (points, color) = backdrop_face(theme, face);
                    assert_ne!(color, SKY_COLORS[theme as usize]);
                    assert!(points
                        .iter()
                        .any(|point| point.y == -544 - BACKDROP_CAMERA_Y));
                    assert!(points
                        .iter()
                        .all(|point| point.z == 2350 && point.y >= -544 - BACKDROP_CAMERA_Y));
                    horizon.triangle(points[0], points[1], points[2], 1, TWO_SIDED);
                }
                let horizon_bottom = occupied_rows(&horizon, true);
                for phase in 0..1024 {
                    // All 64 scroll positions through all 16 repeating rows.
                    let terrain = terrain_scene(theme, phase);
                    let mut ground = crate::render::Renderer::new();
                    for (points, _) in &terrain.faces[..terrain.count - 14] {
                        let points = points.map(|point| camera_point(point, camera_y));
                        ground.triangle(points[0], points[1], points[2], 1, TWO_SIDED);
                    }
                    let ground_top = occupied_rows(&ground, false);
                    for column in 0..160 {
                        let bottom =
                            horizon_bottom[column].expect("horizon must span the viewport");
                        let top = ground_top[column].expect("terrain must span the viewport");
                        let gap = top as i16 - bottom as i16 - 1;
                        if gap > worst.0 {
                            worst = (gap, theme, phase, camera_y, column, bottom, top);
                        }
                    }
                }
            }
        }
        assert!(
            worst.0 <= 0,
            "sky gap (pixels, theme, phase, camera, x, horizon, terrain): {worst:?}"
        );
    }
    #[test]
    fn resident_state_fits_budget() {
        assert_eq!(core::mem::size_of::<PresentationEvent>(), 1);
        assert!(
            core::mem::size_of::<Game>() <= 640,
            "{}",
            core::mem::size_of::<Game>()
        );
    }
    #[test]
    fn attract_silences_all_effects_and_gameplay_restores_them() {
        let mut g = Game::new();
        g.reset_scene(Mode::Attract, 0);
        let effects = [
            Effect::Fire,
            Effect::EnemyFire,
            Effect::MissileLaunch,
            Effect::Hit,
            Effect::Pickup,
            Effect::Roll,
            Effect::Bomb,
            Effect::Death,
        ];
        for effect in effects {
            g.sound(effect, 64);
            assert!(g.pop_audio().is_none(), "{effect:?} during attract");
        }
        g.reset_run();
        for effect in effects {
            g.sound(effect, 64);
            assert_eq!(g.pop_audio(), Some(AudioEvent { effect, pan: 64 }));
        }
    }
    #[test]
    fn attract_enemy_and_carrier_shots_spawn_and_move_silently() {
        for carrier in [false, true] {
            let mut g = Game::new();
            if carrier {
                g.reset_scene(Mode::Attract, SECTION_TICKS * 3 + 150);
                g.boss_z = 680;
                g.ticks = 39;
            } else {
                g.reset_scene(Mode::Attract, 0);
                g.actors[0] = enemy(assets::FIGHTER, 50, 0, 1000);
                g.actors[0].age = 64;
            }
            let bytes = empty_segment(g.segment_index());
            let segment = Segment::new(&bytes).unwrap();
            g.tick(&segment, &test_bank(g.segment_index()));
            assert_eq!(
                g.shots.iter().filter(|shot| shot.kind == 2).count(),
                if carrier { 2 } else { 1 }
            );
            assert!(g.pop_audio().is_none());
            let z = g.shots[0].pos.z;
            g.tick(&segment, &test_bank(g.segment_index()));
            assert_eq!(g.shots[0].kind, 2);
            assert_eq!(g.shots[0].pos.z, z - 18);
            assert!(g.pop_audio().is_none());
        }
    }
    #[test]
    fn attract_has_no_player_collisions_or_pickup_collection() {
        let mut g = Game::new();
        g.reset_scene(Mode::Attract, 0);
        g.shield = 30;
        g.bombs = 0;
        g.actors[0] = enemy(assets::FIGHTER, 0, 0, PLAYER_Z + SPEED);
        g.shots[0] = Shot {
            pos: Vec3 {
                x: 0,
                y: 0,
                z: PLAYER_Z + 18,
            },
            damage: 12,
            kind: 2,
            ..Shot::EMPTY
        };
        g.pickups[0] = Pickup {
            pos: Vec3 {
                x: 0,
                y: 0,
                z: PLAYER_Z + SPEED,
            },
            kind: 1,
        };
        g.pickups[1] = Pickup {
            kind: 2,
            ..g.pickups[0]
        };
        g.update_actors(&test_bank(g.segment_index()));
        g.update_shots(&test_bank(g.segment_index()));
        g.update_pickups();
        assert_ne!(g.actors[0].kind, 0, "enemies fly through the absent ship");
        assert_ne!(g.shots[0].kind, 0, "hostile shots continue past the camera");
        assert!(g.pickups[..2].iter().all(|p| p.kind != 0));
        assert_eq!((g.shield, g.bombs, g.lives, g.score), (30, 0, 3, 0));
        assert!(g.explosions.iter().all(|e| e.ticks == 0));
        assert!(g.pop_audio().is_none());
        for _ in 0..30 {
            g.update_actors(&test_bank(g.segment_index()));
            g.update_shots(&test_bank(g.segment_index()));
            g.update_pickups();
        }
        assert!(g.actors.iter().all(|a| a.kind == 0));
        assert!(g.shots.iter().all(|s| s.kind == 0));
        assert!(g.pickups.iter().all(|p| p.kind == 0));
    }
    #[test]
    fn attract_section_cuts_clear_encounters_boss_audio_and_gestures() {
        let mut g = Game::new();
        g.reset_scene(Mode::Attract, SECTION_TICKS * 2);
        g.menu_ticks = ATTRACT_TICKS - 1;
        g.actors[0] = enemy(assets::FIGHTER, 50, 0, 500);
        g.shots[0] = Shot {
            kind: 2,
            ..Shot::EMPTY
        };
        g.pickups[0] = Pickup {
            kind: 1,
            ..Pickup::EMPTY
        };
        g.explosions[0] = Explosion {
            ticks: 10,
            ..Explosion::EMPTY
        };
        g.boss_started = true;
        g.boss_left = 0;
        g.controls.now = 0xffff_fff0;
        g.controls.fire_armed = true;
        g.controls.shot_pending = true;
        g.controls.roll_pending = -1;
        g.controls.bomb_pending = true;
        // Seed stale audio directly: attract mode suppresses new sound events.
        g.audio[0] = AudioEvent {
            effect: Effect::EnemyFire,
            pan: 0,
        };
        g.audio_len = 1;
        let bytes = empty_segment(2);
        g.tick(
            &Segment::new(&bytes).unwrap(),
            &test_bank(g.segment_index()),
        );
        assert_eq!(g.mode, Mode::Attract);
        assert_eq!(g.rail_ticks, SECTION_TICKS * 3);
        assert_eq!(g.spawn_segment, 3);
        assert_eq!((g.menu_ticks, g.spawn_cursor), (0, 0));
        assert!(g.actors.iter().all(|a| a.kind == 0));
        assert!(g.shots.iter().all(|s| s.kind == 0));
        assert!(g.pickups.iter().all(|p| p.kind == 0));
        assert!(g.explosions.iter().all(|e| e.ticks == 0));
        assert!(!g.boss_started);
        assert_eq!((g.boss_left, g.boss_right, g.boss_core), (20, 20, 60));
        assert_eq!(g.controls.now, 0xffff_fff0);
        assert_eq!(g.controls.released_at, g.controls.now);
        assert!(!g.controls.fire_armed);
        assert!(!g.controls.shot_pending);
        assert!(!g.controls.bomb_pending);
        assert_eq!(g.controls.roll_pending, 0);
        assert!(g.pop_audio().is_none());
    }
    #[test]
    fn result_screens_do_not_enter_attract_mode() {
        let bytes = empty_segment(0);
        let segment = Segment::new(&bytes).unwrap();
        for mode in [Mode::Victory, Mode::GameOver] {
            let mut g = Game::new();
            g.mode = mode;
            for _ in 0..ATTRACT_TICKS * 2 {
                g.tick(&segment, &test_bank(g.segment_index()));
            }
            assert_eq!(g.mode, mode);
            assert_eq!(g.rail_ticks, 0);
        }
    }
    #[test]
    fn swept_projectile_hits_between_endpoints() {
        let mut g = running();
        g.actors[0] = Actor {
            pos: Vec3 { x: 0, y: 0, z: 230 },
            kind: assets::FIGHTER,
            hp: 1,
            ..Actor::EMPTY
        };
        g.shots[0] = Shot {
            pos: Vec3 { x: 0, y: 0, z: 205 },
            kind: 1,
            damage: 1,
            ..Shot::EMPTY
        };
        g.update_shots(&test_bank(g.segment_index()));
        assert_eq!(g.actors[0].kind, 0);
        assert_eq!(g.score, 100);
    }
    #[test]
    fn joystick_vertical_axis_follows_direction_and_opposing_inputs_cancel() {
        let bytes = empty_segment(0);
        let segment = Segment::new(&bytes).unwrap();
        for (bits, expected_y) in [(UP, 3), (DOWN, -3), (UP | DOWN, 0)] {
            let mut g = running();
            sample(&mut g, bits, 20);
            g.tick(&segment, &test_bank(g.segment_index()));
            assert_eq!(g.player.y, expected_y);
        }
    }
    #[test]
    fn released_roll_moves_immediately_fifty_units_and_does_not_snap_back() {
        let bytes = empty_segment(0);
        let segment = Segment::new(&bytes).unwrap();
        for (direction, sign) in [(LEFT, -1), (RIGHT, 1)] {
            let mut g = running();
            queue_roll(&mut g, direction);
            for tick in 1..=25 {
                g.tick(&segment, &test_bank(g.segment_index()));
                if let Some(distance) = match tick {
                    1 => Some(6),
                    5 => Some(30),
                    10 => Some(45),
                    15 | 25 => Some(50),
                    _ => None,
                } {
                    assert_eq!(g.player.x, sign * distance, "roll tick {tick}");
                }
                assert_eq!(g.roll_ticks, 26 - tick);
                assert_eq!(g.bank, 0, "dodge velocity must not change steering bank");
            }
            for _ in 0..30 {
                g.tick(&segment, &test_bank(g.segment_index()));
                assert_eq!(g.player.x, sign * 50);
            }
            assert_eq!(g.roll_ticks, 0);
            assert_eq!(g.roll_cooldown, 0);
        }
    }
    #[test]
    fn dodge_keeps_horizontal_vertical_and_fire_controls_live() {
        let bytes = empty_segment(0);
        let segment = Segment::new(&bytes).unwrap();
        let mut g = running();
        queue_roll(&mut g, RIGHT);
        sample(&mut g, RIGHT | UP | FIRE, 240);
        g.tick(&segment, &test_bank(g.segment_index()));
        assert_eq!(g.player.x, 9);
        assert_eq!(g.player.y, 3);
        assert_eq!(g.shots[0].kind, 1);
        assert_eq!(g.shots[0].pos.x, 9, "fire uses the displaced position");
        assert_eq!(g.shots[0].pos.y, 3);
        assert_eq!(g.bank, -2);

        sample(&mut g, LEFT | DOWN | FIRE, 260);
        g.tick(&segment, &test_bank(g.segment_index()));
        assert_eq!(g.player.x, 12, "opposite steering brakes the first burst");
        assert_eq!(g.player.y, 0);
        assert_eq!(g.bank, 0, "bank follows steering, not dodge velocity");
        for _ in 0..8 {
            g.tick(&segment, &test_bank(g.segment_index()));
        }
        assert_eq!(g.roll_ticks, 16);
        assert_eq!(g.player.x, 21);
        assert!(g.shots.iter().filter(|shot| shot.kind == 1).count() >= 2);
        g.tick(&segment, &test_bank(g.segment_index()));
        assert_eq!(
            g.player.x, 19,
            "opposite steering reverses the tapering tail"
        );
    }
    #[test]
    fn dodge_and_steering_share_the_arena_limits() {
        let bytes = empty_segment(0);
        let segment = Segment::new(&bytes).unwrap();
        for (direction, sign) in [(LEFT, -1), (RIGHT, 1)] {
            let mut g = running();
            g.player.x = sign * 99;
            g.player.y = 64;
            queue_roll(&mut g, direction);
            sample(&mut g, direction | UP, 240);
            for _ in 0..25 {
                g.tick(&segment, &test_bank(g.segment_index()));
                assert_eq!(g.player.x, sign * 100);
                assert_eq!(g.player.y, 65);
            }
        }
    }
    #[test]
    fn sidestep_avoids_old_lane_scenery_but_can_hit_the_new_lane() {
        let bytes = empty_segment(0);
        let segment = Segment::new(&bytes).unwrap();
        for (rolling, obstacle_x, expected_shield) in
            [(false, 0, 70), (true, 0, 100), (true, 45, 70)]
        {
            let mut g = running();
            g.actors[0] = Actor {
                pos: Vec3 {
                    x: obstacle_x,
                    y: -20,
                    z: PLAYER_Z + SPEED * 10 + 31,
                },
                kind: assets::ROCK,
                hp: 255,
                ..Actor::EMPTY
            };
            if rolling {
                queue_roll(&mut g, RIGHT);
            }
            for _ in 0..10 {
                g.tick(&segment, &test_bank(g.segment_index()));
            }
            assert_eq!(g.shield, expected_shield);
        }
    }
    #[test]
    fn roll_deflects_projectiles_but_not_scenery() {
        let mut g = running();
        g.roll_ticks = 10;
        g.shots[0] = Shot {
            pos: Vec3 { x: 0, y: 0, z: 190 },
            kind: 2,
            damage: 12,
            ..Shot::EMPTY
        };
        g.update_shots(&test_bank(g.segment_index()));
        assert_eq!(g.shield, 100);
        g.actors[0] = Actor {
            pos: Vec3 { x: 0, y: 0, z: 190 },
            kind: assets::ROCK,
            hp: 255,
            ..Actor::EMPTY
        };
        g.update_actors(&test_bank(g.segment_index()));
        assert_eq!(g.shield, 70);
    }
    #[test]
    fn respawn_uses_section_checkpoint_and_retains_score() {
        let mut g = running();
        g.rail_ticks = 8000;
        g.score = 1234;
        g.damage_player(100);
        assert_eq!(g.mode, Mode::Respawning);
        assert_eq!(g.lives, 2);
        let bytes = empty_segment(2);
        let segment = Segment::new(&bytes).unwrap();
        for _ in 0..75 {
            g.tick(&segment, &test_bank(g.segment_index()));
        }
        assert_eq!(g.mode, Mode::Playing);
        assert_eq!(g.hud().distance, 60000);
        assert_eq!(g.shield, 100);
        assert_eq!(g.score, 1234);
    }
    #[test]
    fn carrier_core_is_protected_until_both_turrets_die() {
        let mut g = running();
        g.boss_started = true;
        g.boss_z = 680;
        g.shots[0] = Shot {
            pos: Vec3 { x: 0, y: 0, z: 450 },
            kind: 1,
            damage: 4,
            ..Shot::EMPTY
        };
        g.update_shots(&test_bank(g.segment_index()));
        assert_eq!(g.boss_core, 60);
        g.boss_left = 0;
        g.boss_right = 0;
        g.shots[0] = Shot {
            pos: Vec3 { x: 0, y: 0, z: 450 },
            kind: 1,
            damage: 4,
            ..Shot::EMPTY
        };
        g.update_shots(&test_bank(g.segment_index()));
        assert_eq!(g.boss_core, 56);
    }
    #[test]
    fn shield_magnet_recovers_near_misses_and_rewards_once() {
        for (x, y) in [(56, -56), (-56, 56)] {
            for (initial_shield, expected_shield) in [(40, 70), (90, 100), (100, 100)] {
                let mut g = running();
                g.shield = initial_shield;
                g.pickups[0] = Pickup {
                    pos: Vec3 {
                        x,
                        y,
                        z: PLAYER_Z + 240 + SPEED,
                    },
                    kind: 1,
                };
                for _ in 0..10 {
                    let before = g.pickups[0].pos;
                    g.update_pickups();
                    let after = g.pickups[0].pos;
                    assert!(after.x.abs() < before.x.abs());
                    assert!(after.y.abs() < before.y.abs());
                    assert_eq!(after.z, before.z - SPEED);
                    assert_eq!(g.pickups[0].kind, 1);
                    assert_eq!((g.shield, g.score), (initial_shield, 0));
                    assert!(g.pop_audio().is_none());
                }
                for _ in 0..30 {
                    g.update_pickups();
                }
                assert_eq!(g.pickups[0].kind, 0);
                assert_eq!((g.shield, g.score), (expected_shield, 50));
                assert_eq!(g.pop_audio().unwrap().effect, Effect::Pickup);
                assert!(g.pop_audio().is_none());
                g.update_pickups();
                assert_eq!((g.shield, g.score), (expected_shield, 50));
                assert!(g.pop_audio().is_none());
            }
        }
    }

    #[test]
    fn shield_magnet_respects_proximity_kind_and_mode() {
        // Depths describe the pickup after its normal forward movement.
        for (kind, mode, x, y, depth, attracted) in [
            (1, Mode::Playing, 64, -64, 240, true),
            (1, Mode::Playing, -64, 64, 0, true),
            (1, Mode::Playing, 65, 0, 120, false),
            (1, Mode::Playing, 0, -65, 120, false),
            (1, Mode::Playing, 56, -56, 241, false),
            (1, Mode::Playing, 56, -56, -1, false),
            (2, Mode::Playing, 56, -56, 120, false),
            (1, Mode::Attract, 56, -56, 120, false),
        ] {
            let mut g = running();
            g.mode = mode;
            g.player.x = 20;
            g.player.y = -10;
            let before = Vec3 {
                x: g.player.x + x,
                y: g.player.y + y,
                z: PLAYER_Z + depth + SPEED,
            };
            g.pickups[0] = Pickup { pos: before, kind };
            g.update_pickups();
            let after = g.pickups[0].pos;
            assert_eq!(after.z, PLAYER_Z + depth);
            if attracted {
                assert!((after.x - g.player.x).abs() < x.abs());
                assert!((after.y - g.player.y).abs() < y.abs());
            } else {
                assert_eq!((after.x, after.y), (before.x, before.y));
            }
            assert_eq!(g.pickups[0].kind, kind);
            assert_eq!(g.score, 0);
        }
    }

    #[test]
    fn shield_magnet_does_not_overshoot_or_expand_collection_hitbox() {
        let mut g = running();
        g.shield = 40;
        g.pickups[0] = Pickup {
            pos: Vec3 {
                x: 1,
                y: -1,
                z: PLAYER_Z + 80 + SPEED,
            },
            kind: 1,
        };
        for _ in 0..2 {
            g.update_pickups();
            assert_eq!((g.pickups[0].pos.x, g.pickups[0].pos.y), (0, 0));
            assert_eq!(g.pickups[0].kind, 1);
        }
        for (x, y) in [(31, 0), (-31, 0), (0, 31), (0, -31)] {
            g.pickups[0] = Pickup {
                pos: Vec3 {
                    x,
                    y,
                    z: PLAYER_Z + 8 + SPEED,
                },
                kind: 1,
            };
            g.update_pickups();
            assert_eq!(g.pickups[0].kind, 1, "29 units is outside the hitbox");
            g.update_pickups();
            assert_eq!(g.pickups[0].kind, 0, "27 units is inside the hitbox");
        }
        g.pickups[0] = Pickup {
            pos: Vec3 {
                x: 0,
                y: 0,
                z: PLAYER_Z + 25 + SPEED,
            },
            kind: 1,
        };
        g.update_pickups();
        assert_eq!(g.pickups[0].kind, 1, "depth 25 is outside the hitbox");
        g.update_pickups();
        assert_eq!(g.pickups[0].kind, 0, "depth 17 is inside the hitbox");
    }

    #[test]
    fn pickups_cap_and_bombs_clear_hostile_shots() {
        let mut g = running();
        g.pickups[0] = Pickup {
            pos: Vec3 {
                x: 0,
                y: 0,
                z: PLAYER_Z + 8,
            },
            kind: 2,
        };
        g.update_pickups();
        assert_eq!(g.bombs, 3);
        g.shots[0] = Shot {
            kind: 2,
            ..Shot::EMPTY
        };
        g.bomb(&test_bank(g.segment_index()));
        assert_eq!(g.bombs, 2);
        assert_eq!(g.shots[0].kind, 0);
    }
    #[test]
    fn bomb_presentation_survives_rewarded_inventory_full_audio_and_catch_up() {
        let mut g = running();
        for actor in &mut g.actors[..5] {
            *actor = Actor {
                kind: assets::FIGHTER,
                hp: 1,
                ..Actor::EMPTY
            };
        }
        for _ in 0..AUDIO_CAPACITY {
            g.sound(Effect::Fire, 0);
        }
        g.controls.bomb_pending = true;
        let bytes = empty_segment(0);
        let segment = Segment::new(&bytes).unwrap();
        g.tick(&segment, &test_bank(g.segment_index()));
        assert_eq!(g.bombs, 3, "the reward hides the inventory decrement");
        assert_eq!(g.audio_len as usize, AUDIO_CAPACITY);
        while g.pop_audio().is_some() {}
        for _ in 0..50 {
            g.tick(&segment, &test_bank(g.segment_index()));
        }
        assert_eq!(g.take_presentation_event(), PresentationEvent::Bomb);
        assert_eq!(g.take_presentation_event(), PresentationEvent::None);
    }
    #[test]
    fn killing_the_carrier_keeps_bomb_presentation_on_the_victory_screen() {
        let mut g = running();
        g.rail_ticks = MISSION_TICKS - 1;
        g.boss_started = true;
        g.boss_left = 0;
        g.boss_right = 0;
        g.boss_core = 8;
        g.controls.bomb_pending = true;
        let bytes = empty_segment(3);
        let segment = Segment::new(&bytes).unwrap();
        g.tick(&segment, &test_bank(g.segment_index()));
        assert_eq!(g.mode, Mode::Victory);
        assert!(g.boss_defeated);
        for _ in 0..50 {
            g.tick(&segment, &test_bank(g.segment_index()));
        }
        assert_eq!(g.take_presentation_event(), PresentationEvent::Bomb);
    }
    #[test]
    fn presentation_resets_and_bombs_keep_only_the_latest_request() {
        let mut g = Game::new();
        assert_eq!(g.take_presentation_event(), PresentationEvent::None);
        g.reset_run();
        assert_eq!(g.take_presentation_event(), PresentationEvent::Reset);
        assert_eq!(g.take_presentation_event(), PresentationEvent::None);
        g.bomb(&test_bank(g.segment_index()));
        g.reset_scene(Mode::Title, 0);
        assert_eq!(g.take_presentation_event(), PresentationEvent::Reset);
        g.reset_run();
        g.bomb(&test_bank(g.segment_index()));
        assert_eq!(g.take_presentation_event(), PresentationEvent::Bomb);
        g.bomb(&test_bank(g.segment_index()));
        g.bomb(&test_bank(g.segment_index()));
        assert_eq!(g.take_presentation_event(), PresentationEvent::Bomb);
        assert_eq!(g.take_presentation_event(), PresentationEvent::None);
        g.reset_run();
        g.bomb(&test_bank(g.segment_index()));
        g.respawn();
        assert_eq!(g.take_presentation_event(), PresentationEvent::Reset);
        assert_eq!(g.take_presentation_event(), PresentationEvent::None);
    }
    #[test]
    fn every_fifth_projectile_kill_adds_a_bomb_up_to_three_without_banking_overflow() {
        let mut g = running();
        g.bombs = 0;
        for kill in 1..=25 {
            if kill == 21 {
                // The reward at 20 was consumed at the cap, not deferred.
                g.bombs = 2;
            }
            g.actors[0] = Actor {
                pos: Vec3 { x: 0, y: 0, z: 210 },
                kind: assets::FIGHTER,
                hp: 1,
                flags: u8::from(kill == 5),
                ..Actor::EMPTY
            };
            g.shots[0] = Shot {
                pos: Vec3 { x: 0, y: 0, z: 180 },
                damage: 1,
                kind: if kill % 2 == 0 { 3 } else { 1 },
                ..Shot::EMPTY
            };
            g.update_shots(&test_bank(g.segment_index()));
            assert_eq!(g.actors[0].kind, 0);
            assert_eq!(g.shots[0].kind, 0);
            assert_eq!(g.kills, kill);
            let expected = if (21..25).contains(&kill) {
                2
            } else {
                (kill / 5).min(3) as u8
            };
            assert_eq!(g.hud().bombs, expected, "kill {kill}");
            if kill == 4 {
                assert_eq!(g.pickups.iter().filter(|p| p.kind == 1).count(), 1);
            }
            if kill == 5 {
                assert_eq!(g.pickups.iter().filter(|p| p.kind == 2).count(), 1);
            }
            // A spent projectile cannot award the same kill twice.
            g.update_shots(&test_bank(g.segment_index()));
            assert_eq!(g.kills, kill);
            assert_eq!(g.bombs, expected);
        }
    }
    #[test]
    fn bomb_multi_kills_award_each_crossed_five_kill_milestone() {
        let mut g = running();
        g.bombs = 1;
        for actor in &mut g.actors[..10] {
            *actor = Actor {
                kind: assets::FIGHTER,
                hp: 1,
                ..Actor::EMPTY
            };
        }
        g.actors[10] = Actor {
            kind: assets::ROCK,
            hp: 255,
            ..Actor::EMPTY
        };
        g.actors[11] = Actor {
            kind: assets::TURRET,
            hp: 9,
            ..Actor::EMPTY
        };
        g.bomb(&test_bank(g.segment_index()));
        assert_eq!(g.kills, 10);
        assert_eq!(g.bombs, 2, "spend one bomb and immediately earn two");
        assert_eq!(g.actors[10].hp, 255);
        assert_eq!(g.actors[11].hp, 1, "damage alone is not a kill");
    }
    #[test]
    fn bomb_reward_progress_survives_respawn_and_kill_total_wrap_but_resets_with_run() {
        let mut g = running();
        for _ in 0..4 {
            g.actors[0] = Actor {
                kind: assets::FIGHTER,
                hp: 1,
                ..Actor::EMPTY
            };
            g.kill_actor(0, &test_bank(g.segment_index()));
        }
        g.bombs = 0;
        g.damage_player(100);
        assert_eq!(g.mode, Mode::Respawning);
        g.respawn();
        assert_eq!(g.bombs, 1);
        g.actors[0] = Actor {
            kind: assets::FIGHTER,
            hp: 1,
            ..Actor::EMPTY
        };
        g.kill_actor(0, &test_bank(g.segment_index()));
        assert_eq!(
            g.bombs, 2,
            "the fifth kill still completes the pre-death progress"
        );

        g.kills = u16::MAX - 1;
        g.bombs = 0;
        for kill in 1..=5 {
            g.actors[0] = Actor {
                kind: assets::FIGHTER,
                hp: 1,
                ..Actor::EMPTY
            };
            g.kill_actor(0, &test_bank(g.segment_index()));
            assert_eq!(g.bombs, u8::from(kill == 5));
        }
        assert_eq!(g.kills, 3);

        for _ in 0..4 {
            g.actors[0] = Actor {
                kind: assets::FIGHTER,
                hp: 1,
                ..Actor::EMPTY
            };
            g.kill_actor(0, &test_bank(g.segment_index()));
        }
        g.reset_run();
        assert_eq!((g.kills, g.bombs, g.bomb_kills), (0, 3, 0));
        g.bombs = 0;
        for kill in 1..=5 {
            g.actors[0] = Actor {
                kind: assets::FIGHTER,
                hp: 1,
                ..Actor::EMPTY
            };
            g.kill_actor(0, &test_bank(g.segment_index()));
            assert_eq!(g.bombs, u8::from(kill == 5));
        }
    }
    #[test]
    fn charge_uses_elapsed_milliseconds_and_wraps_safely() {
        let mut g = running();
        sample(&mut g, FIRE, u32::MAX - 1500);
        sample(&mut g, 0, u32::MAX - 1490);
        sample(&mut g, 0, 509);
        assert_eq!(g.hud().charge, 255);
        sample(&mut g, FIRE, 510);
        assert_eq!(g.controls.shot_charge, 255);
    }
    #[test]
    fn three_deaths_reach_game_over_and_fresh_press_restarts() {
        let bytes = empty_segment(0);
        let segment = Segment::new(&bytes).unwrap();
        let mut g = running();
        for life in (0..3).rev() {
            g.invulnerability = 0;
            g.damage_player(100);
            assert_eq!(g.lives, life);
            if life > 0 {
                for _ in 0..75 {
                    g.tick(&segment, &test_bank(g.segment_index()));
                }
            }
        }
        assert_eq!(g.mode, Mode::GameOver);
        g.controls.bits = FIRE;
        sample(&mut g, FIRE, 5000);
        g.tick(&segment, &test_bank(g.segment_index()));
        assert_eq!(g.mode, Mode::GameOver);
        sample(&mut g, 0, 5020);
        sample(&mut g, FIRE, 5040);
        g.tick(&segment, &test_bank(g.segment_index()));
        assert_eq!(g.mode, Mode::Playing);
        assert_eq!(g.lives, 3);
        assert_eq!(g.score, 0);
        assert_eq!(g.hud().distance, 0);
    }
    #[test]
    fn partial_charge_scales_damage_and_full_charge_deals_four() {
        let mut g = running();
        g.fire_player(0);
        g.fire_player(127);
        g.fire_player(254);
        g.fire_player(255);
        assert_eq!(
            [
                g.shots[0].damage,
                g.shots[1].damage,
                g.shots[2].damage,
                g.shots[3].damage
            ],
            [1, 2, 3, 4]
        );
        assert_eq!(
            [
                g.shots[0].kind,
                g.shots[1].kind,
                g.shots[2].kind,
                g.shots[3].kind
            ],
            [1, 1, 1, 3]
        );
        g.update_shots(&test_bank(g.segment_index()));
        assert!(g.shots[..3]
            .iter()
            .all(|shot| shot.pos.z == PLAYER_Z + 18 + 48));
        assert_eq!(g.shots[3].pos.z, PLAYER_Z + 18 + 32);
    }
    #[test]
    fn missile_launch_is_retained_when_the_game_audio_queue_is_full() {
        let mut g = running();
        while g.pop_audio().is_some() {}
        for _ in 0..AUDIO_CAPACITY {
            g.sound(Effect::EnemyFire, 0);
        }
        g.fire_player(255);
        for _ in 0..AUDIO_CAPACITY - 1 {
            assert_eq!(g.pop_audio().unwrap().effect, Effect::EnemyFire);
        }
        assert_eq!(
            g.pop_audio(),
            Some(AudioEvent {
                effect: Effect::MissileLaunch,
                pan: 0,
            })
        );
        assert!(g.pop_audio().is_none());
    }
    fn enemy(kind: u8, x: i16, y: i16, z: i16) -> Actor {
        Actor {
            pos: Vec3 { x, y, z },
            origin_x: x,
            origin_y: y,
            kind,
            hp: if kind == assets::TURRET { 6 } else { 3 },
            approach_speed: if kind == assets::DIVER { 12 } else { 8 },
            ..Actor::EMPTY
        }
    }
    #[test]
    fn missile_selects_nearest_ahead_then_lateral_distance_and_stable_slot() {
        let mut g = running();
        let origin = Vec3 { x: 0, y: 0, z: 200 };
        g.actors[0] = enemy(assets::ROCK, 0, 0, 300);
        g.actors[1] = enemy(assets::FIGHTER, 0, 0, 190);
        g.actors[2] = enemy(assets::DIVER, 0, 0, 400);
        g.actors[2].hp = 0;
        g.actors[3] = enemy(assets::FIGHTER, 0, 0, 800);
        g.actors[4] = enemy(assets::DIVER, -40, 0, 700);
        g.actors[5] = enemy(assets::DIVER, 40, 0, 700);
        assert_eq!(
            g.missile_target(origin, &test_bank(g.segment_index())),
            Some((g.actors[4].pos, 44))
        );
        g.actors[5].pos.x = 39;
        assert_eq!(
            g.missile_target(origin, &test_bank(g.segment_index())),
            Some((g.actors[5].pos, 44))
        );
        g.actors[5] = enemy(assets::TURRET, 0, -28, 600);
        assert_eq!(
            g.missile_target(origin, &test_bank(g.segment_index())),
            Some((Vec3 { x: 0, y: 0, z: 600 }, 40))
        );
    }
    #[test]
    fn missile_reacquires_after_death_slot_reuse_and_passing_and_coasts_without_targets() {
        let mut g = running();
        let mut s = Shot {
            pos: Vec3 { x: 0, y: 0, z: 200 },
            kind: 3,
            damage: 4,
            ..Shot::EMPTY
        };
        g.actors[0] = enemy(assets::FIGHTER, 100, 0, 700);
        g.steer_missile(&mut s, &test_bank(g.segment_index()));
        assert_eq!(s.dx, 1);
        g.actors[0].kind = 0;
        g.steer_missile(&mut s, &test_bank(g.segment_index()));
        assert_eq!(
            s.dx, 0,
            "lost target reduces steering without stale references"
        );
        g.actors[0] = enemy(assets::FIGHTER, -100, 0, 700);
        g.steer_missile(&mut s, &test_bank(g.segment_index()));
        assert_eq!(s.dx, -1, "a reused slot is considered at its new position");
        g.actors[0].pos.z = 199;
        g.steer_missile(&mut s, &test_bank(g.segment_index()));
        assert_eq!(
            s.dx, 0,
            "the seeker cannot turn back toward a passed target"
        );
        g.shots[0] = s;
        g.update_shots(&test_bank(g.segment_index()));
        assert_eq!(g.shots[0].pos, Vec3 { x: 0, y: 0, z: 232 });
        for _ in 0..70 {
            g.update_shots(&test_bank(g.segment_index()));
        }
        assert_eq!(
            g.shots[0].kind, 0,
            "unguided missiles expire at the existing range limit"
        );
    }
    #[test]
    fn missile_guidance_reaches_moving_fighters_divers_and_distant_opposite_side_turrets() {
        for kind in [assets::FIGHTER, assets::DIVER, assets::TURRET] {
            for z in [1000, 1200, 1400, 1800] {
                let mut g = running();
                g.player.x = 100;
                g.player.y = if kind == assets::TURRET { 65 } else { -65 };
                g.actors[0] = if kind == assets::TURRET {
                    enemy(kind, -95, -90, z)
                } else {
                    enemy(kind, -80, 35, z)
                };
                g.fire_player(255);
                let initial_hp = g.actors[0].hp;
                let mut hit = false;
                for _ in 0..60 {
                    let before = g.shots[0];
                    g.update_actors(&test_bank(g.segment_index()));
                    g.update_shots(&test_bank(g.segment_index()));
                    let after = g.shots[0];
                    assert!((after.dx - before.dx).abs() <= 1);
                    assert!((after.dy - before.dy).abs() <= 1);
                    assert!(after.dx.abs() <= 12 && after.dy.abs() <= 12);
                    if g.actors[0].kind == 0 || g.actors[0].hp < initial_hp {
                        hit = true;
                        assert_eq!(g.shots[0].kind, 0);
                        if kind == assets::TURRET {
                            assert_eq!(g.actors[0].hp, 2, "missile damage remains four");
                        } else {
                            assert_eq!(g.score, 100, "missile kills use ordinary scoring");
                        }
                        break;
                    }
                }
                assert!(hit, "missile missed kind {kind} at initial z {z}");
            }
        }
    }
    #[test]
    fn missile_boss_targeting_tracks_live_turrets_then_exposed_core() {
        let mut g = running();
        g.boss_started = true;
        g.boss_z = 900;
        let origin = Vec3 { x: 0, y: 0, z: 200 };
        assert_eq!(
            g.missile_target(origin, &test_bank(g.segment_index())),
            Some((
                Vec3 {
                    x: -130,
                    y: 30,
                    z: 830
                },
                40
            ))
        );
        g.boss_left = 0;
        g.boss_z = 680;
        assert_eq!(
            g.missile_target(origin, &test_bank(g.segment_index())),
            Some((
                Vec3 {
                    x: 130,
                    y: 30,
                    z: 610
                },
                32
            ))
        );
        g.boss_right = 0;
        assert_eq!(
            g.missile_target(origin, &test_bank(g.segment_index())),
            Some((Vec3 { x: 0, y: 0, z: 480 }, 32))
        );
        g.fire_player(255);
        for _ in 0..20 {
            g.update_shots(&test_bank(g.segment_index()));
        }
        assert_eq!(g.boss_core, 56);
        g.boss_defeated = true;
        assert_eq!(
            g.missile_target(origin, &test_bank(g.segment_index())),
            None
        );
    }
    #[test]
    fn enlarged_enemy_shot_boxes_cover_their_new_boundaries_in_every_roster() {
        for level in 0..4 {
            let bank = test_bank(level);
            for kind in 1..=bank.enemy_count() {
                let center_y = bank.enemy(kind).center_y as i16;
                for (offset, hit) in [(38, true), (39, false), (-38, true), (-39, false)] {
                    for axis in 0..3 {
                        let mut g = running();
                        g.actors[0] = enemy(kind, 0, -center_y, 700);
                        let hp = g.actors[0].hp;
                        // Positive depth probes start at the rear boundary;
                        // negative probes finish at the front boundary.
                        g.shots[0] = Shot {
                            pos: Vec3 {
                                x: if axis == 0 { offset } else { 0 },
                                y: if axis == 1 { offset } else { 0 },
                                z: if axis == 2 {
                                    700 + offset - if offset < 0 { 48 } else { 0 }
                                } else {
                                    652
                                },
                            },
                            kind: 1,
                            damage: 1,
                            ..Shot::EMPTY
                        };
                        g.update_shots(&bank);
                        assert_eq!(
                            g.actors[0].hp,
                            hp - u8::from(hit),
                            "{level}/{kind}, axis {axis}, offset {offset}"
                        );
                        assert_eq!(g.shots[0].kind == 0, hit);
                    }
                }
            }
        }
    }
    #[test]
    fn enlarged_ram_boxes_include_depth_while_obstacles_keep_their_original_depth() {
        for (kind, center_y, depth) in [(assets::TURRET, 28, 64), (assets::ROCK, 20, 32)] {
            for (x, dz, hit) in [(36, depth - 1, true), (37, 0, false), (0, depth, false)] {
                let mut g = running();
                g.actors[0] = enemy(kind, x, -center_y, PLAYER_Z + dz + 8);
                g.update_actors(&test_bank(0));
                assert_eq!(
                    g.shield,
                    if hit { 70 } else { 100 },
                    "kind {kind}, x {x}, depth {dz}"
                );
            }
        }
    }
    #[test]
    fn enlarged_enemy_proxies_cover_distant_and_budget_limited_rendering() {
        #[derive(Default)]
        struct Capture(std::vec::Vec<Vec3>);
        impl GeometrySink for Capture {
            fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, _: u8, _: u8) {
                self.0.extend([a, b, c]);
            }
        }
        for level in 0..4 {
            let bank = test_bank(level);
            for kind in (1..=bank.enemy_count()).chain([assets::ROCK, assets::BUILDING]) {
                for (z, limit) in [(1800, 174), (700, 2)] {
                    let mut g = running();
                    g.actors[0] = enemy(kind, 0, 0, z);
                    let mut capture = Capture::default();
                    g.render_combat(
                        &mut SceneBudget {
                            sink: &mut capture,
                            count: 0,
                            limit,
                            camera_y: 0,
                            color_override: 0,
                        },
                        &bank,
                    );
                    assert_eq!(capture.0.len(), 6);
                    let radius = match kind {
                        assets::FIGHTER => 44,
                        assets::ROCK | assets::BUILDING => 18,
                        _ => 36,
                    };
                    let center_y = actor_center(g.actors[0], &bank).y;
                    let xs = capture.0.iter().map(|v| v.x);
                    let ys = capture.0.iter().map(|v| v.y);
                    assert_eq!((xs.clone().min(), xs.max()), (Some(-radius), Some(radius)));
                    assert_eq!(
                        (ys.clone().min(), ys.max()),
                        (Some(center_y - radius), Some(center_y + radius))
                    );
                }
            }
        }
    }
    #[test]
    fn enlarged_carrier_targets_are_reachable_and_fire_from_the_visible_parts() {
        for side in [-1, 1] {
            for (x, hit) in [
                (83, false),
                (84, true),
                (100, true),
                (176, true),
                (177, false),
            ] {
                let mut g = running();
                g.boss_started = true;
                g.boss_z = 680;
                g.shots[0] = Shot {
                    pos: Vec3 {
                        x: x * side,
                        y: 30,
                        z: 562,
                    },
                    kind: 1,
                    damage: 1,
                    ..Shot::EMPTY
                };
                g.update_shots(&test_bank(3));
                assert_eq!(
                    if side < 0 { g.boss_left } else { g.boss_right },
                    20 - u8::from(hit)
                );
            }
            // Center launches have room to turn during approach; at the parked
            // carrier the player can launch from the corresponding arena edge.
            for (boss_z, player_x) in [(900, 0), (680, 100 * side)] {
                let mut g = running();
                g.boss_started = true;
                g.boss_z = boss_z;
                g.player.x = player_x;
                if side < 0 {
                    g.boss_right = 0;
                } else {
                    g.boss_left = 0;
                }
                g.fire_player(255);
                for _ in 0..30 {
                    g.update_shots(&test_bank(3));
                }
                assert_eq!(
                    if side < 0 { g.boss_left } else { g.boss_right },
                    16,
                    "side {side}, boss_z {boss_z}"
                );
            }
        }
        let mut g = running();
        g.rail_ticks = 11_400;
        g.boss_z = 680;
        g.ticks = 40;
        g.update_boss();
        assert_eq!(
            g.shots[0].pos,
            Vec3 {
                x: -130,
                y: 66,
                z: 554
            }
        );
        assert_eq!(
            g.shots[1].pos,
            Vec3 {
                x: 130,
                y: 66,
                z: 554
            }
        );
        g.shots.fill(Shot::EMPTY);
        g.boss_left = 0;
        g.boss_right = 0;
        g.ticks = 50;
        g.update_boss();
        assert_eq!(
            g.shots[0].pos,
            Vec3 {
                x: -28,
                y: 0,
                z: 480
            }
        );
        assert_eq!(
            g.shots[1].pos,
            Vec3 {
                x: 28,
                y: 0,
                z: 480
            }
        );
    }
    #[test]
    fn missiles_obey_obstacle_collision_and_boss_core_protection() {
        let mut g = running();
        g.actors[0] = enemy(assets::ROCK, 0, -20, 230);
        g.actors[0].hp = 255;
        g.fire_player(255);
        g.update_shots(&test_bank(g.segment_index()));
        assert_eq!(g.shots[0].kind, 0);
        assert_eq!(g.actors[0].hp, 255);
        assert_eq!(g.score, 0);
        g.actors[0] = Actor::EMPTY;
        g.boss_started = true;
        g.boss_z = 680;
        g.shots[0] = Shot {
            pos: Vec3 { x: 0, y: 0, z: 450 },
            kind: 3,
            damage: 4,
            ..Shot::EMPTY
        };
        g.update_shots(&test_bank(g.segment_index()));
        assert_eq!(g.shots[0].kind, 0);
        assert_eq!(g.boss_core, 60, "a missile cannot damage a shielded core");
    }
    #[test]
    fn missile_pool_priority_preserves_charge_and_ordinary_shot_rules() {
        let missile = Shot {
            pos: Vec3 { x: 0, y: 0, z: 198 },
            kind: 3,
            damage: 4,
            ..Shot::EMPTY
        };
        let mut g = running();
        g.add_shot(missile);
        assert_eq!(g.shots[0].kind, 3);
        for (i, shot) in g.shots.iter_mut().enumerate() {
            *shot = Shot {
                pos: Vec3 {
                    x: 0,
                    y: 0,
                    z: 300 + i as i16 * 10,
                },
                kind: 1,
                ..Shot::EMPTY
            };
        }
        g.shots[2].kind = 2;
        g.add_shot(missile);
        assert_eq!(g.shots[2].kind, 3, "hostiles have first eviction priority");
        assert_eq!(g.shots[23].kind, 1);
        g.add_shot(missile);
        assert_eq!(
            g.shots[23].kind, 3,
            "then replace the farthest ordinary laser"
        );
        g.add_shot(Shot {
            kind: 1,
            ..Shot::EMPTY
        });
        assert_eq!(g.shots[23].kind, 3, "ordinary lasers never evict missiles");
        for (i, shot) in g.shots.iter_mut().enumerate() {
            *shot = Shot {
                pos: Vec3 {
                    x: 0,
                    y: 0,
                    z: 300 + i as i16,
                },
                ..missile
            };
        }
        g.add_shot(missile);
        assert_eq!(
            g.shots[23].pos.z, 198,
            "all-missile saturation still fires the charge"
        );
        g.shots[0].kind = 2;
        g.bomb(&test_bank(g.segment_index()));
        assert_eq!(g.shots[0].kind, 0);
        assert_eq!(g.shots[23].kind, 3, "bombs clear only hostile shots");
    }
    #[test]
    fn damage_flash_is_independent_of_immunity_and_resets_with_the_run() {
        let mut g = Game::new();
        g.reset_run();
        assert_eq!((g.invulnerability, g.hit_flash_ticks), (50, 0));
        g.invulnerability = 0;
        g.damage_player(12);
        assert_eq!((g.shield, g.hit_flash_ticks), (88, 35));
        let bytes = empty_segment(0);
        let segment = Segment::new(&bytes).unwrap();
        for _ in 0..10 {
            g.tick(&segment, &test_bank(g.segment_index()));
        }
        g.damage_player(12);
        assert_eq!(
            (g.shield, g.hit_flash_ticks),
            (88, 25),
            "ignored hits do not restart the flash"
        );
        for _ in 0..25 {
            g.tick(&segment, &test_bank(g.segment_index()));
        }
        assert_eq!(g.hit_flash_ticks, 0);
        g.damage_player(12);
        assert_eq!(g.hit_flash_ticks, 35);
        g.mode = Mode::Victory;
        assert_eq!(
            g.player_appearance() >> 8,
            0,
            "victory never freezes a white hull"
        );
        g.respawn();
        assert_eq!((g.invulnerability, g.hit_flash_ticks), (100, 0));
        g.hit_flash_ticks = 35;
        g.reset_run();
        assert_eq!(g.hit_flash_ticks, 0);
    }
    #[test]
    fn full_mission_with_three_scenic_bridges_is_winnable_with_regular_joystick_input() {
        let banks = [
            mission_segment(0),
            mission_segment(1),
            mission_segment(2),
            mission_segment(3),
        ];
        let mut g = running();
        let mut visited = 0u8;
        let mut scenic_ticks = 0u16;
        for tick in 1..=18_000u32 {
            let section = g.segment_index();
            visited |= 1 << section;
            let segment = Segment::new(&banks[section as usize]).unwrap();
            #[cfg(feature = "host-tools")]
            let model_bytes = enemies::authored::model_bank(section);
            #[cfg(feature = "host-tools")]
            let bank = AssetBank::new(&model_bytes).unwrap();
            #[cfg(not(feature = "host-tools"))]
            let bank = test_bank(section);
            let mut target = Vec3 { x: 0, y: 0, z: 800 };
            if g.boss_started && !g.boss_defeated {
                target = if g.boss_left > 0 {
                    Vec3 {
                        x: -100,
                        y: 30,
                        z: 610,
                    }
                } else if g.boss_right > 0 {
                    Vec3 {
                        x: 100,
                        y: 30,
                        z: 610,
                    }
                } else {
                    Vec3 { x: 0, y: 0, z: 480 }
                };
            } else {
                let mut nearest = 2400;
                for actor in &g.actors {
                    if actor.kind != 0
                        && actor.hp != 255
                        && actor.pos.z > 300
                        && actor.pos.z < nearest
                    {
                        target = actor_center(*actor, &bank);
                        nearest = actor.pos.z;
                    }
                }
                // Follow collectable shields when no attack run is active.
                if nearest == 2400 {
                    for pickup in &g.pickups {
                        if pickup.kind == 1 && pickup.pos.z > 200 {
                            target = pickup.pos;
                            break;
                        }
                    }
                }
            }
            let mut bits = FIRE;
            if target.x - g.player.x > 3 {
                bits |= RIGHT;
            } else if target.x - g.player.x < -3 {
                bits |= LEFT;
            }
            if target.y - g.player.y > 3 {
                bits |= UP;
            } else if target.y - g.player.y < -3 {
                bits |= DOWN;
            }
            // Defensive double taps use the same input path as the hardware.
            match tick % 50 {
                49 | 1 => bits &= !(LEFT | RIGHT),
                0 | 2 => bits = (bits & !RIGHT) | LEFT,
                _ => {}
            }
            if g.boss_started && matches!(tick % 200, 0 | 2) {
                bits &= !FIRE;
            }
            sample(&mut g, bits, tick * 20);
            g.tick(&segment, &bank);
            if g.mode == Mode::Transition {
                scenic_ticks += 1;
            }
            while g.pop_audio().is_some() {}
            assert_ne!(
                g.mode,
                Mode::GameOver,
                "pilot lost all lives at distance {}",
                g.hud().distance
            );
            if g.mode == Mode::Victory {
                assert_eq!(visited, 15);
                assert!(tick >= 15_000 + 3 * TRANSITION_TICKS as u32);
                assert!(scenic_ticks >= 3 * TRANSITION_TICKS);
                assert!(g.score >= 5000);
                return;
            }
        }
        panic!(
            "mission did not finish: {:?}, distance {}, boss {}/{}/{}, lives {}",
            g.mode,
            g.hud().distance,
            g.boss_left,
            g.boss_right,
            g.boss_core,
            g.lives
        );
    }
    #[test]
    fn level_rosters_have_distinct_movement_and_attack_tactics() {
        let player = Vec3 {
            x: 90,
            y: 30,
            z: PLAYER_Z,
        };
        let mut poses = [[[Vec3::default(); 4]; 3]; 4];
        let mut attacks = [[0u16; 3]; 4];
        for level in 0..4 {
            for kind in 1..=test_bank(level).enemy_count() {
                let mut actor = enemy(kind, -90, 0, 1800);
                for age in 1..=180 {
                    actor.age = age;
                    attacks[level as usize][kind as usize - 1] +=
                        enemies::step(level, &mut actor, &player, 0) as u16;
                    if let Some(sample) = [16, 64, 112, 152].iter().position(|&v| v == age) {
                        poses[level as usize][kind as usize - 1][sample] = actor.pos;
                    }
                }
            }
        }
        assert_ne!(poses[0][0][0].x, -90, "patrol weaves");
        assert!(poses[0][1][1].x > poses[0][1][0].x, "diver pursues");
        assert_eq!(poses[0][2][2].x, -90, "ground gun holds its lane");
        assert!(poses[1][0][1].x > 0, "skimmer crosses the canyon");
        assert!(
            poses[1][1][0].y > 0 && poses[1][1][1].y < 0,
            "swooper changes altitude"
        );
        assert_eq!(attacks[1][2], 2, "wall battery fires a paired burst");
        assert_eq!(poses[2][0][0].z, 1800 - 16 * 6, "gunship approaches slowly");
        assert_eq!(
            poses[2][1][2].z, poses[2][1][3].z,
            "lance holds before rushing"
        );
        assert_eq!(attacks[2][1], 2, "lance fires twice during its hold");
        assert_eq!(attacks[2][2], 2, "bunker requests a fan attack");
        assert!(
            poses[3][0].iter().all(|p| (-106..=-74).contains(&p.x)),
            "guard holds the flank"
        );
        assert!(
            poses[3][1][1].x > poses[3][1][0].x,
            "rammer tracks the player"
        );
        assert_eq!(
            poses[3][1][2].x, poses[3][1][3].x,
            "rammer commits near the player"
        );
        assert_eq!(attacks[3][1], 0, "rammer has no gun");
    }

    #[test]
    fn every_level_enemy_uses_banked_health_rewards_and_missile_guidance() {
        let mut exercised = 0;
        for level in 0..4 {
            let bank = test_bank(level);
            for kind in 1..=bank.enemy_count() {
                exercised += 1;
                let mut g = running();
                g.rail_ticks = level as u16 * SECTION_TICKS;
                let spawn = Spawn {
                    kind,
                    x: 85,
                    y: 25,
                    ..Spawn::default()
                };
                g.spawn_actor(spawn, 0, &bank);
                assert_eq!(g.actors[0].hp, bank.enemy(kind).hp);
                let center = actor_center(g.actors[0], &bank);
                for _ in 0..bank.enemy(kind).hp {
                    g.shots[0] = Shot {
                        pos: Vec3 {
                            z: center.z - 48,
                            ..center
                        },
                        damage: 1,
                        kind: 1,
                        ..Shot::EMPTY
                    };
                    g.update_shots(&bank);
                }
                assert_eq!(g.actors[0].kind, 0, "laser kill level {level} kind {kind}");
                assert_eq!(g.score, bank.enemy(kind).score as u32);
                assert_eq!(g.bomb_kills, 1);

                g.spawn_actor(spawn, 0, &bank);
                g.player.x = -100;
                g.player.y = -65;
                let initial_hp = g.actors[0].hp;
                g.fire_player(255);
                for _ in 0..100 {
                    g.update_actors(&bank);
                    g.update_shots(&bank);
                    if g.actors[0].kind == 0 || g.actors[0].hp < initial_hp {
                        break;
                    }
                }
                assert!(
                    g.actors[0].kind == 0 || g.actors[0].hp < initial_hp,
                    "missile failed level {level} kind {kind}"
                );

                g.actors = [Actor::EMPTY; ACTORS];
                g.spawn_actor(spawn, 0, &bank);
                g.bomb(&bank);
                assert_eq!(g.actors[0].kind, 0, "bomb kill level {level} kind {kind}");
            }
        }
        assert_eq!(exercised, 11);
    }

    #[test]
    fn lance_guidance_uses_hold_and_rush_speed_and_fans_keep_one_audio_cue() {
        let bank = test_bank(2);
        let mut g = running();
        g.rail_ticks = 2 * SECTION_TICKS;
        g.actors[0] = enemy(2, 50, 0, 904);
        g.actors[0].age = 112;
        g.update_actors(&bank);
        assert_eq!(g.missile_target(g.player, &bank).unwrap().1, 32);
        g.actors[0].age = 152;
        g.update_actors(&bank);
        assert_eq!(g.missile_target(g.player, &bank).unwrap().1, 46);
        g.enemy_attack(Vec3 { x: 0, y: 0, z: 900 }, 2);
        let mut velocities = [0; 3];
        for (at, shot) in g.shots.iter().filter(|s| s.kind == 2).enumerate() {
            velocities[at] = shot.dx;
        }
        velocities.sort();
        assert_eq!(velocities, [-2, 0, 2]);
        assert_eq!(g.pop_audio().unwrap().effect, Effect::EnemyFire);
        assert!(g.pop_audio().is_none());
    }

    #[test]
    fn a_defeated_carrier_does_not_spawn_remaining_escorts() {
        let mut bytes = empty_segment(3);
        bytes[7] = 1;
        bytes[18] = assets::FIGHTER;
        let segment = Segment::new(&bytes).unwrap();
        let mut g = running();
        g.rail_ticks = 3 * SECTION_TICKS;
        g.boss_defeated = true;
        g.tick(&segment, &test_bank(3));
        assert_eq!(g.spawn_cursor, 1);
        assert!(g.actors.iter().all(|a| a.kind == 0));
    }

    fn model_bank_for_tests() -> [u8; assets::COMMON_SIZE] {
        let mut data = test_bank_bytes(0);
        let mut cursor = assets::MODEL_DATA_OFFSET;
        for id in 0..8 {
            let directory = 16 + id * 8;
            data[directory..directory + 2].copy_from_slice(&(cursor as u16).to_le_bytes());
            data[directory + 2] = 3;
            data[directory + 3] = if id == 4 { 17 } else { 12 };
            for vertex in [[-20i16, -10, -5], [20, -10, 5], [0, 20, 0]] {
                for axis in vertex {
                    data[cursor..cursor + 2].copy_from_slice(&axis.to_le_bytes());
                    cursor += 2;
                }
            }
            data[directory + 4..directory + 6].copy_from_slice(&(cursor as u16).to_le_bytes());
            for _ in 0..data[directory + 3] {
                data[cursor..cursor + 4].copy_from_slice(&[0, 1, 2, 15]);
                cursor += 4;
            }
        }
        data
    }
    #[test]
    fn white_hit_flash_preserves_ship_and_exhaust_geometry_without_recoloring_the_scene() {
        struct Capture {
            faces: [([Vec3; 3], u8); 256],
            count: usize,
        }
        impl GeometrySink for Capture {
            fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, _flags: u8) {
                self.faces[self.count] = ([a, b, c], color);
                self.count += 1;
            }
        }
        fn capture(game: &Game, bank: &AssetBank<'_>, segment: &Segment<'_>) -> Capture {
            let mut result = Capture {
                faces: [([Vec3::default(); 3], 0); 256],
                count: 0,
            };
            game.render(&mut result, bank, segment);
            result
        }
        let data = model_bank_for_tests();
        let bank = AssetBank::new(&data).unwrap();
        let bytes = empty_segment(0);
        let segment = Segment::new(&bytes).unwrap();
        let mut g = running();
        g.ticks = 2; // This was a hidden phase under the old immunity blink.
        let normal = capture(&g, &bank, &segment);
        let hull_faces = bank.face_count(assets::PLAYER) as usize;
        g.invulnerability = 35;
        for timer in 0..=35 {
            g.hit_flash_ticks = timer;
            let actual = capture(&g, &bank, &segment);
            let white = timer != 0 && ((35 - timer) / 10) % 2 == 0;
            assert_eq!(
                actual.count, normal.count,
                "flash timer {timer} hid geometry"
            );
            for face in 0..hull_faces {
                assert_eq!(actual.faces[face].0, normal.faces[face].0);
                assert_eq!(
                    actual.faces[face].1,
                    if white { 1 } else { normal.faces[face].1 }
                );
            }
            assert_eq!(
                actual.faces[hull_faces..actual.count],
                normal.faces[hull_faces..normal.count],
                "flash timer {timer} changed exhaust, reticle or terrain"
            );
        }
        g.hit_flash_ticks = 0;
        for protection in [50, 35, 1, 100] {
            g.invulnerability = protection;
            let actual = capture(&g, &bank, &segment);
            assert_eq!(actual.faces[..actual.count], normal.faces[..normal.count]);
        }
        g.hit_flash_ticks = 35;
        g.mode = Mode::Victory;
        let victory = capture(&g, &bank, &segment);
        assert_eq!(victory.faces[..hull_faces], normal.faces[..hull_faces]);
        for mode in [Mode::Respawning, Mode::GameOver] {
            g.mode = mode;
            let dead = capture(&g, &bank, &segment);
            assert_eq!(
                dead.count,
                normal.count - hull_faces - 2 - 2,
                "destroyed ships retain the existing death sequence"
            );
        }
    }
    #[test]
    fn fully_occupied_scene_keeps_critical_geometry_inside_command_budget() {
        struct CountScenery {
            total: usize,
            cloud_faces: usize,
        }
        impl GeometrySink for CountScenery {
            fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, _: u8, _: u8) {
                self.total += 1;
                if a.z > 2400 && b.z > 2400 && c.z > 2400 {
                    self.cloud_faces += 1;
                }
            }
        }
        let data = model_bank_for_tests();
        let bank = AssetBank::new(&data).unwrap();
        let mut g = running();
        g.boss_started = true;
        g.boss_z = 680;
        for a in &mut g.actors {
            *a = Actor {
                pos: Vec3 { x: 0, y: 0, z: 100 },
                kind: assets::TURRET,
                hp: 6,
                ..Actor::EMPTY
            };
        }
        for s in &mut g.shots {
            *s = Shot {
                pos: Vec3 { x: 0, y: 0, z: 300 },
                kind: 2,
                damage: 12,
                ..Shot::EMPTY
            };
        }
        for p in &mut g.pickups {
            *p = Pickup {
                pos: Vec3 { x: 0, y: 0, z: 400 },
                kind: 1,
            };
        }
        for e in &mut g.explosions {
            *e = Explosion {
                pos: Vec3 { x: 0, y: 0, z: 500 },
                ticks: 9,
                color: 7,
            };
        }
        let mut renderer = crate::render::Renderer::new();
        for theme in 0..4 {
            let mut scene = empty_segment(theme);
            scene[5] = theme;
            let segment = Segment::new(&scene).unwrap();
            for phase in 0..64 {
                g.rail_ticks = ((theme as u16 * SECTION_TICKS + 63) & !63) + phase;
                let mut scenery = CountScenery {
                    total: 0,
                    cloud_faces: 0,
                };
                g.render(&mut scenery, &bank, &segment);
                assert_eq!(
                    scenery.cloud_faces, 9,
                    "busy scenes retain every cloud facet"
                );
                assert!(scenery.total <= 233);
                renderer.begin(g.sky_color());
                g.render(&mut renderer, &bank, &segment);
                renderer.finish();
                assert_eq!(renderer.dropped, 0, "terrain phase {phase}");
                assert!(renderer.triangle_count > 100);
                assert!(renderer.triangle_count <= 256);
            }
        }
    }
    #[cfg(feature = "host-tools")]
    #[test]
    fn real_enemy_rosters_fit_saturated_and_near_scenes_with_identical_staged_pixels() {
        use crate::render::{Renderer, BAND_PIXELS};
        for level in 0..4 {
            let models = enemies::authored::model_bank(level);
            let bank = AssetBank::new(&models).unwrap();
            let bytes = mission_segment(level);
            let segment = Segment::new(&bytes).unwrap();
            for &(x, y) in &[(0, 0), (-100, -65), (100, 65)] {
                for near in [false, true] {
                    let mut g = running();
                    g.rail_ticks = level as u16 * SECTION_TICKS + 60;
                    g.player.x = x;
                    g.player.y = y;
                    g.boss_started = level == 3;
                    g.boss_z = if near { 100 } else { 680 };
                    for (i, actor) in g.actors.iter_mut().enumerate() {
                        let kind = 1 + i as u8 % bank.enemy_count();
                        *actor = enemy(
                            kind,
                            (i as i16 % 3 - 1) * 50,
                            20,
                            if near { 80 } else { 700 + i as i16 * 20 },
                        );
                        actor.hp = bank.enemy(kind).hp;
                    }
                    for (i, shot) in g.shots.iter_mut().enumerate() {
                        *shot = Shot {
                            pos: Vec3 {
                                x: (i as i16 % 3 - 1) * 40,
                                y: 0,
                                z: if near { 40 } else { 300 },
                            },
                            kind: 2,
                            damage: 12,
                            ..Shot::EMPTY
                        };
                    }
                    for pickup in &mut g.pickups {
                        *pickup = Pickup {
                            pos: Vec3 {
                                x: 60,
                                y: 0,
                                z: 300,
                            },
                            kind: 1,
                        };
                    }
                    for explosion in &mut g.explosions {
                        *explosion = Explosion {
                            pos: Vec3 {
                                x: -60,
                                y: 20,
                                z: 300,
                            },
                            ticks: 10,
                            color: 7,
                        };
                    }
                    let mut direct = Renderer::new();
                    let mut staged = Renderer::new();
                    direct.begin(g.sky_color());
                    g.render(&mut direct, &bank, &segment);
                    direct.finish();
                    staged.begin(g.sky_color());
                    let raw_count = {
                        let mut stage = staged.scenery_stage();
                        g.render_scenery(&mut stage, &segment);
                        stage.count()
                    };
                    let prefix = staged.project_scenery(raw_count);
                    g.render_foreground(&mut staged, &bank, 174.min(233 - raw_count as u16));
                    staged.scenery_last(prefix);
                    staged.finish();
                    assert_eq!(
                        (direct.dropped, staged.dropped),
                        (0, 0),
                        "level {level}, near {near}"
                    );
                    assert_eq!(direct.triangle_count, staged.triangle_count);
                    for band in 0..25 {
                        let mut a = [0; BAND_PIXELS];
                        let mut b = [0; BAND_PIXELS];
                        direct.render_band(band, &mut a);
                        staged.render_band(band, &mut b);
                        assert_eq!(
                            a, b,
                            "level {level}, near {near}, player ({x},{y}), band {band}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn saturated_bridge_weapons_preserve_foreground_and_match_staged_rendering() {
        use crate::render::{Renderer, BAND_PIXELS};

        let data = model_bank_for_tests();
        let bank = AssetBank::new(&data).unwrap();
        let hull_faces = bank.face_count(assets::PLAYER) as u16 + 2; // Ship and exhaust.
        let essential_faces = hull_faces + 2 + SHOTS as u16 * 2 + 6 * 2;
        let mut direct = Renderer::new();
        let mut staged = Renderer::new();
        let mut isolated_ship = Renderer::new();
        for section in 0..3 {
            let bytes = empty_segment(section);
            let segment = Segment::new(&bytes).unwrap();
            for x in [-100, 100] {
                for y in [-65, 65] {
                    let mut game = running();
                    game.mode = Mode::Transition;
                    game.rail_ticks = (section as u16 + 1) * SECTION_TICKS;
                    game.player.x = x;
                    game.player.y = y;
                    for (index, shot) in game.shots.iter_mut().enumerate() {
                        *shot = Shot {
                            pos: Vec3 {
                                x: (index as i16 % 7 - 3) * 30,
                                y: (index as i16 % 5 - 2) * 22,
                                z: 198 + index as i16 * 83,
                            },
                            kind: if index & 1 == 0 { 1 } else { 3 },
                            damage: if index & 1 == 0 { 1 } else { 4 },
                            ..Shot::EMPTY
                        };
                    }
                    for (index, explosion) in game.explosions.iter_mut().enumerate() {
                        *explosion = Explosion {
                            pos: Vec3 {
                                x: (index as i16 - 3) * 36,
                                y: (index as i16 % 3 - 1) * 35,
                                z: 400 + index as i16 * 150,
                            },
                            ticks: 1 + index as u8 * 3,
                            color: 7,
                        };
                    }
                    for elapsed in 0..TRANSITION_TICKS {
                        game.transition_ticks = elapsed;
                        game.ticks = elapsed;
                        game.roll_ticks = 1 + (elapsed % 25) as u8;
                        game.roll_direction = if elapsed & 1 == 0 { -1 } else { 1 };
                        direct.begin(game.sky_color());
                        game.render(&mut direct, &bank, &segment);
                        direct.finish();

                        staged.begin(game.sky_color());
                        let raw_count = {
                            let mut stage = staged.scenery_stage();
                            game.render_scenery(&mut stage, &segment);
                            stage.count()
                        };
                        let foreground_limit = 174.min(233 - raw_count as u16);
                        assert!(foreground_limit >= essential_faces);
                        let prefix = staged.project_scenery(raw_count);
                        game.render_foreground(&mut staged, &bank, foreground_limit);
                        staged.scenery_last(prefix);
                        staged.finish();
                        assert_eq!(
                            (direct.dropped, staged.dropped),
                            (0, 0),
                            "bridge {section}, tick {elapsed}, player ({x},{y})"
                        );
                        assert_eq!(direct.triangle_count, staged.triangle_count);

                        if matches!(elapsed, 0 | 199 | 399) {
                            isolated_ship.begin(0);
                            game.render_foreground(&mut isolated_ship, &bank, hull_faces);
                            isolated_ship.finish();
                            let mut visible_ship_pixels = 0;
                            let mut ship_pixels = 0;
                            for band in 0..25 {
                                let mut expected = [0; BAND_PIXELS];
                                let mut actual = [0; BAND_PIXELS];
                                let mut ship = [0; BAND_PIXELS];
                                direct.render_band(band, &mut expected);
                                staged.render_band(band, &mut actual);
                                isolated_ship.render_band(band, &mut ship);
                                assert_eq!(
                                    actual, expected,
                                    "bridge {section}, tick {elapsed}, player ({x},{y}), band {band}"
                                );
                                for (&hull, &scene) in ship.iter().zip(&actual) {
                                    if hull != 0 {
                                        ship_pixels += 1;
                                        visible_ship_pixels += usize::from(scene == hull);
                                    }
                                }
                            }
                            assert!(ship_pixels > 0, "the rolled ship left the viewport");
                            // Terrain can overlap a rolled hull at camera extremes;
                            // saturation must not remove all essential foreground.
                            assert!(visible_ship_pixels > 0,
                                "bridge {section}, tick {elapsed}, player ({x},{y}): only {visible_ship_pixels}/{ship_pixels} ship pixels visible");
                        }
                    }
                }
            }
        }
    }
}
