use crate::domain::{PointSystem, Score};
use crate::swiss::{MatchDisposition, PlayerRoundRecord, SwissMatchRecord};

fn single_game_unplayed_award(
    record: PlayerRoundRecord,
    point_system: PointSystem,
) -> Option<Score> {
    match record {
        PlayerRoundRecord::PairingAllocatedBye => None,
        PlayerRoundRecord::RequestedBye | PlayerRoundRecord::Withdrawn => Some(Score::new(0)),
        PlayerRoundRecord::Match { state, .. } => {
            let games = state.games();
            if games.iter().any(|game| game.outcome.counts_as_played()) {
                None
            } else {
                games
                    .iter()
                    .next()
                    .map(|game| point_system.points_for(game.outcome))
            }
        }
    }
}

/// Dutch and Burstein pairing-allocated-bye eligibility.
///
/// General Swiss handling makes a player ineligible after either a previous
/// PAB or any unplayed result worth the same real score as a configured win.
/// This remains intentionally numeric for custom scoring.
pub(crate) fn eligible_for_single_game_pairing_allocated_bye(
    records: &[PlayerRoundRecord],
    point_system: PointSystem,
) -> bool {
    !records.iter().copied().any(|record| {
        matches!(record, PlayerRoundRecord::PairingAllocatedBye)
            || single_game_unplayed_award(record, point_system) == Some(point_system.win)
    })
}

/// Double-Swiss pairing-allocated-bye eligibility.
///
/// C.04.5 C2 is match-semantic: a previous PAB or a full match-forfeit win
/// consumes eligibility. Split and single-game forfeits remain regular.
pub(crate) fn eligible_for_double_swiss_pairing_allocated_bye(
    records: &[PlayerRoundRecord],
) -> bool {
    !records.iter().any(|record| {
        matches!(
            record,
            PlayerRoundRecord::PairingAllocatedBye
                | PlayerRoundRecord::Match {
                    state: SwissMatchRecord::Complete {
                        disposition: MatchDisposition::ForfeitWin,
                        ..
                    },
                    ..
                }
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AdjudicatedSideResult, Color, GameSideOutcome, MatchScore, PlayerId};
    use crate::swiss::{LogicalGame, LogicalGames, SwissMatchRecord};

    const POINTS: PointSystem = PointSystem {
        win: Score::new(2),
        draw: Score::new(1),
        loss: Score::new(0),
        forfeit_loss: Score::new(0),
    };

    fn match_record(outcome: GameSideOutcome, disposition: MatchDisposition) -> PlayerRoundRecord {
        PlayerRoundRecord::Match {
            opponent: PlayerId::new(1),
            scheduled_first_color: Color::White,
            state: SwissMatchRecord::Complete {
                games: LogicalGames::single(LogicalGame {
                    color: Color::White,
                    outcome,
                }),
                disposition,
                aggregate: MatchScore::Win,
            },
        }
    }

    #[test]
    fn explicit_pab_and_full_forfeit_win_consume_native_eligibility() {
        assert!(!eligible_for_single_game_pairing_allocated_bye(
            &[PlayerRoundRecord::PairingAllocatedBye],
            POINTS,
        ));
        assert!(!eligible_for_double_swiss_pairing_allocated_bye(&[
            match_record(
                GameSideOutcome::Adjudicated(AdjudicatedSideResult::ForfeitWin),
                MatchDisposition::ForfeitWin,
            )
        ]));
    }

    #[test]
    fn custom_win_equivalent_adjudicated_draw_consumes_single_game_eligibility() {
        let equal_win_draw = PointSystem {
            win: Score::new(2),
            draw: Score::new(2),
            ..POINTS
        };
        let draw = match_record(
            GameSideOutcome::Adjudicated(AdjudicatedSideResult::Draw),
            MatchDisposition::Regular,
        );
        assert!(!eligible_for_single_game_pairing_allocated_bye(
            &[draw],
            equal_win_draw,
        ));
        assert!(eligible_for_double_swiss_pairing_allocated_bye(&[draw]));
    }
}
