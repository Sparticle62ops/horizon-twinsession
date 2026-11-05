# twinsession (Horizon port)

A Rust port of [TwinSession](https://github.com/TheMisterFish/Fabric_TwinSession)
(CC0-1.0, by TheMisterFish), which lets several clients join one server from the
same Minecraft account: the server mints each duplicate a fresh UUID and name so
they play side by side without kicking the original.

## Ported (1:1)

- The twin registry: live duplicates per original UUID, lowest free join
  position, cleanup on leave (TwinSession.twinMap, playerLeft).
- createNewGameProfile: the N_name name truncated to 16 chars, its MD5-v3 UUID
  (hand-rolled MD5, byte-exact with Java MessageDigest), the texture property
  copied when copyTexture is on, the number prefix toggle.
- canJoin with the original inequality (size + 1 < maxPlayers).
- The spawn-nearby placement: sample points in the configured radius, relaxed
  minimum distance, safe-Y scan top down, nether-roof guard, creative-flight
  passthrough, source-position fallback.
- All seven config values with the original defaults (maxPlayers 8,
  autoWhitelist on, autoOp on, spawnNearPlayer on, radius 10, copyTexture on,
  prefixWithNumber on).

## Horizon seam

The native-mod API hears a joining player once accepted through
native_player_login (API 172) and may cancel the join with a reason, but it
cannot rewrite the joining player's profile. The heart of TwinSession is that
rewrite: the duplicate's UUID is replaced before placement so two sessions of
one account coexist. Without it the port can detect a duplicate join and refuse
over-cap joins with the original reason, but cannot mint the twin identity.

Filed on horizon-rust/horizon as an API ask (profile rewrite in the login
negotiation). When it lands, the wiring is: hear the login, look up the
original in the registry, mint the twin profile, hand it to the server; on
placeNewPlayer copy op status and game mode from the source and teleport near
them when spawnNearPlayer is on and they have no saved data; on leave, clean
the registry.

## License

Upstream TwinSession: CC0-1.0 (TheMisterFish). This port carries the same
license; see LICENSE.
