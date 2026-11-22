# TwinSession (Horizon Rust port)

A Rust port of [TwinSession](https://github.com/TheMisterFish/Fabric_TwinSession)
(CC0-1.0, by TheMisterFish). Ported for the Horizon client by sparticle.

## Status

The twin registry, profile minting (MD5-v3 UUIDs), canJoin and the spawn
placement are carried 1:1 and tested. Nothing calls them yet: Horizon's
native_player_login can cancel a join but cannot rewrite the joining player's
profile (horizon-rust/horizon #10057), which is the heart of the mod, so the
mod does nothing in game yet. Ships as the tested 1:1 library until that
surface lands.

The spawn-sampling loop is bounded so a radius of 2 or less terminates instead
of stalling; the autoOp default stays upstream's, with the gate documented (a
twin gets op only if the original account already has it) for when the wiring
lands.

## Build

`cargo test` (16 tests). Package: the repo files zipped as
`twinsession-1.0.0.hmod`. Pinned to Horizon
f00125f61f794aaa3d4e950d301f353b35b6ee3d and horizon-mixins
e59da9094f0c2fdda6c627fe9eff22dec6a906f0.

## Licence

CC0-1.0 (see LICENSE).
