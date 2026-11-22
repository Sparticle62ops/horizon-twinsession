// twinsession runtime (Rust port of TheMisterFish/Fabric_TwinSession, CC0-1.0).
//
// Core registry and profile logic, ported 1:1 from TwinSession.java,
// ServerLoginPacketListenerImpl_TwinSessionMixin.java and
// PlayerList_TwinSessionMixin.java. Server wiring in Horizon rides
// native_player_login (API 172); see README for the one seam this
// surface cannot cover yet (profile rewrite at login).

use std::collections::{BTreeMap, HashMap};

pub use md5::md5;
pub use spawn::{
    Block, SpawnChoice, find_safe_y_within_range, is_player_on_nether_roof, is_safe_spawn,
    pick_spawn_position,
};

pub const MOD_ID: &str = "twinsession";

/// A UUID as its 16 bytes, the two halves the original games with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Uuid([u8; 16]);

impl Uuid {
    pub const NIL: Uuid = Uuid([0u8; 16]);

    pub fn from_u64_pair(most: u64, least: u64) -> Self {
        let mut bytes = [0u8; 16];
        bytes[..8].copy_from_slice(&most.to_be_bytes());
        bytes[8..].copy_from_slice(&least.to_be_bytes());
        Uuid(bytes)
    }

    pub fn as_u64_pair(&self) -> (u64, u64) {
        (
            u64::from_be_bytes(self.0[..8].try_into().unwrap()),
            u64::from_be_bytes(self.0[8..].try_into().unwrap()),
        )
    }
}

/// The seven settings of the original ModConfigs, with its defaults.
#[derive(Clone, Debug)]
pub struct TwinConfig {
    pub max_players: i32,
    pub auto_whitelist: bool,
    pub auto_op: bool,
    pub spawn_near_player: bool,
    pub spawn_near_player_radius: i32,
    pub copy_texture: bool,
    pub prefix_with_number: bool,
}

impl Default for TwinConfig {
    fn default() -> Self {
        Self {
            max_players: 8,
            auto_whitelist: true,
            // Upstream default (autoOp on). When the login wiring lands, a twin
            // gets op only if the original account already has it (review note).
            auto_op: true,
            spawn_near_player: true,
            spawn_near_player_radius: 10,
            copy_texture: true,
            prefix_with_number: true,
        }
    }
}

/// A server player identity: the GameProfile fields the logic reads (id,
/// name) plus the textures property payloads copied on a duplicate join.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub id: Uuid,
    pub name: String,
    pub textures: Vec<String>,
}

impl Profile {
    pub fn new(id: Uuid, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            textures: Vec::new(),
        }
    }
}

/// The original twinMap: for each original UUID, the live duplicate ids by
/// their join position (1-based, the lowest free slot taken next).
pub struct TwinRegistry {
    map: HashMap<Uuid, BTreeMap<u32, Uuid>>,
}

impl Default for TwinRegistry {
    fn default() -> Self {
        Self {
            map: HashMap::new(),
        }
    }
}

impl TwinRegistry {
    /// createNewGameProfile: next free position, the truncated
    /// position_name name and its MD5-v3 UUID.
    pub fn create_new_game_profile(
        &mut self,
        source: &Profile,
        cfg: &TwinConfig,
    ) -> (Profile, u32) {
        let uuid_map = self.map.entry(source.id).or_default();

        let mut next_position = 1u32;
        while uuid_map.contains_key(&next_position) {
            next_position += 1;
        }

        let truncated: String = format!("{}_{}", next_position, source.name)
            .chars()
            .take(16)
            .collect();
        let new_uuid = uuid_from_name(&format!("{}$", truncated));

        uuid_map.insert(next_position, new_uuid);

        let name_to_use = if cfg.prefix_with_number {
            truncated
        } else {
            source.name.clone()
        };

        let mut profile = Profile::new(new_uuid, name_to_use);
        if cfg.copy_texture {
            profile.textures = source.textures.clone();
        }
        (profile, next_position)
    }

    /// canJoin: no entry means first join, else one more duplicate must fit
    /// under the cap (the original size() + 1 < maxPlayers, ported as is).
    pub fn can_join(&self, original: Uuid, cfg: &TwinConfig) -> bool {
        match self.map.get(&original) {
            None => true,
            Some(entry) => (entry.len() as i32 + 1) < cfg.max_players,
        }
    }

    /// getSourcePlayer: the original id whose duplicate map holds this id.
    pub fn source_of(&self, duplicate: Uuid) -> Option<Uuid> {
        self.map
            .iter()
            .find(|(_, m)| m.values().any(|v| *v == duplicate))
            .map(|(k, _)| *k)
    }

    /// playerLeft: drop this id from whichever original's map holds it, and
    /// drop the map itself once its last duplicate leaves.
    pub fn player_left(&mut self, profile_id: Uuid) {
        let mut emptied: Option<Uuid> = None;
        for (original, uuid_map) in self.map.iter_mut() {
            if let Some(pos) = uuid_map
                .iter()
                .find(|(_, v)| **v == profile_id)
                .map(|(k, _)| *k)
            {
                uuid_map.remove(&pos);
                if uuid_map.is_empty() {
                    emptied = Some(*original);
                }
                break;
            }
        }
        if let Some(original) = emptied {
            self.map.remove(&original);
        }
    }

    /// Live duplicate ids for one original (the CreateNewGameProfileTest view).
    pub fn duplicates_of(&self, original: Uuid) -> Vec<Uuid> {
        self.map
            .get(&original)
            .map(|m| m.values().copied().collect())
            .unwrap_or_default()
    }
}

/// generateUUIDFromName: MD5 of the UTF-8 name folded into a UUID v3
/// (version nibble 3, RFC 4122 variant), byte-exact with the original.
pub fn uuid_from_name(name: &str) -> Uuid {
    let hash = md5(name.as_bytes());
    let mut most = u64::from_be_bytes(hash[..8].try_into().unwrap());
    let mut least = u64::from_be_bytes(hash[8..].try_into().unwrap());
    most &= !0x0000_0000_0000_F000u64;
    most |= 0x0000_0000_0000_3000u64;
    least &= !0xC000_0000_0000_0000u64;
    least |= 0x8000_0000_0000_0000u64;
    Uuid::from_u64_pair(most, least)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uuid(most: u64, least: u64) -> Uuid {
        Uuid::from_u64_pair(most, least)
    }

    fn hex(bytes: [u8; 16]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }

    #[test]
    fn md5_rfc1321_vectors() {
        assert_eq!(hex(md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex(md5(b"a")), "0cc175b9c0f1b6a831c399e269772661");
        assert_eq!(hex(md5(b"abc")), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            hex(md5(b"abcdefghijklmnopqrstuvwxyz")),
            "c3fcd3d76192e4007dfb496cca67e13b"
        );
    }

    #[test]
    fn uuid_from_name_matches_java_semantics() {
        // Deterministic, and version/variant stamped like UUID v3.
        let a = uuid_from_name("1_Danil$");
        let b = uuid_from_name("1_Danil$");
        assert_eq!(a, b);
        let (hi, lo) = a.as_u64_pair();
        assert_eq!(hi & 0xF000, 0x3000);
        assert_eq!(lo & 0xC000000000000000, 0x8000000000000000);
        // A different name gives a different id.
        assert_ne!(a, uuid_from_name("2_Danil$"));
    }

    #[test]
    fn create_profile_truncates_names_to_16_chars() {
        let mut reg = TwinRegistry::default();
        let cfg = TwinConfig::default();
        let source = Profile::new(uuid(1, 2), "averylongoriginalname");
        let (profile, pos) = reg.create_new_game_profile(&source, &cfg);
        assert_eq!(pos, 1);
        assert_eq!(profile.name.chars().count(), 16);
        assert!(profile.name.starts_with("1_"));
        // The same original again takes the next slot with a fresh UUID.
        let (p2, pos2) = reg.create_new_game_profile(&source, &cfg);
        assert_eq!(pos2, 2);
        assert_ne!(profile.id, p2.id);
        assert!(p2.name.starts_with("2_"));
    }

    #[test]
    fn no_prefix_keeps_the_original_name() {
        let mut reg = TwinRegistry::default();
        let cfg = TwinConfig {
            prefix_with_number: false,
            ..TwinConfig::default()
        };
        let source = Profile::new(uuid(1, 2), "Danil");
        let (profile, _) = reg.create_new_game_profile(&source, &cfg);
        assert_eq!(profile.name, "Danil");
    }

    #[test]
    fn textures_copied_only_when_configured() {
        let mut reg = TwinRegistry::default();
        let mut source = Profile::new(uuid(1, 2), "Danil");
        source.textures = vec!["base64blob".into()];
        let cfg = TwinConfig::default();
        let (with, _) = reg.create_new_game_profile(&source, &cfg);
        assert_eq!(with.textures, vec!["base64blob".to_string()]);
        let cfg = TwinConfig {
            copy_texture: false,
            ..TwinConfig::default()
        };
        let (without, _) = reg.create_new_game_profile(&source, &cfg);
        assert!(without.textures.is_empty());
    }

    #[test]
    fn can_join_cap_is_the_original_inequality() {
        let mut reg = TwinRegistry::default();
        let cfg = TwinConfig {
            max_players: 3,
            ..TwinConfig::default()
        };
        let source = Profile::new(uuid(1, 2), "Danil");
        // First join: no entry, always allowed.
        assert!(reg.can_join(source.id, &cfg));
        reg.create_new_game_profile(&source, &cfg);
        reg.create_new_game_profile(&source, &cfg);
        // size() + 1 < 3 with size 2 refuses, exactly as upstream.
        assert!(!reg.can_join(source.id, &cfg));
    }

    #[test]
    fn player_left_cleans_positions_and_empty_maps() {
        let mut reg = TwinRegistry::default();
        let cfg = TwinConfig::default();
        let source = Profile::new(uuid(1, 2), "Danil");
        let (t1, _) = reg.create_new_game_profile(&source, &cfg);
        let (t2, _) = reg.create_new_game_profile(&source, &cfg);
        // A stranger leaving changes nothing.
        reg.player_left(uuid(9, 9));
        assert_eq!(reg.duplicates_of(source.id).len(), 2);
        // Leaving the middle twin frees its slot for the next join.
        reg.player_left(t1.id);
        let (t3, pos) = reg.create_new_game_profile(&source, &cfg);
        assert_eq!(pos, 1);
        assert_eq!(t3.id, t1.id); // same name and position, same MD5-v3 UUID
        // Last duplicate out removes the map: back to a fresh first join.
        reg.player_left(t2.id);
        reg.player_left(t3.id);
        assert!(reg.duplicates_of(source.id).is_empty());
        let (fresh, pos) = reg.create_new_game_profile(&source, &cfg);
        assert_eq!(pos, 1);
        assert_eq!(fresh.id, t1.id);
    }

    #[test]
    fn source_of_reverses_to_the_original() {
        let mut reg = TwinRegistry::default();
        let cfg = TwinConfig::default();
        let source = Profile::new(uuid(1, 2), "Danil");
        let (twin, _) = reg.create_new_game_profile(&source, &cfg);
        assert_eq!(reg.source_of(twin.id), Some(source.id));
        assert_eq!(reg.source_of(source.id), None);
    }
}

pub mod md5 {
    // Hand-rolled MD5 (RFC 1321), for the UUID-v3 ids the original derives
    // from the duplicate's name. No external crates.

    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];

    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];

    /// MD5 digest of the input, as the 16 output bytes.
    pub fn md5(input: &[u8]) -> [u8; 16] {
        let mut state: [u32; 4] = [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];

        let mut msg = input.to_vec();
        let bit_len = (input.len() as u64).wrapping_mul(8);
        msg.push(0x80);
        while msg.len() % 64 != 56 {
            msg.push(0);
        }
        msg.extend_from_slice(&bit_len.to_le_bytes());

        for chunk in msg.chunks_exact(64) {
            let mut m = [0u32; 16];
            for i in 0..16 {
                m[i] = u32::from_le_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap());
            }
            let (mut a, mut b, mut c, mut d) = (state[0], state[1], state[2], state[3]);
            for i in 0..64 {
                let (f, g) = match i / 16 {
                    0 => ((b & c) | (!b & d), i),
                    1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                    2 => (b ^ c ^ d, (3 * i + 5) % 16),
                    _ => (c ^ (b | !d), (7 * i) % 16),
                };
                let tmp = d;
                d = c;
                c = b;
                let x = a.wrapping_add(f).wrapping_add(K[i]).wrapping_add(m[g]);
                b = b.wrapping_add(x.rotate_left(S[i]));
                a = tmp;
            }
            state[0] = state[0].wrapping_add(a);
            state[1] = state[1].wrapping_add(b);
            state[2] = state[2].wrapping_add(c);
            state[3] = state[3].wrapping_add(d);
        }

        let mut out = [0u8; 16];
        for (i, s) in state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&s.to_le_bytes());
        }
        out
    }
}

pub mod spawn {
    // Spawn-nearby placement, ported from TwinSession.spawnPlayerNearby and its
    // helpers. The world-facing pieces run through a BlockLookup so the logic
    // stays testable without a live level.

    /// The three block facts the original's safe-spawn test reads.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Block {
        Air,
        Solid,
        Lava,
    }

    /// Reads one block position of the level the joining player lands in.

    /// A spawn candidate: world position and whether creative flight should stay on.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct SpawnChoice {
        pub x: f64,
        pub y: f64,
        pub z: f64,
        pub creative_flying: bool,
    }

    /// isPlayerOnNetherRoof: a ceiling dimension (the Nether) and the roof level.
    pub fn is_player_on_nether_roof(has_ceiling: bool, pos_y: f64, logical_height: i32) -> bool {
        has_ceiling && pos_y >= logical_height as f64
    }

    /// findSafeYWithinRange: scan top down for a safe Y within range of pos.y.
    /// min_y and max_y come from the level bounds; returns None when nothing is safe.
    pub fn find_safe_y_within_range(
        lookup: &impl Fn(i32, i32, i32) -> Block,
        min_y: i32,
        max_y: i32,
        x: i32,
        y: i32,
        z: i32,
        range: i32,
    ) -> Option<i32> {
        let top = (max_y - 1).min(y + range);
        let bottom = min_y.max(y - range);
        let mut yy = top;
        while yy >= bottom {
            if is_safe_spawn(lookup, x, yy, z) {
                return Some(yy);
            }
            yy -= 1;
        }
        None
    }

    /// isSafeSpawn: two blocks of air standing on something solid that is not lava.
    pub fn is_safe_spawn(lookup: &impl Fn(i32, i32, i32) -> Block, x: i32, y: i32, z: i32) -> bool {
        lookup(x, y, z) == Block::Air
            && lookup(x, y + 1, z) == Block::Air
            && lookup(x, y - 1, z) != Block::Air
            && lookup(x, y - 1, z) != Block::Lava
    }

    /// xorshift64*, a tiny deterministic stand-in for the original's RandomSource.
    struct Rng(u64);

    impl Rng {
        fn next_f64(&mut self) -> f64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            let v = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
            ((v >> 11) as f64) / ((1u64 << 53) as f64)
        }
    }

    /// spawnPlayerNearby: sample points in a radius around the source player,
    /// relaxing the minimum distance until one lands safe, then fall back to the
    /// source position. Returns where the duplicate should be placed.
    pub fn pick_spawn_position(
        source: (f64, f64, f64),
        creative_flying: bool,
        on_nether_roof: bool,
        radius: i32,
        level_min_y: i32,
        level_max_y: i32,
        logical_height: i32,
        lookup: &impl Fn(i32, i32, i32) -> Block,
    ) -> SpawnChoice {
        let (sx, sy, sz) = source;
        let spawn_diameter = (radius * 2) as f64;
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15 | (sy.to_bits() | 1));
        let mut best: Option<SpawnChoice> = None;

        // Bounded: upstream's rejection loop cannot terminate when the radius is
        // 2 or less (max achievable distance radius*sqrt(2) stays under 3), which
        // would stall the caller. Clamp the threshold so a sample always exists.
        let mut distance_threshold = 3i32.min(radius);
        while best.is_none() && distance_threshold >= 1 {
            for _ in 0..10 {
                let (x_off, z_off, dist) = loop {
                    let x_off = (rng.next_f64() - 0.5) * spawn_diameter;
                    let z_off = (rng.next_f64() - 0.5) * spawn_diameter;
                    let dist = (x_off * x_off + z_off * z_off).sqrt();
                    if dist >= distance_threshold as f64 {
                        break (x_off, z_off, dist);
                    }
                };
                let nx = sx + x_off;
                let nz = sz + z_off;

                if creative_flying {
                    let y_off = (rng.next_f64() - 0.5) * 6.0;
                    best = Some(SpawnChoice {
                        x: nx,
                        y: sy + y_off,
                        z: nz,
                        creative_flying: true,
                    });
                    break;
                }
                let safe_y = find_safe_y_within_range(
                    lookup,
                    level_min_y,
                    level_max_y,
                    nx as i32,
                    sy as i32,
                    nz as i32,
                    3,
                );
                if let Some(ny) = safe_y {
                    if !on_nether_roof && ny > logical_height {
                        continue;
                    }
                    best = Some(SpawnChoice {
                        x: nx,
                        y: ny as f64,
                        z: nz,
                        creative_flying: false,
                    });
                    break;
                }
            }
            if best.is_none() {
                distance_threshold -= 1;
            }
        }

        best.unwrap_or(SpawnChoice {
            x: sx,
            y: sy,
            z: sz,
            creative_flying: false,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn nether_roof_matches_the_original_check() {
            assert!(is_player_on_nether_roof(true, 128.0, 128));
            assert!(!is_player_on_nether_roof(true, 100.0, 128));
            assert!(!is_player_on_nether_roof(false, 500.0, 128));
        }

        #[test]
        fn safe_spawn_rejects_air_all_the_way_down() {
            let lookup = |_, _, _| Block::Air;
            assert!(!is_safe_spawn(&lookup, 0, 4, 0));
        }

        #[test]
        fn safe_spawn_rejects_lava_floor() {
            let lookup = |_x: i32, y: i32, _z: i32| {
                if y == 4 || y == 5 {
                    Block::Air
                } else if y == 3 {
                    Block::Lava
                } else {
                    Block::Air
                }
            };
            assert!(!is_safe_spawn(&lookup, 0, 4, 0));
        }

        #[test]
        fn safe_spawn_accepts_a_solid_floor() {
            let lookup = |_x: i32, y: i32, _z: i32| {
                if y == 4 || y == 5 {
                    Block::Air
                } else if y == 3 {
                    Block::Solid
                } else {
                    Block::Air
                }
            };
            assert!(is_safe_spawn(&lookup, 0, 4, 0));
        }

        #[test]
        fn safe_y_scans_top_down_within_range() {
            let lookup = |_x: i32, y: i32, _z: i32| {
                if y == 10 || y == 11 {
                    Block::Air
                } else if y == 9 {
                    Block::Solid
                } else {
                    Block::Air
                }
            };
            assert_eq!(
                find_safe_y_within_range(&lookup, -64, 320, 0, 20, 0, 15),
                Some(10)
            );
            let empty = |_, _, _| Block::Air;
            assert_eq!(
                find_safe_y_within_range(&empty, -64, 320, 0, 20, 0, 3),
                None
            );
        }

        #[test]
        fn spawn_nearby_falls_back_to_the_source_position() {
            // All air, nothing solid: no safe Y anywhere, so the source position wins.
            let empty = |_, _, _| Block::Air;
            let pick =
                pick_spawn_position((10.0, 64.0, -20.0), false, false, 10, -64, 320, 384, &empty);
            assert_eq!((pick.x, pick.y, pick.z), (10.0, 64.0, -20.0));
            assert!(!pick.creative_flying);
        }

        #[test]
        fn spawn_nearby_accepts_a_safe_column() {
            // Solid floor at 63, air on top: the scan around y=64 finds it.
            let lookup = |_x: i32, y: i32, _z: i32| {
                if y == 64 || y == 65 {
                    Block::Air
                } else if y == 63 {
                    Block::Solid
                } else {
                    Block::Air
                }
            };
            let pick = pick_spawn_position(
                (10.0, 64.0, -20.0),
                false,
                false,
                10,
                -64,
                320,
                384,
                &lookup,
            );
            // Lands within the radius of the source and on a found safe Y.
            let dx = pick.x - 10.0;
            let dz = pick.z + 20.0;
            assert!((dx * dx + dz * dz).sqrt() <= 10.0 + 1e-9);
            assert_eq!(pick.y, 64.0);
        }

        #[test]
        fn creative_flight_picks_directly() {
            let empty = |_, _, _| Block::Air;
            let pick =
                pick_spawn_position((0.0, 100.0, 0.0), true, false, 10, -64, 320, 384, &empty);
            assert!(pick.creative_flying);
            assert!((pick.x, pick.z) != (0.0, 0.0));
        }
    }
}
