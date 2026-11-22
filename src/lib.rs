//! TwinSession for Horizon: the 1:1 Rust port of TheMisterFish's TwinSession
//! (CC0-1.0). The twin registry, profile minting and spawn placement are
//! carried and tested; nothing calls them yet, because Horizon's
//! native_player_login can cancel a join but cannot rewrite the joining
//! player's profile (horizon-rust/horizon #10057), which is the heart of the
//! mod. Ships as the tested library until that surface lands.

pub mod runtime;

use mod_api::Hooks;

#[derive(Default)]
struct Mod;

#[mod_api::horizon_mod]
impl Hooks for Mod {}
