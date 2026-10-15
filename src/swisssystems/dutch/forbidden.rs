use super::input::DutchInput;
use super::rules::{
    DutchCompatibilityInput, DutchCompatibilityKind, DutchCompatibilityScores, dutch_compatible,
    dutch_is_bye_candidate,
};
use crate::domain::{PointSystem, Score};
use crate::swisssystems::bracketing::BracketPlayer;
use crate::swisssystems::bracketing::BracketStatus;
use crate::swisssystems::common::{
    BuildForbiddenPairMatrixError, ForbiddenPairMatrix, ForbiddenPairPlayer,
    build_forbidden_pair_matrix, eligible_for_single_game_pairing_allocated_bye,
};

pub(super) type ForbiddenPairs = ForbiddenPairMatrix;

pub(super) fn build_forbidden_pairs(
    input: &DutchInput<'_>,
    players: &[BracketPlayer<'_>],
) -> Result<ForbiddenPairs, BracketStatus> {
    build_forbidden_pair_matrix(
        input.pairing.tournament.forbidden_pairs(),
        input.pairing.played_rounds,
        players.len(),
        players.iter().map(|player| ForbiddenPairPlayer {
            id: player.id,
            is_valid: player.is_valid,
            records: player.records,
        }),
    )
    .map_err(|error| match error {
        BuildForbiddenPairMatrixError::MalformedTournament => BracketStatus::MalformedTournament,
    })
}

pub(super) fn eligible_for_bye(player: &BracketPlayer<'_>, point_system: PointSystem) -> bool {
    eligible_for_single_game_pairing_allocated_bye(player.records, point_system)
}

pub(super) fn is_bye_candidate(
    eligible_for_bye: bool,
    player_score_with_acceleration: usize,
    bye_assignee_score: usize,
) -> bool {
    dutch_is_bye_candidate(
        eligible_for_bye,
        player_score_with_acceleration,
        bye_assignee_score,
    )
}

pub(super) fn compatible(
    input: &DutchInput<'_>,
    players: &[BracketPlayer<'_>],
    forbidden_pairs: &ForbiddenPairs,
    player0_index: usize,
    player1_index: usize,
) -> bool {
    let player0 = players[player0_index];
    let player1 = players[player1_index];
    let player0_color = player0.color_data;
    let player1_color = player1.color_data;
    let forbidden_pair = forbidden_pairs.contains(player0.id, player1.id);
    matches!(
        dutch_compatible(DutchCompatibilityInput {
            forbidden_pair,
            player0: player0_color,
            player1: player1_color,
            played_rounds: input.pairing.played_rounds,
            expected_rounds: input.expected_rounds,
            point_system: input.pairing.point_system,
            scores: DutchCompatibilityScores {
                // Dutch C3 defines a topscorer from real standings points.
                // Virtual acceleration still controls scoregroups and order,
                // but cannot create this colour-exception eligibility.
                player0_real: Score::new(
                    u32::try_from(player0.score_without_acceleration)
                        .expect("validated Dutch score fits the public score domain"),
                ),
                player1_real: Score::new(
                    u32::try_from(player1.score_without_acceleration)
                        .expect("validated Dutch score fits the public score domain"),
                ),
            },
        }),
        DutchCompatibilityKind::Compatible
    )
}
