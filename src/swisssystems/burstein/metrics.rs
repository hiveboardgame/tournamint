use core::cmp::Ordering;

use crate::swiss::PlayerRoundRecord;

use super::errors::BursteinStatus;
use super::input::BursteinInput;
use super::player::BursteinPlayer;

#[derive(Clone, Copy)]
pub(super) struct BursteinMetricScores {
    pub(super) buchholz: usize,
    pub(super) sonneborn_berger: usize,
    pub(super) rank_index: usize,
}

struct BursteinMetricContext<'a> {
    input: &'a BursteinInput<'a>,
    adjusted_scores: &'a [usize],
    played_rounds: usize,
    metric_type_max: usize,
}

fn checked_add(
    left: usize,
    right: usize,
    maximum: usize,
    exceeded: BursteinStatus,
) -> Result<usize, BursteinStatus> {
    let value = (left as u128) + (right as u128);
    if value > maximum as u128 {
        Err(exceeded)
    } else {
        Ok(value as usize)
    }
}

fn checked_product(left: usize, right: usize, maximum: usize) -> Result<usize, BursteinStatus> {
    let value = (left as u128) * (right as u128);
    if value > maximum as u128 {
        Err(BursteinStatus::LimitSonnebornBergerProduct)
    } else {
        Ok(value as usize)
    }
}

fn opponent_score(
    player: BursteinPlayer<'_>,
    record: PlayerRoundRecord,
    context: &BursteinMetricContext<'_>,
) -> Result<usize, BursteinStatus> {
    if let PlayerRoundRecord::Match {
        opponent, state, ..
    } = record
        && state
            .games()
            .iter()
            .any(|game| game.outcome.counts_as_played())
    {
        return context
            .adjusted_scores
            .get(opponent.index())
            .copied()
            .ok_or(BursteinStatus::MalformedTournament);
    }
    // C.04.4.2 Article 1.7.2.1: an unplayed round is treated as a game
    // against oneself with the actual awarded result. The player's real
    // current score excludes pairing-only acceleration.
    Ok(player.base.score_without_acceleration)
}

fn calculate_metrics(
    records: &[PlayerRoundRecord],
    player: BursteinPlayer<'_>,
    context: &BursteinMetricContext<'_>,
) -> Result<(usize, usize), BursteinStatus> {
    if !player.base.is_valid {
        return Ok((0, 0));
    }
    let mut buchholz = 0usize;
    let mut sonneborn_berger = 0usize;
    let limit = context.played_rounds.min(records.len());
    for index in 0..limit {
        let record = records
            .get(index)
            .copied()
            .ok_or(BursteinStatus::MalformedTournament)?;
        let opponent_score = opponent_score(player, record, context)?;
        buchholz = checked_add(
            buchholz,
            opponent_score,
            context.metric_type_max,
            BursteinStatus::LimitBuchholzTotal,
        )?;
        let primary_points = context
            .input
            .pairing
            .tournament
            .primary_points(record, context.input.pairing.point_system)
            .ok_or(BursteinStatus::MalformedTournament)?;
        let product = checked_product(
            primary_points.value() as usize,
            opponent_score,
            context.metric_type_max,
        )?;
        sonneborn_berger = checked_add(
            sonneborn_berger,
            product,
            context.metric_type_max,
            BursteinStatus::LimitSonnebornBergerTotal,
        )?;
    }
    Ok((buchholz, sonneborn_berger))
}

/// C.04.4.2 Articles 1.7–1.8: the pairing Index is BH, then SB, then
/// ascending TPN. Player score and Median are deliberately absent.
pub(super) fn metric_scores_order(
    player: BursteinMetricScores,
    opponent: BursteinMetricScores,
) -> Ordering {
    player
        .buchholz
        .cmp(&opponent.buchholz)
        .then_with(|| player.sonneborn_berger.cmp(&opponent.sonneborn_berger))
        .then_with(|| opponent.rank_index.cmp(&player.rank_index))
}

pub(super) fn metric_scores_less_than(
    player: BursteinMetricScores,
    opponent: BursteinMetricScores,
) -> bool {
    metric_scores_order(player, opponent) == Ordering::Less
}

pub(super) fn compute_metric_scores(
    input: &BursteinInput<'_>,
    players: &[BursteinPlayer<'_>],
    adjusted_scores: &[usize],
) -> Result<Vec<BursteinMetricScores>, BursteinStatus> {
    let context = BursteinMetricContext {
        input,
        adjusted_scores,
        played_rounds: input.pairing.played_rounds,
        metric_type_max: input.metric_type_max,
    };
    let mut metric_scores = Vec::with_capacity(players.len());
    for player in players {
        let (buchholz, sonneborn_berger) =
            calculate_metrics(player.base.records, *player, &context)?;
        metric_scores.push(BursteinMetricScores {
            buchholz,
            sonneborn_berger,
            rank_index: player.base.rank_index,
        });
    }
    Ok(metric_scores)
}

pub(super) fn sort_burstein_players(
    sorted_players: &mut [usize],
    players: &[BursteinPlayer<'_>],
    metric_scores: &[BursteinMetricScores],
) {
    sorted_players.sort_unstable_by(|&player0, &player1| {
        let score0 = players[player0].score_with_acceleration;
        let score1 = players[player1].score_with_acceleration;
        if score0 != score1 {
            return score1.cmp(&score0);
        }
        metric_scores_order(
            metric_scores[players[player1].base.id],
            metric_scores[players[player0].base.id],
        )
        .then_with(|| players[player0].base.id.cmp(&players[player1].base.id))
    });
}

pub(super) fn sorted_pairable_players(
    input: &BursteinInput<'_>,
    players: &[BursteinPlayer<'_>],
    metric_scores: &[BursteinMetricScores],
) -> Result<Vec<usize>, BursteinStatus> {
    let mut sorted_players = Vec::with_capacity(players.len());
    for (player_index, player) in players.iter().enumerate() {
        if player.base.is_valid && player.base.records.len() <= input.pairing.played_rounds {
            sorted_players.push(player_index);
        }
    }
    sort_burstein_players(&mut sorted_players, players, metric_scores);
    Ok(sorted_players)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        Color, GameSideOutcome, MatchScore, PlayerColorData, PlayerId, PointSystem, Score,
    };
    use crate::swiss::{
        LogicalGame, LogicalGames, MatchDisposition, PairingError, PairingLimit, SwissMatchRecord,
        SwissPairingSnapshot,
    };
    use crate::swisssystems::burstein::errors::compute_matching_error_from_status;
    use crate::swisssystems::common::{PairingContext, SwissPlayerProjection};

    fn played_win(opponent: usize) -> PlayerRoundRecord {
        PlayerRoundRecord::Match {
            opponent: PlayerId::new(opponent),
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

    fn player<'a>(
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
                    .filter(|record| matches!(record, PlayerRoundRecord::Match { .. }))
                    .count(),
                color_data: PlayerColorData::no_played_color_history(),
                is_valid,
                records,
                accelerations: &[],
            },
            score_with_acceleration: score,
        }
    }

    fn input(
        snapshot: &SwissPairingSnapshot,
        played_rounds: usize,
        point_system: PointSystem,
        metric_type_max: usize,
    ) -> BursteinInput<'_> {
        BursteinInput {
            pairing: PairingContext {
                tournament: snapshot,
                played_rounds,
                point_system,
                initial_color: Color::White,
            },
            max_points: Score::new(u32::MAX),
            adjusted_score_type_max: usize::MAX,
            metric_type_max,
        }
    }

    fn assert_exact_limit(status: BursteinStatus, limit: PairingLimit) {
        assert_eq!(
            compute_matching_error_from_status(status),
            PairingError::LimitExceeded(limit).into()
        );
    }

    #[test]
    fn metric_resources_equal_to_the_cap_succeed() {
        let snapshot = SwissPairingSnapshot::new();
        let records = [played_win(1)];
        let point_system = PointSystem {
            win: Score::new(5),
            draw: Score::new(0),
            loss: Score::new(0),
            forfeit_loss: Score::new(0),
        };
        let scores = compute_metric_scores(
            &input(&snapshot, 1, point_system, 10),
            &[player(&records, 0, true)],
            &[0, 2],
        )
        .expect("BH, one SB product, and the SB total fit exactly at the cap");
        assert_eq!(scores[0].buchholz, 2);
        assert_eq!(scores[0].sonneborn_berger, 10);

        let virtual_records = [PlayerRoundRecord::RequestedBye];
        let virtual_scores = compute_metric_scores(
            &input(&snapshot, 1, point_system, 10),
            &[player(&virtual_records, 10, true)],
            &[999],
        )
        .expect("the virtual opponent's own score fits at the BH cap");
        assert_eq!(virtual_scores[0].buchholz, 10);
        assert_eq!(virtual_scores[0].sonneborn_berger, 0);
    }

    #[test]
    fn buchholz_addition_reports_only_the_buchholz_total_limit() {
        let snapshot = SwissPairingSnapshot::new();
        let records = [PlayerRoundRecord::RequestedBye];
        let status = compute_metric_scores(
            &input(&snapshot, 1, PointSystem::STANDARD, 5),
            &[player(&records, 6, true)],
            &[0],
        )
        .err()
        .expect("the virtual-opponent BH contribution exceeds the cap");
        assert_eq!(status, BursteinStatus::LimitBuchholzTotal);
        assert_exact_limit(status, PairingLimit::BuchholzTotal);
    }

    #[test]
    fn sonneborn_berger_product_reports_the_exact_failing_operation() {
        let snapshot = SwissPairingSnapshot::new();
        let records = [played_win(1)];
        let point_system = PointSystem {
            win: Score::new(2),
            draw: Score::new(0),
            loss: Score::new(0),
            forfeit_loss: Score::new(0),
        };
        let status = compute_metric_scores(
            &input(&snapshot, 1, point_system, 5),
            &[player(&records, 0, true)],
            &[0, 3],
        )
        .err()
        .expect("BH fits, then the single 2 × 3 SB product first exceeds the shared cap");
        assert_eq!(status, BursteinStatus::LimitSonnebornBergerProduct);
        assert_exact_limit(status, PairingLimit::SonnebornBergerProduct);
    }

    #[test]
    fn sonneborn_berger_accumulation_reports_only_its_total_limit() {
        let snapshot = SwissPairingSnapshot::new();
        let records = [played_win(1), played_win(2)];
        let point_system = PointSystem {
            win: Score::new(3),
            draw: Score::new(0),
            loss: Score::new(0),
            forfeit_loss: Score::new(0),
        };
        let status = compute_metric_scores(
            &input(&snapshot, 2, point_system, 5),
            &[player(&records, 0, true)],
            &[0, 1, 1],
        )
        .err()
        .expect("both products and BH fit, but the second SB addition does not");
        assert_eq!(status, BursteinStatus::LimitSonnebornBergerTotal);
        assert_exact_limit(status, PairingLimit::SonnebornBergerTotal);
    }

    #[test]
    fn inactive_players_keep_zero_metrics_without_evaluating_records() {
        let snapshot = SwissPairingSnapshot::new();
        let records = [PlayerRoundRecord::RequestedBye];
        let scores = compute_metric_scores(
            &input(&snapshot, 1, PointSystem::STANDARD, 0),
            &[player(&records, usize::MAX, false)],
            &[0],
        )
        .expect("an inactive player's metrics remain zero");
        assert_eq!(scores[0].buchholz, 0);
        assert_eq!(scores[0].sonneborn_berger, 0);
    }

    #[test]
    fn pairing_index_is_buchholz_then_sonneborn_berger_then_ascending_tpn() {
        let lower_bh = BursteinMetricScores {
            buchholz: 10,
            sonneborn_berger: 999,
            rank_index: 0,
        };
        let higher_bh = BursteinMetricScores {
            buchholz: 11,
            sonneborn_berger: 0,
            rank_index: 99,
        };
        assert_eq!(metric_scores_order(higher_bh, lower_bh), Ordering::Greater);

        let higher_sb = BursteinMetricScores {
            buchholz: 11,
            sonneborn_berger: 2,
            rank_index: 99,
        };
        assert_eq!(metric_scores_order(higher_sb, higher_bh), Ordering::Greater);

        let smaller_tpn = BursteinMetricScores {
            buchholz: 11,
            sonneborn_berger: 2,
            rank_index: 1,
        };
        assert_eq!(
            metric_scores_order(smaller_tpn, higher_sb),
            Ordering::Greater
        );
    }

    #[test]
    fn player_score_and_median_cannot_affect_the_pairing_index() {
        let first = BursteinMetricScores {
            buchholz: 8,
            sonneborn_berger: 4,
            rank_index: 2,
        };
        let second = BursteinMetricScores {
            buchholz: 8,
            sonneborn_berger: 4,
            rank_index: 3,
        };
        assert_eq!(metric_scores_order(first, second), Ordering::Greater);
    }
}
