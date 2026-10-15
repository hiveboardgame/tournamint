use core::cmp::{Ordering, Reverse};

use super::config::ArenaPairingPolicy;
use super::weights::{ArenaWeightContext, WeightError, pair_weight};
use crate::domain::{Color, Pairing, PlayerId};
use crate::matching::{BigWeight, MatchingError, MatchingSolver};

const EXACT_GROUP_SIZE: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PairingError {
    ArithmeticOverflow,
}

impl From<WeightError> for PairingError {
    fn from(error: WeightError) -> Self {
        match error {
            WeightError::ArithmeticOverflow => Self::ArithmeticOverflow,
        }
    }
}

pub(super) fn pair_pool(
    context: &ArenaWeightContext<'_>,
    pool: &[PlayerId],
) -> Result<Vec<Pairing>, PairingError> {
    let pairs = match context.config.pairing_policy {
        ArenaPairingPolicy::Grouped => pair_grouped(context, pool)?,
        ArenaPairingPolicy::Greedy => pair_greedy(context, pool)?,
    };
    let mut colored = Vec::with_capacity(pairs.len());
    colored.extend(
        pairs
            .into_iter()
            .map(|(first, second)| assign_colors(context, first, second)),
    );
    Ok(colored)
}

struct ExactSelection {
    pairs: Vec<(PlayerId, PlayerId)>,
    unmatched: Vec<PlayerId>,
}

/// Exactly maximizes cardinality and then aggregate quality for one supplied
/// player slice. The caller decides whether that slice is an initial group or
/// the complete ordered leftover pool.
fn pair_exact(
    context: &ArenaWeightContext<'_>,
    players: &[PlayerId],
) -> Result<ExactSelection, PairingError> {
    let mut pairs = Vec::with_capacity(players.len() / 2);
    let mut unmatched = Vec::with_capacity(players.len());
    if players.len() < 2 {
        unmatched.extend_from_slice(players);
        return Ok(ExactSelection { pairs, unmatched });
    }
    let mut solver = MatchingSolver::with_vertices(players.len());

    let mut edges = Vec::new();
    let edge_capacity = players
        .len()
        .checked_mul(players.len() - 1)
        .and_then(|value| value.checked_div(2))
        .expect("Arena exact group dimensions fit addressable memory");
    edges.reserve(edge_capacity);
    let mut max_quality = BigWeight::zero();
    for (first_vertex, &first) in players.iter().enumerate() {
        for (second_vertex, &second) in players.iter().enumerate().take(first_vertex) {
            if let Some(weight) = pair_weight(context, first, second)? {
                if weight > max_quality {
                    max_quality.clone_from(&weight);
                }
                edges.push((first_vertex, second_vertex, weight));
            }
        }
    }

    // One extra edge must dominate every possible total quality difference.
    let mut cardinality_base = BigWeight::from_usize(1);
    for _ in 0..players.len() / 2 {
        cardinality_base.add_assign(&max_quality);
    }
    for (first, second, mut weight) in edges {
        weight.add_assign(&cardinality_base);
        if let Err(error) = solver.set_big_edge_weight(first, second, weight) {
            match error {
                MatchingError::InvalidEdge => panic!(
                    "Arena exact pairing generated invalid internal edge {first}-{second} for {} vertices",
                    players.len()
                ),
            }
        }
    }
    solver.compute_matching();
    let matching = solver
        .current_matching()
        .expect("Arena exact pairing computed without a current matching");
    for (vertex, &partner) in matching.iter().enumerate() {
        match vertex.cmp(&partner) {
            Ordering::Less => pairs.push((players[vertex], players[partner])),
            Ordering::Equal => unmatched.push(players[vertex]),
            Ordering::Greater => {}
        }
    }
    Ok(ExactSelection { pairs, unmatched })
}

/// Solves bounded ordered groups exactly, fixes those pairs, then solves the
/// complete leftover pool exactly. The repair does not reconsider group pairs
/// and therefore makes no complete-pool global-optimality claim. Its work is
/// intentionally not capped when adversarial exclusions produce many
/// leftovers.
fn pair_grouped(
    context: &ArenaWeightContext<'_>,
    pool: &[PlayerId],
) -> Result<Vec<(PlayerId, PlayerId)>, PairingError> {
    pair_grouped_observed(context, pool, |_, _| {})
}

fn pair_grouped_observed(
    context: &ArenaWeightContext<'_>,
    pool: &[PlayerId],
    mut observe_repair: impl FnMut(usize, &ExactSelection),
) -> Result<Vec<(PlayerId, PlayerId)>, PairingError> {
    let order = ordered_pool(context, pool)?;
    let mut pairs = Vec::with_capacity(order.len() / 2);
    let mut leftovers = Vec::with_capacity(order.len());

    for group in order.chunks(EXACT_GROUP_SIZE) {
        let selection = pair_exact(context, group)?;
        pairs.extend(selection.pairs);
        leftovers.extend(selection.unmatched);
    }
    if order.len() > EXACT_GROUP_SIZE && leftovers.len() >= 2 {
        let repair_input_len = leftovers.len();
        let repair = pair_exact(context, &leftovers)?;
        observe_repair(repair_input_len, &repair);
        pairs.extend(repair.pairs);
    }
    Ok(pairs)
}

/// Explicit deterministic heuristic: longest wait, fixed Arena rank, join
/// order; each player takes the best remaining legal candidate.
fn pair_greedy(
    context: &ArenaWeightContext<'_>,
    pool: &[PlayerId],
) -> Result<Vec<(PlayerId, PlayerId)>, PairingError> {
    let order = ordered_pool(context, pool)?;
    let mut taken = vec![false; order.len()];
    let mut pairs = Vec::with_capacity(order.len() / 2);
    for first_index in 0..order.len() {
        if taken[first_index] {
            continue;
        }
        let mut best: Option<(usize, BigWeight)> = None;
        for second_index in first_index + 1..order.len() {
            if taken[second_index] {
                continue;
            }
            let Some(weight) = pair_weight(context, order[first_index], order[second_index])?
            else {
                continue;
            };
            if best
                .as_ref()
                .is_none_or(|(_, best_weight)| weight > *best_weight)
            {
                best = Some((second_index, weight));
            }
        }
        if let Some((second_index, _)) = best {
            taken[first_index] = true;
            taken[second_index] = true;
            pairs.push((order[first_index], order[second_index]));
        }
    }
    Ok(pairs)
}

fn ordered_pool(
    context: &ArenaWeightContext<'_>,
    pool: &[PlayerId],
) -> Result<Vec<PlayerId>, PairingError> {
    let mut order = pool.to_vec();
    order.sort_unstable_by_key(|player| pool_order_key(context, *player));
    Ok(order)
}

fn pool_order_key(
    context: &ArenaWeightContext<'_>,
    player: PlayerId,
) -> (Reverse<u64>, usize, usize) {
    let entry = &context.players[player.index()];
    let waited = entry.waiting_millis().unwrap_or(0);
    (
        Reverse(waited),
        context.ranks[player.index()],
        player.index(),
    )
}

fn assign_colors(context: &ArenaWeightContext<'_>, first: PlayerId, second: PlayerId) -> Pairing {
    let first_entry = &context.players[first.index()];
    let second_entry = &context.players[second.index()];
    let first_is_white = match first_entry
        .color_history_key()
        .cmp(&second_entry.color_history_key())
    {
        Ordering::Less => true,
        Ordering::Greater => false,
        Ordering::Equal => {
            let first_leads = (context.ranks[first.index()], first.index())
                < (context.ranks[second.index()], second.index());
            match context.config.initial_color {
                Color::White => first_leads,
                Color::Black => !first_leads,
            }
        }
    };
    if first_is_white {
        Pairing::new(first, second)
    } else {
        Pairing::new(second, first)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::ArenaConfig;
    use crate::arena::player::ArenaPlayerProjection;

    #[test]
    fn grouped_pool_order_is_wait_then_rank_then_player_id() {
        let config = ArenaConfig::plain();
        let players = vec![
            ArenaPlayerProjection::new(Some(100), None),
            ArenaPlayerProjection::new(Some(90), None),
            ArenaPlayerProjection::new(Some(90), None),
            ArenaPlayerProjection::new(Some(90), None),
        ];
        let context = ArenaWeightContext {
            config: &config,
            players: &players,
            ranks: vec![3, 0, 2, 2],
            max_rank: 3,
            max_cost: 0,
            longest_wait: 100,
            duel: false,
        };
        let pool = [
            PlayerId::new(3),
            PlayerId::new(2),
            PlayerId::new(1),
            PlayerId::new(0),
        ];

        assert_eq!(
            ordered_pool(&context, &pool).unwrap(),
            vec![
                PlayerId::new(0),
                PlayerId::new(1),
                PlayerId::new(2),
                PlayerId::new(3),
            ]
        );
        assert_eq!(EXACT_GROUP_SIZE, 64);
        assert_eq!(EXACT_GROUP_SIZE % 2, 0);
    }
}
