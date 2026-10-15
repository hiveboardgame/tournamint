use super::errors::BursteinStatus;
use super::input::BursteinInput;
use super::player::BursteinPlayer;
use crate::swisssystems::common::{
    BuildForbiddenPairMatrixError, ForbiddenPairMatrix, ForbiddenPairPlayer,
    build_forbidden_pair_matrix,
};

pub(super) fn build_forbidden_pairs(
    input: &BursteinInput<'_>,
    players: &[BursteinPlayer<'_>],
) -> Result<ForbiddenPairMatrix, BursteinStatus> {
    build_forbidden_pair_matrix(
        input.pairing.tournament.forbidden_pairs(),
        input.pairing.played_rounds,
        players.len(),
        players.iter().map(|player| ForbiddenPairPlayer {
            id: player.base.id,
            is_valid: player.base.is_valid,
            records: player.base.records,
        }),
    )
    .map_err(|error| match error {
        BuildForbiddenPairMatrixError::MalformedTournament => BursteinStatus::MalformedTournament,
    })
}
