//! Score acceleration: the pinned FIDE Baku policy and a project-defined
//! flat bonus.
//!
//! Derived from BBP Pairings (Apache-2.0, Copyright 2016-2026 Jeremy Bierema),
//! and modified; the flat bonus is new.

use crate::domain::{PlayerCount, PointSystem, Score};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BakuScoreScale {
    pub(crate) win: Score,
    pub(crate) draw: Score,
    pub(crate) loss: Score,
}

impl BakuScoreScale {
    pub(crate) const fn new(win: Score, draw: Score, loss: Score) -> Self {
        Self { win, draw, loss }
    }

    pub(crate) const fn from_game_points(point_system: PointSystem) -> Self {
        Self::new(point_system.win, point_system.draw, point_system.loss)
    }
}

/// Why FIDE Baku acceleration could not be applied.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum BakuAccelerationError {
    /// Baku's bonus is defined in half-points, so it only makes sense in a
    /// point system scaled like 2 / 1 / 0.
    #[error(
        "Baku acceleration needs a point system where a win is worth exactly two draws and a loss nothing"
    )]
    InconsistentPointSystem,
    /// Exact acceleration configuration arithmetic overflowed.
    #[error("acceleration configuration arithmetic overflowed")]
    ArithmeticOverflow,
}

/// FIDE C.04.7: Baku's accelerated group has size
/// `2 * ceil(player_count / 4)`.
pub(crate) fn baku_accelerated_rank_count(
    players_by_rank_count: PlayerCount,
) -> Result<PlayerCount, BakuAccelerationError> {
    let players_by_rank_len = players_by_rank_count.value();
    if players_by_rank_len == 0 {
        return Ok(PlayerCount::new(0));
    }
    let quarter_groups = (players_by_rank_len - 1) / 4 + 1;
    Ok(PlayerCount::new(quarter_groups * 2))
}

/// Project-defined flat acceleration retains its independent top-half,
/// rounded-up cutoff.
pub(crate) fn flat_accelerated_rank_count(players_by_rank_count: PlayerCount) -> PlayerCount {
    let players_by_rank_len = players_by_rank_count.value();
    if players_by_rank_len == 0 {
        return PlayerCount::new(0);
    }
    PlayerCount::new((players_by_rank_len - 1) / 2 + 1)
}

fn baku_acceleration_plan(
    players_by_rank_count: PlayerCount,
    expected_rounds: usize,
    played_rounds: usize,
    score_scale: BakuScoreScale,
) -> Result<BakuAccelerationPlan, BakuAccelerationError> {
    let players_by_rank_len = players_by_rank_count.value();
    validate_baku_score_scale(score_scale)?;

    if players_by_rank_len == 0 || expected_rounds == 0 {
        return Ok(BakuAccelerationPlan {
            accelerated_rank_count: PlayerCount::new(0),
            win_acceleration_count: 0,
            draw_acceleration_count: 0,
        });
    }

    let accelerated_rank_count = baku_accelerated_rank_count(players_by_rank_count)?.value();
    let last_acceleration_round = (expected_rounds - 1) / 2;
    let last_full_acceleration_round = last_acceleration_round / 2;

    let last_win_round = core::cmp::min(last_full_acceleration_round, played_rounds);
    let win_acceleration_count = last_win_round + 1;
    let next_round = win_acceleration_count;
    let last_draw_round = core::cmp::min(last_acceleration_round, played_rounds);
    let draw_acceleration_count = if next_round <= last_draw_round {
        last_draw_round - next_round + 1
    } else {
        0
    };

    Ok(BakuAccelerationPlan {
        accelerated_rank_count: PlayerCount::new(accelerated_rank_count),
        win_acceleration_count,
        draw_acceleration_count,
    })
}

pub(crate) fn validate_baku_score_scale(
    score_scale: BakuScoreScale,
) -> Result<(), BakuAccelerationError> {
    let double_draw = score_scale
        .draw
        .value()
        .checked_add(score_scale.draw.value())
        .ok_or(BakuAccelerationError::ArithmeticOverflow)?;
    if score_scale.loss != Score::new(0) || double_draw != score_scale.win.value() {
        return Err(BakuAccelerationError::InconsistentPointSystem);
    }
    Ok(())
}

struct BakuAccelerationPlan {
    accelerated_rank_count: PlayerCount,
    win_acceleration_count: usize,
    draw_acceleration_count: usize,
}

pub(crate) fn apply_baku_acceleration(
    players_by_rank_count: PlayerCount,
    expected_rounds: usize,
    played_rounds: usize,
    score_scale: BakuScoreScale,
) -> Result<BakuAccelerationResult, BakuAccelerationError> {
    let plan = baku_acceleration_plan(
        players_by_rank_count,
        expected_rounds,
        played_rounds,
        score_scale,
    )?;
    let acceleration_points_len = plan.win_acceleration_count + plan.draw_acceleration_count;
    let mut acceleration_points = Vec::with_capacity(acceleration_points_len);
    acceleration_points.extend(std::iter::repeat_n(
        score_scale.win,
        plan.win_acceleration_count,
    ));
    acceleration_points.extend(std::iter::repeat_n(
        score_scale.draw,
        plan.draw_acceleration_count,
    ));

    Ok(BakuAccelerationResult {
        accelerated_rank_count: plan.accelerated_rank_count,
        acceleration_points,
    })
}

pub(crate) struct BakuAccelerationResult {
    pub(crate) accelerated_rank_count: PlayerCount,
    pub(crate) acceleration_points: Vec<Score>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn baku_acceleration_returns_typed_points() {
        let acceleration = apply_baku_acceleration(
            PlayerCount::new(4),
            5,
            3,
            BakuScoreScale::from_game_points(PointSystem {
                win: Score::new(10),
                draw: Score::new(5),
                loss: Score::new(0),
                forfeit_loss: Score::new(0),
            }),
        )
        .unwrap();

        assert_eq!(acceleration.accelerated_rank_count, PlayerCount::new(2));
        assert_eq!(
            acceleration.acceleration_points,
            vec![Score::new(10), Score::new(10), Score::new(5)]
        );
    }

    #[test]
    fn baku_acceleration_rejects_inconsistent_point_system() {
        assert!(matches!(
            apply_baku_acceleration(
                PlayerCount::new(4),
                5,
                3,
                BakuScoreScale::from_game_points(PointSystem {
                    win: Score::new(10),
                    draw: Score::new(4),
                    loss: Score::new(0),
                    forfeit_loss: Score::new(0),
                }),
            ),
            Err(BakuAccelerationError::InconsistentPointSystem)
        ));
    }

    #[test]
    fn baku_score_scale_overflow_is_arithmetic_not_capacity() {
        assert_eq!(
            validate_baku_score_scale(BakuScoreScale::new(
                Score::new(u32::MAX),
                Score::new(u32::MAX / 2 + 1),
                Score::new(0),
            )),
            Err(BakuAccelerationError::ArithmeticOverflow)
        );
    }

    #[test]
    fn baku_group_size_uses_two_times_ceiling_of_a_quarter() {
        assert_eq!(
            baku_accelerated_rank_count(PlayerCount::new(6)).unwrap(),
            PlayerCount::new(4)
        );
        assert_eq!(
            baku_accelerated_rank_count(PlayerCount::new(161)).unwrap(),
            PlayerCount::new(82)
        );
    }

    #[test]
    fn flat_and_baku_cutoffs_are_independent() {
        assert_eq!(
            flat_accelerated_rank_count(PlayerCount::new(161)),
            PlayerCount::new(81)
        );
        assert_eq!(
            baku_accelerated_rank_count(PlayerCount::new(161)).unwrap(),
            PlayerCount::new(82)
        );
    }
}
