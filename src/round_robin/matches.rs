use super::*;

/// All scheduled games between two opponents, in repeat order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoundRobinMatchProjection {
    /// Participants in the first game's White, Black order.
    pub participants: [PlayerId; 2],
    /// Stable identities of every game in this match.
    pub games: Vec<RoundRobinGameId>,
    /// Present only when every configured game has an effective result.
    pub completed: Option<RoundRobinCompletedMatchProjection>,
}

/// Aggregate result of one complete round-robin match.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct RoundRobinCompletedMatchProjection {
    /// Full-match forfeit classification, in participant order.
    pub dispositions: [MatchDisposition; 2],
    /// Comparison of aggregate game points, in participant order.
    pub aggregate: [MatchScore; 2],
    /// Sum of game awards, in participant order.
    pub game_points: [Score; 2],
    /// Match awards, including full-match and double-forfeit treatment.
    pub match_points: [Score; 2],
}

pub(super) fn project_matches(
    definition: &RoundRobinCalculation,
    slots: &[Option<GameOutcome>],
) -> Result<Vec<RoundRobinMatchProjection>, StandingsError> {
    if !definition.config.has_matches() {
        return Ok(Vec::new());
    }
    let scheduled = definition.scheduled_games().collect::<Vec<_>>();
    scheduled
        .iter()
        .filter(|game| game.previous_repeat.is_none())
        .map(|first| {
            let participants = [first.white, first.black];
            let mut games = Vec::new();
            let mut totals = [0u64; 2];
            let mut forfeited_all = [true; 2];
            let mut complete = true;
            let mut current = Some(first.id);
            while let Some(id) = current {
                let game = scheduled[id.value()];
                games.push(id);
                current = game.next_repeat;
                let Some(outcome) = slots[id.value()] else {
                    complete = false;
                    continue;
                };
                let sides = if game.white == participants[0] {
                    [outcome.white(), outcome.black()]
                } else {
                    [outcome.black(), outcome.white()]
                };
                for side in 0..2 {
                    totals[side] = totals[side]
                        .checked_add(u64::from(
                            definition
                                .config
                                .point_system
                                .points_for(sides[side])
                                .value(),
                        ))
                        .ok_or(StandingsError::ArithmeticOverflow)?;
                    forfeited_all[side] &= matches!(
                        sides[side],
                        GameSideOutcome::Adjudicated(
                            AdjudicatedSideResult::ForfeitLoss
                                | AdjudicatedSideResult::DoubleForfeit
                        )
                    );
                }
            }
            let completed = if complete {
                let game_points = [score_from_total(totals[0])?, score_from_total(totals[1])?];
                let result = MatchScore::from_ordering(totals[0].cmp(&totals[1]));
                let aggregate = [result, result.invert()];
                let dispositions = MatchDisposition::from_forfeits(forfeited_all);
                let match_points = [0, 1].map(|side| {
                    definition
                        .config
                        .match_point_system
                        .projected_points(dispositions[side], aggregate[side])
                });
                Some(RoundRobinCompletedMatchProjection {
                    dispositions,
                    aggregate,
                    game_points,
                    match_points,
                })
            } else {
                None
            };
            Ok(RoundRobinMatchProjection {
                participants,
                games,
                completed,
            })
        })
        .collect()
}

pub(super) struct ScoredEncounter {
    pub participants: [PlayerId; 2],
    pub round_index: usize,
    pub completed: Option<CompletedEncounter>,
}

#[derive(Clone, Copy)]
pub(super) struct CompletedEncounter {
    pub points: [Score; 2],
    pub adjudicated: bool,
}

impl ScoredEncounter {
    pub(super) fn project(
        definition: &RoundRobinCalculation,
        slots: &[Option<GameOutcome>],
        matches: &[RoundRobinMatchProjection],
    ) -> Vec<Self> {
        match definition.config.primary_score {
            RoundRobinPrimaryScore::GamePoints => definition
                .scheduled_games()
                .map(|game| Self {
                    participants: [game.white, game.black],
                    round_index: game.round_index,
                    completed: slots[game.id.value()].map(|outcome| CompletedEncounter {
                        points: [outcome.white(), outcome.black()]
                            .map(|side| definition.config.point_system.points_for(side)),
                        adjudicated: matches!(outcome, GameOutcome::Adjudicated(_)),
                    }),
                })
                .collect(),
            RoundRobinPrimaryScore::MatchPoints => {
                let scheduled = definition.scheduled_games().collect::<Vec<_>>();
                let mut encounters = matches
                    .iter()
                    .map(|encounter| Self {
                        participants: encounter.participants,
                        // Fixed schedule order makes Progressive independent of fact arrival order.
                        round_index: scheduled
                            [encounter.games.last().expect("matches have games").value()]
                        .round_index,
                        completed: encounter.completed.map(|completed| CompletedEncounter {
                            points: completed.match_points,
                            adjudicated: completed.dispositions != [MatchDisposition::Regular; 2],
                        }),
                    })
                    .collect::<Vec<_>>();
                encounters.sort_by_key(|encounter| encounter.round_index);
                encounters
            }
        }
    }
}
