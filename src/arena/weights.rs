use super::config::ArenaConfig;
use super::player::ArenaPlayerProjection;
use crate::domain::PlayerId;
use crate::matching::BigWeight;

const WAIT_BITS: usize = 10;
const WAIT_RESOLUTION: u128 = (1 << WAIT_BITS) - 1;
const BOTTOM_RANK_FACTOR: usize = 300;
const RANK_FACTOR_SPAN: usize = 1700;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WeightError {
    ArithmeticOverflow,
}

pub(super) struct ArenaWeightContext<'a> {
    pub(super) config: &'a ArenaConfig,
    pub(super) players: &'a [ArenaPlayerProjection],
    /// Fixed Arena competition ranks: exact standings ties share a value.
    pub(super) ranks: Vec<usize>,
    pub(super) max_rank: usize,
    pub(super) max_cost: usize,
    pub(super) longest_wait: u64,
    /// Exactly two active entrants, including those not currently waiting.
    pub(super) duel: bool,
}

/// Lichess-inspired rank/rating proximity, with waiting and colour balance as
/// deterministic lower tiers. Both pairing policies use this exact function.
pub(super) fn pair_weight(
    context: &ArenaWeightContext<'_>,
    a: PlayerId,
    b: PlayerId,
) -> Result<Option<BigWeight>, WeightError> {
    if a == b || is_excluded_rematch(context, a, b) {
        return Ok(None);
    }
    let first = &context.players[a.index()];
    let second = &context.players[b.index()];
    if !first.could_play(second, context.config.max_color_strike) {
        return Ok(None);
    }

    let cost = pair_cost(context, a, b)?.min(context.max_cost);
    let mut weight = BigWeight::from_usize(context.max_cost - cost);
    weight.shl_assign(WAIT_BITS);
    weight.or_assign_usize(wait_value(context, first, second));
    // Prefer complementary colour histories only after rank/rating and wait.
    weight.shl_assign(1);
    weight.or_assign_usize(usize::from(
        first.color_history_key() != second.color_history_key(),
    ));
    // Zero is a non-edge to the matching solver.
    weight.shl_assign(1);
    weight.or_assign_usize(1);
    Ok(Some(weight))
}

fn pair_cost(
    context: &ArenaWeightContext<'_>,
    a: PlayerId,
    b: PlayerId,
) -> Result<usize, WeightError> {
    let rank_gap = context.ranks[a.index()].abs_diff(context.ranks[b.index()]);
    let rating_gap = match (
        context.players[a.index()].arena_rating,
        context.players[b.index()].arena_rating,
    ) {
        (Some(first), Some(second)) => first.abs_diff(second),
        _ => 0,
    };
    rank_gap
        .checked_mul(rank_factor(context, a, b)?)
        .and_then(|rank_cost| rank_cost.checked_add(usize::try_from(rating_gap).ok()?))
        .ok_or(WeightError::ArithmeticOverflow)
}

fn rank_factor(
    context: &ArenaWeightContext<'_>,
    a: PlayerId,
    b: PlayerId,
) -> Result<usize, WeightError> {
    if context.max_rank == 0 {
        return Ok(BOTTOM_RANK_FACTOR
            .checked_add(RANK_FACTOR_SPAN)
            .expect("fixed Arena rank factors fit usize"));
    }
    let best_rank = context.ranks[a.index()]
        .min(context.ranks[b.index()])
        .min(context.max_rank);
    let scaled = RANK_FACTOR_SPAN
        .checked_mul(context.max_rank - best_rank)
        .and_then(|scaled| scaled.checked_div(context.max_rank))
        .ok_or(WeightError::ArithmeticOverflow)?;
    Ok(BOTTOM_RANK_FACTOR
        .checked_add(scaled)
        .expect("fixed Arena bottom-rank factor fits scaled rank space"))
}

pub(super) fn max_pair_cost(rank_span: usize, rating_span: usize) -> Result<usize, WeightError> {
    let maximum_rank_factor = BOTTOM_RANK_FACTOR
        .checked_add(RANK_FACTOR_SPAN)
        .expect("fixed Arena rank factors fit usize");
    rank_span
        .checked_mul(maximum_rank_factor)
        .and_then(|cost| cost.checked_add(rating_span))
        .ok_or(WeightError::ArithmeticOverflow)
}

fn is_excluded_rematch(context: &ArenaWeightContext<'_>, a: PlayerId, b: PlayerId) -> bool {
    let first = &context.players[a.index()];
    let second = &context.players[b.index()];
    let cooldown = context.config.rematch_cooldown;
    if !first.played_recently(b, cooldown) && !second.played_recently(a, cooldown) {
        return false;
    }
    let required_wait = u64::from(context.config.rematch_wait_secs) * 1000;
    !context.duel
        || first
            .waiting_millis()
            .is_none_or(|waited| waited < required_wait)
        || second
            .waiting_millis()
            .is_none_or(|waited| waited < required_wait)
}

fn wait_value(
    context: &ArenaWeightContext<'_>,
    first: &ArenaPlayerProjection,
    second: &ArenaPlayerProjection,
) -> usize {
    if context.longest_wait == 0 {
        return 0;
    }
    let waited = |player: &ArenaPlayerProjection| player.waiting_millis().unwrap_or(0);
    let combined = u128::from(waited(first)) + u128::from(waited(second));
    let ceiling = 2 * u128::from(context.longest_wait);
    (combined * WAIT_RESOLUTION / ceiling).min(WAIT_RESOLUTION) as usize
}
