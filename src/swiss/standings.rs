//! Configurable Swiss standings projected from semantic round records.

use core::cmp::Ordering;
use std::collections::HashMap;

use crate::domain::{AdjudicatedSideResult, Color, GameSideOutcome, MatchScore, PlayerId, Score};
use crate::standings::{
    CriterionValue, DirectEncounterForfeitPolicy, DirectEncounterOptions, ExactRatio,
    ProgressiveScoreOptions, RepeatedEncounterPolicy, SonnebornBergerContribution, StandingsError,
    StandingsTable, TournamentPairingNumberOrder, apply_criteria, partition_ascending,
    partition_descending, sum_sonneborn_berger_contributions,
};

use super::calculation::SwissCalculation;
use super::config::{
    BuchholzOptions, DoubleSwissPrimaryScore, ScoreBasis, SwissCriterion, SwissSystemConfig,
    UnplayedRoundPolicy,
};
use super::{
    MatchDisposition, PlayerRoundRecord, SwissHistoryStatus, SwissMatchRecord, SwissRoundProjection,
};

#[derive(Clone)]
struct PlayerProjection {
    records: Vec<PlayerRoundRecord>,
    game_points: u64,
    match_points: u64,
    primary_score: u64,
    counts: SwissPlayerCounts,
}

#[derive(Clone, Copy, Default)]
struct SwissPlayerCounts {
    games_played: u32,
    matches_played: u32,
    game_wins: u32,
    game_draws: u32,
    game_losses: u32,
    match_wins: u32,
    match_draws: u32,
    match_losses: u32,
    requested_byes: u32,
    pairing_allocated_byes: u32,
}

/// One participant's complete Swiss score and result summary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SwissPlayerSummary {
    /// Participant owning this summary.
    pub player: PlayerId,
    /// Real score selected by the configured Swiss system.
    pub primary_score: Score,
    /// Sum of configured game-point awards.
    pub game_points: Score,
    /// Sum of configured match-point awards. For single-game systems this is
    /// the same value as `game_points`.
    pub match_points: Score,
    /// Logical games that actually began.
    pub games_played: u32,
    /// Regular completed Double-Swiss matches. Single-game systems report zero.
    pub matches_played: u32,
    /// Semantic game wins, including adjudicated wins.
    pub game_wins: u32,
    /// Semantic game draws, including adjudicated draws.
    pub game_draws: u32,
    /// Semantic game losses, including forfeits and double forfeits.
    pub game_losses: u32,
    /// Completed Double-Swiss match wins, including forfeit wins.
    pub match_wins: u32,
    /// Completed Double-Swiss match draws.
    pub match_draws: u32,
    /// Completed Double-Swiss match losses, including forfeits and double forfeits.
    pub match_losses: u32,
    /// Caller-requested zero-point byes.
    pub requested_byes: u32,
    /// Pairing-allocated byes.
    pub pairing_allocated_byes: u32,
}

/// The configured Swiss standings and every participant summary from one
/// history projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwissProjection {
    /// Standings columns and positionally aligned row values.
    pub standings: StandingsTable<SwissCriterion, CriterionValue>,
    /// Summaries in immutable initial rank order.
    pub players: Vec<SwissPlayerSummary>,
    /// Canonical accepted rounds in history order.
    pub rounds: Vec<SwissRoundProjection>,
    /// State of the accepted history without attempting another pairing.
    pub history_status: SwissHistoryStatus,
}

struct SwissStandingsContext<'a> {
    tournament: &'a SwissCalculation,
    players: Vec<PlayerProjection>,
    initial_order: Vec<PlayerId>,
    double_primary: Option<DoubleSwissPrimaryScore>,
}

impl<'a> SwissStandingsContext<'a> {
    fn new(tournament: &'a SwissCalculation) -> Result<Self, StandingsError> {
        let double_primary = match tournament.config().system {
            SwissSystemConfig::DoubleSwiss(config) => Some(config.primary_score),
            SwissSystemConfig::Dutch(_) | SwissSystemConfig::Burstein(_) => None,
        };
        let mut players = Vec::with_capacity(tournament.player_count());
        for index in 0..tournament.player_count() {
            let records = tournament
                .player_round_records(PlayerId::new(index))
                .expect("Swiss standings traverse the dense tournament player range")
                .to_vec();
            let game_points = records.iter().copied().try_fold(0u64, |total, record| {
                total
                    .checked_add(round_game_points(tournament, record)?)
                    .ok_or(StandingsError::ArithmeticOverflow)
            })?;
            let match_points = records.iter().copied().try_fold(0u64, |total, record| {
                total
                    .checked_add(round_match_points(tournament, record)?)
                    .ok_or(StandingsError::ArithmeticOverflow)
            })?;
            let primary_score = match double_primary {
                Some(DoubleSwissPrimaryScore::MatchPoints) => match_points,
                Some(DoubleSwissPrimaryScore::GamePoints) | None => game_points,
            };
            let counts = player_counts(&records, double_primary.is_some())?;
            players.push(PlayerProjection {
                records,
                game_points,
                match_points,
                primary_score,
                counts,
            });
        }
        let mut ranked = (0..players.len())
            .map(|index| {
                let player = PlayerId::new(index);
                let rank_index = tournament
                    .rank_index(player)
                    .expect("Swiss standings traverse the dense tournament rank range");
                (rank_index, player)
            })
            .collect::<Vec<_>>();
        ranked.sort_by_key(|(rank_index, _)| *rank_index);
        let initial_order = ranked.into_iter().map(|(_, player)| player).collect();
        Ok(Self {
            tournament,
            players,
            initial_order,
            double_primary,
        })
    }

    fn normalized_basis(&self, basis: ScoreBasis) -> Result<ScoreBasis, StandingsError> {
        match basis {
            ScoreBasis::Primary => Ok(match self.double_primary {
                Some(DoubleSwissPrimaryScore::MatchPoints) => ScoreBasis::MatchPoints,
                Some(DoubleSwissPrimaryScore::GamePoints) | None => ScoreBasis::GamePoints,
            }),
            ScoreBasis::GamePoints => Ok(ScoreBasis::GamePoints),
            ScoreBasis::MatchPoints if self.double_primary.is_some() => Ok(ScoreBasis::MatchPoints),
            ScoreBasis::MatchPoints => Err(StandingsError::InvalidCriterion {
                criterion_index: 0,
                reason: "MatchPoints is available only for Double-Swiss",
            }),
        }
    }

    fn total(&self, player: PlayerId, basis: ScoreBasis) -> Result<u64, StandingsError> {
        let projection = self
            .players
            .get(player.index())
            .expect("standings criteria receive a player from the dense Swiss context");
        Ok(match basis {
            ScoreBasis::Primary => projection.primary_score,
            ScoreBasis::GamePoints => projection.game_points,
            ScoreBasis::MatchPoints if self.double_primary.is_some() => projection.match_points,
            ScoreBasis::MatchPoints => {
                return Err(StandingsError::InvalidCriterion {
                    criterion_index: 0,
                    reason: "MatchPoints is available only for Double-Swiss",
                });
            }
        })
    }

    fn records(&self, player: PlayerId) -> &[PlayerRoundRecord] {
        &self
            .players
            .get(player.index())
            .expect("standings criteria receive a player from the dense Swiss context")
            .records
    }
}

fn player_counts(
    records: &[PlayerRoundRecord],
    double_swiss: bool,
) -> Result<SwissPlayerCounts, StandingsError> {
    let mut counts = SwissPlayerCounts::default();
    for record in records.iter().copied() {
        match record {
            PlayerRoundRecord::PairingAllocatedBye => {
                increment_count(&mut counts.pairing_allocated_byes)?
            }
            PlayerRoundRecord::RequestedBye => increment_count(&mut counts.requested_byes)?,
            PlayerRoundRecord::Withdrawn => {}
            PlayerRoundRecord::Match { state, .. } => {
                for game in state.games().iter() {
                    counts.games_played = counts
                        .games_played
                        .checked_add(u32::from(game.outcome.counts_as_played()))
                        .ok_or(StandingsError::ArithmeticOverflow)?;
                    match game.outcome.score() {
                        MatchScore::Win => increment_count(&mut counts.game_wins)?,
                        MatchScore::Draw => increment_count(&mut counts.game_draws)?,
                        MatchScore::Loss => increment_count(&mut counts.game_losses)?,
                    }
                }
                if double_swiss && let Some((disposition, aggregate)) = state.completion() {
                    counts.matches_played = counts
                        .matches_played
                        .checked_add(u32::from(disposition == MatchDisposition::Regular))
                        .ok_or(StandingsError::ArithmeticOverflow)?;
                    let result = match disposition {
                        MatchDisposition::Regular => aggregate,
                        MatchDisposition::ForfeitWin => MatchScore::Win,
                        MatchDisposition::ForfeitLoss | MatchDisposition::DoubleForfeit => {
                            MatchScore::Loss
                        }
                    };
                    match result {
                        MatchScore::Win => increment_count(&mut counts.match_wins)?,
                        MatchScore::Draw => increment_count(&mut counts.match_draws)?,
                        MatchScore::Loss => increment_count(&mut counts.match_losses)?,
                    }
                }
            }
        }
    }
    Ok(counts)
}

fn increment_count(count: &mut u32) -> Result<(), StandingsError> {
    *count = count
        .checked_add(1)
        .ok_or(StandingsError::ArithmeticOverflow)?;
    Ok(())
}

fn round_game_points(
    tournament: &SwissCalculation,
    record: PlayerRoundRecord,
) -> Result<u64, StandingsError> {
    Ok(match record {
        PlayerRoundRecord::PairingAllocatedBye => match tournament.config().system {
            SwissSystemConfig::Dutch(config) => config.bye_points.pairing_allocated.value() as u64,
            SwissSystemConfig::Burstein(config) => {
                config.bye_points.pairing_allocated.value() as u64
            }
            SwissSystemConfig::DoubleSwiss(config) => config.bye_points.game_points.value() as u64,
        },
        PlayerRoundRecord::Match { state, .. } => {
            state.games().iter().try_fold(0u64, |total, game| {
                total
                    .checked_add(
                        tournament
                            .config()
                            .game_point_system
                            .points_for(game.outcome)
                            .value() as u64,
                    )
                    .ok_or(StandingsError::ArithmeticOverflow)
            })?
        }
        PlayerRoundRecord::RequestedBye | PlayerRoundRecord::Withdrawn => 0,
    })
}

fn round_match_points(
    tournament: &SwissCalculation,
    record: PlayerRoundRecord,
) -> Result<u64, StandingsError> {
    let SwissSystemConfig::DoubleSwiss(config) = tournament.config().system else {
        return round_game_points(tournament, record);
    };
    Ok(match record {
        PlayerRoundRecord::PairingAllocatedBye => config.bye_points.match_points.value() as u64,
        PlayerRoundRecord::Match {
            state:
                SwissMatchRecord::Complete {
                    disposition,
                    aggregate,
                    ..
                },
            ..
        } => config
            .match_point_system
            .projected_points(disposition, aggregate)
            .value() as u64,
        PlayerRoundRecord::Match {
            state: SwissMatchRecord::Pending { .. },
            ..
        } => 0,
        PlayerRoundRecord::RequestedBye | PlayerRoundRecord::Withdrawn => 0,
    })
}

fn round_points(
    context: &SwissStandingsContext<'_>,
    record: PlayerRoundRecord,
    basis: ScoreBasis,
) -> Result<u64, StandingsError> {
    Ok(match context.normalized_basis(basis)? {
        ScoreBasis::GamePoints => round_game_points(context.tournament, record)?,
        ScoreBasis::MatchPoints => round_match_points(context.tournament, record)?,
        ScoreBasis::Primary => unreachable!("the basis is normalized above"),
    })
}

fn record_has_played_game(record: PlayerRoundRecord) -> bool {
    matches!(record, PlayerRoundRecord::Match { state, .. } if state.games().iter().any(|game| game.outcome.counts_as_played()))
}

/// FIDE C.04.5 treats every Double-Swiss match except a full-match forfeit as
/// regularly played for pairing and tie-break purposes. This deliberately
/// differs from `record_has_played_game`, which preserves whether a game
/// actually began.
fn record_is_tie_break_played(
    context: &SwissStandingsContext<'_>,
    record: PlayerRoundRecord,
) -> bool {
    let PlayerRoundRecord::Match { state, .. } = record else {
        return false;
    };
    match context.tournament.config().system {
        SwissSystemConfig::DoubleSwiss(_) => matches!(
            state,
            SwissMatchRecord::Complete {
                disposition: MatchDisposition::Regular,
                ..
            }
        ),
        SwissSystemConfig::Dutch(_) | SwissSystemConfig::Burstein(_) => {
            record_has_played_game(record)
        }
    }
}

fn record_is_vur(record: PlayerRoundRecord) -> bool {
    match record {
        PlayerRoundRecord::RequestedBye | PlayerRoundRecord::Withdrawn => true,
        PlayerRoundRecord::PairingAllocatedBye => false,
        PlayerRoundRecord::Match {
            state:
                SwissMatchRecord::Complete {
                    disposition: MatchDisposition::ForfeitLoss | MatchDisposition::DoubleForfeit,
                    ..
                },
            ..
        } => true,
        PlayerRoundRecord::Match { .. } => false,
    }
}

fn basis_draw_points(
    context: &SwissStandingsContext<'_>,
    basis: ScoreBasis,
) -> Result<u64, StandingsError> {
    Ok(match context.normalized_basis(basis)? {
        ScoreBasis::GamePoints => {
            let draw = context.tournament.config().game_point_system.draw.value() as u64;
            match context.tournament.config().system {
                SwissSystemConfig::DoubleSwiss(_) => draw
                    .checked_mul(2)
                    .ok_or(StandingsError::ArithmeticOverflow)?,
                SwissSystemConfig::Dutch(_) | SwissSystemConfig::Burstein(_) => draw,
            }
        }
        ScoreBasis::MatchPoints => match context.tournament.config().system {
            SwissSystemConfig::DoubleSwiss(config) => config.match_point_system.draw.value() as u64,
            _ => unreachable!("MatchPoints was validated above"),
        },
        ScoreBasis::Primary => unreachable!("the basis is normalized above"),
    })
}

/// Article 16.3 opponent score, evaluated independently in each requested
/// score basis over materialized history. An incomplete history is treated
/// provisionally as if the tournament ended now: a requested/withdrawal-tail
/// record followed only by VURs is category 16.2.5 and becomes a draw, while a
/// later non-VUR changes that earlier record to category 16.2.3 so it retains
/// its actual award. The record's VUR status does not change.
fn adjusted_opponent_score(
    context: &SwissStandingsContext<'_>,
    player: PlayerId,
    basis: ScoreBasis,
) -> Result<u64, StandingsError> {
    let records = context.records(player);
    let mut adjusted = context.total(player, basis)?;
    let draw = basis_draw_points(context, basis)?;
    let mut suffix_is_vur = true;
    for record in records.iter().copied().rev() {
        let requested_bye_tail = suffix_is_vur
            && matches!(
                record,
                PlayerRoundRecord::RequestedBye | PlayerRoundRecord::Withdrawn
            );
        suffix_is_vur &= record_is_vur(record);
        if !requested_bye_tail {
            continue;
        }
        let awarded = round_points(context, record, basis)?;
        adjusted = adjusted
            .checked_sub(awarded)
            .and_then(|score| score.checked_add(draw))
            .ok_or(StandingsError::ArithmeticOverflow)?;
    }
    Ok(adjusted)
}

#[derive(Clone, Copy)]
struct OpponentContribution {
    value: u64,
    vur: bool,
}

/// Article 16.4 own-tie-break contribution. An unplayed round uses a dummy
/// whose score is the participant's real score, capped by the scheduled
/// opponent's adjusted score for forfeits and by tournament par otherwise.
fn opponent_contribution(
    context: &SwissStandingsContext<'_>,
    player: PlayerId,
    record: PlayerRoundRecord,
    basis: ScoreBasis,
    policy: UnplayedRoundPolicy,
) -> Result<Option<OpponentContribution>, StandingsError> {
    if matches!(
        record,
        PlayerRoundRecord::Match {
            state: SwissMatchRecord::Pending { .. },
            ..
        }
    ) {
        return Ok(None);
    }
    if record_is_tie_break_played(context, record) {
        let PlayerRoundRecord::Match { opponent, .. } = record else {
            unreachable!("only match records contain played games")
        };
        let value = match policy {
            UnplayedRoundPolicy::FideMarch2026 => {
                adjusted_opponent_score(context, opponent, basis)?
            }
            UnplayedRoundPolicy::Exclude => context.total(opponent, basis)?,
        };
        return Ok(Some(OpponentContribution { value, vur: false }));
    }
    if policy == UnplayedRoundPolicy::Exclude {
        return Ok(None);
    }

    let own_score = context.total(player, basis)?;
    let cap = match record {
        PlayerRoundRecord::Match { opponent, .. } => {
            adjusted_opponent_score(context, opponent, basis)?
        }
        PlayerRoundRecord::PairingAllocatedBye
        | PlayerRoundRecord::RequestedBye
        | PlayerRoundRecord::Withdrawn => basis_draw_points(context, basis)?
            .checked_mul(u64::from(context.tournament.config().expected_rounds))
            .ok_or(StandingsError::ArithmeticOverflow)?,
    };
    Ok(Some(OpponentContribution {
        value: own_score.min(cap),
        vur: record_is_vur(record),
    }))
}

fn buchholz(
    context: &SwissStandingsContext<'_>,
    player: PlayerId,
    options: BuchholzOptions,
) -> Result<u64, StandingsError> {
    let mut contributions = Vec::new();
    for record in context.records(player).iter().copied() {
        if let Some(contribution) = opponent_contribution(
            context,
            player,
            record,
            ScoreBasis::Primary,
            options.unplayed,
        )? {
            contributions.push(contribution);
        }
    }

    for _ in 0..(options.cut_lowest as usize).min(contributions.len()) {
        let cut_index = if options.unplayed == UnplayedRoundPolicy::FideMarch2026 {
            contributions
                .iter()
                .enumerate()
                .filter(|(_, contribution)| contribution.vur)
                .min_by_key(|(_, contribution)| contribution.value)
                .map(|(index, _)| index)
        } else {
            None
        }
        .or_else(|| {
            contributions
                .iter()
                .enumerate()
                .min_by_key(|(_, contribution)| contribution.value)
                .map(|(index, _)| index)
        })
        .expect("a bounded cut has a contribution");
        contributions.swap_remove(cut_index);
    }
    for _ in 0..(options.cut_highest as usize).min(contributions.len()) {
        let cut_index = contributions
            .iter()
            .enumerate()
            .max_by_key(|(_, contribution)| contribution.value)
            .map(|(index, _)| index)
            .expect("a bounded cut has a contribution");
        contributions.swap_remove(cut_index);
    }
    contributions.iter().try_fold(0u64, |total, contribution| {
        total
            .checked_add(contribution.value)
            .ok_or(StandingsError::ArithmeticOverflow)
    })
}

fn sonneborn_berger(
    context: &SwissStandingsContext<'_>,
    player: PlayerId,
    options: crate::standings::SonnebornBergerOptions<ScoreBasis, UnplayedRoundPolicy>,
) -> Result<u64, StandingsError> {
    let mut contributions = Vec::new();
    for (order, record) in context.records(player).iter().copied().enumerate() {
        let Some(opponent) = opponent_contribution(
            context,
            player,
            record,
            options.score_basis,
            options.unplayed,
        )?
        else {
            continue;
        };
        let earned = round_points(context, record, options.score_basis)?;
        contributions.push(SonnebornBergerContribution {
            opponent_score: opponent.value,
            product: earned
                .checked_mul(opponent.value)
                .ok_or(StandingsError::ArithmeticOverflow)?,
            vur: opponent.vur,
            order,
        });
    }
    sum_sonneborn_berger_contributions(
        contributions,
        options.cut_lowest as usize,
        options.unplayed == UnplayedRoundPolicy::FideMarch2026,
    )
}

fn progressive(
    context: &SwissStandingsContext<'_>,
    player: PlayerId,
    options: ProgressiveScoreOptions<ScoreBasis>,
) -> Result<u64, StandingsError> {
    let mut running = 0u64;
    let mut total = 0u64;
    for (round_index, record) in context.records(player).iter().copied().enumerate() {
        running = running
            .checked_add(round_points(context, record, options.score_basis)?)
            .ok_or(StandingsError::ArithmeticOverflow)?;
        if options.cut_first_round && round_index == 0 {
            continue;
        }
        total = total
            .checked_add(running)
            .ok_or(StandingsError::ArithmeticOverflow)?;
    }
    Ok(total)
}

fn tournament_pairing_number(
    context: &SwissStandingsContext<'_>,
    player: PlayerId,
) -> Result<u64, StandingsError> {
    let rank_index = context
        .tournament
        .rank_index(player)
        .expect("standings criteria receive a player from the dense Swiss tournament");
    u64::try_from(rank_index)
        .map_err(|_| StandingsError::ArithmeticOverflow)?
        .checked_add(1)
        .ok_or(StandingsError::ArithmeticOverflow)
}

fn number_of_wins(
    context: &SwissStandingsContext<'_>,
    player: PlayerId,
) -> Result<u64, StandingsError> {
    let mut wins = 0u64;
    for record in context.records(player).iter().copied() {
        let counts = match context.tournament.config().system {
            SwissSystemConfig::Dutch(_) | SwissSystemConfig::Burstein(_) => {
                !matches!(
                    record,
                    PlayerRoundRecord::Match {
                        state: SwissMatchRecord::Pending { .. },
                        ..
                    }
                ) && round_game_points(context.tournament, record)?
                    == context.tournament.config().game_point_system.win.value() as u64
            }
            SwissSystemConfig::DoubleSwiss(config) => match record {
                PlayerRoundRecord::PairingAllocatedBye => config.bye_points.counts_as_win,
                PlayerRoundRecord::Match {
                    state:
                        SwissMatchRecord::Complete {
                            disposition,
                            aggregate,
                            ..
                        },
                    ..
                } => {
                    (disposition == MatchDisposition::Regular && aggregate == MatchScore::Win)
                        || disposition == MatchDisposition::ForfeitWin
                }
                PlayerRoundRecord::Match {
                    state: SwissMatchRecord::Pending { .. },
                    ..
                }
                | PlayerRoundRecord::RequestedBye
                | PlayerRoundRecord::Withdrawn => false,
            },
        };
        wins = wins
            .checked_add(u64::from(counts))
            .ok_or(StandingsError::ArithmeticOverflow)?;
    }
    Ok(wins)
}

fn games_won(
    context: &SwissStandingsContext<'_>,
    player: PlayerId,
    black_only: bool,
) -> Result<u64, StandingsError> {
    let double_swiss = matches!(
        context.tournament.config().system,
        SwissSystemConfig::DoubleSwiss(_)
    );
    let mut wins = 0u64;
    for record in context.records(player).iter().copied() {
        let PlayerRoundRecord::Match { state, .. } = record else {
            continue;
        };
        let completion = state.completion();
        for game in state.games().iter() {
            if black_only && game.color != Color::Black {
                continue;
            }
            let counts = matches!(game.outcome, GameSideOutcome::Played(MatchScore::Win))
                || (double_swiss
                    && completion
                        .is_some_and(|(disposition, _)| disposition == MatchDisposition::Regular)
                    && matches!(
                        game.outcome,
                        GameSideOutcome::Adjudicated(AdjudicatedSideResult::ForfeitWin)
                    ));
            wins = wins
                .checked_add(u64::from(counts))
                .ok_or(StandingsError::ArithmeticOverflow)?;
        }
    }
    Ok(wins)
}

fn scalar_value(
    context: &SwissStandingsContext<'_>,
    criterion_index: usize,
    criterion: SwissCriterion,
    player: PlayerId,
) -> Result<u64, StandingsError> {
    let result = match criterion {
        SwissCriterion::PrimaryScore => context.total(player, ScoreBasis::Primary),
        SwissCriterion::GamePoints => context.total(player, ScoreBasis::GamePoints),
        SwissCriterion::MatchPoints => context.total(player, ScoreBasis::MatchPoints),
        SwissCriterion::Buchholz(options) => buchholz(context, player, options),
        SwissCriterion::SonnebornBerger(options) => sonneborn_berger(context, player, options),
        SwissCriterion::Progressive(options) => progressive(context, player, options),
        SwissCriterion::TournamentPairingNumber(_) => tournament_pairing_number(context, player),
        SwissCriterion::NumberOfWins => number_of_wins(context, player),
        SwissCriterion::GamesWon => games_won(context, player, false),
        SwissCriterion::GamesWonWithBlack => games_won(context, player, true),
        SwissCriterion::DirectEncounter(_) => {
            unreachable!("Direct Encounter has no standalone scalar value")
        }
    };
    result.map_err(|error| reindex_invalid_criterion(error, criterion_index))
}

fn reindex_invalid_criterion(error: StandingsError, criterion_index: usize) -> StandingsError {
    match error {
        StandingsError::InvalidCriterion { reason, .. } => StandingsError::InvalidCriterion {
            criterion_index,
            reason,
        },
        StandingsError::ArithmeticOverflow => StandingsError::ArithmeticOverflow,
    }
}

fn direct_encounter_record_score(
    context: &SwissStandingsContext<'_>,
    record: PlayerRoundRecord,
    options: DirectEncounterOptions<ScoreBasis>,
) -> Result<Option<u64>, StandingsError> {
    let PlayerRoundRecord::Match { state, .. } = record else {
        return Ok(None);
    };
    let SwissMatchRecord::Complete {
        games, disposition, ..
    } = state
    else {
        return Ok(None);
    };
    let regular_double = matches!(
        context.tournament.config().system,
        SwissSystemConfig::DoubleSwiss(_)
    ) && disposition == MatchDisposition::Regular;
    match context.normalized_basis(options.score_basis)? {
        ScoreBasis::GamePoints => {
            let mut included = 0usize;
            let mut total = 0u64;
            for game in games.iter() {
                if regular_double
                    || game.outcome.counts_as_played()
                    || options.forfeits == DirectEncounterForfeitPolicy::IncludeAsScored
                {
                    included += 1;
                    total = total
                        .checked_add(
                            context
                                .tournament
                                .config()
                                .game_point_system
                                .points_for(game.outcome)
                                .value() as u64,
                        )
                        .ok_or(StandingsError::ArithmeticOverflow)?;
                }
            }
            Ok((included != 0).then_some(total))
        }
        ScoreBasis::MatchPoints => {
            if !regular_double && options.forfeits == DirectEncounterForfeitPolicy::Exclude {
                Ok(None)
            } else {
                round_points(context, record, ScoreBasis::MatchPoints).map(Some)
            }
        }
        ScoreBasis::Primary => unreachable!("the basis is normalized above"),
    }
}

#[derive(Clone, Copy, Default)]
struct DirectEncounterTally {
    points: u64,
    encounters: u64,
}

type DirectEncounterTable = HashMap<PlayerId, HashMap<PlayerId, DirectEncounterTally>>;

fn direct_encounter_tallies(
    context: &SwissStandingsContext<'_>,
    group: &[PlayerId],
    options: DirectEncounterOptions<ScoreBasis>,
) -> Result<DirectEncounterTable, StandingsError> {
    let mut result = HashMap::with_capacity(group.len());
    for &player in group {
        let mut by_opponent: HashMap<PlayerId, DirectEncounterTally> = HashMap::new();
        for record in context.records(player).iter().copied() {
            let PlayerRoundRecord::Match { opponent, .. } = record else {
                continue;
            };
            if !group.contains(&opponent) {
                continue;
            }
            if let Some(score) = direct_encounter_record_score(context, record, options)? {
                let entry = by_opponent.entry(opponent).or_default();
                entry.points = entry
                    .points
                    .checked_add(score)
                    .ok_or(StandingsError::ArithmeticOverflow)?;
                entry.encounters = entry
                    .encounters
                    .checked_add(1)
                    .ok_or(StandingsError::ArithmeticOverflow)?;
            }
        }
        result.insert(player, by_opponent);
    }
    Ok(result)
}

fn direct_encounter_tally(
    table: &DirectEncounterTable,
    player: PlayerId,
    opponent: PlayerId,
) -> DirectEncounterTally {
    table
        .get(&player)
        .and_then(|opponents| opponents.get(&opponent))
        .copied()
        .unwrap_or_default()
}

fn direct_encounter_contribution(
    tally: DirectEncounterTally,
    additional_score: Option<u64>,
    repeated_encounters: RepeatedEncounterPolicy,
) -> Result<ExactRatio, StandingsError> {
    let points = additional_score.map_or(Ok(tally.points), |score| {
        tally
            .points
            .checked_add(score)
            .ok_or(StandingsError::ArithmeticOverflow)
    })?;
    let encounters = tally
        .encounters
        .checked_add(u64::from(additional_score.is_some()))
        .ok_or(StandingsError::ArithmeticOverflow)?;
    if encounters == 0 {
        return Ok(ExactRatio::ZERO);
    }
    match repeated_encounters {
        RepeatedEncounterPolicy::Average => Ok(ExactRatio::new(points, encounters)),
        RepeatedEncounterPolicy::Sum => Ok(ExactRatio::integer(points)),
    }
}

fn direct_encounter_scores(
    table: &DirectEncounterTable,
    group: &[PlayerId],
    repeated_encounters: RepeatedEncounterPolicy,
) -> Result<Option<HashMap<PlayerId, ExactRatio>>, StandingsError> {
    let mut result = HashMap::with_capacity(group.len());
    for &player in group {
        let mut score = ExactRatio::ZERO;
        for &opponent in group {
            if opponent == player {
                continue;
            }
            let tally = direct_encounter_tally(table, player, opponent);
            if tally.encounters == 0 {
                return Ok(None);
            }
            score = score.checked_add(direct_encounter_contribution(
                tally,
                None,
                repeated_encounters,
            )?)?;
        }
        result.insert(player, score);
    }
    Ok(Some(result))
}

/// Scores for both participants in every ordinary played result of one
/// encounter. Article 6.3 varies missing sporting results; the forfeit policy
/// applies only to adjudications already present in the historical table.
fn ordinary_direct_encounter_outcomes(
    context: &SwissStandingsContext<'_>,
    basis: ScoreBasis,
) -> Result<Vec<(u64, u64)>, StandingsError> {
    let points = context.tournament.config().game_point_system;
    let one_game = [
        (points.win.value() as u64, points.loss.value() as u64),
        (points.draw.value() as u64, points.draw.value() as u64),
        (points.loss.value() as u64, points.win.value() as u64),
    ];
    if !matches!(
        context.tournament.config().system,
        SwissSystemConfig::DoubleSwiss(_)
    ) {
        return Ok(one_game.to_vec());
    }

    let normalized = context.normalized_basis(basis)?;
    let mut outcomes = Vec::with_capacity(9);
    for first in one_game {
        for second in one_game {
            let left_game_points = first
                .0
                .checked_add(second.0)
                .ok_or(StandingsError::ArithmeticOverflow)?;
            let right_game_points = first
                .1
                .checked_add(second.1)
                .ok_or(StandingsError::ArithmeticOverflow)?;
            let outcome = match normalized {
                ScoreBasis::GamePoints => (left_game_points, right_game_points),
                ScoreBasis::MatchPoints => {
                    let SwissSystemConfig::DoubleSwiss(config) = context.tournament.config().system
                    else {
                        unreachable!("Double-Swiss was checked above")
                    };
                    let left = MatchScore::from_ordering(left_game_points.cmp(&right_game_points));
                    (
                        config.match_point_system.regular_points(left).value() as u64,
                        config
                            .match_point_system
                            .regular_points(left.invert())
                            .value() as u64,
                    )
                }
                ScoreBasis::Primary => unreachable!("the basis is normalized above"),
            };
            if !outcomes.contains(&outcome) {
                outcomes.push(outcome);
            }
        }
    }
    Ok(outcomes)
}

#[derive(Clone, Copy)]
struct DirectEncounterBounds {
    lower: ExactRatio,
    upper: ExactRatio,
}

impl DirectEncounterBounds {
    const ZERO: Self = Self {
        lower: ExactRatio::ZERO,
        upper: ExactRatio::ZERO,
    };

    fn checked_add(self, other: Self) -> Result<Self, StandingsError> {
        Ok(Self {
            lower: self.lower.checked_add(other.lower)?,
            upper: self.upper.checked_add(other.upper)?,
        })
    }
}

fn missing_contribution_bounds(
    tally: DirectEncounterTally,
    ordinary_outcomes: &[(u64, u64)],
    repeated_encounters: RepeatedEncounterPolicy,
) -> Result<DirectEncounterBounds, StandingsError> {
    let mut scores = ordinary_outcomes.iter().map(|outcome| outcome.0);
    let first = scores
        .next()
        .expect("ordinary Swiss encounter outcomes must be non-empty");
    let first = direct_encounter_contribution(tally, Some(first), repeated_encounters)?;
    let mut bounds = DirectEncounterBounds {
        lower: first,
        upper: first,
    };
    for score in scores {
        let contribution = direct_encounter_contribution(tally, Some(score), repeated_encounters)?;
        bounds.lower = bounds.lower.checked_min(contribution)?;
        bounds.upper = bounds.upper.checked_max(contribution)?;
    }
    Ok(bounds)
}

fn independent_direct_encounter_bounds(
    table: &DirectEncounterTable,
    group: &[PlayerId],
    player: PlayerId,
    excluded_opponent: PlayerId,
    ordinary_outcomes: &[(u64, u64)],
    repeated_encounters: RepeatedEncounterPolicy,
) -> Result<DirectEncounterBounds, StandingsError> {
    let mut total = DirectEncounterBounds::ZERO;
    for &opponent in group {
        if opponent == player || opponent == excluded_opponent {
            continue;
        }
        let tally = direct_encounter_tally(table, player, opponent);
        let contribution = if tally.encounters == 0 {
            missing_contribution_bounds(tally, ordinary_outcomes, repeated_encounters)?
        } else {
            let exact = direct_encounter_contribution(tally, None, repeated_encounters)?;
            DirectEncounterBounds {
                lower: exact,
                upper: exact,
            }
        };
        total = total.checked_add(contribution)?;
    }
    Ok(total)
}

fn guaranteed_above_in_incomplete_table(
    table: &DirectEncounterTable,
    group: &[PlayerId],
    candidate: PlayerId,
    rival: PlayerId,
    ordinary_outcomes: &[(u64, u64)],
    repeated_encounters: RepeatedEncounterPolicy,
) -> Result<bool, StandingsError> {
    let candidate_bounds = independent_direct_encounter_bounds(
        table,
        group,
        candidate,
        rival,
        ordinary_outcomes,
        repeated_encounters,
    )?;
    let rival_bounds = independent_direct_encounter_bounds(
        table,
        group,
        rival,
        candidate,
        ordinary_outcomes,
        repeated_encounters,
    )?;
    let candidate_tally = direct_encounter_tally(table, candidate, rival);
    let rival_tally = direct_encounter_tally(table, rival, candidate);
    match (candidate_tally.encounters == 0, rival_tally.encounters == 0) {
        (false, false) => {
            let candidate_total =
                candidate_bounds
                    .lower
                    .checked_add(direct_encounter_contribution(
                        candidate_tally,
                        None,
                        repeated_encounters,
                    )?)?;
            let rival_total = rival_bounds
                .upper
                .checked_add(direct_encounter_contribution(
                    rival_tally,
                    None,
                    repeated_encounters,
                )?)?;
            Ok(candidate_total.checked_cmp(rival_total)? == Ordering::Greater)
        }
        (true, true) => {
            for &(candidate_score, rival_score) in ordinary_outcomes {
                let candidate_total =
                    candidate_bounds
                        .lower
                        .checked_add(direct_encounter_contribution(
                            candidate_tally,
                            Some(candidate_score),
                            repeated_encounters,
                        )?)?;
                let rival_total = rival_bounds
                    .upper
                    .checked_add(direct_encounter_contribution(
                        rival_tally,
                        Some(rival_score),
                        repeated_encounters,
                    )?)?;
                if candidate_total.checked_cmp(rival_total)? != Ordering::Greater {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        _ => panic!("Swiss Direct Encounter records must be symmetric"),
    }
}

fn direct_encounter_partition(
    context: &SwissStandingsContext<'_>,
    group: &[PlayerId],
    options: DirectEncounterOptions<ScoreBasis>,
) -> Result<Vec<Vec<PlayerId>>, StandingsError> {
    if group.len() < 2 {
        return Ok(vec![group.to_vec()]);
    }
    let table = direct_encounter_tallies(context, group, options)?;
    if table.values().all(|opponents| opponents.is_empty()) {
        return Ok(vec![group.to_vec()]);
    }
    let Some(scores) = direct_encounter_scores(&table, group, options.repeated_encounters)? else {
        // FIDE Play-Off and Tie-Break Regulations, effective 1 March 2026,
        // Article 6.3: assign only a participant guaranteed to be alone first
        // whatever the ordinary results of the missing internal encounters.
        // Continue assigning the guaranteed prefix in this same separate
        // table, then reapply Article 6 to the remaining unranked subgroup. A
        // guaranteed lower place never splits an unresolved higher position.
        let ordinary_outcomes = ordinary_direct_encounter_outcomes(context, options.score_basis)?;
        let mut unranked = group.to_vec();
        let mut partitions = Vec::new();
        loop {
            let mut guaranteed_player = None;
            for &candidate in &unranked {
                let mut guaranteed = true;
                for &rival in &unranked {
                    if rival != candidate
                        && !guaranteed_above_in_incomplete_table(
                            &table,
                            group,
                            candidate,
                            rival,
                            &ordinary_outcomes,
                            options.repeated_encounters,
                        )?
                    {
                        guaranteed = false;
                        break;
                    }
                }
                if guaranteed {
                    guaranteed_player = Some(candidate);
                    break;
                }
            }
            let Some(player) = guaranteed_player else {
                break;
            };
            partitions.push(vec![player]);
            unranked.retain(|candidate| *candidate != player);
            if unranked.is_empty() {
                return Ok(partitions);
            }
        }
        if partitions.is_empty() {
            return Ok(vec![group.to_vec()]);
        }
        partitions.extend(direct_encounter_partition(context, &unranked, options)?);
        return Ok(partitions);
    };
    let mut ordered = group.to_vec();
    let mut comparison_error = None;
    ordered.sort_by(
        |left, right| match scores[right].checked_cmp(scores[left]) {
            Ok(ordering) => ordering,
            Err(error) => {
                comparison_error = Some(error);
                Ordering::Equal
            }
        },
    );
    if let Some(error) = comparison_error {
        return Err(error);
    }

    let mut partitions: Vec<Vec<PlayerId>> = Vec::new();
    for player in ordered {
        let starts_new = partitions
            .last()
            .is_none_or(|previous| scores[&previous[0]] != scores[&player]);
        if starts_new {
            partitions.push(Vec::new());
        }
        partitions
            .last_mut()
            .expect("a partition exists")
            .push(player);
    }
    if partitions.len() == 1 {
        return Ok(partitions);
    }

    let mut recursively_resolved = Vec::new();
    for partition in partitions {
        if partition.len() > 1 {
            recursively_resolved.extend(direct_encounter_partition(context, &partition, options)?);
        } else {
            recursively_resolved.push(partition);
        }
    }
    Ok(recursively_resolved)
}

fn criterion_metric(
    context: &SwissStandingsContext<'_>,
    criterion_index: usize,
    criterion: SwissCriterion,
    player: PlayerId,
) -> Result<CriterionValue, StandingsError> {
    Ok(match criterion {
        SwissCriterion::DirectEncounter(_) => CriterionValue::NotApplicable,
        SwissCriterion::PrimaryScore | SwissCriterion::GamePoints | SwissCriterion::MatchPoints => {
            CriterionValue::Score(Score::new(
                u32::try_from(scalar_value(context, criterion_index, criterion, player)?)
                    .map_err(|_| StandingsError::ArithmeticOverflow)?,
            ))
        }
        SwissCriterion::Buchholz(_)
        | SwissCriterion::SonnebornBerger(_)
        | SwissCriterion::Progressive(_)
        | SwissCriterion::TournamentPairingNumber(_)
        | SwissCriterion::NumberOfWins
        | SwissCriterion::GamesWon
        | SwissCriterion::GamesWonWithBlack => {
            CriterionValue::Integer(scalar_value(context, criterion_index, criterion, player)?)
        }
    })
}

fn configured_standings(
    context: &SwissStandingsContext<'_>,
) -> Result<StandingsTable<SwissCriterion, CriterionValue>, StandingsError> {
    let plan = &context.tournament.config().standings;
    let groups = apply_criteria(
        &context.initial_order,
        &plan.criteria,
        |criterion_index, criterion, group| match *criterion {
            SwissCriterion::DirectEncounter(options) => {
                direct_encounter_partition(context, group, options)
                    .map_err(|error| reindex_invalid_criterion(error, criterion_index))
            }
            SwissCriterion::TournamentPairingNumber(TournamentPairingNumberOrder::Ascending) => {
                let mut values = HashMap::with_capacity(group.len());
                for &player in group {
                    values.insert(player, tournament_pairing_number(context, player)?);
                }
                Ok(partition_ascending(group, |player| values[&player]))
            }
            scalar => {
                let mut values = HashMap::with_capacity(group.len());
                for &player in group {
                    values.insert(
                        player,
                        scalar_value(context, criterion_index, scalar, player)?,
                    );
                }
                Ok(partition_descending(group, |player| values[&player]))
            }
        },
        |player| {
            plan.criteria
                .iter()
                .copied()
                .enumerate()
                .map(|(criterion_index, criterion)| {
                    criterion_metric(context, criterion_index, criterion, player)
                })
                .collect()
        },
    )?;
    Ok(StandingsTable {
        columns: plan.criteria.clone(),
        groups,
    })
}

impl SwissCalculation {
    /// Projects the configured standings and all score, result, and bye totals
    /// from the tournament's materialized history in one operation.
    ///
    /// Before all configured rounds are complete, Article 16 values are
    /// project-provided provisional "as if the tournament ended now"
    /// projections over materialized history. Later records may change an
    /// earlier requested/withdrawal-tail record from category 16.2.5 to 16.2.3
    /// without changing its VUR status. Completed-tournament values are
    /// normative for the supported rules.
    pub(crate) fn projection(
        &self,
        rounds: Vec<SwissRoundProjection>,
        history_status: SwissHistoryStatus,
    ) -> Result<SwissProjection, StandingsError> {
        let context = SwissStandingsContext::new(self)?;
        let standings = configured_standings(&context)?;
        let mut players = Vec::with_capacity(self.player_count());
        for &player in &context.initial_order {
            let projected = &context.players[player.index()];
            let counts = projected.counts;
            let score = |value| {
                u32::try_from(value)
                    .map(Score::new)
                    .map_err(|_| StandingsError::ArithmeticOverflow)
            };
            players.push(SwissPlayerSummary {
                player,
                primary_score: score(projected.primary_score)?,
                game_points: score(projected.game_points)?,
                match_points: score(projected.match_points)?,
                games_played: counts.games_played,
                matches_played: counts.matches_played,
                game_wins: counts.game_wins,
                game_draws: counts.game_draws,
                game_losses: counts.game_losses,
                match_wins: counts.match_wins,
                match_draws: counts.match_draws,
                match_losses: counts.match_losses,
                requested_byes: counts.requested_byes,
                pairing_allocated_byes: counts.pairing_allocated_byes,
            });
        }
        Ok(SwissProjection {
            standings,
            players,
            rounds,
            history_status,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Color;
    use crate::domain::{
        AdjudicatedGameOutcome, AdjudicatedSideResult, GameOutcome, Pairing, PointSystem,
    };
    use crate::standings::StandingsPlan;
    use crate::swiss::{
        AccelerationPolicy, ByeAssignment, ByeKind, DutchConfig, RoundPairings,
        SingleGameSwissByePoints, SwissConfig, SwissSystemConfig,
    };

    fn tournament(expected_rounds: u32) -> SwissCalculation {
        let points = PointSystem::STANDARD;
        SwissCalculation::new(
            SwissConfig {
                system: SwissSystemConfig::Dutch(DutchConfig {
                    bye_points: SingleGameSwissByePoints::game_win(points),
                }),
                expected_rounds,
                initial_color: Color::White,
                game_point_system: points,
                acceleration: AccelerationPolicy::None,
                standings: StandingsPlan::new(vec![SwissCriterion::PrimaryScore]),
            },
            &[PlayerId::new(0), PlayerId::new(1), PlayerId::new(2)],
        )
        .unwrap()
    }

    #[test]
    fn projection_emits_one_schema_and_complete_player_summaries() {
        let mut tournament = tournament(2);
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
            .apply_outcome_fact(PlayerId::new(0), PlayerId::new(1), GameOutcome::white_win())
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
            .apply_outcome_fact(
                PlayerId::new(0),
                PlayerId::new(1),
                GameOutcome::Adjudicated(
                    AdjudicatedGameOutcome::new(
                        AdjudicatedSideResult::ForfeitWin,
                        AdjudicatedSideResult::ForfeitLoss,
                    )
                    .unwrap(),
                ),
            )
            .unwrap();

        let projection = tournament
            .projection(Vec::new(), SwissHistoryStatus::ConfiguredRoundsComplete)
            .unwrap();
        assert_eq!(
            projection.standings.columns,
            vec![SwissCriterion::PrimaryScore]
        );
        assert!(
            projection
                .standings
                .groups
                .iter()
                .flat_map(|group| &group.players)
                .all(|row| row.values.len() == 1)
        );

        let player0 = projection
            .players
            .iter()
            .find(|summary| summary.player == PlayerId::new(0))
            .unwrap();
        assert_eq!(player0.games_played, 1);
        assert_eq!(player0.game_wins, 2);

        let player2 = projection
            .players
            .iter()
            .find(|summary| summary.player == PlayerId::new(2))
            .unwrap();
        assert_eq!(player2.pairing_allocated_byes, 1);
        assert_eq!(player2.requested_byes, 1);
        assert_eq!(player2.games_played, 0);
    }
}
