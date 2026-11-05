//! twinsession runtime (Rust port of TheMisterFish/Fabric_TwinSession, CC0-1.0).
//!
//! Core registry and profile logic, ported 1:1 from TwinSession.java,
//! ServerLoginPacketListenerImpl_TwinSessionMixin.java and
//! PlayerList_TwinSessionMixin.java. Server wiring in Horizon rides
//! native_player_login (API 172); see README for the one seam this
//! surface cannot cover yet (profile rewrite at login).

use std::collections::{BTreeMap, HashMap};

pub mod md5;
pub mod spawn;

pub use md5::md5;
pub use spawn::{
    find_safe_y_within_range, is_player_on_nether_roof, is_safe_spawn, pick_spawn_position, Block,
    SpawnChoice,
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
    pub fn create_new_game_profile(&mut self, source: &Profile, cfg: &TwinConfig) -> (Profile, u32) {
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
