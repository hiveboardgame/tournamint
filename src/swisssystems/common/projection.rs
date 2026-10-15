use crate::domain::{Color, PlayerColorData, Score};
use crate::limits::WrappingLimit;
use crate::swiss::{PlayerRoundRecord, SwissPairingSnapshot};

use super::colors::NeutralColorInput;

/// The read-only view of a player that every Swiss system pairs from - built
/// once from the typed pairing snapshot, then shared by Dutch, Burstein and
/// Double-Swiss so none of them reaches into the snapshot directly.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SwissPlayerProjection<'a> {
    pub(crate) id: usize,
    pub(crate) rank_index: usize,
    pub(crate) score_without_acceleration: usize,
    pub(crate) played_games: usize,
    pub(crate) played_matches: usize,
    pub(crate) color_data: PlayerColorData,
    pub(crate) is_valid: bool,
    pub(crate) records: &'a [PlayerRoundRecord],
    pub(crate) accelerations: &'a [Score],
}

impl SwissPlayerProjection<'_> {
    pub(crate) fn neutral_color_input(
        self,
        first_difference_color: Option<Color>,
    ) -> NeutralColorInput {
        NeutralColorInput {
            color_data: self.color_data,
            first_difference_color,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum BuildSwissPlayerProjectionsError {
    #[error("malformed pairing snapshot")]
    MalformedTournament,
}

pub(crate) fn build_swiss_player_projections(
    snapshot: &SwissPairingSnapshot,
) -> Result<Vec<SwissPlayerProjection<'_>>, BuildSwissPlayerProjectionsError> {
    let snapshot_players = snapshot.players();
    let players_len = snapshot_players.len();
    let mut players = Vec::with_capacity(players_len);

    for player_entry in snapshot_players {
        let player_id = player_entry.id.index();
        if player_id >= players_len {
            return Err(BuildSwissPlayerProjectionsError::MalformedTournament);
        }

        players.push(SwissPlayerProjection {
            id: player_id,
            rank_index: player_entry.rank_index,
            score_without_acceleration: player_entry.score_without_acceleration.value() as usize,
            played_games: player_entry.played_games,
            played_matches: player_entry.played_matches,
            color_data: player_entry.color_data,
            is_valid: player_entry.is_valid,
            records: player_entry.records,
            accelerations: player_entry.accelerations,
        });
    }

    Ok(players)
}

pub(crate) fn player_acceleration_at_round(
    player: SwissPlayerProjection<'_>,
    round_index: usize,
) -> usize {
    player
        .accelerations
        .get(round_index)
        .map_or(0, |acceleration| acceleration.value() as usize)
}

pub(crate) fn checked_accelerated_score(
    score_without_acceleration: usize,
    acceleration: usize,
    max_points: usize,
) -> Option<usize> {
    WrappingLimit::from_type_max(max_points).checked_add(score_without_acceleration, acceleration)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::PlayerId;
    use crate::swiss::SwissPairingPlayerEntry;

    #[test]
    fn player_acceleration_at_round_uses_player_slice() {
        let accelerations = [Score::new(7), Score::new(3)];
        let mut snapshot = SwissPairingSnapshot::new();
        snapshot.push_player(SwissPairingPlayerEntry {
            id: PlayerId::new(0),
            score_without_acceleration: Score::new(0),
            rank_index: 0,
            played_games: 0,
            played_matches: 0,
            color_data: PlayerColorData::no_played_color_history(),
            is_valid: true,
            records: &[],
            accelerations: &accelerations,
        });
        let player = build_swiss_player_projections(&snapshot).unwrap()[0];

        assert_eq!(player_acceleration_at_round(player, 0), 7);
        assert_eq!(player_acceleration_at_round(player, 1), 3);
        assert_eq!(player_acceleration_at_round(player, 2), 0);
        assert_eq!(player_acceleration_at_round(player, usize::MAX), 0);
    }
}
