use tournamint::round_robin::{
    self, KoyaOptions, RoundRobinConfig, RoundRobinCriterion as Criterion, RoundRobinError,
    RoundRobinGameFact, RoundRobinPrimaryScore as PrimaryScore, RoundRobinProgressiveOptions,
    RoundRobinStandingsPlan,
};
use tournamint::standings::{CriterionValue, DirectEncounterForfeitPolicy};
use tournamint::{
    AdjudicatedGameOutcome, AdjudicatedSideResult as Award, Color, GameOutcome, MatchDisposition,
    MatchPointSystem, PlayerId, PointSystem, Score,
};

fn config(repeats: u32, primary_score: PrimaryScore, criteria: Vec<Criterion>) -> RoundRobinConfig {
    RoundRobinConfig {
        repeats,
        primary_score,
        initial_color: Color::White,
        point_system: PointSystem::STANDARD,
        match_point_system: MatchPointSystem::STANDARD,
        standings: RoundRobinStandingsPlan::new(criteria),
    }
}

fn facts(
    config: &RoundRobinConfig,
    players: &[PlayerId],
    mut outcome: impl FnMut(usize, usize, usize) -> GameOutcome,
) -> Vec<RoundRobinGameFact> {
    round_robin::schedule(config, players)
        .unwrap()
        .rounds
        .into_iter()
        .flat_map(|round| round.games)
        .map(|game| RoundRobinGameFact {
            id: game.id,
            outcome: outcome(game.white.index(), game.black.index(), game.pass_index),
        })
        .collect()
}

fn award(white: Award, black: Award) -> GameOutcome {
    GameOutcome::Adjudicated(AdjudicatedGameOutcome::new(white, black).unwrap())
}

#[test]
fn all_even_repeats_form_one_match_and_wait_for_every_result_including_after_correction() {
    let players = [PlayerId::new(0), PlayerId::new(1)];
    for repeats in [2, 4, 6] {
        let config = config(
            repeats,
            PrimaryScore::MatchPoints,
            vec![Criterion::PrimaryScore],
        );
        let mut results = facts(&config, &players, |white, _, _| {
            if white == 0 {
                GameOutcome::white_win()
            } else {
                GameOutcome::black_win()
            }
        });
        let completed = round_robin::project(&config, &players, &results).unwrap();
        assert_eq!(completed.matches.len(), 1);
        assert_eq!(completed.matches[0].games.len(), repeats as usize);
        assert_eq!(completed.players[0].primary_score, Score::new(2));
        assert_eq!(completed.players[0].game_points, Score::new(repeats * 2));
        assert_eq!(completed.players[0].match_wins, 1);
        assert_eq!(completed.players[0].matches_played, Some(1));
        // Reopening any game removes the match award even when its winner is already certain.
        results.remove(0);
        results.reverse();
        let pending = round_robin::project(&config, &players, &results).unwrap();
        assert!(pending.matches[0].completed.is_none());
        assert_eq!(pending.players[0].primary_score, Score::new(0));
        assert_eq!(
            pending.players[0].game_points,
            Score::new((repeats - 1) * 2)
        );
        assert_eq!(pending.players[0].matches_played, Some(0));
    }
}

#[test]
fn full_partial_split_and_double_forfeits_have_the_agreed_match_awards() {
    let players = [PlayerId::new(0), PlayerId::new(1)];
    let config = config(4, PrimaryScore::MatchPoints, vec![Criterion::PrimaryScore]);
    for (case, expected, dispositions) in [
        (0, [2, 0], [MatchDisposition::Regular; 2]),
        (
            1,
            [0, 2],
            [MatchDisposition::ForfeitLoss, MatchDisposition::ForfeitWin],
        ),
        (2, [0, 0], [MatchDisposition::DoubleForfeit; 2]),
        (3, [1, 1], [MatchDisposition::Regular; 2]),
        (4, [1, 1], [MatchDisposition::Regular; 2]),
    ] {
        let results = facts(&config, &players, |white, _, pass| {
            let (first, second) = match case {
                0 if pass < 3 => {
                    return if white == 0 {
                        GameOutcome::white_win()
                    } else {
                        GameOutcome::black_win()
                    };
                }
                0 | 1 => (Award::ForfeitLoss, Award::ForfeitWin),
                2 => (Award::DoubleForfeit, Award::DoubleForfeit),
                3 if pass < 2 => (Award::ForfeitLoss, Award::ForfeitWin),
                3 => (Award::ForfeitWin, Award::ForfeitLoss),
                4 if pass == 0 => return GameOutcome::draw(),
                _ => (Award::DoubleForfeit, Award::DoubleForfeit),
            };
            if white == 0 {
                award(first, second)
            } else {
                award(second, first)
            }
        });
        let projected = round_robin::project(&config, &players, &results).unwrap();
        assert_eq!(
            projected
                .players
                .iter()
                .map(|player| player.match_points.value())
                .collect::<Vec<_>>(),
            expected
        );
        let completion = projected.matches[0].completed.unwrap();
        let expected_dispositions = if projected.matches[0].participants[0] == players[0] {
            dispositions
        } else {
            [dispositions[1], dispositions[0]]
        };
        assert_eq!(completion.dispositions, expected_dispositions);
    }
}

#[test]
fn match_scoring_is_rejected_for_odd_repeats_at_schedule_and_projection_boundaries() {
    let players = [PlayerId::new(0), PlayerId::new(1)];
    for repeats in [1, 3, 5] {
        for (primary, criteria) in [
            (PrimaryScore::MatchPoints, vec![Criterion::PrimaryScore]),
            (
                PrimaryScore::GamePoints,
                vec![Criterion::PrimaryScore, Criterion::MatchPoints],
            ),
            (PrimaryScore::GamePoints, vec![Criterion::MatchesWon]),
        ] {
            let config = config(repeats, primary, criteria);
            assert_eq!(
                round_robin::schedule(&config, &players),
                Err(RoundRobinError::MatchScoringRequiresEvenRepeats)
            );
            assert!(round_robin::project(&config, &players, &[]).is_err());
        }
    }
}

#[test]
fn score_based_tiebreaks_use_primary_match_totals_and_one_encounter_per_opponent() {
    let players = [0, 1, 2].map(PlayerId::new);
    let mut config = config(
        2,
        PrimaryScore::MatchPoints,
        vec![
            Criterion::PrimaryScore,
            Criterion::SonnebornBerger(Default::default()),
            Criterion::Koya(KoyaOptions::default()),
            Criterion::GamePoints,
        ],
    );
    // A beats B 1½–½, B sweeps C, C beats A 1½–½.
    let results = facts(&config, &players, |white, black, pass| {
        let winner = match (white.min(black), white.max(black)) {
            (0, 1) => 0,
            (1, 2) => 1,
            _ => 2,
        };
        if pass == 1 && (white.min(black), white.max(black)) != (1, 2) {
            GameOutcome::draw()
        } else if white == winner {
            GameOutcome::white_win()
        } else {
            GameOutcome::black_win()
        }
    });
    let projected = round_robin::project(&config, &players, &results).unwrap();
    let rows = projected
        .standings
        .groups
        .iter()
        .flat_map(|group| &group.players)
        .collect::<Vec<_>>();
    assert_eq!(
        rows.iter()
            .map(|row| row.player.index())
            .collect::<Vec<_>>(),
        [1, 0, 2]
    );
    for row in &rows {
        assert_eq!(
            &row.values[..3],
            &[
                CriterionValue::Score(Score::new(2)),
                CriterionValue::Integer(4),
                CriterionValue::Score(Score::new(2))
            ]
        );
    }
    // An odd-field resting round never becomes an awarded match.
    assert_eq!(projected.matches.len(), 3);
    assert!(
        projected
            .players
            .iter()
            .all(|player| player.matches_played == Some(2))
    );
    config.primary_score = PrimaryScore::GamePoints;
    config.standings = RoundRobinStandingsPlan::new(vec![
        Criterion::SonnebornBerger(Default::default()),
        Criterion::Koya(KoyaOptions::default()),
    ]);
    let projected = round_robin::project(&config, &players, &results).unwrap();
    for row in projected
        .standings
        .groups
        .iter()
        .flat_map(|group| &group.players)
    {
        let index = row.player.index();
        assert_eq!(
            row.values,
            vec![
                CriterionValue::Integer([18, 16, 12][index]),
                CriterionValue::Score(Score::new([3, 1, 3][index]))
            ]
        );
    }
}

#[test]
fn match_points_break_game_point_ties_and_direct_encounter_uses_completed_matches() {
    let players = [0, 1, 2].map(PlayerId::new);
    let mut config = config(
        4,
        PrimaryScore::GamePoints,
        vec![Criterion::PrimaryScore, Criterion::MatchPoints],
    );
    let results = facts(&config, &players, |white, black, pass| {
        let winner = match (white.min(black), white.max(black)) {
            (0, 1) => {
                if pass < 3 {
                    0
                } else {
                    1
                }
            }
            (0, 2) => 2,
            _ => {
                if pass < 2 {
                    1
                } else {
                    2
                }
            }
        };
        if white == winner {
            GameOutcome::white_win()
        } else {
            GameOutcome::black_win()
        }
    });
    let projected = round_robin::project(&config, &players, &results).unwrap();
    assert_eq!(
        projected
            .standings
            .groups
            .iter()
            .flat_map(|group| &group.players)
            .map(|row| row.player.index())
            .collect::<Vec<_>>(),
        [2, 0, 1]
    );
    assert_eq!(
        projected.players[0].game_points,
        projected.players[1].game_points
    );
    config.primary_score = PrimaryScore::MatchPoints;
    config.standings =
        RoundRobinStandingsPlan::new(vec![Criterion::DirectEncounter(Default::default())]);
    let projected = round_robin::project(&config, &players, &results).unwrap();
    assert_eq!(
        projected
            .standings
            .groups
            .iter()
            .flat_map(|group| &group.players)
            .map(|row| row.player.index())
            .collect::<Vec<_>>(),
        [2, 0, 1]
    );
}

#[test]
fn match_progressive_uses_scheduled_completion_and_custom_awards_remain_exact() {
    let players = [0, 1].map(PlayerId::new);
    let mut config = config(
        4,
        PrimaryScore::MatchPoints,
        vec![
            Criterion::PrimaryScore,
            Criterion::Progressive(RoundRobinProgressiveOptions::default()),
        ],
    );
    config.match_point_system.win = Score::new(7);
    let mut results = facts(&config, &players, |white, _, _| {
        if white == 0 {
            GameOutcome::white_win()
        } else {
            GameOutcome::black_win()
        }
    });
    results.reverse();
    let projected = round_robin::project(&config, &players, &results).unwrap();
    assert_eq!(
        projected.standings.groups[0].players[0].values,
        [
            CriterionValue::Score(Score::new(7)),
            CriterionValue::Integer(7)
        ]
    );
    // Excluding forfeits treats a partially forfeited aggregate as a regular match.
    config.standings = RoundRobinStandingsPlan::new(vec![Criterion::DirectEncounter(
        round_robin::RoundRobinDirectEncounterOptions {
            forfeits: DirectEncounterForfeitPolicy::Exclude,
            ..Default::default()
        },
    )]);
    let results = facts(&config, &players, |white, _, pass| {
        if pass == 3 {
            if white == 0 {
                award(Award::ForfeitLoss, Award::ForfeitWin)
            } else {
                award(Award::ForfeitWin, Award::ForfeitLoss)
            }
        } else if white == 0 {
            GameOutcome::white_win()
        } else {
            GameOutcome::black_win()
        }
    });
    let projected = round_robin::project(&config, &players, &results).unwrap();
    assert_eq!(projected.standings.groups[0].players[0].player, players[0]);
    assert_eq!(projected.standings.groups.len(), 2);
}
