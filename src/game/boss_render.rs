use super::*;

impl Game {
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.geometry.combat")]
    pub(super) fn render_bosses(
        &self,
        sink: &mut SceneBudget<'_, impl GeometrySink>,
        bank: &AssetBank<'_>,
    ) {
        if !self.boss_started || self.boss_defeated {
            return;
        }
        let previous_flags = sink.extra_flags;
        // Preserve the Dreadnought's protection from long flight-deck faces.
        if self.stage_info.boss_kind == 4 {
            sink.extra_flags |= TERRAIN_CLEAR_GEOMETRY;
        }
        let shift = if self.boss_phase == 0 {
            0
        } else {
            self.boss_phase + 1
        };
        let turret_x = (BOSS_TURRET_X as u8 >> shift) as i16;
        let turret_y = (BOSS_TURRET_Y as u8 >> shift) as i16;
        let turret_z = (-BOSS_TURRET_Z as u8 >> shift) as i16;
        let core_z = (-BOSS_CORE_Z as u8 >> shift) as i16;
        for boss in &self.bosses[..self.boss_count()] {
            if boss.core == 0 {
                continue;
            }
            let checkpoint = sink.begin_group();
            sink.color_override = u8::from(boss.flash > ENEMY_HIT_FLASH_TICKS);
            // A common emission site keeps the native overlay small: mesh zero
            // denotes a flat marker here, never the player mesh.
            for part in 0..4 {
                let mut position = boss.pos;
                let mut mesh = 0;
                let mut radius = 40u8 >> shift;
                let mut color = 3;
                if part == 0 {
                    mesh = if self.boss_phase == 0 {
                        assets::CARRIER
                    } else {
                        assets::CRYSTAL
                    };
                } else if part < 3 {
                    if (if part == 1 { boss.left } else { boss.right }) == 0 {
                        continue;
                    }
                    position.x += if part == 1 { -turret_x } else { turret_x };
                    position.y += turret_y;
                    position.z -= turret_z;
                    if self.boss_phase == 0 {
                        mesh = assets::TURRET;
                    }
                } else {
                    if boss.left != 0 || boss.right != 0 {
                        continue;
                    }
                    position.z -= core_z;
                    radius = (22 * assets::BOSS_SCALE) as u8 >> shift;
                    color = if self.ticks & 8 == 0 { 7 } else { 2 };
                }
                if mesh == 0 {
                    diamond(sink, position, i16::from(radius), color);
                } else {
                    emit_mesh(sink, bank, mesh, position, 0, 0, 0, self.boss_phase == 2);
                }
            }
            sink.color_override = 0;
            sink.finish_group(checkpoint);
        }
        sink.extra_flags = previous_flags;
    }
}
