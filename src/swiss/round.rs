use crate::domain::Score;
use crate::limits::NATIVE_USIZE_MAX;
use crate::swisssystems::common::{
    PairingContext, eligible_for_double_swiss_pairing_allocated_bye,
    eligible_for_single_game_pairing_allocated_bye,
};
use std::collections::HashSet;

use super::calculation::{RoundCommitError, SwissCalculation};
use super::{
    ByeAssignment, ByeKind, PairingError, PairingFailure, PairingInternalError, RoundPairings,
    SwissPairingSnapshot, SwissPairingSystem,
};

/// Why the next Swiss round could not be paired.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PairNextRoundError {
    /// The configured round count has already been reached.
    #[error("the configured number of rounds has already been completed")]
    RoundLimitReached,
    /// Nobody is left to pair.
    #[error("nobody is left to pair: every player has withdrawn, or the roster is empty")]
    NoPairablePlayers,
    /// The selected pairing system could not find a semantic pairing result.
    #[error("pairing failed: {0}")]
    Pairing(#[source] PairingError),
    /// The selector violated an internal invariant; this is not field exhaustion.
    #[error("internal pairing failure: {0}")]
    Internal(#[source] PairingInternalError),
    /// Caller-controlled bye score arithmetic overflowed while committing the round.
    #[error("round score arithmetic overflowed")]
    ArithmeticOverflow,
}

#[derive(Clone, Copy)]
struct BursteinPairingLimits {
    accelerated_score: u32,
    adjusted_score: usize,
    metric: usize,
}

impl BursteinPairingLimits {
    const NATIVE: Self = Self {
        accelerated_score: u32::MAX,
        adjusted_score: NATIVE_USIZE_MAX,
        metric: NATIVE_USIZE_MAX,
    };
}

fn compute_matching(
    tournament: &SwissCalculation,
    snapshot: &SwissPairingSnapshot,
    burstein_limits: BursteinPairingLimits,
) -> Result<RoundPairings, PairNextRoundError> {
    let pairing = PairingContext {
        tournament: snapshot,
        played_rounds: tournament.played_rounds(),
        point_system: tournament.config().game_point_system,
        initial_color: tournament.config().initial_color,
    };

    match tournament.effective_pairing_system() {
        SwissPairingSystem::Dutch => {
            crate::swisssystems::dutch::compute_matching(crate::swisssystems::dutch::DutchInput {
                pairing,
                expected_rounds: tournament.config().expected_rounds_usize(),
                points_type_max: NATIVE_USIZE_MAX,
            })
            .map_err(pairing_failure_error)
        }
        SwissPairingSystem::Burstein => crate::swisssystems::burstein::compute_matching(
            crate::swisssystems::burstein::BursteinInput {
                pairing,
                max_points: Score::new(burstein_limits.accelerated_score),
                adjusted_score_type_max: burstein_limits.adjusted_score,
                metric_type_max: burstein_limits.metric,
            },
        )
        .map_err(pairing_failure_error),
        SwissPairingSystem::DoubleSwiss => crate::swisssystems::double_swiss::compute_matching(
            crate::swisssystems::double_swiss::DoubleSwissInput {
                pairing,
                expected_rounds: tournament.config().expected_rounds_usize(),
                points_type_max: NATIVE_USIZE_MAX,
            },
        )
        .map_err(pairing_failure_error),
    }
}

fn pairing_failure_error(failure: PairingFailure) -> PairNextRoundError {
    match failure {
        PairingFailure::Pairing(error) => PairNextRoundError::Pairing(error),
        PairingFailure::Internal(error) => PairNextRoundError::Internal(error),
    }
}

impl SwissCalculation {
    /// Selects and begins the next round.
    ///
    /// The host may persist the returned exact pairings as its next accepted
    /// round fact.
    pub(crate) fn pair_next_round(&mut self) -> Result<RoundPairings, PairNextRoundError> {
        self.pair_next_round_with_burstein_limits(BursteinPairingLimits::NATIVE)
    }

    fn pair_next_round_with_burstein_limits(
        &mut self,
        burstein_limits: BursteinPairingLimits,
    ) -> Result<RoundPairings, PairNextRoundError> {
        assert!(
            !self.is_round_in_progress(),
            "Swiss selector requires a complete accepted-round prefix"
        );
        if self.played_rounds() >= self.config().expected_rounds_usize() {
            return Err(PairNextRoundError::RoundLimitReached);
        }
        if !self.has_live_participants() {
            return Err(PairNextRoundError::NoPairablePlayers);
        }
        if !self.has_selector_eligible_participants() {
            let byes = self
                .scheduled_sit_outs()
                .map(|player| ByeAssignment {
                    player,
                    kind: ByeKind::Requested,
                })
                .collect();
            let round = RoundPairings {
                byes,
                games: Vec::new(),
            };
            self.commit_generated_round(&round)?;
            return Ok(round);
        }

        let snapshot = self.snapshot();
        let mut round = compute_matching(self, &snapshot, burstein_limits)?;
        round
            .byes
            .extend(self.scheduled_sit_outs().map(|player| ByeAssignment {
                player,
                kind: ByeKind::Requested,
            }));

        self.commit_generated_round(&round)?;

        Ok(round)
    }

    fn commit_generated_round(&mut self, round: &RoundPairings) -> Result<(), PairNextRoundError> {
        let eligible = self.live_participants().collect::<HashSet<_>>();
        if !round.assigns_exactly(&eligible) {
            return Err(PairNextRoundError::Internal(
                PairingInternalError::InvalidGeneratedRound,
            ));
        }
        let requested = self.scheduled_sit_outs().collect::<HashSet<_>>();
        let returned_requested = round
            .byes
            .iter()
            .filter(|bye_| bye_.kind == ByeKind::Requested)
            .map(|bye_| bye_.player)
            .collect::<HashSet<_>>();
        let mut automatic = round
            .byes
            .iter()
            .filter(|bye_| bye_.kind == ByeKind::PairingAllocated);
        let automatic_bye = automatic.next();
        let expected_automatic = (eligible.len() - requested.len()) & 1;
        let valid_automatic = automatic_bye.is_none_or(|bye_| {
            self.player_round_records(bye_.player).is_ok_and(|records| {
                match self.effective_pairing_system() {
                    SwissPairingSystem::DoubleSwiss => {
                        eligible_for_double_swiss_pairing_allocated_bye(records)
                    }
                    SwissPairingSystem::Dutch | SwissPairingSystem::Burstein => {
                        eligible_for_single_game_pairing_allocated_bye(
                            records,
                            self.config().game_point_system,
                        )
                    }
                }
            })
        });
        if returned_requested != requested
            || automatic.next().is_some()
            || usize::from(automatic_bye.is_some()) != expected_automatic
            || !valid_automatic
        {
            return Err(PairNextRoundError::Internal(
                PairingInternalError::InvalidGeneratedByes,
            ));
        }
        self.commit_round(round)
            .map_err(|RoundCommitError::ArithmeticOverflow| PairNextRoundError::ArithmeticOverflow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swiss::{
        BursteinConfig, PairingLimit, RoundPairings, SingleGameSwissByePoints, SwissConfig,
        SwissStandingsPlan, SwissSystemConfig,
    };
    use crate::{Color, GameOutcome, Pairing, PlayerId, PointSystem};

    fn burstein_config(point_system: PointSystem) -> SwissConfig {
        SwissConfig {
            system: SwissSystemConfig::Burstein(BursteinConfig {
                bye_points: SingleGameSwissByePoints::game_win(point_system),
            }),
            expected_rounds: 6,
            initial_color: Color::White,
            game_point_system: point_system,
            acceleration: crate::swiss::AccelerationPolicy::None,
            standings: SwissStandingsPlan::default(),
        }
    }

    #[test]
    fn invalid_generated_rounds_fail_before_mutating_the_calculation() {
        let field = [0, 1, 2, 3].map(PlayerId::new);
        let mut tournament =
            SwissCalculation::new(burstein_config(PointSystem::STANDARD), &field).unwrap();
        tournament.exclude_player(field[3]).unwrap();
        let before = tournament.snapshot();
        let bye = ByeAssignment {
            player: field[2],
            kind: ByeKind::PairingAllocated,
        };
        let invalid = [
            // Missing player.
            RoundPairings {
                games: vec![Pairing::new(field[0], field[1])],
                byes: vec![],
            },
            // Duplicate player across games.
            RoundPairings {
                games: vec![
                    Pairing::new(field[0], field[1]),
                    Pairing::new(field[0], field[2]),
                ],
                byes: vec![],
            },
            // Self-pairing.
            RoundPairings {
                games: vec![
                    Pairing::new(field[0], field[0]),
                    Pairing::new(field[1], field[2]),
                ],
                byes: vec![],
            },
            // Unknown or withdrawn player: both must fail before indexing.
            RoundPairings {
                games: vec![Pairing::new(field[0], PlayerId::new(4))],
                byes: vec![bye],
            },
            RoundPairings {
                games: vec![Pairing::new(field[0], field[3])],
                byes: vec![bye],
            },
            // Duplicate bye, or a player present in both a game and a bye.
            RoundPairings {
                games: vec![Pairing::new(field[0], field[1])],
                byes: vec![bye, bye],
            },
            RoundPairings {
                games: vec![Pairing::new(field[0], field[2])],
                byes: vec![bye],
            },
        ];
        for round in invalid {
            assert_eq!(
                tournament.commit_generated_round(&round),
                Err(PairNextRoundError::Internal(
                    PairingInternalError::InvalidGeneratedRound,
                )),
            );
            assert_eq!(tournament.snapshot(), before);
            assert_eq!(tournament.played_rounds(), 0);
            assert!(!tournament.is_round_in_progress());
        }
    }

    #[test]
    fn requested_bye_only_rounds_cover_live_players() {
        let field = [0, 1, 2].map(PlayerId::new);
        let mut tournament =
            SwissCalculation::new(burstein_config(PointSystem::STANDARD), &field).unwrap();
        tournament.exclude_player(field[2]).unwrap();
        tournament.request_bye_for_calculation(field[0]).unwrap();
        tournament.request_bye_for_calculation(field[1]).unwrap();
        let round = tournament.pair_next_round().unwrap();
        assert!(round.games.is_empty());
        assert_eq!(round.byes.len(), 2);
        assert_eq!(tournament.played_rounds(), 1);
    }

    #[test]
    fn generated_byes_must_respect_requests_and_field_parity() {
        let field = [0, 1].map(PlayerId::new);
        let mut tournament =
            SwissCalculation::new(burstein_config(PointSystem::STANDARD), &field).unwrap();
        let before = tournament.snapshot();
        let automatic_byes = RoundPairings {
            games: Vec::new(),
            byes: field
                .iter()
                .map(|&player| ByeAssignment {
                    player,
                    kind: ByeKind::PairingAllocated,
                })
                .collect(),
        };
        assert_eq!(
            tournament.commit_generated_round(&automatic_byes),
            Err(PairNextRoundError::Internal(
                PairingInternalError::InvalidGeneratedByes,
            )),
        );
        assert_eq!(tournament.snapshot(), before);

        tournament.request_bye_for_calculation(field[0]).unwrap();
        let before = tournament.snapshot();
        let ignored_request = RoundPairings {
            games: vec![Pairing::new(field[0], field[1])],
            byes: Vec::new(),
        };
        assert_eq!(
            tournament.commit_generated_round(&ignored_request),
            Err(PairNextRoundError::Internal(
                PairingInternalError::InvalidGeneratedByes,
            )),
        );
        assert_eq!(tournament.snapshot(), before);
        let accepted = tournament.pair_next_round().unwrap();
        assert_eq!(accepted.byes.len(), 2);
        assert_eq!(tournament.played_rounds(), 1);
    }

    #[test]
    fn generated_automatic_bye_cannot_repeat_an_earlier_bye() {
        use crate::MatchPointSystem;
        use crate::swiss::{DoubleSwissByePoints, DoubleSwissConfig, DoubleSwissPrimaryScore};

        let field = [0, 1, 2].map(PlayerId::new);
        let single_config = burstein_config(PointSystem::STANDARD);
        let double_config = SwissConfig {
            system: SwissSystemConfig::DoubleSwiss(DoubleSwissConfig {
                primary_score: DoubleSwissPrimaryScore::GamePoints,
                match_point_system: MatchPointSystem::STANDARD,
                bye_points: DoubleSwissByePoints::standard(
                    PointSystem::STANDARD,
                    MatchPointSystem::STANDARD,
                )
                .unwrap(),
            }),
            ..single_config.clone()
        };
        for config in [single_config, double_config] {
            let is_double = matches!(config.system, SwissSystemConfig::DoubleSwiss(_));
            let mut tournament = SwissCalculation::new(config, &field).unwrap();
            let round = RoundPairings {
                games: vec![Pairing::new(field[0], field[1])],
                byes: vec![ByeAssignment {
                    player: field[2],
                    kind: ByeKind::PairingAllocated,
                }],
            };
            tournament.accept_round_fact(round.clone()).unwrap();
            tournament
                .apply_outcome_fact(field[0], field[1], GameOutcome::draw())
                .unwrap();
            if is_double {
                tournament
                    .apply_outcome_fact(field[1], field[0], GameOutcome::draw())
                    .unwrap();
            }
            let before = tournament.snapshot();
            assert_eq!(
                tournament.commit_generated_round(&round),
                Err(PairNextRoundError::Internal(
                    PairingInternalError::InvalidGeneratedByes,
                )),
            );
            assert_eq!(tournament.snapshot(), before);
            assert_eq!(tournament.played_rounds(), 1);
        }
    }

    #[test]
    fn internal_solver_failure_stays_distinct_from_semantic_pairing_failure() {
        assert_eq!(
            pairing_failure_error(PairingFailure::Internal(
                PairingInternalError::EmptyScoreGroup
            )),
            PairNextRoundError::Internal(PairingInternalError::EmptyScoreGroup),
        );
    }

    fn burstein_metric_fixture(
        point_system: PointSystem,
        outcomes: [[GameOutcome; 2]; 3],
    ) -> SwissCalculation {
        let field = [
            PlayerId::new(0),
            PlayerId::new(1),
            PlayerId::new(2),
            PlayerId::new(3),
        ];
        let rounds = [[(0, 1), (2, 3)], [(0, 2), (3, 1)], [(3, 0), (1, 2)]];
        let mut tournament = SwissCalculation::new(burstein_config(point_system), &field).unwrap();
        for (round, round_outcomes) in rounds.into_iter().zip(outcomes) {
            let pairings = round
                .into_iter()
                .map(|(white, black)| Pairing::new(field[white], field[black]))
                .collect::<Vec<_>>();
            tournament
                .accept_round_fact(RoundPairings {
                    byes: Vec::new(),
                    games: pairings.clone(),
                })
                .expect("the fixture round is structurally legal");
            for (pairing, outcome) in pairings.into_iter().zip(round_outcomes) {
                tournament
                    .apply_outcome_fact(pairing.white(), pairing.black(), outcome)
                    .expect("the fixture outcome completes its game");
            }
        }
        assert_eq!(tournament.played_rounds(), 3);
        tournament
    }

    fn assert_metric_limit_is_transactional(
        mut tournament: SwissCalculation,
        metric_type_max: usize,
        expected_limit: PairingLimit,
    ) {
        let field = [
            PlayerId::new(0),
            PlayerId::new(1),
            PlayerId::new(2),
            PlayerId::new(3),
        ];
        tournament
            .request_bye_for_calculation(field[3])
            .expect("the request is live before selection");
        let records_before =
            field.map(|player| tournament.player_round_records(player).unwrap().to_vec());
        let participation_before = field.map(|player| {
            (
                tournament.calculation_excludes(player).unwrap(),
                tournament.calculation_requests_bye(player).unwrap(),
            )
        });
        let played_rounds_before = tournament.played_rounds();
        let pending_before = tournament.is_round_in_progress();

        assert_eq!(
            tournament.pair_next_round_with_burstein_limits(BursteinPairingLimits {
                accelerated_score: u32::MAX,
                adjusted_score: usize::MAX,
                metric: metric_type_max,
            }),
            Err(PairNextRoundError::Pairing(PairingError::LimitExceeded(
                expected_limit
            )))
        );
        assert_eq!(tournament.played_rounds(), played_rounds_before);
        assert_eq!(tournament.is_round_in_progress(), pending_before);
        for ((player, expected_records), expected_participation) in field
            .into_iter()
            .zip(records_before)
            .zip(participation_before)
        {
            assert_eq!(
                tournament.player_round_records(player).unwrap(),
                expected_records
            );
            assert_eq!(
                (
                    tournament.calculation_excludes(player).unwrap(),
                    tournament.calculation_requests_bye(player).unwrap(),
                ),
                expected_participation
            );
        }
    }

    #[test]
    fn every_burstein_metric_limit_preserves_round_history_and_participation_state() {
        let buchholz_points = PointSystem {
            win: Score::new(1),
            draw: Score::new(0),
            loss: Score::new(0),
            forfeit_loss: Score::new(0),
        };
        assert_metric_limit_is_transactional(
            burstein_metric_fixture(
                buchholz_points,
                [
                    [GameOutcome::black_win(), GameOutcome::draw()],
                    [GameOutcome::black_win(), GameOutcome::draw()],
                    [GameOutcome::draw(), GameOutcome::draw()],
                ],
            ),
            1,
            PairingLimit::BuchholzTotal,
        );

        let product_points = PointSystem {
            win: Score::new(2),
            draw: Score::new(0),
            loss: Score::new(0),
            forfeit_loss: Score::new(0),
        };
        assert_metric_limit_is_transactional(
            burstein_metric_fixture(
                product_points,
                [
                    [GameOutcome::white_win(), GameOutcome::draw()],
                    [GameOutcome::draw(), GameOutcome::black_win()],
                    [GameOutcome::draw(), GameOutcome::draw()],
                ],
            ),
            3,
            PairingLimit::SonnebornBergerProduct,
        );

        let total_points = PointSystem {
            win: Score::new(2),
            draw: Score::new(1),
            loss: Score::new(0),
            forfeit_loss: Score::new(0),
        };
        assert_metric_limit_is_transactional(
            burstein_metric_fixture(
                total_points,
                [
                    [GameOutcome::white_win(), GameOutcome::black_win()],
                    [GameOutcome::white_win(), GameOutcome::white_win()],
                    [GameOutcome::white_win(), GameOutcome::draw()],
                ],
            ),
            3,
            PairingLimit::SonnebornBergerTotal,
        );
    }
}
