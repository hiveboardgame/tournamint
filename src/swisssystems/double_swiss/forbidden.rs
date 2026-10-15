use super::input::DoubleSwissInput;
use crate::swisssystems::bracketing::BracketPlayer;
use crate::swisssystems::bracketing::BracketStatus;
use crate::swisssystems::common::{
    BuildForbiddenPairMatrixError, ForbiddenPairMatrix, ForbiddenPairPlayer,
    build_forbidden_pair_matrix, eligible_for_double_swiss_pairing_allocated_bye,
};

pub(super) type ForbiddenPairs = ForbiddenPairMatrix;

fn forbidden_pair_matrix_error(error: BuildForbiddenPairMatrixError) -> BracketStatus {
    match error {
        BuildForbiddenPairMatrixError::MalformedTournament => BracketStatus::MalformedTournament,
    }
}

pub(super) fn empty_forbidden_pairs(player_count: usize) -> Result<ForbiddenPairs, BracketStatus> {
    Ok(ForbiddenPairs::new(player_count))
}

pub(super) fn build_forbidden_pairs(
    input: &DoubleSwissInput<'_>,
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
    .map_err(forbidden_pair_matrix_error)
}

pub(super) fn eligible_for_bye(player: &BracketPlayer<'_>) -> bool {
    eligible_for_double_swiss_pairing_allocated_bye(player.records)
}

pub(super) fn compatible(
    players: &[BracketPlayer<'_>],
    forbidden_pairs: &ForbiddenPairs,
    player0_index: usize,
    player1_index: usize,
) -> bool {
    let player0 = players[player0_index];
    let player1 = players[player1_index];
    !forbidden_pairs.contains(player0.id, player1.id)
}
