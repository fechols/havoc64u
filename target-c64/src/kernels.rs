//! One `extern "C"` entry per cue for the per-song music kernels.
//!
//! Each shim inlines its kernel from `havoc::audio::kernels` into a section
//! the linker places at the resident slot (`music_kernel_tick`) and stores at
//! that cue's REU offset behind the `SMK1` directory. Entries take no
//! arguments and modulate the fixed resident engine, so the slot ABI is a bare
//! `JSR`. The IRQ never calls the slot until the foreground has copied and
//! verified the playing cue's image.

use crate::{HardwareSid, AUDIO};
use havoc::audio::kernels;

macro_rules! shim {
    ($entry:ident, $kernel:ident, $section:literal) => {
        #[no_mangle]
        #[link_section = $section]
        pub unsafe extern "C" fn $entry() {
            kernels::$kernel(AUDIO.assume_init_mut(), &mut HardwareSid);
        }
    };
}

shim!(music_kernel_title, title, ".music.kernel.title.entry");
shim!(music_kernel_lowlands, lowlands, ".music.kernel.lowlands.entry");
shim!(music_kernel_canyon, canyon, ".music.kernel.canyon.entry");
shim!(music_kernel_fortress, fortress, ".music.kernel.fortress.entry");
shim!(music_kernel_carrier, carrier, ".music.kernel.carrier.entry");
shim!(music_kernel_victory, victory, ".music.kernel.victory.entry");
shim!(music_kernel_crystal_current, crystal_current, ".music.kernel.crystal_current.entry");
shim!(music_kernel_ashes_and_steel, ashes_and_steel, ".music.kernel.ashes_and_steel.entry");
shim!(music_kernel_tempest_drive, tempest_drive, ".music.kernel.tempest_drive.entry");
shim!(music_kernel_gorge_warden, gorge_warden, ".music.kernel.gorge_warden.entry");
shim!(music_kernel_citadel_heart, citadel_heart, ".music.kernel.citadel_heart.entry");
shim!(music_kernel_carrier_overdrive, carrier_overdrive, ".music.kernel.carrier_overdrive.entry");
shim!(music_kernel_asteroid_belt, asteroid_belt, ".music.kernel.asteroid_belt.entry");
shim!(music_kernel_shattered_moon, shattered_moon, ".music.kernel.shattered_moon.entry");
shim!(music_kernel_ship_graveyard, ship_graveyard, ".music.kernel.ship_graveyard.entry");
shim!(music_kernel_orbital_shipyard, orbital_shipyard, ".music.kernel.orbital_shipyard.entry");
shim!(music_kernel_space_carrier, space_carrier, ".music.kernel.space_carrier.entry");
shim!(music_kernel_jump_gate, jump_gate, ".music.kernel.jump_gate.entry");
shim!(music_kernel_ascent, ascent, ".music.kernel.ascent.entry");
shim!(music_kernel_gate_guardian, gate_guardian, ".music.kernel.gate_guardian.entry");
