use std::collections::HashMap;

use crate::matching::BigWeight;

use super::colors::insert_color_bits;
use super::forbidden::{ForbiddenPairs, compatible, is_bye_candidate};
use super::input::DutchInput;
use super::rules::dutch_get_float;
use super::scores::{match_points, player_score_with_acceleration};
use crate::swisssystems::bracketing::BracketSlot;
use crate::swisssystems::bracketing::BracketStatus;
use crate::swisssystems::bracketing::FloatDirection;
use crate::swisssystems::bracketing::{BaseWeightRequest, BracketPlayer, BracketSystem};

fn shift_edge_weight(edge_weight: &mut BigWeight, shift: usize) -> Result<(), BracketStatus> {
    edge_weight.shl_assign(shift);
    Ok(())
}

pub(super) fn get_float(
    input: &DutchInput<'_>,
    players: &[BracketPlayer<'_>],
    player_index: usize,
    rounds_back: usize,
) -> Result<FloatDirection, BracketStatus> {
    let player = players[player_index];
    let records = player.records;
    let played_rounds = input.pairing.played_rounds;
    if rounds_back > played_rounds {
        return Err(BracketStatus::MalformedTournament);
    }
    let match_index = played_rounds
        .checked_sub(rounds_back)
        .ok_or(BracketStatus::MalformedTournament)?;
    let Some(record) = records.get(match_index).copied() else {
        return Err(BracketStatus::MalformedTournament);
    };
    let game_was_played = matches!(
        record,
        crate::swiss::PlayerRoundRecord::Match { state, .. }
            if state.games().iter().any(|game| game.outcome.counts_as_played())
    );
    let pairing_allocated_bye =
        matches!(record, crate::swiss::PlayerRoundRecord::PairingAllocatedBye);
    let match_points = if game_was_played {
        0
    } else {
        match_points(input, records, match_index)?
    };
    let (player_score, opponent_score) = if game_was_played {
        let crate::swiss::PlayerRoundRecord::Match { opponent, .. } = record else {
            return Err(BracketStatus::MalformedTournament);
        };
        let opponent = opponent.index();
        if opponent >= players.len() {
            return Err(BracketStatus::MalformedTournament);
        }
        (
            player_score_with_acceleration(input, &player, rounds_back)?,
            player_score_with_acceleration(input, &players[opponent], rounds_back)?,
        )
    } else {
        (0, 0)
    };
    Ok(dutch_get_float(
        game_was_played,
        pairing_allocated_bye,
        match_points,
        input.pairing.point_system,
        player_score,
        opponent_score,
    ))
}

pub(super) type FloatHistory = [Option<FloatDirection>; 2];

pub(super) fn build_float_history(
    input: &DutchInput<'_>,
    players: &[BracketPlayer<'_>],
) -> Result<Vec<FloatHistory>, BracketStatus> {
    let rounds_to_cache = input.pairing.played_rounds.min(2);
    let mut history = vec![[None; 2]; players.len()];
    for (player_index, player) in players.iter().enumerate() {
        if !player.is_valid {
            continue;
        }
        for rounds_back in 1..=rounds_to_cache {
            history[player_index][rounds_back - 1] =
                Some(get_float(input, players, player_index, rounds_back)?);
        }
    }
    Ok(history)
}

pub(super) struct DutchEdgeWeightContext<'a> {
    pub(super) input: &'a DutchInput<'a>,
    pub(super) players: &'a [BracketPlayer<'a>],
    pub(super) current_scores: &'a [usize],
    pub(super) forbidden_pairs: &'a ForbiddenPairs,
    pub(super) eligible_for_bye: &'a [bool],
    pub(super) float_history: &'a [FloatHistory],
    pub(super) score_group_size_bits: usize,
    pub(super) score_groups_shift: usize,
    pub(super) score_group_shifts: &'a HashMap<usize, usize>,
    pub(super) is_single_downfloater_the_bye_assignee: bool,
    pub(super) unplayed_game_ranks: &'a HashMap<usize, usize>,
}

fn cached_float(
    context: &DutchEdgeWeightContext<'_>,
    player_index: usize,
    rounds_back: usize,
) -> Result<FloatDirection, BracketStatus> {
    context
        .float_history
        .get(player_index)
        .and_then(|history| history.get(rounds_back - 1))
        .copied()
        .flatten()
        .ok_or(BracketStatus::MalformedTournament)
}

pub(super) struct DutchEdgeWeightRequest {
    pub(super) higher_player_index: usize,
    pub(super) lower_player_index: usize,
    pub(super) lower_player_in_current_bracket: bool,
    pub(super) lower_player_in_next_bracket: bool,
    pub(super) bye_assignee_score: usize,
    pub(super) max: bool,
}

#[derive(Clone, Copy)]
struct EdgeWeightData<'a> {
    higher_player_index: usize,
    lower_player_index: usize,
    lower_player_in_current_bracket: bool,
    lower_player_in_next_bracket: bool,
    bye_assignee_score: usize,
    max: bool,
    higher_player: BracketPlayer<'a>,
    lower_player: BracketPlayer<'a>,
    higher_score: usize,
    lower_score: usize,
}

impl<'a> EdgeWeightData<'a> {
    fn new(context: &DutchEdgeWeightContext<'a>, request: DutchEdgeWeightRequest) -> Self {
        let higher_player = context.players[request.higher_player_index];
        let lower_player = context.players[request.lower_player_index];
        Self {
            higher_player_index: request.higher_player_index,
            lower_player_index: request.lower_player_index,
            lower_player_in_current_bracket: request.lower_player_in_current_bracket,
            lower_player_in_next_bracket: request.lower_player_in_next_bracket,
            bye_assignee_score: request.bye_assignee_score,
            max: request.max,
            higher_player,
            lower_player,
            higher_score: context.current_scores[request.higher_player_index],
            lower_score: context.current_scores[request.lower_player_index],
        }
    }
}

fn edge_is_compatible(context: &DutchEdgeWeightContext<'_>, edge: EdgeWeightData<'_>) -> bool {
    edge.max
        || compatible(
            context.input,
            context.players,
            context.forbidden_pairs,
            edge.higher_player_index,
            edge.lower_player_index,
        )
}

fn shifted_score_group_bit(
    score_group_shifts: &HashMap<usize, usize>,
    score: usize,
) -> Result<BigWeight, BracketStatus> {
    let mut addend = BigWeight::from_usize(1);
    addend.shl_assign(
        *score_group_shifts
            .get(&score)
            .ok_or(BracketStatus::MalformedTournament)?,
    );
    Ok(addend)
}

fn add_bye_priority(
    result: &mut BigWeight,
    context: &DutchEdgeWeightContext<'_>,
    edge: EdgeWeightData<'_>,
) -> Result<(), BracketStatus> {
    if edge.max {
        result.or_assign_usize(2);
    } else {
        let higher_bye = is_bye_candidate(
            context.eligible_for_bye[edge.higher_player_index],
            edge.higher_score,
            edge.bye_assignee_score,
        );
        let lower_bye = is_bye_candidate(
            context.eligible_for_bye[edge.lower_player_index],
            edge.lower_score,
            edge.bye_assignee_score,
        );
        result.or_assign_usize(1 + usize::from(!higher_bye) + usize::from(!lower_bye));
    }
    Ok(())
}

fn add_bracket_priority(
    result: &mut BigWeight,
    context: &DutchEdgeWeightContext<'_>,
    edge: EdgeWeightData<'_>,
    lower_player_in_bracket: bool,
) -> Result<(), BracketStatus> {
    shift_edge_weight(result, context.score_group_size_bits)?;
    result.or_assign_usize(usize::from(!edge.max && lower_player_in_bracket));

    shift_edge_weight(result, context.score_groups_shift)?;
    if !edge.max && lower_player_in_bracket {
        result.add_assign(&shifted_score_group_bit(
            context.score_group_shifts,
            edge.higher_score,
        )?);
    }
    Ok(())
}

fn add_unplayed_game_priority(
    result: &mut BigWeight,
    context: &DutchEdgeWeightContext<'_>,
    edge: EdgeWeightData<'_>,
) -> Result<(), BracketStatus> {
    shift_edge_weight(result, context.score_group_size_bits)?;
    shift_edge_weight(result, context.score_group_size_bits)?;
    if !edge.max && context.is_single_downfloater_the_bye_assignee {
        if edge.higher_score == edge.bye_assignee_score {
            result.or_assign_usize(
                *context
                    .unplayed_game_ranks
                    .get(&edge.higher_player.played_games)
                    .ok_or(BracketStatus::MalformedTournament)?,
            );
        }
        if edge.lower_score == edge.bye_assignee_score {
            result.add_assign_usize(
                *context
                    .unplayed_game_ranks
                    .get(&edge.lower_player.played_games)
                    .ok_or(BracketStatus::MalformedTournament)?,
            );
        }
    }
    Ok(())
}

fn add_color_priority(
    result: &mut BigWeight,
    context: &DutchEdgeWeightContext<'_>,
    edge: EdgeWeightData<'_>,
) -> Result<(), BracketStatus> {
    insert_color_bits(
        result,
        edge.lower_player,
        edge.higher_player,
        edge.lower_player_in_current_bracket,
        context.score_group_size_bits,
        edge.max,
    )
}

fn add_recent_float_priority(
    result: &mut BigWeight,
    context: &DutchEdgeWeightContext<'_>,
    edge: EdgeWeightData<'_>,
    rounds_back: usize,
) -> Result<(), BracketStatus> {
    shift_edge_weight(result, context.score_group_size_bits)?;
    if !edge.max && edge.lower_player_in_current_bracket {
        result.or_assign_usize(usize::from(matches!(
            cached_float(context, edge.lower_player_index, rounds_back)?,
            FloatDirection::Down
        )));
        result.add_assign_usize(usize::from(
            edge.higher_score <= edge.lower_score
                && matches!(
                    cached_float(context, edge.higher_player_index, rounds_back)?,
                    FloatDirection::Down
                ),
        ));
    }

    shift_edge_weight(result, context.score_group_size_bits)?;
    if !edge.max && edge.lower_player_in_current_bracket {
        result.or_assign_usize(usize::from(
            !(edge.higher_score > edge.lower_score
                && matches!(
                    cached_float(context, edge.lower_player_index, rounds_back)?,
                    FloatDirection::Up
                )),
        ));
    }
    Ok(())
}

fn add_score_group_float_priority(
    result: &mut BigWeight,
    context: &DutchEdgeWeightContext<'_>,
    edge: EdgeWeightData<'_>,
    rounds_back: usize,
) -> Result<(), BracketStatus> {
    shift_edge_weight(result, context.score_groups_shift)?;
    if !edge.max && edge.lower_player_in_current_bracket {
        let mut lower_addend = BigWeight::from_usize(usize::from(matches!(
            cached_float(context, edge.lower_player_index, rounds_back)?,
            FloatDirection::Down
        )));
        lower_addend.shl_assign(
            *context
                .score_group_shifts
                .get(&edge.lower_score)
                .ok_or(BracketStatus::MalformedTournament)?,
        );
        result.add_assign(&lower_addend);

        let mut higher_addend = BigWeight::from_usize(usize::from(matches!(
            cached_float(context, edge.higher_player_index, rounds_back)?,
            FloatDirection::Down
        )));
        higher_addend.shl_assign(
            *context
                .score_group_shifts
                .get(&edge.higher_score)
                .ok_or(BracketStatus::MalformedTournament)?,
        );
        result.add_assign(&higher_addend);
    }

    shift_edge_weight(result, context.score_groups_shift)?;
    if !edge.max
        && edge.lower_player_in_current_bracket
        && !(matches!(
            cached_float(context, edge.lower_player_index, rounds_back)?,
            FloatDirection::Up
        ) && edge.higher_score > edge.lower_score)
    {
        result.add_assign(&shifted_score_group_bit(
            context.score_group_shifts,
            edge.higher_score,
        )?);
    }
    Ok(())
}

fn reserve_final_priority_bits(
    result: &mut BigWeight,
    context: &DutchEdgeWeightContext<'_>,
) -> Result<(), BracketStatus> {
    shift_edge_weight(result, context.score_group_size_bits)?;
    shift_edge_weight(result, context.score_group_size_bits)?;
    shift_edge_weight(result, context.score_group_size_bits)?;
    shift_edge_weight(result, 1)
}

fn adjust_max_edge_weight(
    result: &mut BigWeight,
    edge: EdgeWeightData<'_>,
) -> Result<(), BracketStatus> {
    if edge.max {
        result.shl_assign(2);
        result.shr_assign(1);
        result.sub_assign_usize(1);
    }
    Ok(())
}

pub(super) fn compute_edge_weight(
    context: &DutchEdgeWeightContext<'_>,
    request: DutchEdgeWeightRequest,
) -> Result<BigWeight, BracketStatus> {
    let edge = EdgeWeightData::new(context, request);
    let mut result = BigWeight::zero();

    if !edge_is_compatible(context, edge) {
        return Ok(result);
    }

    add_bye_priority(&mut result, context, edge)?;
    add_bracket_priority(
        &mut result,
        context,
        edge,
        edge.lower_player_in_current_bracket,
    )?;
    add_bracket_priority(
        &mut result,
        context,
        edge,
        edge.lower_player_in_next_bracket,
    )?;
    add_unplayed_game_priority(&mut result, context, edge)?;
    add_color_priority(&mut result, context, edge)?;

    let played_rounds = context.input.pairing.played_rounds;
    if played_rounds != 0 {
        add_recent_float_priority(&mut result, context, edge, 1)?;
    }
    if played_rounds > 1 {
        add_recent_float_priority(&mut result, context, edge, 2)?;
    }

    if played_rounds != 0 {
        add_score_group_float_priority(&mut result, context, edge, 1)?;
    }
    if played_rounds > 1 {
        add_score_group_float_priority(&mut result, context, edge, 2)?;
    }

    reserve_final_priority_bits(&mut result, context)?;
    adjust_max_edge_weight(&mut result, edge)?;

    Ok(result)
}

pub(super) struct DutchBaseEdgeWeightInput<'a> {
    pub(super) bracket: &'a [BracketSlot],
    pub(super) score_group_begin: usize,
    pub(super) next_score_group_begin: usize,
    pub(super) bye_assignee_score: usize,
}

pub(super) fn compute_base_edge_weights(
    context: &DutchEdgeWeightContext<'_>,
    input: DutchBaseEdgeWeightInput<'_>,
) -> Result<Vec<Vec<BigWeight>>, BracketStatus> {
    let DutchBaseEdgeWeightInput {
        bracket,
        score_group_begin,
        next_score_group_begin,
        bye_assignee_score,
    } = input;

    let mut result = Vec::with_capacity(bracket.len());
    for player_index in 0..bracket.len() {
        result.push(Vec::with_capacity(player_index));
    }
    for larger_player_index in score_group_begin..bracket.len() {
        for smaller_player_index in 0..larger_player_index {
            result[larger_player_index].push(compute_edge_weight(
                context,
                DutchEdgeWeightRequest {
                    higher_player_index: bracket[smaller_player_index].player_index,
                    lower_player_index: bracket[larger_player_index].player_index,
                    lower_player_in_current_bracket: larger_player_index < next_score_group_begin,
                    lower_player_in_next_bracket: larger_player_index >= next_score_group_begin,
                    bye_assignee_score,
                    max: false,
                },
            )?);
        }
    }
    Ok(result)
}

/// Adapts this system's edge weights to the shared bracket machinery. Holds
/// the parts of the weight context that stay fixed for a whole pairing run;
/// the per-score-group parts arrive in the request.
pub(super) struct DutchWeights<'a> {
    pub(super) input: &'a DutchInput<'a>,
    pub(super) players: &'a [BracketPlayer<'a>],
    pub(super) current_scores: &'a [usize],
    pub(super) forbidden_pairs: &'a ForbiddenPairs,
    pub(super) eligible_for_bye: &'a [bool],
    pub(super) float_history: &'a [FloatHistory],
}

impl<'a> DutchWeights<'a> {
    pub(super) fn edge_weight_context<'context>(
        &self,
        score_group_size_bits: usize,
        score_groups_shift: usize,
        score_group_shifts: &'context HashMap<usize, usize>,
        is_single_downfloater_the_bye_assignee: bool,
        unplayed_game_ranks: &'context HashMap<usize, usize>,
    ) -> DutchEdgeWeightContext<'context>
    where
        'a: 'context,
    {
        DutchEdgeWeightContext {
            input: self.input,
            players: self.players,
            current_scores: self.current_scores,
            forbidden_pairs: self.forbidden_pairs,
            eligible_for_bye: self.eligible_for_bye,
            float_history: self.float_history,
            score_group_size_bits,
            score_groups_shift,
            score_group_shifts,
            is_single_downfloater_the_bye_assignee,
            unplayed_game_ranks,
        }
    }
}

impl BracketSystem for DutchWeights<'_> {
    fn base_edge_weights(
        &self,
        request: BaseWeightRequest<'_>,
    ) -> Result<Vec<Vec<BigWeight>>, BracketStatus> {
        let context = self.edge_weight_context(
            request.score_group_size_bits,
            request.score_groups_shift,
            request.score_group_shifts,
            request.is_single_downfloater_the_bye_assignee,
            request.unplayed_game_ranks,
        );
        compute_base_edge_weights(
            &context,
            DutchBaseEdgeWeightInput {
                bracket: request.bracket,
                score_group_begin: request.score_group_begin,
                next_score_group_begin: request.next_score_group_begin,
                bye_assignee_score: request.bye_assignee_score,
            },
        )
    }
}
