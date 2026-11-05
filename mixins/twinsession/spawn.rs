//! Spawn-nearby placement, ported from TwinSession.spawnPlayerNearby and its
//! helpers. The world-facing pieces run through a BlockLookup so the logic
//! stays testable without a live level.

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

    let mut distance_threshold = 3i32;
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
                best = Some(SpawnChoice { x: nx, y: sy + y_off, z: nz, creative_flying: true });
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
                best = Some(SpawnChoice { x: nx, y: ny as f64, z: nz, creative_flying: false });
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
        assert_eq!(find_safe_y_within_range(&empty, -64, 320, 0, 20, 0, 3), None);
    }

    #[test]
    fn spawn_nearby_falls_back_to_the_source_position() {
        // All air, nothing solid: no safe Y anywhere, so the source position wins.
        let empty = |_, _, _| Block::Air;
        let pick = pick_spawn_position((10.0, 64.0, -20.0), false, false, 10, -64, 320, 384, &empty);
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
        let pick = pick_spawn_position((10.0, 64.0, -20.0), false, false, 10, -64, 320, 384, &lookup);
        // Lands within the radius of the source and on a found safe Y.
        let dx = pick.x - 10.0;
        let dz = pick.z + 20.0;
        assert!((dx * dx + dz * dz).sqrt() <= 10.0 + 1e-9);
        assert_eq!(pick.y, 64.0);
    }

    #[test]
    fn creative_flight_picks_directly() {
        let empty = |_, _, _| Block::Air;
        let pick = pick_spawn_position((0.0, 100.0, 0.0), true, false, 10, -64, 320, 384, &empty);
        assert!(pick.creative_flying);
        assert!((pick.x, pick.z) != (0.0, 0.0));
    }
}
