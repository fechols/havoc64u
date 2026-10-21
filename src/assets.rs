//! Bounded, little-endian asset views shared by the host baker and 6510 runtime.
use crate::Vec3;
pub const COMMON_SIZE: usize = 3072;
/// Models remain resident after the score has been copied into the audio engine.
pub const MODEL_SIZE: usize = 2048;
pub const SEGMENT_SIZE: usize = 2048;
/// Only this prefix of a packaged segment is needed during gameplay.
pub const SEGMENT_CACHE_SIZE: usize = 512;
pub const MAX_SEGMENT_SPAWNS: u8 = 62;
pub const SEGMENT_COUNT: u8 = 18;
pub const SEGMENT_DISTANCE: u32 = 30_000;
pub const REU_SIZE: u32 = 0x100000;
pub const COMMON_OFFSET: u32 = 0x1000;
pub const SEGMENTS_OFFSET: u32 = 0x2000;
pub const MUSIC_OFFSET: u32 = 0x0c0000;
pub const MUSIC_CAPACITY: u32 = crate::music::BANK_CAPACITY;
/// Per-song music kernel images (`SMK1`) sit between the model banks and the
/// score bank; the packer writes them from the linked ELF with the code images.
pub const MUSIC_KERNELS_OFFSET: u32 = 0x0b9000;
pub const PACKAGE_HEADER_SIZE: usize = 64;
pub const PACKAGE_CHECKSUM_OFFSET: usize = PACKAGE_HEADER_SIZE - 2;
pub const SEGMENT_CHECKSUM_OFFSET: usize = 20;
pub const MODEL_DIRECTORY_OFFSET: u32 = 0x40;
pub const MODEL_DIRECTORY_SIZE: usize = 64;
pub const MODEL_DIRECTORY_CHECKSUM_OFFSET: usize = MODEL_DIRECTORY_SIZE - 2;
pub const MODEL_BANKS_OFFSET: u32 = 0x0b0000;
pub const MODEL_COUNT: u8 = 9;
pub const ENEMY_SPECS_OFFSET: usize = 16 + MODEL_COUNT as usize * 8;
pub const ENEMY_SPEC_SIZE: usize = 8;
pub const MODEL_DATA_OFFSET: usize = ENEMY_SPECS_OFFSET + 3 * ENEMY_SPEC_SIZE;
// Version 8 packs code and music into a 1 MiB REU without changing bank formats.
// Version 7 stores mesh vertices as i8 with a per-mesh shift (SMA4 banks);
// version 6 extended the checked campaign through orbital combat and the gate.
pub const PACKAGE_ID: u32 = 0x534D0800;
pub const PLAYER: u8 = 0;
pub const FIGHTER: u8 = 1;
pub const DIVER: u8 = 2;
pub const TURRET: u8 = 3;
pub const CARRIER: u8 = 4;
pub const CRYSTAL: u8 = 5;
pub const BUILDING: u8 = 6;
pub const ROCK: u8 = 7;
pub const MEDAL: u8 = 8;
/// A ROCK encounter is a small shootable fragment instead of a solid obstacle.
pub const SMALL_DEBRIS: u8 = 2;
/// Per-mesh descriptor flag (byte 7): cull back faces instead of drawing both sides.
pub const MESH_ONE_SIDED: u8 = 1;
/// Largest per-mesh coordinate shift: `i8` vertices in units of up to eight.
pub const MAX_MESH_SHIFT: u8 = 3;
/// Bytes per stored vertex: `i8` x, y, z in units of `1 << shift`.
pub const VERTEX_BYTES: usize = 3;
/// Boss hulls and their side units keep the original uniform enlargement,
/// baked into meshes, collision descriptors and the boss layout constants.
pub const BOSS_SCALE: i16 = 2;
/// Regular enemies are baked at three halves of their authored size: a quarter
/// smaller than bosses. Truncation toward zero keeps mirrored meshes symmetric.
pub const fn enemy_scaled(authored: i16) -> i16 {
    authored * 3 / 2
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetError {
    Size,
    Magic,
    Version,
    Bounds,
    Index,
    Checksum,
}
pub fn u16_at(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([data[at], data[at + 1]])
}
pub fn u32_at(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}
#[inline(never)]
pub fn checksum(data: &[u8]) -> u16 {
    let mut a = 0u16;
    let mut b = 0u16;
    // Each sum is at most 509, so one subtraction is exactly modulo 255.
    // Avoid dragging a general divide/remainder helper into the resident loader.
    for &v in data {
        a += v as u16;
        if a >= 255 {
            a -= 255;
        }
        b += a;
        if b >= 255 {
            b -= 255;
        }
    }
    (b << 8) | a
}
pub fn validate_header(data: &[u8]) -> Result<(), AssetError> {
    if data.len() < PACKAGE_HEADER_SIZE {
        return Err(AssetError::Size);
    }
    if &data[..4] != b"SM64" {
        return Err(AssetError::Magic);
    }
    if u32_at(data, 4) != PACKAGE_ID {
        return Err(AssetError::Version);
    }
    if u32_at(data, 8) != COMMON_OFFSET
        || u32_at(data, 12) != SEGMENTS_OFFSET
        || data[16] != SEGMENT_COUNT
        || data[17] != crate::music::SID_COUNT as u8
        || data[SEGMENT_CHECKSUM_OFFSET + SEGMENT_COUNT as usize * 2..PACKAGE_CHECKSUM_OFFSET]
            .iter()
            .any(|&byte| byte != 0)
    {
        return Err(AssetError::Bounds);
    }
    if checksum(&data[..PACKAGE_CHECKSUM_OFFSET]) != u16_at(data, PACKAGE_CHECKSUM_OFFSET) {
        return Err(AssetError::Checksum);
    }
    Ok(())
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.boot")]
pub fn validate_model_directory(data: &[u8]) -> Result<(), AssetError> {
    if data.len() != MODEL_DIRECTORY_SIZE {
        return Err(AssetError::Size);
    }
    if &data[..4] != b"MDL2" {
        return Err(AssetError::Magic);
    }
    if data[5] != 2 {
        return Err(AssetError::Version);
    }
    if data[4] != SEGMENT_COUNT
        || u16_at(data, 6) as usize != MODEL_SIZE
        || u32_at(data, 8) != MODEL_BANKS_OFFSET
        || data[12 + SEGMENT_COUNT as usize * 2..MODEL_DIRECTORY_CHECKSUM_OFFSET]
            .iter()
            .any(|&byte| byte != 0)
    {
        return Err(AssetError::Bounds);
    }
    if checksum(&data[..MODEL_DIRECTORY_CHECKSUM_OFFSET])
        != u16_at(data, MODEL_DIRECTORY_CHECKSUM_OFFSET)
    {
        return Err(AssetError::Checksum);
    }
    Ok(())
}

pub fn model_checksum(directory: &[u8], level: u8) -> u16 {
    if directory.len() == MODEL_DIRECTORY_SIZE && level < SEGMENT_COUNT {
        u16_at(directory, 12 + level as usize * 2)
    } else {
        0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EnemySpec {
    pub hp: u8,
    pub center_y: i8,
    pub shot_radius: u8,
    pub ram_radius: u8,
    pub score: u16,
    pub speed: u8,
    pub render_mode: u8,
}

pub struct AssetBank<'a> {
    data: &'a [u8],
}
impl<'a> AssetBank<'a> {
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.boot")]
    pub fn new(data: &'a [u8]) -> Result<Self, AssetError> {
        if data.len() != COMMON_SIZE && data.len() != MODEL_SIZE {
            return Err(AssetError::Size);
        }
        if &data[..4] != b"SMA4" {
            return Err(AssetError::Magic);
        }
        if data[4] != MODEL_COUNT {
            return Err(AssetError::Version);
        }
        if data[5] >= SEGMENT_COUNT || data[7] > 5 || data[6] != if data[7] != 0 { 2 } else { 3 } {
            return Err(AssetError::Bounds);
        }
        let bank = Self { data };
        for kind in 1..=bank.enemy_count() {
            let spec = bank.enemy(kind);
            if spec.hp == 0
                || spec.hp == 255
                || spec.shot_radius == 0
                || spec.ram_radius == 0
                || spec.score == 0
                || spec.speed == 0
                || spec.speed > 31
                || spec.render_mode > 1
            {
                return Err(AssetError::Bounds);
            }
        }
        for id in 0..MODEL_COUNT {
            let d = 16 + id as usize * 8;
            let vo = u16_at(data, d) as usize;
            let fo = u16_at(data, d + 4) as usize;
            let vc = data[d + 2] as usize;
            let fc = data[d + 3] as usize;
            if vc == 0
                || vc > 128
                || vo < MODEL_DATA_OFFSET
                || vo > MODEL_SIZE
                || fo > MODEL_SIZE
                || fo < vo + vc * VERTEX_BYTES
                || fo + fc * 4 > MODEL_SIZE
                || data[d + 6] > MAX_MESH_SHIFT
                || data[d + 7] > MESH_ONE_SIDED
            {
                return Err(AssetError::Bounds);
            }
            for f in 0..fc {
                if data[fo + f * 4..fo + f * 4 + 3]
                    .iter()
                    .any(|&i| i as usize >= vc)
                    || data[fo + f * 4 + 3] > 15
                {
                    return Err(AssetError::Index);
                }
            }
        }
        Ok(bank)
    }
    /// Reconstruct a borrowed view without walking the mesh directory again.
    ///
    /// # Safety
    /// `data` must contain the unchanged bytes of a bank accepted by `new`, or
    /// the complete `MODEL_SIZE` prefix of such a full bank. The backing memory
    /// must remain unchanged and valid for the returned view's entire lifetime;
    /// in particular, a REU DMA must not overwrite it while this view is in use.
    #[inline]
    pub unsafe fn from_validated(data: &'a [u8]) -> Self {
        Self { data }
    }
    pub fn mesh_count(&self) -> u8 {
        MODEL_COUNT
    }
    #[inline(always)]
    pub fn level(&self) -> u8 {
        // SAFETY: both constructors require a complete, validated bank.
        unsafe { *self.data.get_unchecked(5) }
    }
    #[inline(always)]
    pub fn enemy_count(&self) -> u8 {
        // SAFETY: both constructors require a complete, validated bank.
        unsafe { *self.data.get_unchecked(6) }
    }
    #[inline(always)]
    pub fn enemy(&self, kind: u8) -> EnemySpec {
        if kind == 0 || kind > self.enemy_count() {
            return EnemySpec::default();
        }
        let at = ENEMY_SPECS_OFFSET + (kind - 1) as usize * ENEMY_SPEC_SIZE;
        // SAFETY: validated banks contain three complete descriptors at 88..112;
        // the guard above restricts kind to the bank's two or three valid slots.
        // Read explicitly to preserve the package's little-endian layout.
        unsafe {
            EnemySpec {
                hp: *self.data.get_unchecked(at),
                center_y: *self.data.get_unchecked(at + 1) as i8,
                shot_radius: *self.data.get_unchecked(at + 2),
                ram_radius: *self.data.get_unchecked(at + 3),
                score: u16::from_le_bytes([
                    *self.data.get_unchecked(at + 4),
                    *self.data.get_unchecked(at + 5),
                ]),
                speed: *self.data.get_unchecked(at + 6),
                render_mode: *self.data.get_unchecked(at + 7),
            }
        }
    }
    #[inline(always)]
    pub fn vertex_count(&self, id: u8) -> u8 {
        if id < MODEL_COUNT {
            // SAFETY: validated banks contain all MODEL_COUNT descriptors.
            unsafe { *self.data.get_unchecked(18 + id as usize * 8) }
        } else {
            0
        }
    }
    #[inline(always)]
    pub fn face_count(&self, id: u8) -> u8 {
        if id < MODEL_COUNT {
            // SAFETY: validated banks contain all MODEL_COUNT descriptors.
            unsafe { *self.data.get_unchecked(19 + id as usize * 8) }
        } else {
            0
        }
    }
    /// Stored vertices are `i8` in units of `1 << shift` (0..=MAX_MESH_SHIFT).
    #[inline(always)]
    pub fn shift(&self, id: u8) -> u8 {
        if id < MODEL_COUNT {
            // SAFETY: validated banks contain all MODEL_COUNT descriptors.
            unsafe { *self.data.get_unchecked(22 + id as usize * 8) }
        } else {
            0
        }
    }
    /// Whether the mesh culls its back faces (`MESH_ONE_SIDED`).
    #[inline(always)]
    pub fn one_sided(&self, id: u8) -> bool {
        if id < MODEL_COUNT {
            // SAFETY: validated banks contain all MODEL_COUNT descriptors.
            unsafe { *self.data.get_unchecked(23 + id as usize * 8) & MESH_ONE_SIDED != 0 }
        } else {
            false
        }
    }
    /// The stored `i8` x, y, z bytes of one vertex, before the mesh shift.
    #[inline(always)]
    pub fn vertex_bytes(&self, id: u8, index: u8) -> [i8; 3] {
        if index >= self.vertex_count(id) {
            return [0; 3];
        }
        // SAFETY: the count guard proves a valid mesh and vertex; validation
        // proves that its directory offset and complete three-byte record fit.
        unsafe {
            let d = 16 + id as usize * 8;
            let p =
                u16::from_le_bytes([*self.data.get_unchecked(d), *self.data.get_unchecked(d + 1)])
                    as usize
                    + index as usize * VERTEX_BYTES;
            [
                *self.data.get_unchecked(p) as i8,
                *self.data.get_unchecked(p + 1) as i8,
                *self.data.get_unchecked(p + 2) as i8,
            ]
        }
    }
    /// Address of the first enemy descriptor (kernel input: `ENEMY_SPEC_SIZE`
    /// bytes per kind, kinds `1..=enemy_count()`).
    #[inline(always)]
    pub fn enemy_specs(&self) -> *const u8 {
        // SAFETY: both constructors require a bank holding every descriptor.
        unsafe { self.data.as_ptr().add(ENEMY_SPECS_OFFSET) }
    }
    /// Address of the first stored vertex byte of a mesh (kernel input).
    #[inline(always)]
    pub fn vertex_data(&self, id: u8) -> *const u8 {
        // SAFETY: validation proves the directory offset lies inside the bank.
        unsafe {
            let d = 16 + (id.min(MODEL_COUNT - 1)) as usize * 8;
            let p =
                u16::from_le_bytes([*self.data.get_unchecked(d), *self.data.get_unchecked(d + 1)])
                    as usize;
            self.data.as_ptr().add(p)
        }
    }
    /// Address of the first face record (three indices, colour) of a mesh.
    #[inline(always)]
    pub fn face_data(&self, id: u8) -> *const u8 {
        // SAFETY: validation proves the directory offset lies inside the bank.
        unsafe {
            let d = 20 + (id.min(MODEL_COUNT - 1)) as usize * 8;
            let p =
                u16::from_le_bytes([*self.data.get_unchecked(d), *self.data.get_unchecked(d + 1)])
                    as usize;
            self.data.as_ptr().add(p)
        }
    }
    /// One vertex in model units: the stored bytes scaled by the mesh shift.
    #[inline(always)]
    pub fn vertex(&self, id: u8, index: u8) -> Vec3 {
        let [x, y, z] = self.vertex_bytes(id, index);
        let shift = self.shift(id);
        Vec3 {
            x: (x as i16) << shift,
            y: (y as i16) << shift,
            z: (z as i16) << shift,
        }
    }
    #[inline(always)]
    pub fn face(&self, id: u8, index: u8) -> ([u8; 3], u8) {
        if index >= self.face_count(id) {
            return ([0; 3], 0);
        }
        // SAFETY: the count guard proves a valid mesh and face; validation
        // proves that its directory offset and complete four-byte record fit.
        unsafe {
            let d = 20 + id as usize * 8;
            let p =
                u16::from_le_bytes([*self.data.get_unchecked(d), *self.data.get_unchecked(d + 1)])
                    as usize
                    + index as usize * 4;
            (
                [
                    *self.data.get_unchecked(p),
                    *self.data.get_unchecked(p + 1),
                    *self.data.get_unchecked(p + 2),
                ],
                *self.data.get_unchecked(p + 3),
            )
        }
    }
    /// A model-only cache has no score; return an empty slice instead of panicking.
    pub fn audio(&self) -> &[u8] {
        if self.data.len() == COMMON_SIZE {
            &self.data[MODEL_SIZE..MODEL_SIZE + 512]
        } else {
            &[]
        }
    }
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Spawn {
    pub distance: u16,
    pub kind: u8,
    pub x: i16,
    pub y: i16,
    pub flags: u8,
}
/// Active stage identity copied from the checked encounter header. The full
/// campaign authoring table belongs to the host, never the resident C64 image.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct StageInfo {
    pub enemy_profile: u8,
    pub music_cue: u8,
    pub bridge_profile: u8,
    pub boss_kind: u8,
    pub theme: u8,
    pub next_theme: u8,
    pub previous_theme: u8,
    pub reserved: u8,
}
impl StageInfo {
    pub const FIRST: Self = Self {
        enemy_profile: 0,
        music_cue: 1,
        bridge_profile: 0,
        boss_kind: 0,
        theme: 0,
        next_theme: 4,
        previous_theme: 0,
        reserved: 0,
    };
    #[inline(always)]
    pub fn simulation_profile(self) -> u8 {
        match self.boss_kind {
            0 => self.enemy_profile,
            4 => 6,
            5 => 5,
            kind => 3 + kind,
        }
    }
}
/// Host authoring/preview metadata, absent from native production binaries.
#[cfg(any(test, feature = "host-tools"))]
pub const STAGE_INFOS: [StageInfo; SEGMENT_COUNT as usize] = {
    const fn info(
        enemy_profile: u8,
        music_cue: u8,
        bridge_profile: u8,
        boss_kind: u8,
        theme: u8,
        next_theme: u8,
        previous_theme: u8,
    ) -> StageInfo {
        StageInfo {
            enemy_profile,
            music_cue,
            bridge_profile,
            boss_kind,
            theme,
            next_theme,
            previous_theme,
            reserved: 0,
        }
    }
    [
        StageInfo::FIRST,
        info(0, 7, 0, 0, 4, 1, 0),
        info(1, 2, 1, 0, 1, 1, 4),
        info(1, 2, 1, 1, 1, 5, 1),
        info(0, 8, 2, 0, 5, 6, 1),
        info(2, 8, 1, 0, 6, 2, 5),
        info(2, 3, 1, 0, 2, 7, 6),
        info(2, 3, 2, 2, 7, 8, 2),
        info(3, 9, 2, 0, 8, 9, 7),
        info(3, 9, 2, 0, 9, 3, 8),
        info(3, 4, 2, 0, 3, 3, 9),
        info(3, 4, 3, 3, 3, 10, 3),
        info(0, 13, 4, 0, 10, 11, 3),
        info(1, 14, 4, 0, 11, 12, 10),
        info(3, 15, 4, 0, 12, 13, 11),
        info(2, 16, 4, 0, 13, 14, 12),
        info(3, 17, 4, 4, 14, 15, 13),
        info(2, 18, 4, 5, 15, 15, 14),
    ]
};
#[cfg(any(test, feature = "host-tools"))]
pub fn preview_stage_info(stage: u8) -> StageInfo {
    STAGE_INFOS[stage.min(SEGMENT_COUNT - 1) as usize]
}
pub struct Segment<'a> {
    data: &'a [u8],
}
impl<'a> Segment<'a> {
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.boot")]
    pub fn new(data: &'a [u8]) -> Result<Self, AssetError> {
        if data.len() != SEGMENT_SIZE && data.len() != SEGMENT_CACHE_SIZE {
            return Err(AssetError::Size);
        }
        if &data[..4] != b"SMS2" {
            return Err(AssetError::Magic);
        }
        if data[4] >= SEGMENT_COUNT
            || data[5] > 3
            || data[6] > 15
            || data[7] > MAX_SEGMENT_SPAWNS
            || data[7] as usize > (data.len() - 16) / 8
            || data[8] > 3
            || data[9] >= crate::music::CUE_COUNT as u8
            || data[10] > 4
            || data[11] > 5
            || data[12] > 15
            || data[13] > 15
            || data[14] > 15
            || data[15] != 0
        {
            return Err(AssetError::Bounds);
        }
        let mut prev = 0;
        for i in 0..data[7] {
            let at = 16 + i as usize * 8;
            let distance = u16_at(data, at);
            let kind = data[at + 2];
            let flags = data[at + 3];
            if distance < prev
                || distance as u32 >= SEGMENT_DISTANCE
                || !matches!(kind, 1 | 2 | 3 | 5 | 6 | 7)
                || (data[11] != 0 && kind == TURRET)
                || flags & !3 != 0
                || (flags & SMALL_DEBRIS != 0 && (kind != ROCK || data[12] < 10))
            {
                return Err(AssetError::Bounds);
            }
            prev = distance;
        }
        Ok(Self { data })
    }
    /// Reconstruct a prefetched, previously checked mission view without scanning it.
    ///
    /// # Safety
    /// `data` must contain unchanged bytes accepted by `new`, or exactly the
    /// `SEGMENT_CACHE_SIZE` prefix of an accepted packaged segment. The memory
    /// must remain valid and unmodified for the view's lifetime. Drop all views
    /// before reusing a segment's DMA destination for another segment.
    #[inline]
    pub unsafe fn from_validated(data: &'a [u8]) -> Self {
        Self { data }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    pub fn index(&self) -> u8 {
        self.data[4]
    }
    pub fn terrain(&self) -> u8 {
        self.data[5]
    }
    pub fn ground_color(&self) -> u8 {
        self.data[6]
    }
    #[inline(always)]
    pub fn stage_info(&self) -> StageInfo {
        StageInfo {
            enemy_profile: self.data[8],
            music_cue: self.data[9],
            bridge_profile: self.data[10],
            boss_kind: self.data[11],
            theme: self.data[12],
            next_theme: self.data[13],
            previous_theme: self.data[14],
            reserved: self.data[15],
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    pub fn spawn_count(&self) -> u8 {
        self.data[7]
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
    pub fn spawn(&self, i: u8) -> Spawn {
        if i >= self.spawn_count() {
            return Spawn::default();
        }
        let p = 16 + i as usize * 8;
        // Validation bounds every complete eight-byte record, including the
        // final record of a 512-byte cache. The guard above proves this index.
        // Decode in place so native code need not spill the partially built
        // Spawn around three general slice-reading calls.
        unsafe {
            Spawn {
                distance: u16::from_le_bytes([
                    *self.data.get_unchecked(p),
                    *self.data.get_unchecked(p + 1),
                ]),
                kind: *self.data.get_unchecked(p + 2),
                flags: *self.data.get_unchecked(p + 3),
                x: i16::from_le_bytes([
                    *self.data.get_unchecked(p + 4),
                    *self.data.get_unchecked(p + 5),
                ]),
                y: i16::from_le_bytes([
                    *self.data.get_unchecked(p + 6),
                    *self.data.get_unchecked(p + 7),
                ]),
            }
        }
    }
}

/// DMA chunks never wrap REU's 512 KiB pages, installed 1 MiB, or CPU RAM.
pub fn dma_chunk(reu: u32, cpu: u16, remaining: u16) -> u16 {
    if remaining == 0 || reu >= REU_SIZE as u32 {
        return 0;
    }
    let reu_left = 0x80000 - (reu & 0x7ffff);
    let cpu_left = 0x10000u32 - cpu as u32;
    (remaining as u32)
        .min(reu_left)
        .min(cpu_left)
        .min(REU_SIZE as u32 - reu) as u16
}
