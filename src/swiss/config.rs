//! Immutable Swiss tournament configuration.

use crate::acceleration::{BakuAccelerationError, BakuScoreScale, validate_baku_score_scale};
use crate::standings::{
    DirectEncounterForfeitPolicy, DirectEncounterOptions, ProgressiveScoreOptions,
    RepeatedEncounterPolicy, SonnebornBergerOptions, StandingsPlan, TournamentPairingNumberOrder,
};
use crate::{Color, PointSystem, Score};

pub use crate::MatchPointSystem;

/// Pairing-allocated-bye score for a single-game Swiss system.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct SingleGameSwissByePoints {
    /// Game points awarded for a pairing-allocated bye.
    pub pairing_allocated: Score,
}

impl SingleGameSwissByePoints {
    /// Uses one configured game win, the built-in default.
    pub const fn game_win(point_system: PointSystem) -> Self {
        Self {
            pairing_allocated: point_system.win,
        }
    }
}

/// Pairing-allocated-bye projections for Double-Swiss.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct DoubleSwissByePoints {
    /// Game-point projection.
    pub game_points: Score,
    /// Match-point projection.
    pub match_points: Score,
    /// Whether the bye contributes one [`SwissCriterion::NumberOfWins`].
    pub counts_as_win: bool,
}

/// Why the standard Double-Swiss bye-point projection could not be built.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("standard Double-Swiss bye-point arithmetic overflowed")]
pub struct DoubleSwissByePointsError;

impl DoubleSwissByePoints {
    /// Uses one game win plus one draw, one match win, and marks the PAB as
    /// win-equivalent for [`SwissCriterion::NumberOfWins`].
    pub fn standard(
        game_points: PointSystem,
        match_points: MatchPointSystem,
    ) -> Result<Self, DoubleSwissByePointsError> {
        Ok(Self {
            game_points: Score::new(
                game_points
                    .win
                    .value()
                    .checked_add(game_points.draw.value())
                    .ok_or(DoubleSwissByePointsError)?,
            ),
            match_points: match_points.win,
            counts_as_win: true,
        })
    }
}

/// Dutch-system options.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct DutchConfig {
    /// Pairing-allocated-bye award.
    pub bye_points: SingleGameSwissByePoints,
}

/// Burstein-system options.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct BursteinConfig {
    /// Pairing-allocated-bye award.
    pub bye_points: SingleGameSwissByePoints,
}

/// Which real score drives Double-Swiss pairing and `PrimaryScore` standings.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub enum DoubleSwissPrimaryScore {
    /// Sum of both semantic game awards. This is the project default.
    #[default]
    GamePoints,
    /// Configured aggregate match points.
    MatchPoints,
}

/// Double-Swiss options.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct DoubleSwissConfig {
    /// Real score used by every score-dependent pairing decision.
    pub primary_score: DoubleSwissPrimaryScore,
    /// Aggregate match scoring.
    pub match_point_system: MatchPointSystem,
    /// Pairing-allocated-bye game and match projections.
    pub bye_points: DoubleSwissByePoints,
}

/// Selected Swiss pairing system and its native options.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub enum SwissSystemConfig {
    /// Dutch Swiss.
    Dutch(DutchConfig),
    /// Burstein Swiss.
    Burstein(BursteinConfig),
    /// Double-Swiss.
    DoubleSwiss(DoubleSwissConfig),
}

impl SwissSystemConfig {
    pub(crate) const fn pairing_system(self) -> SwissPairingSystem {
        match self {
            Self::Dutch(_) => SwissPairingSystem::Dutch,
            Self::Burstein(_) => SwissPairingSystem::Burstein,
            Self::DoubleSwiss(_) => SwissPairingSystem::DoubleSwiss,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SwissPairingSystem {
    Dutch,
    Burstein,
    DoubleSwiss,
}

/// Caller-defined flat acceleration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct FlatAcceleration {
    /// Number of early pairing rounds receiving the bonus.
    pub rounds: u32,
    /// Virtual pairing points in each accelerated round.
    pub bonus: Score,
}

/// Immutable pairing acceleration policy.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub enum AccelerationPolicy {
    /// No acceleration.
    #[default]
    None,
    /// The supported subset of FIDE C.04.7 Baku acceleration.
    Baku,
    /// Deliberate caller-defined flat acceleration.
    Flat(FlatAcceleration),
}

/// Score projection used by a Swiss standings criterion.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub enum ScoreBasis {
    /// Configured real primary score.
    Primary,
    /// Game-point projection.
    GamePoints,
    /// Match-point projection. Valid only for Double-Swiss.
    MatchPoints,
}

impl Default for DirectEncounterOptions<ScoreBasis> {
    fn default() -> Self {
        Self {
            score_basis: ScoreBasis::Primary,
            forfeits: DirectEncounterForfeitPolicy::Exclude,
            repeated_encounters: RepeatedEncounterPolicy::Average,
        }
    }
}

/// Opponent-score replacement for an unplayed round.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub enum UnplayedRoundPolicy {
    /// Apply FIDE Tie-Break Regulations (effective March 2026), Articles
    /// 16.3–16.5, including adjusted opponent scores, dummy caps, VURs, and
    /// the iterative low-cut exception. An incomplete tournament is projected
    /// provisionally from materialized history as if it ended now; later
    /// records may change an earlier requested/withdrawal-tail record from
    /// category 16.2.5 to 16.2.3 without changing its VUR status.
    /// Completed-tournament values are normative for the supported rules.
    #[default]
    FideMarch2026,
    /// Explicit FIDE Article 16.6 alternative. Unplayed own BH/SB elements are
    /// omitted, retained encounters use raw real opponent totals, no future
    /// rounds are synthesized, and this normalization occurs before cuts.
    Exclude,
}

/// Buchholz options supported by this project.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct BuchholzOptions {
    /// Number of lowest contributions to discard.
    pub cut_lowest: u32,
    /// Number of highest contributions to discard.
    pub cut_highest: u32,
    /// Treatment of unplayed rounds.
    pub unplayed: UnplayedRoundPolicy,
}

impl Default for SonnebornBergerOptions<ScoreBasis, UnplayedRoundPolicy> {
    fn default() -> Self {
        Self {
            score_basis: ScoreBasis::Primary,
            cut_lowest: 0,
            unplayed: UnplayedRoundPolicy::FideMarch2026,
        }
    }
}

impl Default for ProgressiveScoreOptions<ScoreBasis> {
    fn default() -> Self {
        Self {
            score_basis: ScoreBasis::Primary,
            cut_first_round: false,
        }
    }
}

/// A configurable Swiss standings criterion.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub enum SwissCriterion {
    /// Configured real primary score, excluding acceleration.
    PrimaryScore,
    /// Game-point projection.
    GamePoints,
    /// Match-point projection, valid only for Double-Swiss.
    MatchPoints,
    /// Buchholz.
    Buchholz(BuchholzOptions),
    /// Sonneborn-Berger, with optional iterative low cuts.
    SonnebornBerger(SonnebornBergerOptions<ScoreBasis, UnplayedRoundPolicy>),
    /// Progressive score, with an explicit basis and optional PS-C1.
    Progressive(ProgressiveScoreOptions<ScoreBasis>),
    /// One-based fixed Swiss Tournament Pairing Number.
    TournamentPairingNumber(TournamentPairingNumberOrder),
    /// Recursive subgroup-local Direct Encounter.
    DirectEncounter(DirectEncounterOptions<ScoreBasis>),
    /// Rounds awarding exactly one configured win, with Double-Swiss using
    /// one semantic match win per completed round.
    NumberOfWins,
    /// Games won over the board, plus single-game forfeits in technically
    /// regular Double-Swiss matches.
    GamesWon,
    /// [`Self::GamesWon`] restricted to games scheduled with Black.
    GamesWonWithBlack,
}

/// Ordered Swiss standings plan.
pub type SwissStandingsPlan = StandingsPlan<SwissCriterion>;

pub(crate) const fn burstein_dutch_opening_rounds(expected_rounds: usize) -> usize {
    let half = expected_rounds / 2;
    if half < 4 { half } else { 4 }
}

fn criterion_uses_match_points(criterion: SwissCriterion) -> bool {
    match criterion {
        SwissCriterion::MatchPoints => true,
        SwissCriterion::Progressive(options) => options.score_basis == ScoreBasis::MatchPoints,
        SwissCriterion::SonnebornBerger(options) => options.score_basis == ScoreBasis::MatchPoints,
        SwissCriterion::DirectEncounter(options) => options.score_basis == ScoreBasis::MatchPoints,
        SwissCriterion::PrimaryScore
        | SwissCriterion::GamePoints
        | SwissCriterion::Buchholz(_)
        | SwissCriterion::TournamentPairingNumber(_)
        | SwissCriterion::NumberOfWins
        | SwissCriterion::GamesWon
        | SwissCriterion::GamesWonWithBlack => false,
    }
}

/// Complete immutable Swiss configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwissConfig {
    /// Pairing system and native options.
    pub system: SwissSystemConfig,
    /// Total configured number of rounds.
    pub expected_rounds: u32,
    /// Initial colour assigned to the highest-ranked participant.
    pub initial_color: Color,
    /// Semantic game scoring.
    pub game_point_system: PointSystem,
    /// Pairing-only acceleration.
    pub acceleration: AccelerationPolicy,
    /// Default immutable standings plan.
    pub standings: SwissStandingsPlan,
}

impl SwissConfig {
    pub(crate) const fn expected_rounds_usize(&self) -> usize {
        self.expected_rounds as usize
    }

    /// Validates format-independent and cross-option invariants.
    pub(crate) fn validate(&self) -> Result<(), SwissConfigError> {
        if self.expected_rounds == 0 {
            return Err(SwissConfigError::ZeroExpectedRounds);
        }
        if !matches!(self.system, SwissSystemConfig::DoubleSwiss(_))
            && let Some(criterion_index) = self
                .standings
                .criteria
                .iter()
                .copied()
                .position(criterion_uses_match_points)
        {
            return Err(SwissConfigError::MatchPointsCriterionRequiresDoubleSwiss {
                criterion_index,
            });
        }
        if self.acceleration == AccelerationPolicy::Baku {
            let scale = match self.system {
                SwissSystemConfig::Dutch(_) | SwissSystemConfig::Burstein(_) => {
                    Some(BakuScoreScale::from_game_points(self.game_point_system))
                }
                SwissSystemConfig::DoubleSwiss(config)
                    if config.primary_score == DoubleSwissPrimaryScore::MatchPoints =>
                {
                    Some(BakuScoreScale::new(
                        config.match_point_system.win,
                        config.match_point_system.draw,
                        config.match_point_system.loss,
                    ))
                }
                SwissSystemConfig::DoubleSwiss(_) => None,
            }
            .ok_or(SwissConfigError::BakuHasNoSinglePrimaryWinValue)?;
            if let Err(error) = validate_baku_score_scale(scale) {
                return Err(match error {
                    BakuAccelerationError::InconsistentPointSystem => {
                        SwissConfigError::BakuIncompatibleScoring
                    }
                    BakuAccelerationError::ArithmeticOverflow => {
                        SwissConfigError::ArithmeticOverflow
                    }
                });
            }
        }
        Ok(())
    }
}

/// Why a Swiss configuration is invalid.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum SwissConfigError {
    /// At least one round is required.
    #[error("expected_rounds must be non-zero")]
    ZeroExpectedRounds,
    /// Baku requires a primary model with one defined participant win value.
    #[error("Baku is unsupported for Double-Swiss game-point primary scoring")]
    BakuHasNoSinglePrimaryWinValue,
    /// Baku requires win = two draws and loss = zero.
    #[error("Baku requires a primary point system where win equals two draws and loss is zero")]
    BakuIncompatibleScoring,
    /// A default standings criterion requests a score projection unavailable
    /// to the selected pairing system.
    #[error("standings criterion {criterion_index} uses MatchPoints outside Double-Swiss")]
    MatchPointsCriterionRequiresDoubleSwiss {
        /// Zero-based index into the configured standings plan.
        criterion_index: usize,
    },
    /// A configured exact value overflowed.
    #[error("Swiss configuration arithmetic overflowed")]
    ArithmeticOverflow,
}

impl Default for SwissStandingsPlan {
    fn default() -> Self {
        Self::new(vec![
            SwissCriterion::PrimaryScore,
            SwissCriterion::Buchholz(BuchholzOptions::default()),
            SwissCriterion::SonnebornBerger(SonnebornBergerOptions::default()),
            SwissCriterion::Progressive(ProgressiveScoreOptions::default()),
            SwissCriterion::DirectEncounter(DirectEncounterOptions::default()),
            SwissCriterion::NumberOfWins,
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn double_swiss(primary_score: DoubleSwissPrimaryScore) -> SwissConfig {
        SwissConfig {
            system: SwissSystemConfig::DoubleSwiss(DoubleSwissConfig {
                primary_score,
                match_point_system: MatchPointSystem::STANDARD,
                bye_points: DoubleSwissByePoints::standard(
                    PointSystem::STANDARD,
                    MatchPointSystem::STANDARD,
                )
                .unwrap(),
            }),
            expected_rounds: 5,
            initial_color: Color::White,
            game_point_system: PointSystem::STANDARD,
            acceleration: AccelerationPolicy::Baku,
            standings: SwissStandingsPlan::default(),
        }
    }

    #[test]
    fn double_swiss_game_points_reject_baku() {
        assert_eq!(
            double_swiss(DoubleSwissPrimaryScore::GamePoints).validate(),
            Err(SwissConfigError::BakuHasNoSinglePrimaryWinValue)
        );
    }

    #[test]
    fn compatible_match_points_accept_baku() {
        assert_eq!(
            double_swiss(DoubleSwissPrimaryScore::MatchPoints).validate(),
            Ok(())
        );
    }

    #[test]
    fn default_plan_is_the_documented_project_order() {
        assert_eq!(
            SwissStandingsPlan::default().criteria,
            vec![
                SwissCriterion::PrimaryScore,
                SwissCriterion::Buchholz(BuchholzOptions::default()),
                SwissCriterion::SonnebornBerger(SonnebornBergerOptions::default()),
                SwissCriterion::Progressive(ProgressiveScoreOptions::default()),
                SwissCriterion::DirectEncounter(DirectEncounterOptions::default()),
                SwissCriterion::NumberOfWins,
            ]
        );
    }

    #[test]
    fn burstein_dutch_opening_round_boundaries_follow_article_1_6() {
        assert_eq!(burstein_dutch_opening_rounds(1), 0);
        assert_eq!(burstein_dutch_opening_rounds(2), 1);
        assert_eq!(burstein_dutch_opening_rounds(8), 4);
        assert_eq!(burstein_dutch_opening_rounds(9), 4);
        assert_eq!(burstein_dutch_opening_rounds(10), 4);
    }

    #[test]
    fn single_game_systems_reject_every_match_point_default_criterion() {
        let criteria = [
            SwissCriterion::MatchPoints,
            SwissCriterion::Progressive(ProgressiveScoreOptions {
                score_basis: ScoreBasis::MatchPoints,
                cut_first_round: false,
            }),
            SwissCriterion::SonnebornBerger(SonnebornBergerOptions {
                score_basis: ScoreBasis::MatchPoints,
                cut_lowest: 0,
                unplayed: UnplayedRoundPolicy::FideMarch2026,
            }),
            SwissCriterion::DirectEncounter(DirectEncounterOptions {
                score_basis: ScoreBasis::MatchPoints,
                ..DirectEncounterOptions::default()
            }),
        ];
        for system in [
            SwissSystemConfig::Dutch(DutchConfig {
                bye_points: SingleGameSwissByePoints::game_win(PointSystem::STANDARD),
            }),
            SwissSystemConfig::Burstein(BursteinConfig {
                bye_points: SingleGameSwissByePoints::game_win(PointSystem::STANDARD),
            }),
        ] {
            for criterion in criteria {
                let config = SwissConfig {
                    system,
                    expected_rounds: 5,
                    initial_color: Color::White,
                    game_point_system: PointSystem::STANDARD,
                    acceleration: AccelerationPolicy::None,
                    standings: SwissStandingsPlan::new(vec![
                        SwissCriterion::PrimaryScore,
                        criterion,
                    ]),
                };
                assert_eq!(
                    config.validate(),
                    Err(SwissConfigError::MatchPointsCriterionRequiresDoubleSwiss {
                        criterion_index: 1,
                    })
                );
            }
        }
    }
}
