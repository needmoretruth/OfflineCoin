//! Bitcoin Cash's ASERT (aserti3-2d, spec 2020-11-15-asert.md), in integer arithmetic only.
//!
//! Bitcoin Cash moves a 256-bit target: `target × 2^((Δt − T(Δh + 1)) / τ)`. OfflineCoin moves a
//! difficulty, the target's inverse (see [`crate::ProofOfWork`]), so it runs the same steps with
//! the exponent's sign turned: `difficulty × 2^(−(Δt − T(Δh + 1)) / τ)`. The exponent, its split
//! into whole shifts and a fraction, and the cubic for `2^fraction` are the spec's own, so they
//! meet its published test vectors exactly (see `tests/asert_vectors.rs`). Times are in
//! milliseconds: the spec's seconds all scale by 1,000, which leaves every exponent as it was.

/// 16 bits after the point, as in the spec.
pub const RADIX: i128 = 1 << 16;

/// The spec's fixed-point exponent: `((Δt − T(Δh + 1)) × 2^16) / τ`, dividing toward zero as
/// its note 3 asks. `time_delta_ms` runs from the anchor's parent to the block measured,
/// `height_delta` from the anchor to that block.
pub fn exponent(time_delta_ms: i128, height_delta: i128, block_time_ms: u64, half_life_ms: u64) -> i128 {
    let ideal = i128::from(block_time_ms).saturating_mul(height_delta.saturating_add(1));
    let ahead = time_delta_ms.saturating_sub(ideal).saturating_mul(RADIX);
    ahead / i128::from(half_life_ms.max(1))
}

/// `2^(exponent / 2^16)` as the spec splits it: whole shifts, rounded down, and a factor from
/// 2^16 up to 2^17 for the fraction left, from its cubic.
pub fn power_of_two(exponent: i128) -> (i128, u128) {
    let shifts = exponent >> 16;
    let fraction = u128::try_from(exponent - shifts * RADIX).unwrap_or(0);
    let cubic = 195_766_423_245_049 * fraction
        + 971_821_376 * fraction * fraction
        + 5_127 * fraction * fraction * fraction
        + (1 << 47);
    (shifts, (cubic >> 48) + (1 << 16))
}

/// The difficulty `exponent` (as [`exponent`] gives it) away from `anchor`: harder when blocks
/// came early, easier when late, never below `floor`, as the spec's target never goes above its
/// limit, and at most `u64::MAX`, as its target never goes below 1.
pub fn difficulty(anchor: u64, exponent: i128, floor: u64) -> u64 {
    let (shifts, factor) = power_of_two(exponent.saturating_neg());
    let product = u128::from(anchor) * factor;
    let shifted = if shifts >= 0 {
        match u32::try_from(shifts) {
            Ok(left) if left < product.leading_zeros() => product << left,
            _ => u128::MAX,
        }
    } else {
        u32::try_from(-shifts).ok().and_then(|right| product.checked_shr(right)).unwrap_or(0)
    };
    u64::try_from(shifted >> 16).unwrap_or(u64::MAX).max(floor.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: u64 = 60_000;

    #[test]
    fn on_time_blocks_keep_the_difficulty() {
        let on_time = exponent(i128::from(MINUTE), 0, MINUTE, 10 * MINUTE);
        assert_eq!(on_time, 0);
        assert_eq!(difficulty(1_000_000, on_time, 1), 1_000_000);
    }

    #[test]
    fn each_half_life_late_halves_and_early_doubles() {
        let late = exponent(i128::from(11 * MINUTE), 0, MINUTE, 10 * MINUTE);
        assert_eq!(difficulty(1_000_000, late, 1), 500_000);
        let early = exponent(-9 * i128::from(MINUTE), 0, MINUTE, 10 * MINUTE);
        assert_eq!(difficulty(1_000_000, early, 1), 2_000_000);
    }

    #[test]
    fn a_long_wait_stops_at_the_floor_and_a_burst_at_the_top() {
        let year = exponent(i128::from(525_600 * MINUTE), 0, MINUTE, 10 * MINUTE);
        assert_eq!(difficulty(u64::MAX, year, 30_000), 30_000);
        let burst = exponent(-i128::from(525_600 * MINUTE), 0, MINUTE, 10 * MINUTE);
        assert_eq!(difficulty(30_000, burst, 30_000), u64::MAX);
    }
}
