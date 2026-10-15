use crate::domain::PointSystem;
use crate::matching::MatchingSolver;
use crate::swisssystems::common::{
    ForbiddenPairMatrix, eligible_for_single_game_pairing_allocated_bye,
};

use super::errors::BursteinStatus;
use super::input::BursteinInput;
use super::metrics::{BursteinMetricScores, metric_scores_order};
use super::player::BursteinPlayer;
use super::weights::{burstein_players_are_compatible, set_fixed_edge_weight};

pub(super) fn eligible_for_burstein_bye(
    player: &BursteinPlayer<'_>,
    point_system: PointSystem,
) -> bool {
    eligible_for_single_game_pairing_allocated_bye(player.base.records, point_system)
}

fn allows_complete_pairing(
    players: &[BursteinPlayer<'_>],
    forbidden_pairs: &ForbiddenPairMatrix,
    sorted_players: &[usize],
    bye_candidate: usize,
) -> Result<bool, BursteinStatus> {
    let remaining_len = sorted_players
        .len()
        .checked_sub(1)
        .ok_or(BursteinStatus::MalformedTournament)?;
    let mut remaining = Vec::with_capacity(remaining_len);
    remaining.extend(
        sorted_players
            .iter()
            .copied()
            .filter(|player| *player != bye_candidate),
    );
    if remaining.len() != remaining_len {
        return Err(BursteinStatus::MalformedTournament);
    }
    if remaining.len() & 1 != 0 {
        return Ok(false);
    }
    let mut solver = MatchingSolver::with_vertices(remaining.len());
    for left in 0..remaining.len() {
        for right in left + 1..remaining.len() {
            if burstein_players_are_compatible(
                players,
                forbidden_pairs,
                remaining[left],
                remaining[right],
            ) {
                set_fixed_edge_weight(&mut solver, left, right, 1)?;
            }
        }
    }
    solver.compute_matching();
    let matching = solver
        .current_matching()
        .ok_or(BursteinStatus::MalformedTournament)?;
    Ok(matching
        .iter()
        .enumerate()
        .all(|(vertex, opponent)| vertex != *opponent))
}

/// FIDE C.04.4.2 Article 3.1: eligibility, complete-pairing feasibility,
/// lowest score, most games played, then lowest pairing ranking.
pub(super) fn choose_bye(
    input: &BursteinInput<'_>,
    players: &[BursteinPlayer<'_>],
    metric_scores: &[BursteinMetricScores],
    forbidden_pairs: &ForbiddenPairMatrix,
    sorted_players: &mut Vec<usize>,
) -> Result<Option<usize>, BursteinStatus> {
    if sorted_players.len() & 1 == 0 {
        return Ok(None);
    }

    let mut candidates = Vec::with_capacity(sorted_players.len());
    candidates.extend(
        sorted_players.iter().copied().filter(|player| {
            eligible_for_burstein_bye(&players[*player], input.pairing.point_system)
        }),
    );
    candidates.sort_unstable_by(|left, right| {
        players[*left]
            .score_with_acceleration
            .cmp(&players[*right].score_with_acceleration)
            .then_with(|| {
                players[*right]
                    .base
                    .played_games
                    .cmp(&players[*left].base.played_games)
            })
            // Lower pairing rank means the smaller Index, then larger TPN.
            .then_with(|| {
                metric_scores_order(
                    metric_scores[players[*left].base.id],
                    metric_scores[players[*right].base.id],
                )
            })
            .then_with(|| players[*left].base.id.cmp(&players[*right].base.id))
    });

    for candidate in candidates {
        if allows_complete_pairing(players, forbidden_pairs, sorted_players, candidate)? {
            let position = sorted_players
                .iter()
                .position(|player| *player == candidate)
                .ok_or(BursteinStatus::MalformedTournament)?;
            return Ok(Some(sorted_players.remove(position)));
        }
    }
    Ok(None)
}
