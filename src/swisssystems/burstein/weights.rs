use crate::matching::{MatchingError, MatchingSolver};
use crate::swisssystems::common::{ForbiddenPairMatrix, color_preferences_are_compatible};

use super::errors::BursteinStatus;
use super::player::BursteinPlayer;

pub(super) fn burstein_players_are_compatible(
    players: &[BursteinPlayer<'_>],
    forbidden_pairs: &ForbiddenPairMatrix,
    player0_index: usize,
    player1_index: usize,
) -> bool {
    let player0 = players[player0_index];
    let player1 = players[player1_index];
    let player0_color = player0.base.color_data;
    let player1_color = player1.base.color_data;

    !forbidden_pairs.contains(player0.base.id, player1.base.id)
        && !(player0_color.has_absolute_preference()
            && player1_color.has_absolute_preference()
            && player0_color.color_preference == player1_color.color_preference)
}

pub(super) fn burstein_pair_grants_both_color_preferences(
    players: &[BursteinPlayer<'_>],
    player0_index: usize,
    player1_index: usize,
) -> bool {
    color_preferences_are_compatible(
        players[player0_index].base.color_data.color_preference,
        players[player1_index].base.color_data.color_preference,
    )
}

pub(super) fn set_fixed_edge_weight(
    solver: &mut MatchingSolver,
    vertex0: usize,
    vertex1: usize,
    edge_weight: usize,
) -> Result<(), BursteinStatus> {
    match solver.set_fixed_edge_weight(vertex0, vertex1, edge_weight) {
        Ok(()) => Ok(()),
        Err(MatchingError::InvalidEdge) => Err(BursteinStatus::MalformedTournament),
    }
}

#[cfg(test)]
mod tests {
    use super::burstein_pair_grants_both_color_preferences;
    use crate::domain::{Color, PlayerColorData};
    use crate::swisssystems::burstein::player::BursteinPlayer;
    use crate::swisssystems::common::SwissPlayerProjection;

    fn player(color_preference: Option<Color>) -> BursteinPlayer<'static> {
        BursteinPlayer {
            base: SwissPlayerProjection {
                id: 0,
                rank_index: 0,
                score_without_acceleration: 0,
                played_games: 0,
                played_matches: 0,
                color_data: PlayerColorData {
                    color_imbalance: 0,
                    color_preference,
                    repeated_color: None,
                    strong_color_preference: false,
                },
                is_valid: true,
                records: &[],
                accelerations: &[],
            },
            score_with_acceleration: 0,
        }
    }

    #[test]
    fn opposite_or_absent_preferences_can_both_be_granted() {
        let players = [
            player(Some(Color::White)),
            player(Some(Color::Black)),
            player(None),
        ];

        assert!(burstein_pair_grants_both_color_preferences(&players, 0, 1));
        assert!(burstein_pair_grants_both_color_preferences(&players, 0, 2));
        assert!(burstein_pair_grants_both_color_preferences(&players, 1, 2));
    }

    #[test]
    fn equal_nonabsolute_preferences_force_one_denial() {
        let players = [player(Some(Color::White)), player(Some(Color::White))];

        assert!(!burstein_pair_grants_both_color_preferences(&players, 0, 1));
    }
}
