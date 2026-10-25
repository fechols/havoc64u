//! Boss instances and the Gate Guardian's synchronized splitting waves.
use super::*;

const BOSS_SPLIT_TICKS: u8 = 50;

/// This ten-byte layout is also read by the native missile guidance kernel.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct BossInstance {
    pub(super) pos: Vec3,
    pub(super) left: u8,
    pub(super) right: u8,
    pub(super) core: u8,
    pub(super) flash: u8,
}

impl BossInstance {
    pub(super) const EMPTY: Self = Self {
        pos: Vec3 { x: 0, y: 0, z: 0 },
        left: 0, right: 0, core: 0, flash: 0,
    };
    pub(super) const ORIGINAL: Self = Self {
        pos: Vec3 { x: 0, y: 0, z: 1800 },
        left: BOSS_SIDE_HP, right: BOSS_SIDE_HP, core: BOSS_CORE_HP, flash: 0,
    };

    #[inline(always)]
    pub(super) fn exposed(&self) -> bool { self.left == 0 && self.right == 0 }

    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim_extra")]
    pub(super) fn part_position(&self, part: u8, divisor: i16) -> Vec3 {
        let shift = if divisor == 1 { 0 } else if divisor == 4 { 2 } else { 3 };
        let side = part < 2;
        let x = (BOSS_TURRET_X as u8 >> shift) as i16;
        let y = if side { (BOSS_TURRET_Y as u8 >> shift) as i16 } else { 0 };
        let z = (if side { (-BOSS_TURRET_Z) as u8 } else { (-BOSS_CORE_Z) as u8 } >> shift) as i16;
        Vec3 { x: self.pos.x + if part == 0 { -x } else if part == 1 { x } else { 0 },
               y: self.pos.y + y, z: self.pos.z - z }
    }

}

pub(super) const INITIAL_BOSSES: [BossInstance; 4] = [
    BossInstance::ORIGINAL, BossInstance::EMPTY, BossInstance::EMPTY, BossInstance::EMPTY,
];

impl Game {
    #[inline(always)]
    pub(super) fn boss_count(&self) -> usize { 1 << self.boss_phase }

    #[inline(always)]
    pub(super) fn boss_scale_shift(&self) -> u8 {
        if self.boss_phase == 0 { 0 } else { self.boss_phase + 1 }
    }

    #[inline(always)]
    pub(super) fn boss_divisor(&self) -> i16 { 1 << self.boss_scale_shift() }

    #[inline(never)]
    pub(super) fn reset_bosses(&mut self) {
        self.bosses = INITIAL_BOSSES;
        self.boss_phase = 0;
        self.boss_split_ticks = 0;
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
    pub(super) fn boss_health(&self) -> u8 {
        if !self.boss_started || self.boss_defeated { return 0; }
        let mut hp = 0u16;
        for boss in &self.bosses[..self.boss_count()] {
            if boss.core > 0 {
                hp += u16::from(boss.left) + u16::from(boss.right) + u16::from(boss.core);
            }
        }
        ((hp + 1) >> 1).min(100) as u8
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim_extra")]
    pub(super) fn split_destination(phase: u8, slot: usize) -> Vec3 {
        Vec3 {
            x: if slot & 1 == 0 { -1 } else { 1 } * if phase == 1 { 74 } else { 55 },
            y: if phase == 1 { 0 } else if slot < 2 { -40 } else { 40 },
            z: 500,
        }
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim_extra")]
    pub(super) fn advance_boss_split(&mut self) {
        let remaining = i16::from(self.boss_split_ticks);
        for i in 0..self.boss_count() {
            let target = Self::split_destination(self.boss_phase, i);
            let pos = &mut self.bosses[i].pos;
            pos.x += (target.x - pos.x) / remaining;
            pos.y += (target.y - pos.y) / remaining;
            pos.z += (target.z - pos.z) / remaining;
        }
        self.boss_split_ticks -= 1;
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim_extra")]
    pub(super) fn begin_boss_split(&mut self) {
        let parents = [self.bosses[0].pos, self.bosses[1].pos];
        self.boss_phase += 1;
        self.boss_split_ticks = BOSS_SPLIT_TICKS;
        self.shield = 100;
        self.bombs = 3;
        self.shots = [Shot::EMPTY; SHOTS];
        self.missile_targets = [0; SHOTS];
        self.actors = [Actor::EMPTY; ACTORS];
        self.actor_hit_flash_ticks = [0; ACTORS];
        self.explosions = [Explosion::EMPTY; 6];
        for i in 0..self.boss_count() {
            let pos = parents[if self.boss_phase == 1 { 0 } else { i & 1 }];
            self.bosses[i] = BossInstance {
                pos, left: BOSS_SIDE_HP >> self.boss_phase,
                right: BOSS_SIDE_HP >> self.boss_phase,
                core: BOSS_CORE_HP >> self.boss_phase, flash: 0,
            };
        }
        self.sound(Effect::BossExplosion, 0);
    }

    /// Consume at most one boss part per projectile, with protected cores
    /// absorbing shots. Wave changes are deferred until the whole damage pass.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    pub(super) fn hit_boss(&mut self, old_z: i16, shot: &Shot) -> Option<Effect> {
        if !self.boss_started || self.boss_defeated || self.boss_split_ticks != 0 { return None; }
        let shift = self.boss_scale_shift();
        let divisor = self.boss_divisor();
        let depth = (BOSS_HIT_DEPTH as u8 >> shift) as i16;
        for i in 0..self.boss_count() {
            let boss = &mut self.bosses[i];
            if boss.core == 0 { continue; }
            // Side bounds are disjoint: use position relative to this boss.
            let side = u8::from(shot.pos.x >= boss.pos.x);
            for part in [side, 2] {
                if (part == 0 && boss.left == 0) || (part == 1 && boss.right == 0) { continue; }
                let target = boss.part_position(part, divisor);
                let radius = ((if part == 2 { BOSS_CORE_RADIUS } else { BOSS_TURRET_RADIUS }) as u8 >> shift) as i16;
                if !swept_z(old_z, shot.pos.z, target.z, depth)
                    || !close_xy(shot.pos, target, radius) { continue; }
                let open = boss.exposed();
                let hp = match part { 0 => &mut boss.left, 1 => &mut boss.right, _ => &mut boss.core };
                if part != 2 || open { damage_boss_part(hp, shot.damage, &mut boss.flash); }
                return Some(if *hp == 0 {
                    if part == 2 { Effect::BossExplosion } else { Effect::EnemyExplosion }
                } else if shot.kind == 3 { Effect::MissileImpact } else { Effect::BossHit });
            }
        }
        None
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim_extra")]
    pub(super) fn check_boss_death(&mut self) {
        if self.boss_defeated || self.boss_split_ticks != 0
            || self.bosses[..self.boss_count()].iter().any(|boss| boss.core != 0) { return; }
        if self.stage_info.boss_kind == 5 && self.boss_phase < 2 {
            self.begin_boss_split();
            return;
        }
        self.boss_defeated = true;
        self.shield = 100;
        self.bombs = 3;
        self.score += 5000;
        self.sound(Effect::BossExplosion, 0);
        for x in [-80, -40, 0, 40, 80] {
            self.explode(Vec3 { x, y: 0, z: self.bosses[0].pos.z }, 7);
        }
        self.clear_hostile_shots();
    }
}
