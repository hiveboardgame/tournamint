use std::ops::Range;

use crate::acceleration::{
    BakuAccelerationError, BakuScoreScale, apply_baku_acceleration, flat_accelerated_rank_count,
};
#[cfg(test)]
use crate::domain::Pairing;
use crate::domain::{Color, ForbiddenPairEntry, GameOutcome, PlayerCount, PlayerId, Score};

#[cfg(test)]
use super::ByeAssignment;
use super::config::{
    AccelerationPolicy, DoubleSwissPrimaryScore, SwissConfig, SwissConfigError, SwissPairingSystem,
    SwissSystemConfig, burstein_dutch_opening_rounds,
};
use super::player_data::compute_player_data;
use super::{
    ByeKind, LogicalGame, LogicalGames, PlayerRoundRecord, RoundPairings, RoundStatus,
    SwissMatchRecord, SwissPairingPlayerEntry, SwissPairingSnapshot, SwissPrimaryScoreProjection,
    project_double_swiss_match, project_single_game_match,
};

#[derive(Clone)]
struct SwissPlayer {
    rank_index: usize,
    game_points: Score,
    match_points: Score,
    records: Vec<PlayerRoundRecord>,
    withdrawn: bool,
    scheduled_sit_out_round: Option<usize>,
}

#[derive(Clone)]
struct SwissForbiddenPair {
    round_range: Range<usize>,
    players: Vec<PlayerId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingMatch {
    player_a: PlayerId,
    player_b: PlayerId,
    game1: Option<GameOutcome>,
    game2: Option<GameOutcome>,
}

struct PendingRound {
    matches: Vec<PendingMatch>,
}

/// Why an accepted round fact could not be folded into a calculation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum AcceptedRoundError {
    /// The previous round still has games without a result.
    #[error("a round is already in progress")]
    RoundAlreadyInProgress,
    /// The configured round count has already been reached.
    #[error("the configured number of rounds has already been completed")]
    RoundLimitReached,
    /// Stored score arithmetic overflowed. Nothing changed.
    #[error("round score arithmetic overflowed")]
    ArithmeticOverflow,
}

/// Why a participant query could not be answered.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum SwissPlayerError {
    /// The player id is outside this tournament.
    #[error("player {0:?} is not registered in this Swiss tournament")]
    UnknownPlayer(PlayerId),
}

/// Why a participant could not be withdrawn.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum CalculationExclusionError {
    /// The player id is outside this tournament.
    #[error("player {0:?} is not registered in this Swiss tournament")]
    UnknownPlayer(PlayerId),
    /// The participant was already withdrawn.
    #[error("player {0:?} is already withdrawn")]
    AlreadyWithdrawn(PlayerId),
}

/// Why a sit-out could not be scheduled for a participant's next round.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum CalculationByeError {
    /// The player id is outside this tournament.
    #[error("player {0:?} is not registered in this Swiss tournament")]
    UnknownPlayer(PlayerId),
    /// A withdrawn participant cannot change sit-out state.
    #[error("player {0:?} is withdrawn")]
    PlayerWithdrawn(PlayerId),
    /// The participant already has a future sit-out request.
    #[error("player {player:?} already has a sit-out scheduled for round {round_index}")]
    SitOutAlreadyScheduled {
        /// Participant whose request already exists.
        player: PlayerId,
        /// Zero-based round index targeted by the existing request.
        round_index: usize,
    },
    /// No unpaired round remains in which to materialize a sit-out.
    #[error("player {0:?} has no future round available for a sit-out")]
    NoFutureRound(PlayerId),
}

/// Why a temporary Swiss pairing restriction could not be registered.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum CalculationRestrictionError {
    /// The restriction names a player outside this tournament.
    #[error("player {0:?} is not registered in this Swiss tournament")]
    UnknownPlayer(PlayerId),
    /// The same player occurs more than once in the restriction group.
    #[error("player {0:?} occurs more than once in the restriction group")]
    DuplicatePlayer(PlayerId),
    /// Fewer than two distinct registered players were supplied.
    #[error("a restriction group needs at least two players, but received {player_count}")]
    TooFewPlayers {
        /// Number of distinct registered players supplied.
        player_count: usize,
    },
    /// The zero-based half-open round range contains no round.
    #[error("the half-open round range {start}..{end} is empty")]
    EmptyRoundRange {
        /// Inclusive zero-based start.
        start: usize,
        /// Exclusive zero-based end.
        end: usize,
    },
    /// The exclusive range end exceeds the configured tournament lifetime.
    #[error("round range end {end} exceeds the configured round count {expected_rounds}")]
    RoundRangeEndsAfterTournament {
        /// Submitted exclusive zero-based end.
        end: usize,
        /// Configured total number of rounds.
        expected_rounds: usize,
    },
    /// No round remains that has not already been paired.
    #[error("no future unpaired round remains")]
    NoFutureRound,
    /// The range begins before the next round that has not been paired.
    #[error("round range starts at {start}, before next unpaired round {next_unpaired_round}")]
    RoundRangeStartsBeforeNextUnpaired {
        /// Submitted inclusive zero-based start.
        start: usize,
        /// Earliest zero-based round index that may still be restricted.
        next_unpaired_round: usize,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RoundCommitError {
    ArithmeticOverflow,
}

/// Why a Swiss tournament could not be constructed.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum CalculationSetupError {
    /// Immutable configuration is invalid.
    #[error(transparent)]
    InvalidConfig(#[from] SwissConfigError),
    /// Initial order is not a permutation of `0..n`.
    #[error("the initial rank order must contain every player exactly once")]
    InvalidRankOrder,
}

/// Why a result could not be recorded.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum OutcomeFactError {
    /// No round has been paired yet, or the last one is already complete.
    #[error("no round is in progress")]
    NoRoundInProgress,
    /// Those two players are not facing each other in the current round.
    #[error("that pairing is not part of the round in progress")]
    UnknownPairing,
    /// This logical game's result is already stored.
    #[error("a result for that game has already been recorded")]
    AlreadySubmitted,
    /// Caller-controlled score arithmetic overflowed. Nothing changed.
    #[error("result score arithmetic overflowed")]
    ArithmeticOverflow,
}

/// Ephemeral accumulator rebuilt from accepted Swiss facts for one operation.
pub(crate) struct SwissCalculation {
    config: SwissConfig,
    players: Vec<SwissPlayer>,
    played_rounds: usize,
    pending_round: Option<PendingRound>,
    forbidden_pairs: Vec<SwissForbiddenPair>,
}

impl SwissCalculation {
    /// Constructs a tournament from immutable configuration and initial TPN
    /// order. The order must be a permutation of `PlayerId(0)..PlayerId(n)`.
    pub(crate) fn new(
        config: SwissConfig,
        initial_rank_order: &[PlayerId],
    ) -> Result<Self, CalculationSetupError> {
        config.validate()?;

        let mut rank_index_by_player = vec![None; initial_rank_order.len()];
        for (rank_position, &player_id) in initial_rank_order.iter().enumerate() {
            let slot = rank_index_by_player
                .get_mut(player_id.index())
                .ok_or(CalculationSetupError::InvalidRankOrder)?;
            if slot.replace(rank_position).is_some() {
                return Err(CalculationSetupError::InvalidRankOrder);
            }
        }

        let players = rank_index_by_player
            .into_iter()
            .map(|rank_index| {
                Ok(SwissPlayer {
                    rank_index: rank_index.ok_or(CalculationSetupError::InvalidRankOrder)?,
                    game_points: Score::new(0),
                    match_points: Score::new(0),
                    records: Vec::new(),
                    withdrawn: false,
                    scheduled_sit_out_round: None,
                })
            })
            .collect::<Result<Vec<_>, CalculationSetupError>>()?;

        Ok(Self {
            config,
            players,
            played_rounds: 0,
            pending_round: None,
            forbidden_pairs: Vec::new(),
        })
    }

    /// Immutable tournament configuration.
    pub(crate) fn config(&self) -> &SwissConfig {
        &self.config
    }

    /// Number of registered participants, including withdrawals.
    pub(crate) fn player_count(&self) -> usize {
        self.players.len()
    }

    /// Initial TPN position used for deterministic presentation and pairing.
    pub(crate) fn rank_index(&self, player: PlayerId) -> Option<usize> {
        self.players
            .get(player.index())
            .map(|player| player.rank_index)
    }

    /// Pairing system effective for the next round.
    ///
    /// FIDE C.04.4.2 Burstein Article 1.6 requires the first
    /// `min(expected_rounds / 2, 4)` rounds to use Dutch pairing.
    pub(crate) fn effective_pairing_system(&self) -> SwissPairingSystem {
        match self.config.system {
            SwissSystemConfig::Burstein(_)
                if self.played_rounds
                    < burstein_dutch_opening_rounds(self.config.expected_rounds_usize()) =>
            {
                SwissPairingSystem::Dutch
            }
            _ => self.config.system.pairing_system(),
        }
    }

    /// Number of completed rounds.
    pub(crate) fn played_rounds(&self) -> usize {
        self.played_rounds
    }

    /// Whether a paired round still has unresolved logical games.
    pub(crate) fn is_round_in_progress(&self) -> bool {
        self.pending_round.is_some()
    }

    /// Round projections for one participant.
    ///
    /// While a round is in progress, the final element is its live typed
    /// projection. A Double-Swiss match therefore exposes zero, one, or two
    /// initialized logical games as its results arrive.
    pub(crate) fn player_round_records(
        &self,
        player: PlayerId,
    ) -> Result<&[PlayerRoundRecord], SwissPlayerError> {
        Ok(&self
            .players
            .get(player.index())
            .ok_or(SwissPlayerError::UnknownPlayer(player))?
            .records)
    }

    /// Withdraws a known participant from every future pairing.
    ///
    /// An already paired match remains live. Any sit-out scheduled for a later
    /// round is cancelled.
    pub(crate) fn exclude_player(
        &mut self,
        player: PlayerId,
    ) -> Result<(), CalculationExclusionError> {
        let entry = self
            .players
            .get_mut(player.index())
            .ok_or(CalculationExclusionError::UnknownPlayer(player))?;
        if entry.withdrawn {
            return Err(CalculationExclusionError::AlreadyWithdrawn(player));
        }
        entry.withdrawn = true;
        entry.scheduled_sit_out_round = None;
        Ok(())
    }

    fn next_unpaired_round(&self) -> Option<usize> {
        let round_index = self
            .played_rounds
            .checked_add(usize::from(self.pending_round.is_some()))?;
        (round_index < self.config.expected_rounds_usize()).then_some(round_index)
    }

    /// Requests a zero-point bye for the next round that has not been paired.
    ///
    /// While a round is in progress, this targets the following round.
    pub(crate) fn request_bye_for_calculation(
        &mut self,
        player: PlayerId,
    ) -> Result<(), CalculationByeError> {
        let entry = self
            .players
            .get(player.index())
            .ok_or(CalculationByeError::UnknownPlayer(player))?;
        if entry.withdrawn {
            return Err(CalculationByeError::PlayerWithdrawn(player));
        }
        let round_index = self
            .next_unpaired_round()
            .ok_or(CalculationByeError::NoFutureRound(player))?;
        if let Some(scheduled_round) = entry.scheduled_sit_out_round {
            return Err(CalculationByeError::SitOutAlreadyScheduled {
                player,
                round_index: scheduled_round,
            });
        }
        self.players[player.index()].scheduled_sit_out_round = Some(round_index);
        Ok(())
    }

    pub(crate) fn scheduled_sit_outs(&self) -> impl Iterator<Item = PlayerId> + '_ {
        let round_index = self.played_rounds;
        self.players
            .iter()
            .enumerate()
            .filter(move |(_, player)| {
                player.scheduled_sit_out_round == Some(round_index) && !player.withdrawn
            })
            .map(|(index, _)| PlayerId::new(index))
    }

    pub(super) fn has_live_participants(&self) -> bool {
        self.players.iter().any(|player| !player.withdrawn)
    }

    pub(super) fn live_participants(&self) -> impl Iterator<Item = PlayerId> + '_ {
        self.players
            .iter()
            .enumerate()
            .filter(|(_, player)| !player.withdrawn)
            .map(|(index, _)| PlayerId::new(index))
    }

    pub(super) fn has_selector_eligible_participants(&self) -> bool {
        self.players.iter().any(|player| {
            !player.withdrawn && player.scheduled_sit_out_round != Some(self.played_rounds)
        })
    }

    #[cfg(test)]
    pub(crate) fn calculation_excludes(&self, player: PlayerId) -> Result<bool, SwissPlayerError> {
        Ok(self
            .players
            .get(player.index())
            .ok_or(SwissPlayerError::UnknownPlayer(player))?
            .withdrawn)
    }

    #[cfg(test)]
    pub(crate) fn calculation_requests_bye(
        &self,
        player: PlayerId,
    ) -> Result<bool, SwissPlayerError> {
        let entry = self
            .players
            .get(player.index())
            .ok_or(SwissPlayerError::UnknownPlayer(player))?;
        Ok(self
            .next_unpaired_round()
            .is_some_and(|round_index| entry.scheduled_sit_out_round == Some(round_index)))
    }

    /// Forbids every pair in `players` during a zero-based half-open round range.
    ///
    /// At least two distinct registered players are required. The range must
    /// be nonempty, must end no later than `expected_rounds`, and must begin at
    /// or after the next round that has not been paired. An end beyond the
    /// tournament lifetime is rejected rather than truncated. Withdrawn
    /// players and players with scheduled sit-outs remain valid group members.
    /// Validation reports, in order, the first unknown player, the first
    /// duplicate player, group size, an empty range, an end beyond the
    /// tournament, exhaustion of future rounds, and a historical start.
    pub(crate) fn add_calculation_restriction(
        &mut self,
        players: &[PlayerId],
        round_range: Range<usize>,
    ) -> Result<(), CalculationRestrictionError> {
        for &player in players {
            if self.players.get(player.index()).is_none() {
                return Err(CalculationRestrictionError::UnknownPlayer(player));
            }
        }
        let mut seen = vec![false; self.players.len()];
        for &player in players {
            let slot = &mut seen[player.index()];
            if *slot {
                return Err(CalculationRestrictionError::DuplicatePlayer(player));
            }
            *slot = true;
        }
        if players.len() < 2 {
            return Err(CalculationRestrictionError::TooFewPlayers {
                player_count: players.len(),
            });
        }
        if round_range.start >= round_range.end {
            return Err(CalculationRestrictionError::EmptyRoundRange {
                start: round_range.start,
                end: round_range.end,
            });
        }
        if round_range.end > self.config.expected_rounds_usize() {
            return Err(CalculationRestrictionError::RoundRangeEndsAfterTournament {
                end: round_range.end,
                expected_rounds: self.config.expected_rounds_usize(),
            });
        }
        let next_unpaired_round = self
            .next_unpaired_round()
            .ok_or(CalculationRestrictionError::NoFutureRound)?;
        if round_range.start < next_unpaired_round {
            return Err(
                CalculationRestrictionError::RoundRangeStartsBeforeNextUnpaired {
                    start: round_range.start,
                    next_unpaired_round,
                },
            );
        }
        self.forbidden_pairs.push(SwissForbiddenPair {
            round_range,
            players: players.to_vec(),
        });
        Ok(())
    }

    fn double_swiss_config(&self) -> Option<super::config::DoubleSwissConfig> {
        match self.config.system {
            SwissSystemConfig::DoubleSwiss(config) => Some(config),
            _ => None,
        }
    }

    pub(crate) fn bye_points(&self, kind: ByeKind) -> (Score, Score) {
        if kind == ByeKind::Requested {
            return (Score::new(0), Score::new(0));
        }
        match self.config.system {
            SwissSystemConfig::Dutch(config) => (
                config.bye_points.pairing_allocated,
                config.bye_points.pairing_allocated,
            ),
            SwissSystemConfig::Burstein(config) => (
                config.bye_points.pairing_allocated,
                config.bye_points.pairing_allocated,
            ),
            SwissSystemConfig::DoubleSwiss(config) => (
                config.bye_points.game_points,
                config.bye_points.match_points,
            ),
        }
    }

    fn checked_total(current: Score, delta: Score) -> Result<Score, OutcomeFactError> {
        current
            .value()
            .checked_add(delta.value())
            .map(Score::new)
            .ok_or(OutcomeFactError::ArithmeticOverflow)
    }

    fn checked_round_total(current: Score, delta: Score) -> Option<Score> {
        current.value().checked_add(delta.value()).map(Score::new)
    }

    fn ensure_round_decision_can_apply(&self) -> Result<(), AcceptedRoundError> {
        if self.pending_round.is_some() {
            return Err(AcceptedRoundError::RoundAlreadyInProgress);
        }
        if self.played_rounds >= self.config.expected_rounds_usize() {
            return Err(AcceptedRoundError::RoundLimitReached);
        }
        Ok(())
    }

    pub(crate) fn commit_round(&mut self, round: &RoundPairings) -> Result<(), RoundCommitError> {
        // Preflight every bye award before changing either totals or flags.
        let mut bye_totals = Vec::with_capacity(round.byes.len());
        for bye in &round.byes {
            let (game_delta, match_delta) = self.bye_points(bye.kind);
            let player = &self.players[bye.player.index()];
            bye_totals.push((
                bye.player,
                Self::checked_round_total(player.game_points, game_delta)
                    .ok_or(RoundCommitError::ArithmeticOverflow)?,
                Self::checked_round_total(player.match_points, match_delta)
                    .ok_or(RoundCommitError::ArithmeticOverflow)?,
            ));
        }

        let mut records = Vec::with_capacity(self.players.len());
        records.extend(
            self.players
                .iter()
                .map(|player| player.withdrawn.then_some(PlayerRoundRecord::Withdrawn)),
        );
        for bye in &round.byes {
            records[bye.player.index()] = Some(match bye.kind {
                ByeKind::PairingAllocated => PlayerRoundRecord::PairingAllocatedBye,
                ByeKind::Requested => PlayerRoundRecord::RequestedBye,
            });
        }
        for pairing in &round.games {
            records[pairing.white().index()] = Some(PlayerRoundRecord::Match {
                opponent: pairing.black(),
                scheduled_first_color: Color::White,
                state: SwissMatchRecord::Pending {
                    games: LogicalGames::empty(),
                },
            });
            records[pairing.black().index()] = Some(PlayerRoundRecord::Match {
                opponent: pairing.white(),
                scheduled_first_color: Color::Black,
                state: SwissMatchRecord::Pending {
                    games: LogicalGames::empty(),
                },
            });
        }
        assert!(
            records.iter().all(Option::is_some),
            "accepted Swiss round assigns every live participant"
        );
        let is_double = self.double_swiss_config().is_some();
        let mut pending_matches = Vec::with_capacity(round.games.len());
        pending_matches.extend(round.games.iter().copied().map(|pairing| PendingMatch {
            player_a: pairing.white(),
            player_b: pairing.black(),
            game1: None,
            game2: (!is_double).then_some(GameOutcome::draw()),
        }));

        // Commit only after the complete record set, score totals, and pending
        // round have all been constructed successfully.
        for (player, game_points, match_points) in bye_totals {
            self.players[player.index()].game_points = game_points;
            self.players[player.index()].match_points = match_points;
        }
        for player in &mut self.players {
            if player.scheduled_sit_out_round == Some(self.played_rounds) {
                player.scheduled_sit_out_round = None;
            }
        }
        for (player, record) in self.players.iter_mut().zip(records) {
            player
                .records
                .push(record.expect("record completeness was checked before Swiss round mutation"));
        }

        self.pending_round = Some(PendingRound {
            matches: pending_matches,
        });

        if self
            .pending_round
            .as_ref()
            .is_some_and(|round| round.matches.is_empty())
        {
            self.finish_round();
        }
        Ok(())
    }

    /// Folds one structurally validated accepted round into the calculation.
    pub(crate) fn accept_round_fact(
        &mut self,
        round: RoundPairings,
    ) -> Result<(), AcceptedRoundError> {
        self.ensure_round_decision_can_apply()?;
        self.commit_round(&round)
            .map_err(|RoundCommitError::ArithmeticOverflow| AcceptedRoundError::ArithmeticOverflow)
    }

    fn finish_round(&mut self) {
        if self.pending_round.is_none() {
            return;
        }
        let next_played_rounds = self
            .played_rounds
            .checked_add(1)
            .expect("completed Swiss round count fits configured tournament lifetime");
        self.pending_round = None;
        self.played_rounds = next_played_rounds;
    }

    fn logical_games(first: Option<LogicalGame>, second: Option<LogicalGame>) -> LogicalGames {
        match (first, second) {
            (Some(first), Some(second)) => LogicalGames::double(first, second),
            (Some(game), None) | (None, Some(game)) => LogicalGames::single(game),
            (None, None) => LogicalGames::empty(),
        }
    }

    fn record_single_game(
        &mut self,
        match_index: usize,
        outcome: GameOutcome,
    ) -> Result<RoundStatus, OutcomeFactError> {
        let pending = self
            .pending_round
            .as_ref()
            .ok_or(OutcomeFactError::NoRoundInProgress)?;
        let scheduled = pending.matches[match_index];
        if scheduled.game1.is_some() {
            return Err(OutcomeFactError::AlreadySubmitted);
        }

        let projection = project_single_game_match(
            outcome.white(),
            outcome.black(),
            self.config.game_point_system,
        )
        .expect("GameOutcome always has reciprocal semantic sides");
        let a_total = Self::checked_total(
            self.players[scheduled.player_a.index()].game_points,
            projection.player.game_points,
        )?;
        let b_total = Self::checked_total(
            self.players[scheduled.player_b.index()].game_points,
            projection.opponent.game_points,
        )?;

        let a_record = PlayerRoundRecord::Match {
            opponent: scheduled.player_b,
            scheduled_first_color: Color::White,
            state: SwissMatchRecord::complete_single(
                LogicalGames::single(LogicalGame {
                    color: Color::White,
                    outcome: outcome.white(),
                }),
                projection.player.disposition,
                projection.player.aggregate,
            )
            .expect("a completed single-game match contains one logical game"),
        };
        let b_record = PlayerRoundRecord::Match {
            opponent: scheduled.player_a,
            scheduled_first_color: Color::Black,
            state: SwissMatchRecord::complete_single(
                LogicalGames::single(LogicalGame {
                    color: Color::Black,
                    outcome: outcome.black(),
                }),
                projection.opponent.disposition,
                projection.opponent.aggregate,
            )
            .expect("a completed single-game match contains one logical game"),
        };

        let round_index = self.played_rounds;
        self.players[scheduled.player_a.index()].game_points = a_total;
        self.players[scheduled.player_a.index()].match_points = a_total;
        self.players[scheduled.player_a.index()].records[round_index] = a_record;
        self.players[scheduled.player_b.index()].game_points = b_total;
        self.players[scheduled.player_b.index()].match_points = b_total;
        self.players[scheduled.player_b.index()].records[round_index] = b_record;
        let pending = self.pending_round.as_mut().expect("checked above");
        pending.matches[match_index].game1 = Some(outcome);

        if pending
            .matches
            .iter()
            .all(|game| game.game1.is_some() && game.game2.is_some())
        {
            self.finish_round();
            Ok(RoundStatus::Complete)
        } else {
            Ok(RoundStatus::InProgress)
        }
    }

    fn record_double_game(
        &mut self,
        match_index: usize,
        is_game1: bool,
        outcome: GameOutcome,
    ) -> Result<RoundStatus, OutcomeFactError> {
        let scheduled = self
            .pending_round
            .as_ref()
            .ok_or(OutcomeFactError::NoRoundInProgress)?
            .matches[match_index];
        if (is_game1 && scheduled.game1.is_some()) || (!is_game1 && scheduled.game2.is_some()) {
            return Err(OutcomeFactError::AlreadySubmitted);
        }

        let (a_side, b_side) = if is_game1 {
            (outcome.white(), outcome.black())
        } else {
            (outcome.black(), outcome.white())
        };
        let a_delta = self.config.game_point_system.points_for(a_side);
        let b_delta = self.config.game_point_system.points_for(b_side);
        let a_game_total = Self::checked_total(
            self.players[scheduled.player_a.index()].game_points,
            a_delta,
        )?;
        let b_game_total = Self::checked_total(
            self.players[scheduled.player_b.index()].game_points,
            b_delta,
        )?;

        let game1 = if is_game1 {
            Some(outcome)
        } else {
            scheduled.game1
        };
        let game2 = if is_game1 {
            scheduled.game2
        } else {
            Some(outcome)
        };
        let completed = game1.zip(game2);

        let mut completion = None;
        let mut match_totals = None;
        if let Some((first, second)) = completed {
            let a_games = [first.white(), second.black()];
            let b_games = [first.black(), second.white()];
            let projection =
                project_double_swiss_match(a_games, b_games, self.config.game_point_system)
                    .ok_or(OutcomeFactError::ArithmeticOverflow)?;
            let match_points = self
                .double_swiss_config()
                .expect("Double-Swiss recording requires Double-Swiss configuration")
                .match_point_system;
            let a_match_delta = match_points
                .projected_points(projection.player.disposition, projection.player.aggregate);
            let b_match_delta = match_points.projected_points(
                projection.opponent.disposition,
                projection.opponent.aggregate,
            );
            let a_match_total = Self::checked_total(
                self.players[scheduled.player_a.index()].match_points,
                a_match_delta,
            )?;
            let b_match_total = Self::checked_total(
                self.players[scheduled.player_b.index()].match_points,
                b_match_delta,
            )?;
            match_totals = Some((a_match_total, b_match_total));
            completion = Some(projection);
        }

        let a_games = Self::logical_games(
            game1.map(|game| LogicalGame {
                color: Color::White,
                outcome: game.white(),
            }),
            game2.map(|game| LogicalGame {
                color: Color::Black,
                outcome: game.black(),
            }),
        );
        let b_games = Self::logical_games(
            game1.map(|game| LogicalGame {
                color: Color::Black,
                outcome: game.black(),
            }),
            game2.map(|game| LogicalGame {
                color: Color::White,
                outcome: game.white(),
            }),
        );
        let (a_state, b_state) = match completion {
            Some(projection) => (
                SwissMatchRecord::complete_double(
                    a_games,
                    projection.player.disposition,
                    projection.player.aggregate,
                )
                .expect("a completed Double-Swiss match contains two logical games"),
                SwissMatchRecord::complete_double(
                    b_games,
                    projection.opponent.disposition,
                    projection.opponent.aggregate,
                )
                .expect("a completed Double-Swiss match contains two logical games"),
            ),
            None => (
                SwissMatchRecord::Pending { games: a_games },
                SwissMatchRecord::Pending { games: b_games },
            ),
        };
        let a_record = PlayerRoundRecord::Match {
            opponent: scheduled.player_b,
            scheduled_first_color: Color::White,
            state: a_state,
        };
        let b_record = PlayerRoundRecord::Match {
            opponent: scheduled.player_a,
            scheduled_first_color: Color::Black,
            state: b_state,
        };

        // Commit only after all player totals and history values are known.
        let round_index = self.played_rounds;
        self.players[scheduled.player_a.index()].game_points = a_game_total;
        self.players[scheduled.player_b.index()].game_points = b_game_total;
        if let Some((a_total, b_total)) = match_totals {
            self.players[scheduled.player_a.index()].match_points = a_total;
            self.players[scheduled.player_b.index()].match_points = b_total;
        }
        self.players[scheduled.player_a.index()].records[round_index] = a_record;
        self.players[scheduled.player_b.index()].records[round_index] = b_record;
        let pending = self.pending_round.as_mut().expect("checked above");
        if is_game1 {
            pending.matches[match_index].game1 = Some(outcome);
        } else {
            pending.matches[match_index].game2 = Some(outcome);
        }
        if pending
            .matches
            .iter()
            .all(|entry| entry.game1.is_some() && entry.game2.is_some())
        {
            self.finish_round();
            Ok(RoundStatus::Complete)
        } else {
            Ok(RoundStatus::InProgress)
        }
    }

    /// Records one scheduled logical game transactionally.
    pub(crate) fn apply_outcome_fact(
        &mut self,
        white: PlayerId,
        black: PlayerId,
        outcome: GameOutcome,
    ) -> Result<RoundStatus, OutcomeFactError> {
        let pending = self
            .pending_round
            .as_ref()
            .ok_or(OutcomeFactError::NoRoundInProgress)?;
        let Some(match_index) = pending.matches.iter().position(|entry| {
            (entry.player_a == white && entry.player_b == black)
                || (entry.player_a == black && entry.player_b == white)
        }) else {
            return Err(OutcomeFactError::UnknownPairing);
        };
        let is_game1 = pending.matches[match_index].player_a == white;
        if self.double_swiss_config().is_some() {
            self.record_double_game(match_index, is_game1, outcome)
        } else {
            if !is_game1 {
                return Err(OutcomeFactError::UnknownPairing);
            }
            self.record_single_game(match_index, outcome)
        }
    }

    fn acceleration_projection(&self) -> (usize, Vec<Score>) {
        match self.config.acceleration {
            AccelerationPolicy::None => (0, Vec::new()),
            AccelerationPolicy::Flat(flat) => {
                let cutoff =
                    flat_accelerated_rank_count(PlayerCount::new(self.players.len())).value();
                if cutoff == 0 {
                    return (0, Vec::new());
                }
                let required_rounds = self
                    .played_rounds
                    .checked_add(1)
                    .expect("active Swiss round index is representable");
                let acceleration_len = (flat.rounds as usize).min(required_rounds);
                let acceleration = std::iter::repeat_n(flat.bonus, acceleration_len).collect();
                (cutoff, acceleration)
            }
            AccelerationPolicy::Baku => {
                if self.players.is_empty() {
                    return (0, Vec::new());
                }
                let scale = match self.config.system {
                    SwissSystemConfig::DoubleSwiss(config) => BakuScoreScale::new(
                        config.match_point_system.win,
                        config.match_point_system.draw,
                        config.match_point_system.loss,
                    ),
                    _ => BakuScoreScale::from_game_points(self.config.game_point_system),
                };
                let result = apply_baku_acceleration(
                    PlayerCount::new(self.players.len()),
                    self.config.expected_rounds_usize(),
                    self.played_rounds,
                    scale,
                )
                .unwrap_or_else(|error| match error {
                    BakuAccelerationError::InconsistentPointSystem
                    | BakuAccelerationError::ArithmeticOverflow => {
                        panic!("validated Swiss Baku score scale became invalid")
                    }
                });
                (
                    result.accelerated_rank_count.value(),
                    result.acceleration_points,
                )
            }
        }
    }

    /// Prepares one typed, ephemeral pairing-engine input.
    pub(crate) fn snapshot(&self) -> SwissPairingSnapshot {
        let (bye_game_points, bye_match_points) = self.bye_points(ByeKind::PairingAllocated);
        let primary_score = match self.double_swiss_config() {
            Some(config) if config.primary_score == DoubleSwissPrimaryScore::MatchPoints => {
                SwissPrimaryScoreProjection::MatchPoints(config.match_point_system)
            }
            _ => SwissPrimaryScoreProjection::GamePoints,
        };
        let mut tournament = SwissPairingSnapshot::with_scoring(
            primary_score,
            primary_score.select(bye_game_points, bye_match_points),
        );
        let (accelerated_rank_count, acceleration_points) = self.acceleration_projection();
        for (index, player) in self.players.iter().enumerate() {
            let computed = compute_player_data(&player.records)
                .expect("private Swiss history counters fit allocated history");
            let accelerations: &[Score] = if player.rank_index < accelerated_rank_count {
                &acceleration_points
            } else {
                &[]
            };
            tournament.push_player(SwissPairingPlayerEntry {
                id: PlayerId::new(index),
                is_valid: !player.withdrawn
                    && player.scheduled_sit_out_round != Some(self.played_rounds),
                rank_index: player.rank_index,
                score_without_acceleration: primary_score
                    .select(player.game_points, player.match_points),
                played_games: computed.played_games,
                played_matches: computed.played_matches,
                color_data: computed.color_data,
                records: &player.records,
                accelerations,
            });
        }
        for entry in &self.forbidden_pairs {
            tournament.push_forbidden_pair(ForbiddenPairEntry {
                round_range: entry.round_range.clone(),
                players: &entry.players,
            });
        }
        tournament
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PointSystem;
    use crate::swiss::{
        BursteinConfig, DoubleSwissByePoints, DoubleSwissConfig, DoubleSwissPrimaryScore,
        DutchConfig, FlatAcceleration, MatchPointSystem, SingleGameSwissByePoints,
        SwissStandingsPlan, SwissSystemConfig,
    };

    fn config(point_system: PointSystem) -> SwissConfig {
        SwissConfig {
            system: SwissSystemConfig::Dutch(DutchConfig {
                bye_points: SingleGameSwissByePoints::game_win(point_system),
            }),
            expected_rounds: 3,
            initial_color: Color::White,
            game_point_system: point_system,
            acceleration: AccelerationPolicy::None,
            standings: SwissStandingsPlan::new(Vec::new()),
        }
    }

    #[derive(Debug, PartialEq)]
    struct RestrictionPlayerState {
        rank_index: usize,
        game_points: Score,
        match_points: Score,
        records: Vec<PlayerRoundRecord>,
        withdrawn: bool,
        scheduled_sit_out_round: Option<usize>,
    }

    #[derive(Debug, PartialEq)]
    struct RestrictionState {
        forbidden_pairs: Vec<(Range<usize>, Vec<PlayerId>)>,
        played_rounds: usize,
        pending_matches: Option<Vec<PendingMatch>>,
        players: Vec<RestrictionPlayerState>,
    }

    fn restriction_state(tournament: &SwissCalculation) -> RestrictionState {
        RestrictionState {
            forbidden_pairs: tournament
                .forbidden_pairs
                .iter()
                .map(|entry| (entry.round_range.clone(), entry.players.clone()))
                .collect(),
            played_rounds: tournament.played_rounds,
            pending_matches: tournament
                .pending_round
                .as_ref()
                .map(|round| round.matches.clone()),
            players: tournament
                .players
                .iter()
                .map(|player| RestrictionPlayerState {
                    rank_index: player.rank_index,
                    game_points: player.game_points,
                    match_points: player.match_points,
                    records: player.records.clone(),
                    withdrawn: player.withdrawn,
                    scheduled_sit_out_round: player.scheduled_sit_out_round,
                })
                .collect(),
        }
    }

    #[test]
    fn nine_round_burstein_uses_dutch_for_four_rounds_then_burstein() {
        let mut tournament = SwissCalculation::new(
            SwissConfig {
                system: SwissSystemConfig::Burstein(BursteinConfig {
                    bye_points: SingleGameSwissByePoints::game_win(PointSystem::STANDARD),
                }),
                expected_rounds: 9,
                initial_color: Color::White,
                game_point_system: PointSystem::STANDARD,
                acceleration: AccelerationPolicy::None,
                standings: SwissStandingsPlan::new(Vec::new()),
            },
            &[],
        )
        .unwrap();

        for played_rounds in 0..4 {
            tournament.played_rounds = played_rounds;
            assert_eq!(
                tournament.effective_pairing_system(),
                SwissPairingSystem::Dutch
            );
        }
        tournament.played_rounds = 4;
        assert_eq!(
            tournament.effective_pairing_system(),
            SwissPairingSystem::Burstein
        );
    }

    #[test]
    fn snapshot_assigns_flat_acceleration_by_initial_rank() {
        let mut configuration = config(PointSystem::STANDARD);
        configuration.acceleration = AccelerationPolicy::Flat(FlatAcceleration {
            rounds: 2,
            bonus: Score::new(2),
        });
        let tournament = SwissCalculation::new(
            configuration,
            &[
                PlayerId::new(2),
                PlayerId::new(0),
                PlayerId::new(3),
                PlayerId::new(1),
            ],
        )
        .unwrap();

        let snapshot = tournament.snapshot();
        assert_eq!(snapshot.player(0).unwrap().accelerations, &[Score::new(2)]);
        assert!(snapshot.player(1).unwrap().accelerations.is_empty());
        assert_eq!(snapshot.player(2).unwrap().accelerations, &[Score::new(2)]);
        assert!(snapshot.player(3).unwrap().accelerations.is_empty());
    }

    #[test]
    fn failed_two_player_update_is_transactional() {
        let points = PointSystem {
            win: Score::new(u32::MAX),
            draw: Score::new(1),
            loss: Score::new(0),
            forfeit_loss: Score::new(0),
        };
        let mut tournament = SwissCalculation::new(
            config(points),
            &[PlayerId::new(0), PlayerId::new(1), PlayerId::new(2)],
        )
        .unwrap();
        tournament
            .request_bye_for_calculation(PlayerId::new(2))
            .unwrap();
        tournament
            .accept_round_fact(RoundPairings {
                byes: vec![ByeAssignment {
                    player: PlayerId::new(2),
                    kind: ByeKind::Requested,
                }],
                games: vec![Pairing::new(PlayerId::new(0), PlayerId::new(1))],
            })
            .unwrap();
        tournament
            .apply_outcome_fact(PlayerId::new(0), PlayerId::new(1), GameOutcome::white_win())
            .unwrap();
        assert_eq!(tournament.played_rounds(), 1);

        tournament
            .request_bye_for_calculation(PlayerId::new(1))
            .unwrap();
        tournament
            .accept_round_fact(RoundPairings {
                byes: vec![ByeAssignment {
                    player: PlayerId::new(1),
                    kind: ByeKind::Requested,
                }],
                games: vec![Pairing::new(PlayerId::new(0), PlayerId::new(2))],
            })
            .unwrap();
        let affected_before = [PlayerId::new(0), PlayerId::new(2)].map(|player| {
            let player = &tournament.players[player.index()];
            (
                player.game_points,
                player.match_points,
                player.records.clone(),
            )
        });
        let pending_before = tournament
            .pending_round
            .as_ref()
            .map(|round| round.matches.clone());
        let played_rounds_before = tournament.played_rounds();
        assert_eq!(
            tournament.apply_outcome_fact(PlayerId::new(0), PlayerId::new(2), GameOutcome::draw()),
            Err(OutcomeFactError::ArithmeticOverflow)
        );
        assert!(tournament.is_round_in_progress());
        assert_eq!(tournament.played_rounds(), played_rounds_before);
        assert_eq!(
            tournament
                .pending_round
                .as_ref()
                .map(|round| round.matches.clone()),
            pending_before
        );
        for (player, before) in [PlayerId::new(0), PlayerId::new(2)]
            .into_iter()
            .zip(affected_before)
        {
            let player = &tournament.players[player.index()];
            assert_eq!(
                (
                    player.game_points,
                    player.match_points,
                    player.records.clone()
                ),
                before
            );
        }
    }

    #[test]
    fn double_swiss_return_game_overflow_preserves_the_pending_match() {
        let points = PointSystem {
            win: Score::new(u32::MAX),
            draw: Score::new(1),
            loss: Score::new(0),
            forfeit_loss: Score::new(0),
        };
        let mut tournament = SwissCalculation::new(
            SwissConfig {
                system: SwissSystemConfig::DoubleSwiss(DoubleSwissConfig {
                    primary_score: DoubleSwissPrimaryScore::GamePoints,
                    match_point_system: MatchPointSystem::STANDARD,
                    bye_points: DoubleSwissByePoints {
                        game_points: Score::new(0),
                        match_points: Score::new(0),
                        counts_as_win: false,
                    },
                }),
                expected_rounds: 1,
                initial_color: Color::White,
                game_point_system: points,
                acceleration: AccelerationPolicy::None,
                standings: SwissStandingsPlan::new(Vec::new()),
            },
            &[PlayerId::new(0), PlayerId::new(1)],
        )
        .unwrap();
        tournament
            .accept_round_fact(RoundPairings {
                byes: Vec::new(),
                games: vec![Pairing::new(PlayerId::new(0), PlayerId::new(1))],
            })
            .unwrap();
        assert_eq!(
            tournament.apply_outcome_fact(
                PlayerId::new(0),
                PlayerId::new(1),
                GameOutcome::white_win(),
            ),
            Ok(RoundStatus::InProgress)
        );

        let records_before = [PlayerId::new(0), PlayerId::new(1)].map(|player| {
            let records = tournament.player_round_records(player).unwrap().to_vec();
            assert!(matches!(
                records.as_slice(),
                [PlayerRoundRecord::Match {
                    state: SwissMatchRecord::Pending { games },
                    ..
                }] if games.len() == 1
            ));
            records
        });
        let totals_before = tournament
            .players
            .iter()
            .map(|player| (player.game_points, player.match_points))
            .collect::<Vec<_>>();
        let pending_before = tournament
            .pending_round
            .as_ref()
            .map(|round| round.matches.clone());
        let played_rounds_before = tournament.played_rounds();

        assert_eq!(
            tournament.apply_outcome_fact(
                PlayerId::new(1),
                PlayerId::new(0),
                GameOutcome::black_win(),
            ),
            Err(OutcomeFactError::ArithmeticOverflow)
        );
        assert!(tournament.is_round_in_progress());
        assert_eq!(tournament.played_rounds(), played_rounds_before);
        assert_eq!(
            tournament
                .pending_round
                .as_ref()
                .map(|round| round.matches.clone()),
            pending_before
        );
        assert_eq!(
            tournament
                .players
                .iter()
                .map(|player| (player.game_points, player.match_points))
                .collect::<Vec<_>>(),
            totals_before
        );
        for (player, expected) in [PlayerId::new(0), PlayerId::new(1)]
            .into_iter()
            .zip(records_before)
        {
            assert_eq!(tournament.player_round_records(player).unwrap(), expected);
        }

        assert_eq!(
            tournament.apply_outcome_fact(
                PlayerId::new(1),
                PlayerId::new(0),
                GameOutcome::white_win(),
            ),
            Ok(RoundStatus::Complete)
        );
        assert!(!tournament.is_round_in_progress());
        assert_eq!(tournament.played_rounds(), 1);
        for player in &tournament.players {
            assert_eq!(player.game_points, Score::new(u32::MAX));
            assert_eq!(player.match_points, MatchPointSystem::STANDARD.draw);
            assert!(matches!(
                player.records.as_slice(),
                [PlayerRoundRecord::Match {
                    state: SwissMatchRecord::Complete {
                        games,
                        disposition: super::super::MatchDisposition::Regular,
                        aggregate: crate::MatchScore::Draw,
                    },
                    ..
                }] if games.len() == 2
            ));
        }
    }

    #[test]
    fn every_completed_round_has_one_explicit_record_per_player() {
        let mut tournament = SwissCalculation::new(
            config(PointSystem::STANDARD),
            &[PlayerId::new(0), PlayerId::new(1), PlayerId::new(2)],
        )
        .unwrap();
        tournament
            .accept_round_fact(RoundPairings {
                byes: vec![ByeAssignment {
                    player: PlayerId::new(2),
                    kind: ByeKind::PairingAllocated,
                }],
                games: vec![Pairing::new(PlayerId::new(0), PlayerId::new(1))],
            })
            .unwrap();
        tournament
            .apply_outcome_fact(PlayerId::new(0), PlayerId::new(1), GameOutcome::draw())
            .unwrap();

        assert_eq!(
            tournament
                .players
                .iter()
                .map(|p| p.records.len())
                .collect::<Vec<_>>(),
            vec![1, 1, 1]
        );
        assert_eq!(
            tournament.players[2].records[0],
            PlayerRoundRecord::PairingAllocatedBye
        );
    }

    #[test]
    fn every_restriction_failure_preserves_all_private_tournament_state() {
        let mut tournament = SwissCalculation::new(
            config(PointSystem::STANDARD),
            &[
                PlayerId::new(0),
                PlayerId::new(1),
                PlayerId::new(2),
                PlayerId::new(3),
            ],
        )
        .unwrap();
        tournament
            .add_calculation_restriction(&[PlayerId::new(0), PlayerId::new(2)], 1..2)
            .unwrap();
        tournament
            .accept_round_fact(RoundPairings {
                byes: Vec::new(),
                games: vec![
                    Pairing::new(PlayerId::new(0), PlayerId::new(1)),
                    Pairing::new(PlayerId::new(2), PlayerId::new(3)),
                ],
            })
            .unwrap();
        tournament
            .request_bye_for_calculation(PlayerId::new(2))
            .unwrap();
        tournament.exclude_player(PlayerId::new(3)).unwrap();

        macro_rules! assert_rejected_without_mutation {
            ($players:expr, $range:expr, $error:expr) => {{
                let before = restriction_state(&tournament);
                assert_eq!(
                    tournament.add_calculation_restriction($players, $range),
                    Err($error)
                );
                assert_eq!(restriction_state(&tournament), before);
            }};
        }

        assert_rejected_without_mutation!(
            &[PlayerId::new(0), PlayerId::new(0), PlayerId::new(4)],
            3..3,
            CalculationRestrictionError::UnknownPlayer(PlayerId::new(4))
        );
        assert_rejected_without_mutation!(
            &[PlayerId::new(1), PlayerId::new(0), PlayerId::new(1)],
            3..3,
            CalculationRestrictionError::DuplicatePlayer(PlayerId::new(1))
        );
        assert_rejected_without_mutation!(
            &[],
            3..3,
            CalculationRestrictionError::TooFewPlayers { player_count: 0 }
        );
        assert_rejected_without_mutation!(
            &[PlayerId::new(0)],
            3..3,
            CalculationRestrictionError::TooFewPlayers { player_count: 1 }
        );
        assert_rejected_without_mutation!(
            &[PlayerId::new(0), PlayerId::new(1)],
            2..2,
            CalculationRestrictionError::EmptyRoundRange { start: 2, end: 2 }
        );
        assert_rejected_without_mutation!(
            &[PlayerId::new(0), PlayerId::new(1)],
            1..4,
            CalculationRestrictionError::RoundRangeEndsAfterTournament {
                end: 4,
                expected_rounds: 3,
            }
        );
        assert_rejected_without_mutation!(
            &[PlayerId::new(0), PlayerId::new(1)],
            0..2,
            CalculationRestrictionError::RoundRangeStartsBeforeNextUnpaired {
                start: 0,
                next_unpaired_round: 1,
            }
        );

        let mut final_config = config(PointSystem::STANDARD);
        final_config.expected_rounds = 1;
        let mut final_round = SwissCalculation::new(
            final_config,
            &[
                PlayerId::new(0),
                PlayerId::new(1),
                PlayerId::new(2),
                PlayerId::new(3),
            ],
        )
        .unwrap();
        final_round
            .add_calculation_restriction(&[PlayerId::new(0), PlayerId::new(2)], 0..1)
            .unwrap();
        final_round
            .accept_round_fact(RoundPairings {
                byes: Vec::new(),
                games: vec![
                    Pairing::new(PlayerId::new(0), PlayerId::new(1)),
                    Pairing::new(PlayerId::new(2), PlayerId::new(3)),
                ],
            })
            .unwrap();
        let pending_before = restriction_state(&final_round);
        assert_eq!(
            final_round.add_calculation_restriction(&[PlayerId::new(0), PlayerId::new(1)], 0..1),
            Err(CalculationRestrictionError::NoFutureRound)
        );
        assert_eq!(restriction_state(&final_round), pending_before);

        final_round
            .apply_outcome_fact(PlayerId::new(0), PlayerId::new(1), GameOutcome::draw())
            .unwrap();
        final_round
            .apply_outcome_fact(PlayerId::new(2), PlayerId::new(3), GameOutcome::draw())
            .unwrap();
        let complete_before = restriction_state(&final_round);
        assert_eq!(
            final_round.add_calculation_restriction(&[PlayerId::new(0), PlayerId::new(1)], 0..1),
            Err(CalculationRestrictionError::NoFutureRound)
        );
        assert_eq!(restriction_state(&final_round), complete_before);
    }

    #[test]
    fn accepting_a_round_while_one_is_in_progress_is_transactional() {
        let initial_order = [PlayerId::new(0), PlayerId::new(1)];
        let pairing = Pairing::new(PlayerId::new(0), PlayerId::new(1));
        let mut tournament =
            SwissCalculation::new(config(PointSystem::STANDARD), &initial_order).unwrap();
        tournament
            .accept_round_fact(RoundPairings {
                byes: Vec::new(),
                games: vec![pairing],
            })
            .unwrap();
        let before = restriction_state(&tournament);

        assert_eq!(
            tournament.accept_round_fact(RoundPairings {
                byes: Vec::new(),
                games: vec![pairing],
            }),
            Err(AcceptedRoundError::RoundAlreadyInProgress)
        );
        assert_eq!(restriction_state(&tournament), before);
    }

    #[test]
    fn exhausted_tournaments_return_public_round_limit_preflight_errors() {
        let mut configuration = config(PointSystem::STANDARD);
        configuration.expected_rounds = 1;
        let mut tournament =
            SwissCalculation::new(configuration, &[PlayerId::new(0), PlayerId::new(1)]).unwrap();
        tournament
            .accept_round_fact(RoundPairings {
                byes: Vec::new(),
                games: vec![Pairing::new(PlayerId::new(0), PlayerId::new(1))],
            })
            .unwrap();
        tournament
            .apply_outcome_fact(PlayerId::new(0), PlayerId::new(1), GameOutcome::draw())
            .unwrap();

        assert_eq!(
            tournament.accept_round_fact(RoundPairings::default()),
            Err(AcceptedRoundError::RoundLimitReached)
        );
        assert_eq!(
            tournament.pair_next_round(),
            Err(super::super::PairNextRoundError::RoundLimitReached)
        );
        assert_eq!(tournament.played_rounds(), 1);
        assert!(!tournament.is_round_in_progress());
    }
}
