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
const PLAYER_Z: i16 = 180;
const SPEED: i16 = 8;
const SECTION_TICKS: u16 = 3750;
const MISSION_TICKS: u16 = 15000;
const ACTORS: usize = 12;
const SHOTS: usize = 24;
const AUDIO_CAPACITY: usize = 8;

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

#[derive(Clone, Copy)]
struct Actor {
    pos: Vec3,
    origin_x: i16,
    origin_y: i16,
    age: u16,
    kind: u8,
    hp: u8,
    flags: u8,
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
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
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
            if self.fire_armed && input.ms.wrapping_sub(self.fire_at) <= 250 {
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
                if self.left_armed && input.ms.wrapping_sub(self.left_at) <= 250 {
                    self.roll_pending = -1;
                    self.left_armed = false;
                } else {
                    self.left_at = input.ms;
                    self.left_armed = true;
                }
            }
            if pressed & RIGHT != 0 {
                if self.right_armed && input.ms.wrapping_sub(self.right_at) <= 250 {
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
    player: Vec3,
    // One rail tick is eight world units. This keeps all simulation distance
    // arithmetic in 16 bits while representing the complete 120000-unit route.
    rail_ticks: u16,
    score: u32,
    ticks: u16,
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
            player: Vec3 {
                x: 0,
                y: 0,
                z: PLAYER_Z,
            },
            rail_ticks: 0,
            score: 0,
            ticks: 0,
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
    #[inline(never)]
    pub fn segment_index(&self) -> u8 {
        if self.rail_ticks < SECTION_TICKS {
            0
        } else if self.rail_ticks < SECTION_TICKS * 2 {
            1
        } else if self.rail_ticks < SECTION_TICKS * 3 {
            2
        } else {
            3
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
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
            Mode::Victory => MusicState::Victory,
            Mode::GameOver => MusicState::Defeat,
            _ if self.boss_started && !self.boss_defeated => MusicState::Boss,
            _ if self.segment_index() == 0 => MusicState::Exploration,
            _ => MusicState::Combat,
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
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn sound(&mut self, effect: Effect, x: i16) {
        let event = AudioEvent {
            effect,
            pan: x.clamp(-127, 127) as i8,
        };
        // Reserve an event for urgent player cues even when a full formation fires.
        if self.audio_len as usize == AUDIO_CAPACITY {
            if matches!(
                effect,
                Effect::Death | Effect::Bomb | Effect::Hit | Effect::ChargedShot
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
        let bits = self.controls.bits;
        let now = self.controls.now;
        *self = Self::new();
        self.mode = Mode::Playing;
        self.controls.bits = bits;
        self.controls.now = now;
        self.controls.released_at = now;
        self.invulnerability = 50;
    }
    /// One 20 ms simulation step. Feed every captured input sample first.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    pub fn tick(&mut self, segment: &Segment<'_>) {
        self.ticks = self.ticks.wrapping_add(1);
        match self.mode {
            Mode::Title | Mode::Victory | Mode::GameOver => {
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
            Mode::Playing => {}
        }
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
            self.bomb();
        }
        self.controls.clear_pending();
        self.rail_ticks = (self.rail_ticks + 1).min(MISSION_TICKS);
        let section = self.segment_index();
        if self.spawn_segment != section {
            self.spawn_segment = section;
            self.spawn_cursor = 0;
        }
        if segment.index() == section {
            let local = (self.rail_ticks - section as u16 * SECTION_TICKS) * 8;
            while self.spawn_cursor < segment.spawn_count() {
                let spawn = segment.spawn(self.spawn_cursor);
                if spawn.distance > local {
                    break;
                }
                self.spawn_cursor += 1;
                self.spawn_actor(spawn, local - spawn.distance);
            }
        }
        self.update_actors();
        self.update_boss();
        self.update_shots();
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
            if charge > 0 {
                Effect::ChargedShot
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
        self.sound(Effect::EnemyFire, position.x);
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn spawn_actor(&mut self, spawn: Spawn, late: u16) {
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
        let hp = match spawn.kind {
            assets::FIGHTER => 3,
            assets::DIVER => 2,
            assets::TURRET => 6,
            _ => 255,
        };
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
            };
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn update_actors(&mut self) {
        for i in 0..ACTORS {
            let mut a = self.actors[i];
            if a.kind == 0 {
                continue;
            }
            a.age = a.age.wrapping_add(1);
            a.pos.z -= if a.kind == assets::DIVER {
                12
            } else {
                SPEED as i16
            };
            if a.kind == assets::FIGHTER {
                a.pos.x = a.origin_x + crate::math::sin8((a.age as u8).wrapping_mul(2)) / 12;
                a.pos.y = a.origin_y + crate::math::sin8((a.age as u8).wrapping_add(64)) / 24;
            } else if a.kind == assets::DIVER && a.pos.z > 300 {
                a.pos.x += (self.player.x - a.pos.x).clamp(-2, 2);
                a.pos.y += (self.player.y - a.pos.y).clamp(-1, 1);
            }
            let shoots = matches!(a.kind, assets::FIGHTER | assets::DIVER | assets::TURRET);
            if shoots && a.pos.z > 320 && a.pos.z < 1400 && (a.age + 17 * i as u16) % 65 == 0 {
                self.enemy_shot(actor_center(a));
            }
            let radius = if matches!(a.kind, assets::BUILDING | assets::ROCK) {
                36
            } else {
                18
            };
            if (a.pos.z - PLAYER_Z).abs() < 32 && close_xy(actor_center(a), self.player, radius) {
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
                    x: -65,
                    y: 15,
                    z: self.boss_z - 20,
                });
            }
            if self.boss_right > 0 {
                self.enemy_shot(Vec3 {
                    x: 65,
                    y: 15,
                    z: self.boss_z - 20,
                });
            }
            if self.boss_left == 0 && self.boss_right == 0 {
                self.enemy_shot(Vec3 {
                    x: -14,
                    y: 0,
                    z: self.boss_z - 40,
                });
                self.enemy_shot(Vec3 {
                    x: 14,
                    y: 0,
                    z: self.boss_z - 40,
                });
            }
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    fn update_shots(&mut self) {
        for i in 0..SHOTS {
            let mut s = self.shots[i];
            if s.kind == 0 {
                continue;
            }
            if s.kind == 3 {
                self.steer_missile(&mut s);
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
                        19
                    };
                    if swept_z(old_z, s.pos.z, target.pos.z, radius)
                        && close_xy(s.pos, actor_center(target), radius)
                    {
                        s.kind = 0;
                        if target.hp != 255 {
                            self.actors[a].hp = self.actors[a].hp.saturating_sub(s.damage);
                            if self.actors[a].hp == 0 {
                                self.kill_actor(a);
                            } else {
                                self.sound(Effect::Hit, target.pos.x);
                            }
                        } else {
                            self.explode(s.pos, 15);
                        }
                        break;
                    }
                }
                if (s.kind == 1 || s.kind == 3)
                    && self.boss_started
                    && !self.boss_defeated
                    && swept_z(old_z, s.pos.z, self.boss_z, 48)
                {
                    let mut hit = false;
                    if self.boss_left > 0
                        && close_xy(
                            s.pos,
                            Vec3 {
                                x: -65,
                                y: 15,
                                z: 0,
                            },
                            23,
                        )
                    {
                        self.boss_left = self.boss_left.saturating_sub(s.damage);
                        hit = true;
                    } else if self.boss_right > 0
                        && close_xy(s.pos, Vec3 { x: 65, y: 15, z: 0 }, 23)
                    {
                        self.boss_right = self.boss_right.saturating_sub(s.damage);
                        hit = true;
                    } else if close_xy(s.pos, Vec3 { x: 0, y: 0, z: 0 }, 30) {
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
            } else if s.kind == 2
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
    fn missile_target(&self, position: Vec3) -> Option<(Vec3, i16)> {
        let mut best = None;
        let mut best_z = i16::MAX;
        let mut best_xy = i16::MAX;
        // Stable order breaks exact ties: actor slots, left turret, right
        // turret, exposed core. Reacquisition needs no persistent actor ID.
        for i in 0..ACTORS + 3 {
            let (target, closing) = if i < ACTORS {
                let actor = self.actors[i];
                if actor.hp == 0
                    || !matches!(actor.kind, assets::FIGHTER | assets::DIVER | assets::TURRET)
                {
                    continue;
                }
                (
                    actor_center(actor),
                    if actor.kind == assets::DIVER { 44 } else { 40 },
                )
            } else {
                if !self.boss_started || self.boss_defeated {
                    continue;
                }
                let (x, y) = match i - ACTORS {
                    0 if self.boss_left > 0 => (-65, 15),
                    1 if self.boss_right > 0 => (65, 15),
                    2 if self.boss_left == 0 && self.boss_right == 0 && self.boss_core > 0 => {
                        (0, 0)
                    }
                    _ => continue,
                };
                (
                    Vec3 {
                        x,
                        y,
                        z: self.boss_z,
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
    fn steer_missile(&self, shot: &mut Shot) {
        let (dx, dy) = if let Some((target, closing)) = self.missile_target(shot.pos) {
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
    fn kill_actor(&mut self, index: usize) {
        let a = self.actors[index];
        self.actors[index].kind = 0;
        self.score += if a.kind == assets::TURRET { 250 } else { 100 };
        self.kills = self.kills.wrapping_add(1);
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
    fn bomb(&mut self) {
        self.bombs -= 1;
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
                    self.kill_actor(i);
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
        self.rail_ticks = self.segment_index() as u16 * SECTION_TICKS;
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
            let mut p = self.pickups[i];
            if p.kind == 0 {
                continue;
            }
            p.pos.z -= SPEED as i16;
            if (p.pos.z - PLAYER_Z).abs() < 25 && close_xy(p.pos, self.player, 28) {
                if p.kind == 1 {
                    self.shield = self.shield.saturating_add(30).min(100);
                } else {
                    self.bombs = (self.bombs + 1).min(3);
                }
                self.score += 50;
                self.sound(Effect::Pickup, 0);
                p.kind = 0;
            } else if p.pos.z < 40 {
                p.kind = 0;
            }
            self.pickups[i] = p;
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
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
    pub fn render(
        &self,
        sink: &mut impl GeometrySink,
        bank: &AssetBank<'_>,
        segment: &Segment<'_>,
    ) {
        // Prioritize player, boss, projectiles and pickups before distant detail.
        // 224 raw faces leave room for near-plane splitting in the 256 commands.
        let mut budget = SceneBudget {
            sink,
            count: 0,
            limit: 174,
            // An elevated chase camera separates the nearby ship from targets
            // on its firing line. Keep the title's original showcase framing.
            // Follow a quarter of vertical movement so the lowest banked hull
            // stays above the cockpit while neutral targets remain exposed.
            camera_y: chase_camera_y(self.mode, self.player.y),
            color_override: 0,
        };
        let sink = &mut budget;
        if matches!(self.mode, Mode::Playing | Mode::Title | Mode::Victory) {
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
        if self.mode == Mode::Playing {
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
                        x: -65,
                        y: 15,
                        z: self.boss_z - 35,
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
                        x: 65,
                        y: 15,
                        z: self.boss_z - 35,
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
                        z: self.boss_z - 100,
                    },
                    22,
                    if self.ticks & 8 == 0 { 7 } else { 2 },
                );
            }
        }
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
            if p.kind != 0 {
                diamond(sink, p.pos, 13, if p.kind == 1 { 5 } else { 7 });
            }
        }
        for e in &self.explosions {
            if e.ticks > 0 {
                diamond(sink, e.pos, 4 + (18 - e.ticks) as i16 * 3, e.color);
            }
        }
        for (i, a) in self.actors.iter().enumerate() {
            if a.kind != 0 && a.pos.z >= 80 {
                let reserve = (ACTORS - i - 1) as u16 * 2;
                if a.pos.z > 1200
                    || sink.count + bank.face_count(a.kind) as u16 + reserve > sink.limit
                {
                    diamond(
                        sink,
                        actor_center(*a),
                        if a.kind == assets::FIGHTER { 22 } else { 18 },
                        if a.hp == 255 { 12 } else { 2 },
                    );
                } else {
                    let roll = if a.kind == assets::DIVER {
                        (a.age as u8).wrapping_mul(2)
                    } else {
                        0
                    };
                    emit_mesh(sink, bank, a.kind, a.pos, 0, 0, roll, 256);
                }
            }
        }
        sink.limit = 224;
        self.terrain(sink, segment);
    }
    // Keep these pure appearance calculations resident: the geometry overlay
    // is close to its limit, and this introduces no extra mesh instantiation.
    #[inline(never)]
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
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
    fn terrain(&self, sink: &mut impl GeometrySink, segment: &Segment<'_>) {
        let row = self.rail_ticks >> 6;
        let offset = ((self.rail_ticks & 63) * 8) as i16;
        let style = segment.terrain();
        let color = segment.ground_color();
        let alternate = if color == 5 { 13 } else { 11 };
        let palette = terrain_palette(style);
        let facets = [palette as u8, (palette >> 8) as u8];
        // Four coarse bands share world-anchored rows. The level flight lane
        // keeps scenery and collisions unchanged while the shoulders undulate.
        // Canyon caps and base guides bring their sections to at most 40 faces.
        for band in 0..4i16 {
            let world_row = row + band as u16;
            let z0 = 40 + band * 512 - offset;
            let z1 = z0 + 512;
            let shade = if world_row & 1 == 0 { color } else { alternate };
            strip(sink, -145, [-105; 2], 145, [-105; 2], z0, z1, [shade; 2]);
            for side in [-1i16, 1] {
                let heights = [
                    terrain_height(world_row, side > 0, style),
                    terrain_height(world_row + 1, side > 0, style),
                ];
                let edge = if style == 1 { 190 } else { 640 };
                strip(
                    sink,
                    side * 145,
                    [-105; 2],
                    side * edge,
                    heights,
                    z0,
                    z1,
                    facets,
                );
                if style == 1 {
                    // A raised outer cap keeps the canyon closed beyond each
                    // steep wall, including where it meets the distant skyline.
                    strip(
                        sink,
                        side * 190,
                        heights,
                        side * 640,
                        heights,
                        z0,
                        z1,
                        facets,
                    );
                } else if style == 2 {
                    strip(
                        sink,
                        side * 95,
                        [-103; 2],
                        side * 100,
                        [-103; 2],
                        z0,
                        z1,
                        [15; 2],
                    );
                }
            }
        }
        // Distant mountain silhouettes do not consume object slots.
        for i in -2..=2i16 {
            tri(
                sink,
                Vec3 {
                    x: i * 450 - 260,
                    y: -105,
                    z: 2350,
                },
                Vec3 {
                    x: i * 450 + 260,
                    y: -105,
                    z: 2350,
                },
                Vec3 {
                    x: i * 450 + 40,
                    y: 150 + (i & 1) * 100,
                    z: 2350,
                },
                if i & 1 == 0 { 6 } else { 14 },
            );
        }
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
// Keep the profile sampler resident: it needs no phase-owned data or mutable
// state, and the small geometry overlay only emits the resulting triangles.
#[inline(never)]
fn terrain_palette(style: u8) -> u16 {
    match style {
        0 => 0x0d05,
        1 => 0x0c09,
        _ => 0x0c0b,
    }
}
#[inline(never)]
fn terrain_height(row: u16, right: bool, style: u8) -> i16 {
    const PROFILE: [u8; 16] = [0, 1, 3, 2, 1, 0, 2, 4, 3, 1, 0, 1, 2, 4, 2, 1];
    let phase = row.wrapping_add(if right { 7 } else { 0 });
    let height = PROFILE[(phase & 15) as usize] as i16;
    if style == 1 {
        96 + (height << 4)
    } else if style == 0 {
        -105 + (height << 5)
    } else {
        -105 + (height << 4)
    }
}
#[inline(always)]
fn actor_center(actor: Actor) -> Vec3 {
    Vec3 {
        x: actor.pos.x,
        z: actor.pos.z,
        y: actor.pos.y
            + match actor.kind {
                assets::TURRET => 10,
                assets::ROCK => 20,
                assets::BUILDING => 30,
                _ => 0,
            },
    }
}
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
fn close_xy(a: Vec3, b: Vec3, radius: i16) -> bool {
    (a.x - b.x).abs() <= radius && (a.y - b.y).abs() <= radius
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
fn strip(
    sink: &mut impl GeometrySink,
    x0: i16,
    y0: [i16; 2],
    x1: i16,
    y1: [i16; 2],
    z0: i16,
    z1: i16,
    colors: [u8; 2],
) {
    let a = Vec3 {
        x: x0,
        y: y0[0],
        z: z0,
    };
    let c = Vec3 {
        x: x1,
        y: y1[1],
        z: z1,
    };
    tri(
        sink,
        a,
        Vec3 {
            x: x1,
            y: y1[0],
            z: z0,
        },
        c,
        colors[0],
    );
    tri(
        sink,
        a,
        c,
        Vec3 {
            x: x0,
            y: y0[1],
            z: z1,
        },
        colors[1],
    );
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

#[cfg(test)]
mod tests {
    use super::*;
    fn empty_segment(index: u8) -> [u8; assets::SEGMENT_SIZE] {
        let mut bytes = [0; assets::SEGMENT_SIZE];
        bytes[..4].copy_from_slice(b"SMS1");
        bytes[4] = index;
        bytes[6] = 5;
        bytes
    }
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
    #[derive(Debug, PartialEq, Eq)]
    struct TerrainCapture {
        faces: [([Vec3; 3], u8); 45],
        count: usize,
    }
    impl GeometrySink for TerrainCapture {
        fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
            assert_eq!(flags, TWO_SIDED);
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
            faces: [([Vec3::default(); 3], 0); 45],
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
                assert_eq!(scene.count, if style == 1 || style == 2 { 45 } else { 29 });
                let surface = &scene.faces[..scene.count - 5];
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
                let faces_per_band = (scene.count - 5) / 4;
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
                let per_band = (before.count - 5) / 4;
                let retired = if tick & 63 == 63 { per_band } else { 0 };
                for i in 0..before.count - 5 - retired {
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
    fn resident_state_fits_budget() {
        assert!(
            core::mem::size_of::<Game>() <= 640,
            "{}",
            core::mem::size_of::<Game>()
        );
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
        g.update_shots();
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
            g.tick(&segment);
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
                g.tick(&segment);
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
                g.tick(&segment);
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
        g.tick(&segment);
        assert_eq!(g.player.x, 9);
        assert_eq!(g.player.y, 3);
        assert_eq!(g.shots[0].kind, 1);
        assert_eq!(g.shots[0].pos.x, 9, "fire uses the displaced position");
        assert_eq!(g.shots[0].pos.y, 3);
        assert_eq!(g.bank, -2);

        sample(&mut g, LEFT | DOWN | FIRE, 260);
        g.tick(&segment);
        assert_eq!(g.player.x, 12, "opposite steering brakes the first burst");
        assert_eq!(g.player.y, 0);
        assert_eq!(g.bank, 0, "bank follows steering, not dodge velocity");
        for _ in 0..8 {
            g.tick(&segment);
        }
        assert_eq!(g.roll_ticks, 16);
        assert_eq!(g.player.x, 21);
        assert!(g.shots.iter().filter(|shot| shot.kind == 1).count() >= 2);
        g.tick(&segment);
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
                g.tick(&segment);
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
                g.tick(&segment);
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
        g.update_shots();
        assert_eq!(g.shield, 100);
        g.actors[0] = Actor {
            pos: Vec3 { x: 0, y: 0, z: 190 },
            kind: assets::ROCK,
            hp: 255,
            ..Actor::EMPTY
        };
        g.update_actors();
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
            g.tick(&segment);
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
            pos: Vec3 { x: 0, y: 0, z: 650 },
            kind: 1,
            damage: 4,
            ..Shot::EMPTY
        };
        g.update_shots();
        assert_eq!(g.boss_core, 60);
        g.boss_left = 0;
        g.boss_right = 0;
        g.shots[0] = Shot {
            pos: Vec3 { x: 0, y: 0, z: 650 },
            kind: 1,
            damage: 4,
            ..Shot::EMPTY
        };
        g.update_shots();
        assert_eq!(g.boss_core, 56);
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
        g.bomb();
        assert_eq!(g.bombs, 2);
        assert_eq!(g.shots[0].kind, 0);
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
                    g.tick(&segment);
                }
            }
        }
        assert_eq!(g.mode, Mode::GameOver);
        g.controls.bits = FIRE;
        sample(&mut g, FIRE, 5000);
        g.tick(&segment);
        assert_eq!(g.mode, Mode::GameOver);
        sample(&mut g, 0, 5020);
        sample(&mut g, FIRE, 5040);
        g.tick(&segment);
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
        g.update_shots();
        assert!(g.shots[..3]
            .iter()
            .all(|shot| shot.pos.z == PLAYER_Z + 18 + 48));
        assert_eq!(g.shots[3].pos.z, PLAYER_Z + 18 + 32);
    }
    fn enemy(kind: u8, x: i16, y: i16, z: i16) -> Actor {
        Actor {
            pos: Vec3 { x, y, z },
            origin_x: x,
            origin_y: y,
            kind,
            hp: if kind == assets::TURRET { 6 } else { 3 },
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
        assert_eq!(g.missile_target(origin), Some((g.actors[4].pos, 44)));
        g.actors[5].pos.x = 39;
        assert_eq!(g.missile_target(origin), Some((g.actors[5].pos, 44)));
        g.actors[5] = enemy(assets::TURRET, 0, -10, 600);
        assert_eq!(
            g.missile_target(origin),
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
        g.steer_missile(&mut s);
        assert_eq!(s.dx, 1);
        g.actors[0].kind = 0;
        g.steer_missile(&mut s);
        assert_eq!(
            s.dx, 0,
            "lost target reduces steering without stale references"
        );
        g.actors[0] = enemy(assets::FIGHTER, -100, 0, 700);
        g.steer_missile(&mut s);
        assert_eq!(s.dx, -1, "a reused slot is considered at its new position");
        g.actors[0].pos.z = 199;
        g.steer_missile(&mut s);
        assert_eq!(
            s.dx, 0,
            "the seeker cannot turn back toward a passed target"
        );
        g.shots[0] = s;
        g.update_shots();
        assert_eq!(g.shots[0].pos, Vec3 { x: 0, y: 0, z: 232 });
        for _ in 0..70 {
            g.update_shots();
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
                    g.update_actors();
                    g.update_shots();
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
            g.missile_target(origin),
            Some((
                Vec3 {
                    x: -65,
                    y: 15,
                    z: 900
                },
                40
            ))
        );
        g.boss_left = 0;
        g.boss_z = 680;
        assert_eq!(
            g.missile_target(origin),
            Some((
                Vec3 {
                    x: 65,
                    y: 15,
                    z: 680
                },
                32
            ))
        );
        g.boss_right = 0;
        assert_eq!(
            g.missile_target(origin),
            Some((Vec3 { x: 0, y: 0, z: 680 }, 32))
        );
        g.fire_player(255);
        for _ in 0..20 {
            g.update_shots();
        }
        assert_eq!(g.boss_core, 56);
        g.boss_defeated = true;
        assert_eq!(g.missile_target(origin), None);
    }
    #[test]
    fn missiles_obey_obstacle_collision_and_boss_core_protection() {
        let mut g = running();
        g.actors[0] = enemy(assets::ROCK, 0, -20, 230);
        g.actors[0].hp = 255;
        g.fire_player(255);
        g.update_shots();
        assert_eq!(g.shots[0].kind, 0);
        assert_eq!(g.actors[0].hp, 255);
        assert_eq!(g.score, 0);
        g.actors[0] = Actor::EMPTY;
        g.boss_started = true;
        g.boss_z = 680;
        g.shots[0] = Shot {
            pos: Vec3 { x: 0, y: 0, z: 650 },
            kind: 3,
            damage: 4,
            ..Shot::EMPTY
        };
        g.update_shots();
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
        g.bomb();
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
            g.tick(&segment);
        }
        g.damage_player(12);
        assert_eq!(
            (g.shield, g.hit_flash_ticks),
            (88, 25),
            "ignored hits do not restart the flash"
        );
        for _ in 0..25 {
            g.tick(&segment);
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
    fn five_minute_mission_is_winnable_with_regular_joystick_input() {
        let banks = [
            mission_segment(0),
            mission_segment(1),
            mission_segment(2),
            mission_segment(3),
        ];
        let mut g = running();
        let mut visited = 0u8;
        for tick in 1..=18_000u32 {
            let section = g.segment_index();
            visited |= 1 << section;
            let segment = Segment::new(&banks[section as usize]).unwrap();
            let mut target = Vec3 { x: 0, y: 0, z: 800 };
            if g.boss_started && !g.boss_defeated {
                target = if g.boss_left > 0 {
                    Vec3 {
                        x: -65,
                        y: 25,
                        z: 680,
                    }
                } else if g.boss_right > 0 {
                    Vec3 {
                        x: 65,
                        y: 25,
                        z: 680,
                    }
                } else {
                    Vec3 { x: 0, y: 0, z: 680 }
                };
            } else {
                let mut nearest = 2400;
                for actor in &g.actors {
                    if actor.kind != 0
                        && actor.hp != 255
                        && actor.pos.z > 300
                        && actor.pos.z < nearest
                    {
                        target = actor_center(*actor);
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
            g.tick(&segment);
            while g.pop_audio().is_some() {}
            assert_ne!(
                g.mode,
                Mode::GameOver,
                "pilot lost all lives at distance {}",
                g.hud().distance
            );
            if g.mode == Mode::Victory {
                assert_eq!(visited, 15);
                assert!(tick >= 15_000);
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
    fn model_bank_for_tests() -> [u8; assets::COMMON_SIZE] {
        let mut data = [0u8; assets::COMMON_SIZE];
        data[..4].copy_from_slice(b"SMA1");
        data[4] = 8;
        let mut cursor = 80usize;
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
        let data = model_bank_for_tests();
        let bank = AssetBank::new(&data).unwrap();
        let mut scene = empty_segment(1);
        scene[5] = 1;
        let segment = Segment::new(&scene).unwrap();
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
        for phase in 0..64 {
            g.rail_ticks = 3712 + phase;
            renderer.begin(0);
            g.render(&mut renderer, &bank, &segment);
            renderer.finish();
            assert_eq!(renderer.dropped, 0, "terrain phase {phase}");
            assert!(renderer.triangle_count > 100);
            assert!(renderer.triangle_count <= 256);
        }
    }
}
