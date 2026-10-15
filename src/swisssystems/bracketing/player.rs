use crate::swiss::SwissPairingSnapshot;
use crate::swisssystems::common::{
    BuildSwissPlayerProjectionsError, SwissPlayerProjection, build_swiss_player_projections,
};

use super::status::BracketStatus;

pub(crate) type BracketPlayer<'a> = SwissPlayerProjection<'a>;

pub(crate) fn build_bracket_players(
    tournament: &SwissPairingSnapshot,
) -> Result<Vec<BracketPlayer<'_>>, BracketStatus> {
    build_swiss_player_projections(tournament).map_err(|error| match error {
        BuildSwissPlayerProjectionsError::MalformedTournament => BracketStatus::MalformedTournament,
    })
}

/// One player's place in the bracket being built: which player, and which
/// vertex of the matching graph represents them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BracketSlot {
    pub(crate) player_index: usize,
    pub(crate) vertex: usize,
}

impl BracketSlot {
    pub(crate) fn new(player_index: usize, vertex: usize) -> Self {
        Self {
            player_index,
            vertex,
        }
    }
}
