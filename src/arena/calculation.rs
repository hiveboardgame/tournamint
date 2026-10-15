use super::pairing::pair_pool;
use super::player::ArenaPlayerProjection;
use super::scoring::{ScoreInput, performance_contribution, score_game};
use super::standings;
use super::weights::{ArenaWeightContext, max_pair_cost};
use super::{
    ArenaAwardedGameFact, ArenaConfig, ArenaError, ArenaGameAward, ArenaPairing, ArenaPlayerFact,
    ArenaProjection, ArenaStandingMetrics, ArenaTerminalGameFact, ArenaTerminalResult,
};
use crate::{Color, MatchScore, PlayedGameOutcome, PlayerId, Score};

pub(super) fn pair_facts(
    config: &ArenaConfig,
    players: &[ArenaPlayerFact],
    terminal_games: &[ArenaAwardedGameFact],
) -> Result<ArenaPairing, ArenaError> {
    let projected = calculate(config, players, terminal_games, true)?;
    let pool: Vec<_> = projected
        .iter()
        .enumerate()
        .filter_map(|(index, player)| player.waiting_millis().map(|_| PlayerId::new(index)))
        .collect();
    let ranks = standings::competition_ranks(&projected);
    let public_ranks = ranks
        .iter()
        .map(|rank| {
            u32::try_from(*rank)
                .ok()
                .and_then(|rank| rank.checked_add(1))
                .ok_or(ArenaError::ArithmeticOverflow)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if pool.len() < 2 {
        return Ok(ArenaPairing {
            pairings: Vec::new(),
            ranks: public_ranks,
        });
    }
    let max_rank = pool
        .iter()
        .map(|player| ranks[player.index()])
        .max()
        .unwrap_or(0);
    let min_rank = pool
        .iter()
        .map(|player| ranks[player.index()])
        .min()
        .unwrap_or(0);
    let max_rating = pool
        .iter()
        .filter_map(|player| projected[player.index()].arena_rating)
        .max()
        .unwrap_or(0);
    let min_rating = pool
        .iter()
        .filter_map(|player| projected[player.index()].arena_rating)
        .min()
        .unwrap_or(0);
    let longest_wait = pool
        .iter()
        .filter_map(|player| projected[player.index()].waiting_millis())
        .max()
        .unwrap_or(0);
    let context = ArenaWeightContext {
        config,
        players: &projected,
        ranks,
        max_rank,
        max_cost: max_pair_cost(
            max_rank - min_rank,
            usize::try_from(max_rating.abs_diff(min_rating))
                .map_err(|_| ArenaError::ArithmeticOverflow)?,
        )
        .map_err(|_| ArenaError::ArithmeticOverflow)?,
        longest_wait,
        duel: players.iter().filter(|player| player.active).count() == 2,
    };
    Ok(ArenaPairing {
        pairings: pair_pool(&context, &pool).map_err(|_| ArenaError::ArithmeticOverflow)?,
        ranks: public_ranks,
    })
}

pub(super) fn project_facts(
    config: &ArenaConfig,
    players: &[ArenaPlayerFact],
    terminal_games: &[ArenaAwardedGameFact],
) -> Result<ArenaProjection, ArenaError> {
    let projected = calculate(config, players, terminal_games, false)?;
    Ok(ArenaProjection {
        standings: standings::rank_standings(&projected)?,
        players: standings::compute_standing_metrics(config, &projected),
    })
}

pub(super) fn player_metrics(
    config: &ArenaConfig,
    players: &[ArenaPlayerFact],
    terminal_games: &[ArenaAwardedGameFact],
) -> Result<Vec<ArenaStandingMetrics>, ArenaError> {
    let projected = calculate(config, players, terminal_games, false)?;
    Ok(standings::compute_standing_metrics(config, &projected))
}

pub(super) fn award_game_from_history(
    config: &ArenaConfig,
    prior_games: &[ArenaTerminalGameFact],
    game: &ArenaTerminalGameFact,
) -> Result<ArenaGameAward, ArenaError> {
    config.validate().map_err(|_| ArenaError::InvalidConfig)?;
    let current = terminal_scoring(config, game)?;
    let players = [game.pairing.white(), game.pairing.black()];
    let mut streaks = [0_u32; 2];
    let mut last_non_loss = [None; 2];
    for prior in prior_games {
        let scoring = terminal_scoring(config, prior)?;
        for (index, player) in players.iter().enumerate() {
            let score = if *player == prior.pairing.white() {
                scoring.scores[0]
            } else if *player == prior.pairing.black() {
                scoring.scores[1]
            } else {
                continue;
            };
            streaks[index] = if score == MatchScore::Win {
                streaks[index]
                    .checked_add(1)
                    .ok_or(ArenaError::ArithmeticOverflow)?
            } else {
                0
            };
            if score != MatchScore::Loss {
                last_non_loss[index] = Some(score);
            }
        }
    }
    let score = |index| {
        score_game(ScoreInput {
            config,
            streak_before: streaks[index],
            in_draw_streak: last_non_loss[index] == Some(MatchScore::Draw),
            score: current.scores[index],
            berserked: current.berserked[index],
            plies: current.plies,
        })
        .map_err(|_| ArenaError::ArithmeticOverflow)
    };
    let white = score(0)?;
    let black = score(1)?;
    Ok(ArenaGameAward {
        awarded_points: [Score::new(white.points), Score::new(black.points)],
        doubled: [white.doubled, black.doubled],
    })
}

struct TerminalScoring {
    scores: [MatchScore; 2],
    plies: Option<u32>,
    berserked: [bool; 2],
    no_start: Option<PlayerId>,
}

fn terminal_scoring(
    config: &ArenaConfig,
    fact: &ArenaTerminalGameFact,
) -> Result<TerminalScoring, ArenaError> {
    let white = fact.pairing.white();
    let black = fact.pairing.black();
    if white == black {
        return Err(ArenaError::IncoherentFacts);
    }
    match fact.result {
        ArenaTerminalResult::Played(outcome) => {
            if (fact.white_berserked || fact.black_berserked) && config.scoring.berserk.is_none() {
                return Err(ArenaError::IncoherentFacts);
            }
            let scores = played_scores(outcome);
            Ok(TerminalScoring {
                scores: [scores.0, scores.1],
                plies: Some(fact.plies),
                berserked: [fact.white_berserked, fact.black_berserked],
                no_start: None,
            })
        }
        ArenaTerminalResult::NoStart { absent } => {
            let scores = if absent == white {
                [MatchScore::Loss, MatchScore::Win]
            } else if absent == black {
                [MatchScore::Win, MatchScore::Loss]
            } else {
                return Err(ArenaError::IncoherentFacts);
            };
            Ok(TerminalScoring {
                scores,
                plies: None,
                berserked: [false; 2],
                no_start: Some(absent),
            })
        }
    }
}

fn calculate(
    config: &ArenaConfig,
    players: &[ArenaPlayerFact],
    terminal_games: &[ArenaAwardedGameFact],
    track_pairing_history: bool,
) -> Result<Vec<ArenaPlayerProjection>, ArenaError> {
    config.validate().map_err(|_| ArenaError::InvalidConfig)?;
    let mut projected = players
        .iter()
        .enumerate()
        .map(|(index, fact)| {
            if fact.player != PlayerId::new(index)
                || (!fact.active && fact.waiting_millis.is_some())
            {
                return Err(ArenaError::IncoherentFacts);
            }
            Ok(ArenaPlayerProjection::new(
                fact.waiting_millis,
                fact.arena_rating,
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    for awarded in terminal_games {
        let fact = &awarded.game;
        let white = fact.pairing.white();
        let black = fact.pairing.black();
        if white.index() >= projected.len() || black.index() >= projected.len() {
            return Err(ArenaError::IncoherentFacts);
        }
        let TerminalScoring {
            scores: [white_score, black_score],
            berserked: [white_berserk, black_berserk],
            no_start,
            ..
        } = terminal_scoring(config, fact)?;
        if track_pairing_history {
            projected[white.index()]
                .record_pairing(black, Color::White)
                .map_err(|_| ArenaError::ArithmeticOverflow)?;
            projected[black.index()]
                .record_pairing(white, Color::Black)
                .map_err(|_| ArenaError::ArithmeticOverflow)?;
        }
        projected[white.index()]
            .record_scored(
                white_score,
                awarded.award.awarded_points[0].value(),
                white_berserk,
                performance_contribution(white_score, fact.black_rating),
            )
            .map_err(|_| ArenaError::ArithmeticOverflow)?;
        projected[black.index()]
            .record_scored(
                black_score,
                awarded.award.awarded_points[1].value(),
                black_berserk,
                performance_contribution(black_score, fact.white_rating),
            )
            .map_err(|_| ArenaError::ArithmeticOverflow)?;
        if let Some(absent) = no_start {
            projected[absent.index()].no_starts = projected[absent.index()]
                .no_starts
                .checked_add(1)
                .ok_or(ArenaError::ArithmeticOverflow)?;
        } else if fact.plies > 0 {
            projected[white.index()].games_played = projected[white.index()]
                .games_played
                .checked_add(1)
                .ok_or(ArenaError::ArithmeticOverflow)?;
            projected[black.index()].games_played = projected[black.index()]
                .games_played
                .checked_add(1)
                .ok_or(ArenaError::ArithmeticOverflow)?;
        }
    }
    Ok(projected)
}

fn played_scores(outcome: PlayedGameOutcome) -> (MatchScore, MatchScore) {
    match outcome {
        PlayedGameOutcome::WhiteWin => (MatchScore::Win, MatchScore::Loss),
        PlayedGameOutcome::Draw => (MatchScore::Draw, MatchScore::Draw),
        PlayedGameOutcome::BlackWin => (MatchScore::Loss, MatchScore::Win),
    }
}
