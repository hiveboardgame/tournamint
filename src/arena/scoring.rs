use super::config::ArenaConfig;
use crate::domain::MatchScore;

pub(super) const STREAK_THRESHOLD: u32 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ScoringError {
    ArithmeticOverflow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ScoredGame {
    pub(super) points: u32,
    pub(super) doubled: bool,
}

pub(super) struct ScoreInput<'a> {
    pub(super) config: &'a ArenaConfig,
    pub(super) streak_before: u32,
    pub(super) in_draw_streak: bool,
    pub(super) score: MatchScore,
    pub(super) berserked: bool,
    pub(super) plies: Option<u32>,
}

pub(super) const fn is_on_fire(config: &ArenaConfig, streak: u32) -> bool {
    config.scoring.fire && streak >= STREAK_THRESHOLD
}

/// Applies the supported scoring subset from lila's `ArenaSheet.scala` at
/// commit `60e225b83429835e08f7175539013f37f5eb5e4e`.
pub(super) fn score_game(input: ScoreInput<'_>) -> Result<ScoredGame, ScoringError> {
    let ScoreInput {
        config,
        streak_before,
        in_draw_streak,
        score,
        berserked,
        plies,
    } = input;
    let base: u32 = match score {
        MatchScore::Win => 2,
        MatchScore::Draw => 1,
        MatchScore::Loss => 0,
    };
    let is_draw = score == MatchScore::Draw;
    let below = |minimum: u32| plies.is_some_and(|plies| plies < minimum);
    let meets = |minimum: u32| plies.is_some_and(|plies| plies >= minimum);

    let quick_draw = is_draw
        && config
            .scoring
            .quick_draw
            .is_some_and(|policy| below(policy.minimum_scoring_plies));
    let on_fire = is_on_fire(config, streak_before);
    let repeated_draw = is_draw
        && !on_fire
        && in_draw_streak
        && config
            .scoring
            .repeated_draw
            .is_some_and(|policy| below(policy.minimum_unsuppressed_plies));
    let voided_draw = quick_draw || repeated_draw;
    let doubled = on_fire && !voided_draw && score != MatchScore::Loss;

    let mut points = if voided_draw {
        0
    } else if doubled {
        base.checked_mul(2)
            .expect("fixed Arena fire scoring fits u32")
    } else {
        base
    };
    if let Some(policy) = config
        .scoring
        .berserk
        .filter(|policy| berserked && score == MatchScore::Win && meets(policy.minimum_bonus_plies))
    {
        points = points
            .checked_add(policy.win_bonus.value())
            .ok_or(ScoringError::ArithmeticOverflow)?;
    }
    Ok(ScoredGame { points, doubled })
}

/// Exact performance contribution for one semantic win, draw, or loss against
/// a supplied opponent rating snapshot.
pub(super) fn performance_contribution(
    score: MatchScore,
    opponent_rating: Option<i32>,
) -> Option<i64> {
    let rating = opponent_rating?;
    let rating = i64::from(rating);
    let contribution = match score {
        MatchScore::Win => rating
            .checked_add(500)
            .expect("an Arena win performance contribution fits signed metric space"),
        MatchScore::Draw => rating,
        MatchScore::Loss => rating
            .checked_sub(500)
            .expect("an Arena loss performance contribution fits signed metric space"),
    };
    Some(contribution)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::{ArenaBerserkPolicy, ArenaQuickDrawPolicy, ArenaRepeatedDrawPolicy};
    use crate::domain::Score;

    fn plain_config() -> ArenaConfig {
        ArenaConfig::plain()
    }

    fn scored(
        config: &ArenaConfig,
        streak_before: u32,
        in_draw_streak: bool,
        score: MatchScore,
        berserked: bool,
        plies: Option<u32>,
    ) -> ScoredGame {
        score_game(ScoreInput {
            config,
            streak_before,
            in_draw_streak,
            score,
            berserked,
            plies,
        })
        .unwrap()
    }

    #[test]
    fn intrinsic_base_scoring_is_fixed() {
        let config = plain_config();
        let cases = [
            (MatchScore::Win, 2),
            (MatchScore::Draw, 1),
            (MatchScore::Loss, 0),
        ];

        for (score, expected_points) in cases {
            let result = scored(&config, 0, false, score, false, None);
            assert_eq!(result.points, expected_points, "{score:?}");
        }
    }

    #[test]
    fn quick_draw_policy_distinguishes_disabled_unknown_zero_below_and_equality() {
        let cases = [
            (None, Some(0), 1),
            (
                Some(ArenaQuickDrawPolicy {
                    minimum_scoring_plies: 10,
                }),
                None,
                1,
            ),
            (
                Some(ArenaQuickDrawPolicy {
                    minimum_scoring_plies: 0,
                }),
                Some(0),
                1,
            ),
            (
                Some(ArenaQuickDrawPolicy {
                    minimum_scoring_plies: 10,
                }),
                Some(9),
                0,
            ),
            (
                Some(ArenaQuickDrawPolicy {
                    minimum_scoring_plies: 10,
                }),
                Some(10),
                1,
            ),
        ];

        for (policy, plies, expected_points) in cases {
            let mut config = plain_config();
            config.scoring.quick_draw = policy;
            let result = scored(&config, 0, false, MatchScore::Draw, false, plies);
            assert_eq!(result.points, expected_points, "{policy:?}, {plies:?}");
        }
    }

    #[test]
    fn berserk_policy_distinguishes_disabled_unknown_zero_below_and_equality() {
        let cases = [
            (None, Some(0), 2),
            (
                Some(ArenaBerserkPolicy {
                    win_bonus: Score::new(3),
                    minimum_bonus_plies: 10,
                }),
                None,
                2,
            ),
            (
                Some(ArenaBerserkPolicy {
                    win_bonus: Score::new(3),
                    minimum_bonus_plies: 0,
                }),
                Some(0),
                5,
            ),
            (
                Some(ArenaBerserkPolicy {
                    win_bonus: Score::new(3),
                    minimum_bonus_plies: 10,
                }),
                Some(9),
                2,
            ),
            (
                Some(ArenaBerserkPolicy {
                    win_bonus: Score::new(3),
                    minimum_bonus_plies: 10,
                }),
                Some(10),
                5,
            ),
        ];

        for (policy, plies, expected_points) in cases {
            let mut config = plain_config();
            config.scoring.berserk = policy;
            let result = scored(&config, 0, false, MatchScore::Win, true, plies);
            assert_eq!(result.points, expected_points, "{policy:?}, {plies:?}");
        }
    }

    #[test]
    fn repeated_draw_policy_distinguishes_disabled_unknown_zero_below_and_equality() {
        let cases = [
            (None, Some(0), 1),
            (
                Some(ArenaRepeatedDrawPolicy {
                    minimum_unsuppressed_plies: 10,
                }),
                None,
                1,
            ),
            (
                Some(ArenaRepeatedDrawPolicy {
                    minimum_unsuppressed_plies: 0,
                }),
                Some(0),
                1,
            ),
            (
                Some(ArenaRepeatedDrawPolicy {
                    minimum_unsuppressed_plies: 10,
                }),
                Some(9),
                0,
            ),
            (
                Some(ArenaRepeatedDrawPolicy {
                    minimum_unsuppressed_plies: 10,
                }),
                Some(10),
                1,
            ),
        ];

        for (policy, plies, expected_points) in cases {
            let mut config = plain_config();
            config.scoring.repeated_draw = policy;
            let result = scored(&config, 0, true, MatchScore::Draw, false, plies);
            assert_eq!(result.points, expected_points, "{policy:?}, {plies:?}");
        }
    }

    #[test]
    fn quick_draw_precedes_fire_repeated_draw_is_off_fire_and_bonus_stays_flat() {
        let mut config = plain_config();
        config.scoring.fire = true;
        config.scoring.quick_draw = Some(ArenaQuickDrawPolicy {
            minimum_scoring_plies: 20,
        });
        config.scoring.repeated_draw = Some(ArenaRepeatedDrawPolicy {
            minimum_unsuppressed_plies: 60,
        });
        config.scoring.berserk = Some(ArenaBerserkPolicy {
            win_bonus: Score::new(1),
            minimum_bonus_plies: 0,
        });

        let quick_on_fire = scored(&config, 2, true, MatchScore::Draw, false, Some(19));
        assert_eq!(quick_on_fire.points, 0, "quick draw wins precedence");
        assert!(
            !quick_on_fire.doubled,
            "a voided draw applies no multiplier"
        );

        config.scoring.quick_draw = None;
        let repeated_but_on_fire = scored(&config, 2, true, MatchScore::Draw, false, Some(59));
        assert_eq!(
            repeated_but_on_fire.points, 2,
            "fire prevents repeated-draw suppression"
        );
        assert!(repeated_but_on_fire.doubled);

        let fire_berserk_win = scored(&config, 2, false, MatchScore::Win, true, Some(0));
        assert_eq!(fire_berserk_win.points, 5, "2 doubled, then flat +1");
        assert!(fire_berserk_win.doubled);

        let fire_loss = scored(&config, 2, false, MatchScore::Loss, false, None);
        assert!(
            !fire_loss.doubled,
            "a zero-point loss applies no multiplier"
        );
    }
}
