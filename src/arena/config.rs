use crate::domain::{Color, Score};

/// Games that must pass before the same opponents may meet again.
pub const DEFAULT_REMATCH_COOLDOWN: u32 = 1;
/// Minimum supported wait before a two-active-player rematch.
pub const MIN_REMATCH_WAIT_SECS: u32 = 8;
/// Maximum supported wait before a two-active-player rematch.
pub const MAX_REMATCH_WAIT_SECS: u32 = 50;
/// Default wait before a two-active-player rematch. Hosts may derive it from their clock.
pub const DEFAULT_REMATCH_WAIT_SECS: u32 = 15;
/// A winning berserker's flat bonus in the default scoring policy.
pub const DEFAULT_BERSERK_BONUS: Score = Score::new(1);
/// Minimum played plies required for the default berserk win bonus.
pub const DEFAULT_BERSERK_MINIMUM_BONUS_PLIES: u32 = 14;
/// Minimum played plies required for a draw to score by default.
pub const DEFAULT_QUICK_DRAW_MINIMUM_SCORING_PLIES: u32 = 20;
/// Minimum played plies at which a repeated draw is not suppressed by default.
pub const DEFAULT_REPEATED_DRAW_MINIMUM_UNSUPPRESSED_PLIES: u32 = 60;
/// Longest permitted run of one colour in the default policy.
pub const DEFAULT_MAX_COLOR_STRIKE: u32 = 3;

/// How the waiting pool is searched.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum ArenaPairingPolicy {
    /// Deterministically order and partition the waiting pool into bounded
    /// groups, solve each group exactly for cardinality then quality, and
    /// exactly repair the unmatched pool. Already-fixed group pairs are not
    /// reconsidered, so the result is not a complete-pool global optimum.
    Grouped,
    /// Deterministically let each longest waiter take their best remaining
    /// legal opponent. This intentionally makes no global-optimality claim.
    Greedy,
}

/// A played berserk's scoring policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct ArenaBerserkPolicy {
    /// Flat bonus added to an eligible win. Fire never multiplies it.
    pub win_bonus: Score,
    /// Minimum played plies required for the winning bonus. Equality qualifies.
    pub minimum_bonus_plies: u32,
}

/// Quick-draw scoring policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct ArenaQuickDrawPolicy {
    /// Minimum played plies required for a draw to score. Equality qualifies;
    /// a known lower count suppresses the draw.
    pub minimum_scoring_plies: u32,
}

/// Repeated-draw scoring policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct ArenaRepeatedDrawPolicy {
    /// Minimum played plies at which a repeated draw is not suppressed.
    /// Equality qualifies; a known lower count may suppress the draw.
    pub minimum_unsuppressed_plies: u32,
}

/// Ply boundaries for Arena scoring adjustments.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScoringThresholds {
    /// Minimum plies at which a quick draw scores instead of being suppressed.
    pub quick_draw_minimum_scoring_plies: u32,
    /// Minimum plies at which a winning berserker receives the flat bonus.
    pub berserk_minimum_bonus_plies: u32,
    /// Minimum plies at which a repeated draw escapes suppression.
    pub repeated_draw_minimum_unsuppressed_plies: u32,
}

impl Default for ScoringThresholds {
    fn default() -> Self {
        Self {
            berserk_minimum_bonus_plies: DEFAULT_BERSERK_MINIMUM_BONUS_PLIES,
            quick_draw_minimum_scoring_plies: DEFAULT_QUICK_DRAW_MINIMUM_SCORING_PLIES,
            repeated_draw_minimum_unsuppressed_plies:
                DEFAULT_REPEATED_DRAW_MINIMUM_UNSUPPRESSED_PLIES,
        }
    }
}

/// Optional Arena scoring policies layered over intrinsic 2/1/0 base scoring.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct ArenaScoringConfig {
    /// Enable fire scoring after two consecutive wins.
    pub fire: bool,
    /// Recognize and count played-result berserk flags, and award eligible win
    /// bonuses. `None` rejects played results with either flag set.
    pub berserk: Option<ArenaBerserkPolicy>,
    /// Quick-draw suppression, or `None` when disabled.
    pub quick_draw: Option<ArenaQuickDrawPolicy>,
    /// Repeated-short-draw suppression, or `None` when disabled.
    pub repeated_draw: Option<ArenaRepeatedDrawPolicy>,
}

impl ArenaScoringConfig {
    /// No fire, berserk, quick-draw, or repeated-draw rules.
    /// Played games with berserk flags are rejected under this policy.
    pub const fn plain() -> Self {
        Self {
            fire: false,
            berserk: None,
            quick_draw: None,
            repeated_draw: None,
        }
    }

    /// The default scoring policy shape with caller-selected ply boundaries.
    pub const fn with_thresholds(thresholds: ScoringThresholds) -> Self {
        Self {
            fire: true,
            berserk: Some(ArenaBerserkPolicy {
                win_bonus: DEFAULT_BERSERK_BONUS,
                minimum_bonus_plies: thresholds.berserk_minimum_bonus_plies,
            }),
            quick_draw: Some(ArenaQuickDrawPolicy {
                minimum_scoring_plies: thresholds.quick_draw_minimum_scoring_plies,
            }),
            repeated_draw: Some(ArenaRepeatedDrawPolicy {
                minimum_unsuppressed_plies: thresholds.repeated_draw_minimum_unsuppressed_plies,
            }),
        }
    }
}

/// Immutable configuration of one Arena.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct ArenaConfig {
    /// Optional policies layered over intrinsic 2/1/0 base scoring.
    pub scoring: ArenaScoringConfig,
    /// Grouped exact-plus-repair search or explicit deterministic Greedy.
    pub pairing_policy: ArenaPairingPolicy,
    /// Per-player game-history window which vetoes a rematch.
    pub rematch_cooldown: u32,
    /// Seconds both players must have waited before a recent-opponent rematch
    /// when exactly two entrants are active. Must be in `8..=50`. First meetings
    /// and opponents outside the history window do not require this wait.
    pub rematch_wait_secs: u32,
    /// Longest run of one colour. Must be non-zero.
    pub max_color_strike: u32,
    /// Colour assigned to the better Arena rank when colour histories are
    /// equal. If the rank also ties, dense player-id order decides who leads.
    pub initial_color: Color,
}

/// Why an Arena configuration cannot be used.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ArenaSetupError {
    /// A zero colour limit makes all games structurally impossible.
    #[error("max_color_strike must be at least one")]
    ZeroMaxColorStrike,
    /// A two-active-player rematch wait must be between 8 and 50 seconds.
    #[error("rematch_wait_secs must be between 8 and 50")]
    InvalidRematchWaitSecs,
}

impl ArenaConfig {
    /// A plain 2/1/0 Arena without fire, berserk, or draw suppression.
    pub const fn plain() -> Self {
        Self {
            scoring: ArenaScoringConfig::plain(),
            pairing_policy: ArenaPairingPolicy::Grouped,
            rematch_cooldown: DEFAULT_REMATCH_COOLDOWN,
            rematch_wait_secs: DEFAULT_REMATCH_WAIT_SECS,
            max_color_strike: DEFAULT_MAX_COLOR_STRIKE,
            initial_color: Color::White,
        }
    }

    /// The default policy shape with caller-selected scoring thresholds.
    pub const fn with_scoring_thresholds(thresholds: ScoringThresholds) -> Self {
        Self {
            scoring: ArenaScoringConfig::with_thresholds(thresholds),
            pairing_policy: ArenaPairingPolicy::Grouped,
            rematch_cooldown: DEFAULT_REMATCH_COOLDOWN,
            rematch_wait_secs: DEFAULT_REMATCH_WAIT_SECS,
            max_color_strike: DEFAULT_MAX_COLOR_STRIKE,
            initial_color: Color::White,
        }
    }

    /// Validates the rules before a host persists a new Arena configuration.
    pub fn validate(&self) -> Result<(), ArenaSetupError> {
        if self.max_color_strike == 0 {
            return Err(ArenaSetupError::ZeroMaxColorStrike);
        }
        if !(MIN_REMATCH_WAIT_SECS..=MAX_REMATCH_WAIT_SECS).contains(&self.rematch_wait_secs) {
            return Err(ArenaSetupError::InvalidRematchWaitSecs);
        }
        Ok(())
    }
}

impl Default for ArenaConfig {
    fn default() -> Self {
        Self::with_scoring_thresholds(ScoringThresholds::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_uses_canonical_policy() {
        let config = ArenaConfig::default();
        assert_eq!(config.pairing_policy, ArenaPairingPolicy::Grouped);
        assert_eq!(config.rematch_cooldown, 1);
        assert_eq!(config.rematch_wait_secs, 15);
        assert_eq!(config.max_color_strike, 3);
        assert_eq!(config.initial_color, Color::White);
        assert!(config.scoring.fire);
        assert_eq!(config.scoring.berserk.unwrap().win_bonus, Score::new(1));
        assert_eq!(config.scoring.berserk.unwrap().minimum_bonus_plies, 14);
        assert_eq!(config.scoring.quick_draw.unwrap().minimum_scoring_plies, 20);
        assert_eq!(
            config
                .scoring
                .repeated_draw
                .unwrap()
                .minimum_unsuppressed_plies,
            60
        );
    }

    #[test]
    fn scoring_thresholds_are_customizable() {
        let thresholds = ScoringThresholds {
            berserk_minimum_bonus_plies: 7,
            quick_draw_minimum_scoring_plies: 11,
            repeated_draw_minimum_unsuppressed_plies: 13,
        };
        let config = ArenaConfig::with_scoring_thresholds(thresholds);

        assert_eq!(config.scoring.berserk.unwrap().minimum_bonus_plies, 7);
        assert_eq!(config.scoring.quick_draw.unwrap().minimum_scoring_plies, 11);
        assert_eq!(
            config
                .scoring
                .repeated_draw
                .unwrap()
                .minimum_unsuppressed_plies,
            13
        );
    }
}
