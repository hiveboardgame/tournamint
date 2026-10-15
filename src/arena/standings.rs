use core::cmp::Ordering;

use super::ArenaError;
use super::config::ArenaConfig;
use super::player::ArenaPlayerProjection;
use super::scoring::is_on_fire;
use crate::domain::{PlayerId, Score};
use crate::standings::{StandingGroup, StandingRow, StandingsTable};

/// Exact Lichess-inspired tournament performance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArenaPerformance {
    numerator: i64,
    denominator: u32,
}

impl ArenaPerformance {
    /// Signed sum of rated-opponent result contributions.
    pub const fn numerator(self) -> i64 {
        self.numerator
    }

    /// Number of rated-opponent contributions.
    pub const fn denominator(self) -> u32 {
        self.denominator
    }
}

/// One participant's fixed Arena standings metrics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArenaStandingMetrics {
    /// Participant owning these metrics.
    pub player_id: PlayerId,
    /// Total scored Arena points, including fire and berserk.
    pub points: Score,
    /// `None` until a supplied played or no-start terminal has an opponent rating snapshot.
    pub performance: Option<ArenaPerformance>,
    /// Current Arena-relative rating supplied by the host.
    pub arena_rating: Option<i32>,
    /// Supplied played and no-start terminal facts.
    pub games_scored: u32,
    /// Played terminals with at least one ply.
    pub games_played: u32,
    /// No-start terminals for which this participant was absent.
    pub no_starts: u32,
    /// Wins in supplied scoring facts.
    pub wins: u32,
    /// Draws in supplied scoring facts, including suppressed draws.
    pub draws: u32,
    /// Losses in supplied scoring facts.
    pub losses: u32,
    /// Current consecutive-win count.
    pub current_streak: u32,
    /// Whether fire is enabled and the current streak has reached two wins.
    pub on_fire: bool,
    /// Largest consecutive-win count reached.
    pub best_streak: u32,
    /// Declarations in supplied played games, whether or not they earned a bonus.
    pub berserks: u32,
}

pub(super) fn compute_standing_metrics(
    config: &ArenaConfig,
    players: &[ArenaPlayerProjection],
) -> Vec<ArenaStandingMetrics> {
    players
        .iter()
        .enumerate()
        .map(|(index, player)| ArenaStandingMetrics {
            player_id: PlayerId::new(index),
            points: Score::new(player.points),
            performance: (player.performance_games != 0).then_some(ArenaPerformance {
                numerator: player.performance_sum,
                denominator: player.performance_games,
            }),
            arena_rating: player.arena_rating,
            games_scored: player.games_scored,
            games_played: player.games_played,
            no_starts: player.no_starts,
            wins: player.wins,
            draws: player.draws,
            losses: player.losses,
            current_streak: player.streak,
            on_fire: is_on_fire(config, player.streak),
            best_streak: player.best_streak,
            berserks: player.berserks,
        })
        .collect()
}

fn secondary(player: &ArenaPlayerProjection) -> Option<(i64, u32)> {
    if player.performance_games != 0 {
        Some((player.performance_sum, player.performance_games))
    } else {
        player.arena_rating.map(|rating| (i64::from(rating), 1))
    }
}

/// Compares signed exact ratios without cross multiplication.
fn compare_ratio(left: (i64, u32), right: (i64, u32)) -> Ordering {
    let (left_numerator, left_denominator) = left;
    let (right_numerator, right_denominator) = right;
    debug_assert!(left_denominator != 0 && right_denominator != 0);

    let left_denominator = i64::from(left_denominator);
    let right_denominator = i64::from(right_denominator);
    let left_integer = left_numerator.div_euclid(left_denominator);
    let right_integer = right_numerator.div_euclid(right_denominator);
    match left_integer.cmp(&right_integer) {
        Ordering::Equal => compare_unsigned_ratio(
            left_numerator.rem_euclid(left_denominator) as u64,
            left_denominator as u64,
            right_numerator.rem_euclid(right_denominator) as u64,
            right_denominator as u64,
        ),
        ordering => ordering,
    }
}

/// Continued-fraction comparison avoids overflowing on synthetic large input.
fn compare_unsigned_ratio(
    mut left_numerator: u64,
    mut left_denominator: u64,
    mut right_numerator: u64,
    mut right_denominator: u64,
) -> Ordering {
    let mut reversed = false;
    loop {
        let left_integer = left_numerator / left_denominator;
        let right_integer = right_numerator / right_denominator;
        if left_integer != right_integer {
            let ordering = left_integer.cmp(&right_integer);
            return if reversed {
                ordering.reverse()
            } else {
                ordering
            };
        }

        let left_remainder = left_numerator % left_denominator;
        let right_remainder = right_numerator % right_denominator;
        if left_remainder == 0 || right_remainder == 0 {
            let ordering = left_remainder.cmp(&right_remainder);
            return if reversed {
                ordering.reverse()
            } else {
                ordering
            };
        }

        left_numerator = left_denominator;
        left_denominator = left_remainder;
        right_numerator = right_denominator;
        right_denominator = right_remainder;
        reversed = !reversed;
    }
}

/// Arena ranking is fixed: points, then performance or current Arena rating.
/// Missing secondary values sort behind present values and remain tied with
/// each other. Dense player-id order is deliberately absent from this comparison.
pub(super) fn compare_rank(
    left: &ArenaPlayerProjection,
    right: &ArenaPlayerProjection,
) -> Ordering {
    right
        .points
        .cmp(&left.points)
        .then_with(|| match (secondary(left), secondary(right)) {
            (Some(left), Some(right)) => compare_ratio(right, left),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        })
}

pub(super) fn sorted_players(players: &[ArenaPlayerProjection]) -> Vec<PlayerId> {
    let mut sorted: Vec<_> = (0..players.len()).map(PlayerId::new).collect();
    sorted.sort_by(|&left, &right| {
        compare_rank(&players[left.index()], &players[right.index()])
            .then_with(|| left.index().cmp(&right.index()))
    });
    sorted
}

pub(super) fn rank_standings(
    players: &[ArenaPlayerProjection],
) -> Result<StandingsTable<(), ()>, ArenaError> {
    let mut groups: Vec<StandingGroup<()>> = Vec::new();
    for (position, player) in sorted_players(players).into_iter().enumerate() {
        match groups.last_mut() {
            Some(group)
                if compare_rank(
                    &players[group.players[0].player.index()],
                    &players[player.index()],
                ) == Ordering::Equal =>
            {
                group.players.push(StandingRow {
                    player,
                    values: Vec::new(),
                });
            }
            _ => groups.push(StandingGroup {
                rank: u32::try_from(position)
                    .ok()
                    .and_then(|position| position.checked_add(1))
                    .ok_or(ArenaError::ArithmeticOverflow)?,
                separated_by: None,
                players: vec![StandingRow {
                    player,
                    values: Vec::new(),
                }],
            }),
        }
    }
    Ok(StandingsTable {
        columns: Vec::new(),
        groups,
    })
}

/// Competition rank indexed by participant. Every exact tie receives the
/// same rank, so pairing and displayed standings cannot disagree.
pub(super) fn competition_ranks(players: &[ArenaPlayerProjection]) -> Vec<usize> {
    let sorted = sorted_players(players);
    let mut ranks = vec![0; players.len()];
    let mut rank = 0;
    for (position, &player) in sorted.iter().enumerate() {
        if position != 0
            && compare_rank(
                &players[sorted[position - 1].index()],
                &players[player.index()],
            ) != Ordering::Equal
        {
            rank = position;
        }
        ranks[player.index()] = rank;
    }
    ranks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_signed_ratios_compare_without_multiplication() {
        assert_eq!(compare_ratio((-1, 2), (-2, 3)), Ordering::Greater);
        assert_eq!(compare_ratio((3, 2), (6, 4)), Ordering::Equal);
    }
}
