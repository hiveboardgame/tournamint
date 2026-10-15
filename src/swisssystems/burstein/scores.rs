use crate::domain::PointSystem;
use crate::swiss::PlayerRoundRecord;

use super::input::BursteinInput;
use super::player::BursteinPlayer;

pub(super) fn input_point_system(input: &BursteinInput<'_>) -> PointSystem {
    input.pairing.point_system
}

pub(super) fn burstein_adjusted_score_from_records(
    records: &[PlayerRoundRecord],
    player: BursteinPlayer<'_>,
    played_rounds: usize,
    point_system: PointSystem,
    adjusted_score_type_max: usize,
) -> Result<usize, super::errors::BursteinStatus> {
    // C.04.4.2 Article 1.7.2: start from the actual awarded score, excluding
    // acceleration. Only earlier members of a zero-bye sequence that still
    // reaches the current round are converted to draws for real opponents.
    let mut adjusted_score = player.base.score_without_acceleration;
    let limit = core::cmp::min(played_rounds, records.len());
    let mut trailing_zero_begin = limit;
    while trailing_zero_begin != 0 {
        let record = records
            .get(trailing_zero_begin - 1)
            .copied()
            .expect("Burstein adjusted score indexes within match history length");
        let is_zero_bye = matches!(
            record,
            PlayerRoundRecord::RequestedBye | PlayerRoundRecord::Withdrawn
        );
        if !is_zero_bye {
            break;
        }
        trailing_zero_begin = trailing_zero_begin
            .checked_sub(1)
            .ok_or(super::errors::BursteinStatus::MalformedTournament)?;
    }
    let trailing_zero_byes = limit
        .checked_sub(trailing_zero_begin)
        .ok_or(super::errors::BursteinStatus::MalformedTournament)?;
    let prior_trailing_zero_byes = if trailing_zero_byes == 0 {
        0
    } else {
        trailing_zero_byes
            .checked_sub(1)
            .ok_or(super::errors::BursteinStatus::MalformedTournament)?
    };
    for _ in 0..prior_trailing_zero_byes {
        adjusted_score = adjusted_score
            .checked_add(point_system.draw.value() as usize)
            .filter(|score| *score <= adjusted_score_type_max)
            .ok_or(super::errors::BursteinStatus::LimitAdjustedScore)?;
    }

    Ok(adjusted_score)
}

pub(super) fn compute_adjusted_scores(
    input: &BursteinInput<'_>,
    players: &[BursteinPlayer<'_>],
) -> Result<Vec<usize>, super::errors::BursteinStatus> {
    let point_system = input_point_system(input);
    let mut adjusted_scores = Vec::with_capacity(players.len());
    for player in players {
        // C.04.4.2 Articles 1.7-1.8 use every opponent's current adjusted
        // score. Pairing eligibility is deliberately irrelevant here: an
        // opponent withdrawing or sitting out the current round must not
        // retroactively turn earlier opponents' Index contribution into zero.
        adjusted_scores.push(burstein_adjusted_score_from_records(
            player.base.records,
            *player,
            input.pairing.played_rounds,
            point_system,
            input.adjusted_score_type_max,
        )?);
    }
    Ok(adjusted_scores)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Color, GameSideOutcome, MatchScore, PlayerColorData, PlayerId, Score};
    use crate::swiss::{
        LogicalGame, LogicalGames, MatchDisposition, SwissMatchRecord, SwissPairingSnapshot,
    };
    use crate::swisssystems::common::{PairingContext, SwissPlayerProjection};

    fn zero_bye() -> PlayerRoundRecord {
        PlayerRoundRecord::RequestedBye
    }

    fn played_win() -> PlayerRoundRecord {
        PlayerRoundRecord::Match {
            opponent: PlayerId::new(1),
            scheduled_first_color: Color::White,
            state: SwissMatchRecord::Complete {
                games: LogicalGames::single(LogicalGame {
                    color: Color::White,
                    outcome: GameSideOutcome::Played(MatchScore::Win),
                }),
                disposition: MatchDisposition::Regular,
                aggregate: MatchScore::Win,
            },
        }
    }

    fn player_with_validity<'a>(
        records: &'a [PlayerRoundRecord],
        score: usize,
        is_valid: bool,
    ) -> BursteinPlayer<'a> {
        BursteinPlayer {
            base: SwissPlayerProjection {
                id: 0,
                rank_index: 0,
                score_without_acceleration: score,
                played_games: records
                    .iter()
                    .flat_map(|record| record.games().iter())
                    .filter(|game| game.outcome.counts_as_played())
                    .count(),
                played_matches: records
                    .iter()
                    .filter(|record| {
                        matches!(
                            record,
                            PlayerRoundRecord::Match {
                                state: SwissMatchRecord::Complete {
                                    disposition: MatchDisposition::Regular,
                                    ..
                                },
                                ..
                            }
                        )
                    })
                    .count(),
                color_data: PlayerColorData::no_played_color_history(),
                is_valid,
                records,
                accelerations: &[],
            },
            score_with_acceleration: score,
        }
    }

    fn player<'a>(records: &'a [PlayerRoundRecord], score: usize) -> BursteinPlayer<'a> {
        player_with_validity(records, score, true)
    }

    fn input(snapshot: &SwissPairingSnapshot, played_rounds: usize) -> BursteinInput<'_> {
        BursteinInput {
            pairing: PairingContext {
                tournament: snapshot,
                played_rounds,
                point_system: PointSystem::STANDARD,
                initial_color: Color::White,
            },
            max_points: Score::new(u32::MAX),
            adjusted_score_type_max: usize::MAX,
            metric_type_max: usize::MAX,
        }
    }

    #[test]
    fn adjusted_scores_include_players_who_are_not_currently_pairable() {
        let snapshot = SwissPairingSnapshot::new();
        let records = [];
        let withdrawn_or_sitting_out = player_with_validity(&records, 7, false);

        assert_eq!(
            compute_adjusted_scores(&input(&snapshot, 0), &[withdrawn_or_sitting_out]),
            Ok(vec![7])
        );
    }

    #[test]
    fn only_previous_members_of_the_trailing_zero_bye_sequence_become_draws() {
        let played = played_win();
        let trailing = [played, zero_bye(), zero_bye()];
        assert_eq!(
            burstein_adjusted_score_from_records(
                &trailing,
                player(&trailing, 2),
                3,
                PointSystem::STANDARD,
                usize::MAX,
            ),
            Ok(3)
        );

        let interrupted = [zero_bye(), played, zero_bye()];
        assert_eq!(
            burstein_adjusted_score_from_records(
                &interrupted,
                player(&interrupted, 2),
                3,
                PointSystem::STANDARD,
                usize::MAX,
            ),
            Ok(2)
        );
    }

    #[test]
    fn adjusted_score_equal_to_the_cap_succeeds() {
        let records = [zero_bye(), zero_bye()];
        assert_eq!(
            burstein_adjusted_score_from_records(
                &records,
                player(&records, 4),
                2,
                PointSystem::STANDARD,
                5,
            ),
            Ok(5)
        );
    }

    #[test]
    fn trailing_zero_adjustment_reports_the_exact_adjusted_score_limit() {
        let records = [zero_bye(), zero_bye()];
        let status = burstein_adjusted_score_from_records(
            &records,
            player(&records, usize::MAX),
            2,
            PointSystem::STANDARD,
            usize::MAX,
        )
        .expect_err("one draw point cannot be added to usize::MAX");
        assert_eq!(
            status,
            super::super::errors::BursteinStatus::LimitAdjustedScore
        );
        assert_eq!(
            super::super::errors::compute_matching_error_from_status(status),
            crate::swiss::PairingError::LimitExceeded(crate::swiss::PairingLimit::AdjustedScore)
                .into()
        );
    }
}
