use std::collections::HashMap;

use crate::limits::bits_to_represent;
use crate::matching::{BigWeight, MatchingSnapshot, MatchingSolver};

use super::player::{BracketPlayer, BracketSlot};
use super::status::BracketStatus;
use super::weights::{BaseWeightRequest, BracketSystem, set_edge_weight};

pub(crate) struct ScoreGroupShifts {
    pub(crate) score_groups_shift: usize,
    pub(crate) score_group_shifts: HashMap<usize, usize>,
    pub(crate) score_group_size_bits: usize,
}

pub(crate) fn build_score_group_shifts(
    sorted_players: &[usize],
    current_scores: &[usize],
) -> Result<ScoreGroupShifts, BracketStatus> {
    let mut score_groups_shift = 0usize;
    let mut score_group_shifts = HashMap::new();
    score_group_shifts.reserve(sorted_players.len());
    let mut max_score_group_size = 0usize;
    let mut repeated_scores = 0usize;
    let mut score_group_score = None;
    for &player_index in sorted_players.iter().rev() {
        let current_score = current_scores[player_index];

        if let Some(group_score) = score_group_score
            && current_score != group_score
        {
            let new_bits = bits_to_represent(repeated_scores);
            score_group_shifts.insert(group_score, score_groups_shift);
            max_score_group_size = max_score_group_size.max(repeated_scores);
            repeated_scores = 0;
            score_groups_shift = score_groups_shift
                .checked_add(new_bits)
                .expect("score-group bit widths fit their player collection");
        }

        score_group_score = Some(current_score);
        repeated_scores = repeated_scores
            .checked_add(1)
            .expect("score-group count fits its player slice");
    }

    if let Some(group_score) = score_group_score
        && repeated_scores != 0
    {
        let new_bits = bits_to_represent(repeated_scores);
        score_group_shifts.insert(group_score, score_groups_shift);
        max_score_group_size = max_score_group_size.max(repeated_scores);
        score_groups_shift = score_groups_shift
            .checked_add(new_bits)
            .expect("score-group bit widths fit their player collection");
    }
    Ok(ScoreGroupShifts {
        score_groups_shift,
        score_group_shifts,
        score_group_size_bits: bits_to_represent(max_score_group_size),
    })
}

pub(crate) struct ByeContext<'a> {
    pub(crate) bye_assignee_score: usize,
    pub(crate) is_single_downfloater_the_bye_assignee: bool,
    pub(crate) unplayed_game_ranks: &'a HashMap<usize, usize>,
}

pub(crate) struct BracketContext<'a> {
    pub(crate) weights: &'a dyn BracketSystem,
    pub(crate) players: &'a [BracketPlayer<'a>],
    pub(crate) current_scores: &'a [usize],
    pub(crate) sorted_players: &'a [usize],
    pub(crate) score_group_size_bits: usize,
    pub(crate) score_groups_shift: usize,
    pub(crate) score_group_shifts: &'a HashMap<usize, usize>,
    pub(crate) max_edge_weight: &'a BigWeight,
    pub(crate) max_edge_weight_limbs: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ScoreGroupRange {
    begin: usize,
    end: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VertexRange {
    begin: usize,
    end: usize,
}

impl VertexRange {
    fn contains(self, vertex: usize) -> bool {
        vertex >= self.begin && vertex < self.end
    }
}

#[derive(Debug, Eq, PartialEq)]
struct RemainderGroup {
    players: Vec<usize>,
    pairs: usize,
    first_group_end: Option<usize>,
}

#[derive(Debug, Eq, PartialEq)]
struct ExchangePlan {
    remainder: Vec<usize>,
    pairs: usize,
    first_group_end: Option<usize>,
    exchange_count: usize,
}

struct MovedDownMatchingInput<'a> {
    current_scores: &'a [usize],
    bracket: &'a [BracketSlot],
    base_edge_weights: &'a [Vec<BigWeight>],
    score_group: ScoreGroupRange,
    vertices: VertexRange,
}

struct ConfirmedPairFinalizationInput<'a> {
    bracket: &'a [BracketSlot],
    base_edge_weights: &'a [Vec<BigWeight>],
    score_group: ScoreGroupRange,
    default_edge_weight: &'a BigWeight,
}

struct ExchangeSearchInput<'a> {
    bracket: &'a [BracketSlot],
    next_score_group_begin: usize,
    next_score_group_begin_vertex: usize,
    player_count: usize,
    score_group_vertices: VertexRange,
    default_edge_weight: &'a BigWeight,
}

struct ScoreGroupMatchingInput<'a> {
    bracket: &'a [BracketSlot],
    current_scores: &'a [usize],
    iteration: ScoreGroupIteration,
    default_edge_weight: &'a BigWeight,
    score_group_size_bits: usize,
    max_edge_weight_limbs: usize,
}

struct CarryUnmatchedPlayersInput<'a> {
    matching: &'a [usize],
    matched: &'a [bool],
    player_ids_by_index: &'a [usize],
    sorted_players: &'a [usize],
    current_scores: &'a [usize],
    bracket: &'a [BracketSlot],
    next_score_group_begin: usize,
    score_group_iterator: usize,
    bye_assignee_score: usize,
}

#[derive(Debug, Eq, PartialEq)]
struct CarryUnmatchedPlayersOutput {
    bracket: Vec<BracketSlot>,
    score_group_begin: usize,
    is_single_downfloater_the_bye_assignee: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ScoreGroupIteration {
    next_score_group_begin: usize,
    next_score_group_begin_vertex: usize,
    score_group: ScoreGroupRange,
    score_group_vertices: VertexRange,
    score_group_iterator: usize,
}

#[derive(Debug, Eq, PartialEq)]
struct BracketState {
    matching_by_id: Vec<Option<usize>>,
    bracket: Vec<BracketSlot>,
    next_score_group_iterator: usize,
    matched: Vec<bool>,
    score_group_begin: usize,
    score_group_begin_vertex: usize,
}

impl BracketState {
    fn new(
        player_count: usize,
        sorted_players: &[usize],
        current_scores: &[usize],
    ) -> Result<Self, BracketStatus> {
        let mut bracket = Vec::with_capacity(sorted_players.len());
        let mut next_score_group_iterator = 0usize;
        while next_score_group_iterator < sorted_players.len()
            && current_scores[sorted_players[next_score_group_iterator]]
                >= current_scores[sorted_players[0]]
        {
            bracket.push(BracketSlot::new(
                sorted_players[next_score_group_iterator],
                bracket.len(),
            ));
            next_score_group_iterator += 1;
        }

        let matching_by_id = vec![None; player_count];
        let matched = vec![false; sorted_players.len()];

        Ok(Self {
            matching_by_id,
            bracket,
            next_score_group_iterator,
            matched,
            score_group_begin: 0,
            score_group_begin_vertex: 0,
        })
    }
}

fn extend_next_score_group(
    bracket_state: &mut BracketState,
    sorted_players: &[usize],
    current_scores: &[usize],
) -> Result<ScoreGroupIteration, BracketStatus> {
    let next_score_group_begin = bracket_state.bracket.len();
    let next_score_group_begin_vertex = bracket_state.score_group_begin_vertex
        + (next_score_group_begin - bracket_state.score_group_begin);
    let score_group = ScoreGroupRange {
        begin: bracket_state.score_group_begin,
        end: next_score_group_begin,
    };
    // Once every score group has been visited, only a single bye may remain.
    // Multiple carried players would otherwise repeat an empty iteration.
    // Do not compare bracket sizes: a valid iteration can carry players while
    // still advancing to a new score group.
    if score_group.begin == score_group.end {
        return Err(BracketStatus::EmptyScoreGroup);
    }
    let score_group_vertices = VertexRange {
        begin: bracket_state.score_group_begin_vertex,
        end: next_score_group_begin_vertex,
    };
    let score_group_iterator = extend_score_group(
        &mut bracket_state.bracket,
        sorted_players,
        current_scores,
        &mut bracket_state.next_score_group_iterator,
    );

    Ok(ScoreGroupIteration {
        next_score_group_begin,
        next_score_group_begin_vertex,
        score_group,
        score_group_vertices,
        score_group_iterator,
    })
}

fn finalize_pair(
    solver: &mut MatchingSolver,
    vertex0: usize,
    vertex1: usize,
    default_edge_weight: &BigWeight,
) -> Result<(), BracketStatus> {
    if vertex0 == vertex1 {
        for unpaired_vertex in 0..solver.size() {
            if vertex0 != unpaired_vertex {
                set_edge_weight(solver, vertex0, unpaired_vertex, BigWeight::zero())?;
            }
        }
        solver.preserve_current_matching_for_isolated_pair(vertex0, vertex1);
        return Ok(());
    }

    for unpaired_vertex in 0..solver.size() {
        if unpaired_vertex == vertex0 || unpaired_vertex == vertex1 {
            continue;
        }
        set_edge_weight(solver, vertex0, unpaired_vertex, BigWeight::zero())?;
        set_edge_weight(solver, vertex1, unpaired_vertex, BigWeight::zero())?;
    }
    set_edge_weight(solver, vertex0, vertex1, default_edge_weight.clone())?;
    solver.preserve_current_matching_for_isolated_pair(vertex0, vertex1);
    Ok(())
}

fn owned_edge_weight(edge_weight: &BigWeight) -> BigWeight {
    if edge_weight.is_zero() {
        BigWeight::zero()
    } else {
        edge_weight.clone()
    }
}

fn owned_nonzero_edge_weight(edge_weight: &BigWeight) -> Option<BigWeight> {
    (!edge_weight.is_zero()).then(|| edge_weight.clone())
}

fn current_solver_matching(solver: &MatchingSolver) -> Result<&[usize], BracketStatus> {
    solver
        .current_matching()
        .ok_or(BracketStatus::MalformedTournament)
}

fn last_solver_matching(solver: &MatchingSolver) -> Result<MatchingSnapshot<'_>, BracketStatus> {
    solver
        .last_matching_snapshot()
        .ok_or(BracketStatus::MalformedTournament)
}

pub(crate) fn compute_solver_matching(
    solver: &mut MatchingSolver,
) -> Result<&[usize], BracketStatus> {
    solver.compute_matching();
    current_solver_matching(solver)
}

fn copied_matching(matching: &[usize]) -> Result<Vec<usize>, BracketStatus> {
    Ok(matching.to_vec())
}

fn first_remainder_group_end(remainder: &[usize], remainder_pairs: usize) -> Option<usize> {
    remainder.get(remainder_pairs).copied()
}

fn build_remainder(
    matching: &[usize],
    bracket: &[BracketSlot],
    score_group: ScoreGroupRange,
    vertices: VertexRange,
) -> Result<RemainderGroup, BracketStatus> {
    let mut players = Vec::with_capacity(score_group.end.saturating_sub(score_group.begin));
    let mut pairs = 0usize;
    for (player_index, slot) in bracket
        .iter()
        .enumerate()
        .take(score_group.end)
        .skip(score_group.begin)
    {
        let player_vertex = slot.vertex;
        if matching[player_vertex] < vertices.begin {
            continue;
        }
        players.push(player_index);
        if matching[player_vertex] < player_vertex {
            pairs += 1;
        }
    }
    let first_group_end = first_remainder_group_end(&players, pairs);
    Ok(RemainderGroup {
        players,
        pairs,
        first_group_end,
    })
}

fn build_exchange_plan(remainder_group: RemainderGroup) -> ExchangePlan {
    ExchangePlan {
        remainder: remainder_group.players,
        pairs: remainder_group.pairs,
        first_group_end: remainder_group.first_group_end,
        exchange_count: 0,
    }
}

fn count_remainder_exchanges(
    matching: &[usize],
    bracket: &[BracketSlot],
    remainder: &[usize],
    first_group_end: Option<usize>,
    vertices: VertexRange,
) -> usize {
    let mut exchange_count = 0usize;
    for &player_index in remainder {
        if first_group_end.is_some_and(|first_group_end| player_index >= first_group_end) {
            break;
        }
        let player_vertex = bracket[player_index].vertex;
        exchange_count += usize::from(!has_forward_match_before(
            matching,
            player_vertex,
            vertices.end,
        ));
    }
    exchange_count
}

fn has_forward_match_before(matching: &[usize], vertex: usize, boundary: usize) -> bool {
    matching[vertex] > vertex && matching[vertex] < boundary
}

fn extend_score_group(
    bracket: &mut Vec<BracketSlot>,
    sorted_players: &[usize],
    current_scores: &[usize],
    next_score_group_iterator: &mut usize,
) -> usize {
    let score_group_iterator = *next_score_group_iterator;
    while *next_score_group_iterator < sorted_players.len()
        && current_scores[sorted_players[*next_score_group_iterator]]
            >= current_scores[sorted_players[score_group_iterator]]
    {
        let next_vertex = bracket
            .last()
            .map(|slot| slot.vertex)
            .unwrap_or(0)
            .wrapping_add(1);
        bracket.push(BracketSlot::new(
            sorted_players[*next_score_group_iterator],
            next_vertex,
        ));
        *next_score_group_iterator += 1;
    }
    score_group_iterator
}

fn install_base_edge_weights(
    solver: &mut MatchingSolver,
    bracket: &[BracketSlot],
    base_edge_weights: &[Vec<BigWeight>],
) -> Result<(), BracketStatus> {
    for (opponent_index, opponent_vector) in base_edge_weights.iter().enumerate() {
        let opponent_vertex = bracket[opponent_index].vertex;
        for (player_index, edge_weight) in opponent_vector.iter().enumerate() {
            set_edge_weight(
                solver,
                opponent_vertex,
                bracket[player_index].vertex,
                owned_edge_weight(edge_weight),
            )?;
        }
    }
    Ok(())
}

fn compute_score_group_base_weights(
    solver: &mut MatchingSolver,
    context: &BracketContext<'_>,
    bye_context: &ByeContext<'_>,
    bracket: &[BracketSlot],
    score_group: ScoreGroupRange,
) -> Result<Vec<Vec<BigWeight>>, BracketStatus> {
    let base_edge_weights = context.weights.base_edge_weights(BaseWeightRequest {
        bracket,
        score_group_begin: score_group.begin,
        next_score_group_begin: score_group.end,
        bye_assignee_score: bye_context.bye_assignee_score,
        score_group_size_bits: context.score_group_size_bits,
        score_groups_shift: context.score_groups_shift,
        score_group_shifts: context.score_group_shifts,
        is_single_downfloater_the_bye_assignee: bye_context.is_single_downfloater_the_bye_assignee,
        unplayed_game_ranks: bye_context.unplayed_game_ranks,
    })?;
    install_base_edge_weights(solver, bracket, &base_edge_weights)?;
    Ok(base_edge_weights)
}

fn finalize_confirmed_pairs(
    solver: &mut MatchingSolver,
    matched: &mut [bool],
    input: ConfirmedPairFinalizationInput<'_>,
) -> Result<(), BracketStatus> {
    let ConfirmedPairFinalizationInput {
        bracket,
        base_edge_weights,
        score_group,
        default_edge_weight,
    } = input;

    for player_index in 0..score_group.begin {
        let player_vertex = bracket[player_index].vertex;
        if !matched[player_vertex] {
            continue;
        }
        let mut addend = BigWeight::from_usize(bracket.len());
        for opponent_index in (score_group.begin..score_group.end).rev() {
            let opponent_vertex = bracket[opponent_index].vertex;
            if matched[opponent_vertex] {
                continue;
            }
            if let Some(mut edge_weight) =
                owned_nonzero_edge_weight(&base_edge_weights[opponent_index][player_index])
            {
                edge_weight.add_assign(&addend);
                set_edge_weight(solver, player_vertex, opponent_vertex, edge_weight)?;
                addend.add_assign_usize(1);
            }
        }
        let matching = compute_solver_matching(solver)?;
        let match_vertex = matching[player_vertex];
        matched[match_vertex] = true;
        finalize_pair(solver, player_vertex, match_vertex, default_edge_weight)?;
    }

    Ok(())
}

fn match_moved_down_players(
    solver: &mut MatchingSolver,
    matched: &mut [bool],
    input: MovedDownMatchingInput<'_>,
) -> Result<(), BracketStatus> {
    let MovedDownMatchingInput {
        current_scores,
        bracket,
        base_edge_weights,
        score_group,
        vertices,
    } = input;

    let mut moved_down_score_group = 0usize;
    let mut remaining_moved_down_score_group_players = 0usize;
    let mut remaining_matched_moved_down_score_group_players = 0usize;
    let mut matching = copied_matching(last_solver_matching(solver)?.as_slice())?;
    for player_index in 0..score_group.begin {
        if player_index == 0
            || current_scores[bracket[player_index].player_index] < moved_down_score_group
        {
            moved_down_score_group = current_scores[bracket[player_index].player_index];
            remaining_matched_moved_down_score_group_players = 0;
            remaining_moved_down_score_group_players = 0;
            let mut moved_down_player_index = player_index;
            while moved_down_player_index < bracket.len()
                && current_scores[bracket[moved_down_player_index].player_index]
                    >= moved_down_score_group
            {
                remaining_moved_down_score_group_players += 1;
                let moved_down_player_vertex = bracket[moved_down_player_index].vertex;
                if vertices.contains(matching[moved_down_player_vertex]) {
                    remaining_matched_moved_down_score_group_players += 1;
                }
                moved_down_player_index += 1;
            }
        }
        if remaining_matched_moved_down_score_group_players == 0 {
            continue;
        }
        let player_vertex = bracket[player_index].vertex;
        if remaining_moved_down_score_group_players
            <= remaining_matched_moved_down_score_group_players
        {
            matched[player_vertex] = true;
            continue;
        }
        remaining_moved_down_score_group_players -= 1;
        if !vertices.contains(matching[player_vertex]) {
            for opponent_index in score_group.begin..score_group.end {
                if let Some(mut edge_weight) =
                    owned_nonzero_edge_weight(&base_edge_weights[opponent_index][player_index])
                {
                    edge_weight.or_assign_usize(1);
                    set_edge_weight(
                        solver,
                        player_vertex,
                        bracket[opponent_index].vertex,
                        edge_weight,
                    )?;
                }
            }
            matching = copied_matching(compute_solver_matching(solver)?)?;
        }
        if vertices.contains(matching[player_vertex]) {
            matched[player_vertex] = true;
            remaining_matched_moved_down_score_group_players -= 1;
            for opponent_index in score_group.begin..score_group.end {
                if let Some(mut edge_weight) =
                    owned_nonzero_edge_weight(&base_edge_weights[opponent_index][player_index])
                {
                    edge_weight.or_assign_usize(score_group.end - score_group.begin);
                    edge_weight.add_assign_usize(1);
                    set_edge_weight(
                        solver,
                        player_vertex,
                        bracket[opponent_index].vertex,
                        edge_weight,
                    )?;
                }
            }
        }
    }

    Ok(())
}

fn install_initial_remainder_edges<F>(
    solver: &mut MatchingSolver,
    base_edge_weights: &[Vec<BigWeight>],
    exchange_plan: &ExchangePlan,
    input: &ExchangeSearchInput<'_>,
    edge_weight_computer: &F,
) -> Result<(), BracketStatus>
where
    F: Fn(&[Vec<BigWeight>], usize, usize, usize, usize) -> BigWeight,
{
    for &opponent_index in &exchange_plan.remainder {
        let opponent_vertex = input.bracket[opponent_index].vertex;
        for (player_remainder_index, &player_index) in exchange_plan.remainder.iter().enumerate() {
            if opponent_index <= player_index {
                break;
            }
            let edge_weight = edge_weight_computer(
                base_edge_weights,
                player_index,
                opponent_index,
                player_remainder_index,
                exchange_plan.pairs,
            );
            set_edge_weight(
                solver,
                input.bracket[player_index].vertex,
                opponent_vertex,
                edge_weight,
            )?;
        }
    }

    Ok(())
}

fn compute_exchange_count(
    solver: &MatchingSolver,
    exchange_plan: &ExchangePlan,
    input: &ExchangeSearchInput<'_>,
) -> Result<usize, BracketStatus> {
    Ok(count_remainder_exchanges(
        current_solver_matching(solver)?,
        input.bracket,
        &exchange_plan.remainder,
        exchange_plan.first_group_end,
        input.score_group_vertices,
    ))
}

fn search_exchange_countdown<F>(
    solver: &mut MatchingSolver,
    base_edge_weights: &mut [Vec<BigWeight>],
    exchange_plan: &ExchangePlan,
    input: &ExchangeSearchInput<'_>,
    edge_weight_computer: &F,
) -> Result<(), BracketStatus>
where
    F: Fn(&[Vec<BigWeight>], usize, usize, usize, usize) -> BigWeight,
{
    let mut exchanges_remaining = exchange_plan.exchange_count;
    let mut player_remainder_index = exchange_plan.pairs;
    let mut player_pos = exchange_plan.pairs;
    let mut matching = copied_matching(last_solver_matching(solver)?.as_slice())?;
    while player_pos != 0 && exchanges_remaining != 0 {
        player_pos -= 1;
        player_remainder_index -= 1;
        let player_index = exchange_plan.remainder[player_pos];
        let player_vertex = input.bracket[player_index].vertex;
        if has_forward_match_before(
            &matching,
            player_vertex,
            input.next_score_group_begin_vertex,
        ) {
            for &opponent_index in exchange_plan.remainder.iter().skip(player_pos + 1) {
                let mut edge_weight = edge_weight_computer(
                    base_edge_weights,
                    player_index,
                    opponent_index,
                    player_remainder_index,
                    exchange_plan.pairs,
                );
                if !edge_weight.is_zero() {
                    edge_weight.sub_assign_usize(1);
                    set_edge_weight(
                        solver,
                        player_vertex,
                        input.bracket[opponent_index].vertex,
                        edge_weight,
                    )?;
                }
            }
            matching = copied_matching(compute_solver_matching(solver)?)?;
        }

        let exchange = !has_forward_match_before(
            &matching,
            player_vertex,
            input.next_score_group_begin_vertex,
        );
        exchanges_remaining -= usize::from(exchange);

        for &opponent_index in exchange_plan.remainder.iter().skip(player_pos + 1) {
            if exchange {
                base_edge_weights[opponent_index][player_index] = BigWeight::zero();
            }
            let edge_weight = edge_weight_computer(
                base_edge_weights,
                player_index,
                opponent_index,
                player_remainder_index,
                exchange_plan.pairs,
            );
            set_edge_weight(
                solver,
                player_vertex,
                input.bracket[opponent_index].vertex,
                edge_weight,
            )?;
        }
    }

    Ok(())
}

fn search_next_score_group_candidate_edges<F>(
    solver: &mut MatchingSolver,
    base_edge_weights: &mut [Vec<BigWeight>],
    exchange_plan: &ExchangePlan,
    input: &ExchangeSearchInput<'_>,
    edge_weight_computer: &F,
) -> Result<(), BracketStatus>
where
    F: Fn(&[Vec<BigWeight>], usize, usize, usize, usize) -> BigWeight,
{
    let mut exchanges_remaining = exchange_plan.exchange_count;
    let mut matching = copied_matching(last_solver_matching(solver)?.as_slice())?;
    for (remainder_index, player_pos) in
        (exchange_plan.pairs..).zip(exchange_plan.pairs..exchange_plan.remainder.len())
    {
        if exchanges_remaining <= 1 {
            break;
        }
        let player_index = exchange_plan.remainder[player_pos];
        let player_vertex = input.bracket[player_index].vertex;
        let already_exchanged = has_forward_match_before(
            &matching,
            player_vertex,
            input.next_score_group_begin_vertex,
        );
        if !already_exchanged {
            for &opponent_index in exchange_plan.remainder.iter().skip(player_pos + 1) {
                let mut edge_weight = edge_weight_computer(
                    base_edge_weights,
                    player_index,
                    opponent_index,
                    remainder_index,
                    exchange_plan.pairs,
                );
                if !edge_weight.is_zero() {
                    edge_weight.add_assign_usize(1);
                    set_edge_weight(
                        solver,
                        player_vertex,
                        input.bracket[opponent_index].vertex,
                        edge_weight,
                    )?;
                }
            }
            matching = copied_matching(compute_solver_matching(solver)?)?;
        }

        let exchange = has_forward_match_before(
            &matching,
            player_vertex,
            input.next_score_group_begin_vertex,
        );
        if exchange {
            exchanges_remaining -= 1;
            for &opponent_index in exchange_plan.remainder.iter().take(player_pos) {
                base_edge_weights[player_index][opponent_index] = BigWeight::zero();
                set_edge_weight(
                    solver,
                    player_vertex,
                    input.bracket[opponent_index].vertex,
                    BigWeight::zero(),
                )?;
            }
            for (opponent_index, opponent_weights) in base_edge_weights
                .iter_mut()
                .enumerate()
                .take(input.player_count)
                .skip(input.next_score_group_begin)
            {
                opponent_weights[player_index] = BigWeight::zero();
                set_edge_weight(
                    solver,
                    player_vertex,
                    input.bracket[opponent_index].vertex,
                    BigWeight::zero(),
                )?;
            }
        }
        if !already_exchanged {
            for &opponent_index in exchange_plan.remainder.iter().skip(player_pos + 1) {
                let edge_weight = edge_weight_computer(
                    base_edge_weights,
                    player_index,
                    opponent_index,
                    remainder_index,
                    exchange_plan.pairs,
                );
                set_edge_weight(
                    solver,
                    player_vertex,
                    input.bracket[opponent_index].vertex,
                    edge_weight,
                )?;
            }
        }
    }

    Ok(())
}

fn restore_remainder_edges(
    solver: &mut MatchingSolver,
    base_edge_weights: &mut [Vec<BigWeight>],
    exchange_plan: &ExchangePlan,
    input: &ExchangeSearchInput<'_>,
) -> Result<(), BracketStatus> {
    let matching = copied_matching(last_solver_matching(solver)?.as_slice())?;
    for player_index in exchange_plan.remainder.iter().copied() {
        let player_vertex = input.bracket[player_index].vertex;
        for &opponent_index in exchange_plan
            .remainder
            .iter()
            .skip_while(|&&value| value != player_index)
            .skip(1)
        {
            let opponent_vertex = input.bracket[opponent_index].vertex;
            if !has_forward_match_before(
                &matching,
                player_vertex,
                input.next_score_group_begin_vertex,
            ) || has_forward_match_before(
                &matching,
                opponent_vertex,
                input.next_score_group_begin_vertex,
            ) {
                base_edge_weights[opponent_index][player_index] = BigWeight::zero();
            }
            set_edge_weight(
                solver,
                player_vertex,
                opponent_vertex,
                owned_edge_weight(&base_edge_weights[opponent_index][player_index]),
            )?;
        }
    }

    Ok(())
}

fn finalize_exchange_matches(
    solver: &mut MatchingSolver,
    matched: &mut [bool],
    base_edge_weights: &[Vec<BigWeight>],
    exchange_plan: &ExchangePlan,
    input: &ExchangeSearchInput<'_>,
) -> Result<(), BracketStatus> {
    let mut matching = copied_matching(last_solver_matching(solver)?.as_slice())?;
    for &player_index in &exchange_plan.remainder {
        let player_vertex = input.bracket[player_index].vertex;
        if has_forward_match_before(
            &matching,
            player_vertex,
            input.next_score_group_begin_vertex,
        ) {
            let mut addend = 0usize;
            for &opponent_index in exchange_plan.remainder.iter().rev() {
                let opponent_vertex = input.bracket[opponent_index].vertex;
                if opponent_index <= player_index || matched[opponent_vertex] {
                    continue;
                }
                if let Some(mut edge_weight) =
                    owned_nonzero_edge_weight(&base_edge_weights[opponent_index][player_index])
                {
                    edge_weight.add_assign_usize(addend);
                    set_edge_weight(solver, player_vertex, opponent_vertex, edge_weight)?;
                }
                addend += 1;
            }
            matching = copied_matching(compute_solver_matching(solver)?)?;
            let match_vertex = matching[player_vertex];
            matched[player_vertex] = true;
            matched[match_vertex] = true;
            finalize_pair(
                solver,
                player_vertex,
                match_vertex,
                input.default_edge_weight,
            )?;
        }
    }

    Ok(())
}

fn apply_exchange_search<F>(
    solver: &mut MatchingSolver,
    matched: &mut [bool],
    base_edge_weights: &mut [Vec<BigWeight>],
    mut exchange_plan: ExchangePlan,
    input: ExchangeSearchInput<'_>,
    edge_weight_computer: F,
) -> Result<(), BracketStatus>
where
    F: Fn(&[Vec<BigWeight>], usize, usize, usize, usize) -> BigWeight,
{
    install_initial_remainder_edges(
        solver,
        base_edge_weights,
        &exchange_plan,
        &input,
        &edge_weight_computer,
    )?;

    compute_solver_matching(solver)?;
    exchange_plan.exchange_count = compute_exchange_count(solver, &exchange_plan, &input)?;

    search_exchange_countdown(
        solver,
        base_edge_weights,
        &exchange_plan,
        &input,
        &edge_weight_computer,
    )?;
    search_next_score_group_candidate_edges(
        solver,
        base_edge_weights,
        &exchange_plan,
        &input,
        &edge_weight_computer,
    )?;
    restore_remainder_edges(solver, base_edge_weights, &exchange_plan, &input)?;
    finalize_exchange_matches(solver, matched, base_edge_weights, &exchange_plan, &input)?;

    Ok(())
}

fn exchange_edge_weight(
    base_edge_weights: &[Vec<BigWeight>],
    score_group_size_bits: usize,
    max_edge_weight_limbs: usize,
    smaller_player_index: usize,
    larger_player_index: usize,
    smaller_player_remainder_index: usize,
    remainder_pairs: usize,
) -> BigWeight {
    let Some(mut result) =
        owned_nonzero_edge_weight(&base_edge_weights[larger_player_index][smaller_player_index])
    else {
        return BigWeight::zero();
    };

    let mut addend = BigWeight::zero();
    addend.or_assign_usize(usize::from(
        smaller_player_remainder_index < remainder_pairs,
    ));
    addend.wrapping_shl_assign(score_group_size_bits, max_edge_weight_limbs);
    addend.wrapping_shl_assign(score_group_size_bits, max_edge_weight_limbs);
    addend.wrapping_sub_assign_usize(smaller_player_remainder_index, max_edge_weight_limbs);
    addend.wrapping_shl_assign(1, max_edge_weight_limbs);
    result.wrapping_add_assign(&addend, max_edge_weight_limbs);
    result
}

fn apply_score_group_matching(
    solver: &mut MatchingSolver,
    matched: &mut [bool],
    base_edge_weights: &mut [Vec<BigWeight>],
    input: ScoreGroupMatchingInput<'_>,
) -> Result<(), BracketStatus> {
    let ScoreGroupMatchingInput {
        bracket,
        current_scores,
        iteration,
        default_edge_weight,
        score_group_size_bits,
        max_edge_weight_limbs,
    } = input;
    let edge_weight_computer = |base_edge_weights: &[Vec<BigWeight>],
                                smaller_player_index: usize,
                                larger_player_index: usize,
                                smaller_player_remainder_index: usize,
                                remainder_pairs: usize| {
        exchange_edge_weight(
            base_edge_weights,
            score_group_size_bits,
            max_edge_weight_limbs,
            smaller_player_index,
            larger_player_index,
            smaller_player_remainder_index,
            remainder_pairs,
        )
    };

    compute_solver_matching(solver)?;

    match_moved_down_players(
        solver,
        matched,
        MovedDownMatchingInput {
            current_scores,
            bracket,
            base_edge_weights,
            score_group: iteration.score_group,
            vertices: iteration.score_group_vertices,
        },
    )?;
    finalize_confirmed_pairs(
        solver,
        matched,
        ConfirmedPairFinalizationInput {
            bracket,
            base_edge_weights,
            score_group: iteration.score_group,
            default_edge_weight,
        },
    )?;

    let remainder_group = build_remainder(
        last_solver_matching(solver)?.as_slice(),
        bracket,
        iteration.score_group,
        iteration.score_group_vertices,
    )?;
    let exchange_plan = build_exchange_plan(remainder_group);
    apply_exchange_search(
        solver,
        matched,
        base_edge_weights,
        exchange_plan,
        ExchangeSearchInput {
            bracket,
            next_score_group_begin: iteration.next_score_group_begin,
            next_score_group_begin_vertex: iteration.next_score_group_begin_vertex,
            player_count: bracket.len(),
            score_group_vertices: iteration.score_group_vertices,
            default_edge_weight,
        },
        edge_weight_computer,
    )
}

fn carry_unmatched_players(
    matching_by_id: &mut [Option<usize>],
    input: CarryUnmatchedPlayersInput<'_>,
) -> Result<CarryUnmatchedPlayersOutput, BracketStatus> {
    let CarryUnmatchedPlayersInput {
        matching,
        matched,
        player_ids_by_index,
        sorted_players,
        current_scores,
        bracket,
        next_score_group_begin,
        score_group_iterator,
        bye_assignee_score,
    } = input;

    let mut new_bracket = Vec::with_capacity(sorted_players.len());
    let mut score_group_begin = 0usize;
    let mut is_single_downfloater_the_bye_assignee = sorted_players.len() & 1 != 0
        && score_group_iterator < sorted_players.len()
        && bye_assignee_score >= current_scores[sorted_players[score_group_iterator]];

    for (player_index, slot) in bracket.iter().copied().enumerate() {
        let player_vertex = slot.vertex;
        if player_index < next_score_group_begin && matched[player_vertex] {
            let player = slot.player_index;
            let match_player = sorted_players[matching[player_vertex]];
            matching_by_id[player_ids_by_index[player]] = Some(match_player);
            matching_by_id[player_ids_by_index[match_player]] = Some(player);
        } else {
            new_bracket.push(slot);
            if player_index < next_score_group_begin {
                score_group_begin += 1;
            }
            if is_single_downfloater_the_bye_assignee
                && current_scores[sorted_players[matching[player_vertex]]]
                    < current_scores[sorted_players[score_group_iterator]]
            {
                is_single_downfloater_the_bye_assignee = false;
            }
        }
    }

    Ok(CarryUnmatchedPlayersOutput {
        bracket: new_bracket,
        score_group_begin,
        is_single_downfloater_the_bye_assignee,
    })
}

pub(crate) fn solve_score_groups(
    solver: &mut MatchingSolver,
    context: BracketContext<'_>,
    mut bye_context: ByeContext<'_>,
) -> Result<Vec<Option<usize>>, BracketStatus> {
    let players = context.players;
    let current_scores = context.current_scores;
    let sorted_players = context.sorted_players;
    let score_group_size_bits = context.score_group_size_bits;
    let max_edge_weight = context.max_edge_weight;
    let max_edge_weight_limbs = context.max_edge_weight_limbs;
    let player_ids_by_index = players.iter().map(|player| player.id).collect::<Vec<_>>();
    let mut bracket_state = BracketState::new(players.len(), sorted_players, current_scores)?;

    while bracket_state.bracket.len() > 1
        || bracket_state.next_score_group_iterator < sorted_players.len()
    {
        let iteration =
            extend_next_score_group(&mut bracket_state, sorted_players, current_scores)?;
        let mut base_edge_weights = compute_score_group_base_weights(
            solver,
            &context,
            &bye_context,
            &bracket_state.bracket,
            iteration.score_group,
        )?;

        apply_score_group_matching(
            solver,
            &mut bracket_state.matched,
            &mut base_edge_weights,
            ScoreGroupMatchingInput {
                bracket: &bracket_state.bracket,
                current_scores,
                iteration,
                default_edge_weight: max_edge_weight,
                score_group_size_bits,
                max_edge_weight_limbs,
            },
        )?;
        let matching = last_solver_matching(solver)?;

        let carried_players = carry_unmatched_players(
            &mut bracket_state.matching_by_id,
            CarryUnmatchedPlayersInput {
                matching: matching.as_slice(),
                matched: &bracket_state.matched,
                player_ids_by_index: &player_ids_by_index,
                sorted_players,
                current_scores,
                bracket: &bracket_state.bracket,
                next_score_group_begin: iteration.next_score_group_begin,
                score_group_iterator: iteration.score_group_iterator,
                bye_assignee_score: bye_context.bye_assignee_score,
            },
        )?;
        bracket_state.bracket = carried_players.bracket;
        bracket_state.score_group_begin = carried_players.score_group_begin;
        bye_context.is_single_downfloater_the_bye_assignee =
            carried_players.is_single_downfloater_the_bye_assignee;
        bracket_state.score_group_begin_vertex = iteration.next_score_group_begin_vertex;
    }

    Ok(bracket_state.matching_by_id)
}

#[cfg(test)]
mod tests {
    use crate::matching::{BigWeight, MatchingSolver};

    use super::{
        BracketState, CarryUnmatchedPlayersInput, CarryUnmatchedPlayersOutput,
        ConfirmedPairFinalizationInput, ExchangePlan, ExchangeSearchInput, MovedDownMatchingInput,
        RemainderGroup, ScoreGroupRange, VertexRange, apply_exchange_search, build_exchange_plan,
        build_remainder, carry_unmatched_players, count_remainder_exchanges,
        extend_next_score_group, extend_score_group, finalize_confirmed_pairs,
        first_remainder_group_end, install_base_edge_weights, match_moved_down_players,
    };
    use crate::swisssystems::bracketing::{BracketSlot, BracketStatus};

    fn fixed_solver(capacity: usize) -> MatchingSolver {
        MatchingSolver::with_vertices(capacity)
    }

    fn slot(player_index: usize, vertex: usize) -> BracketSlot {
        BracketSlot::new(player_index, vertex)
    }

    #[test]
    fn bracket_state_initializes_top_score_group() {
        assert_eq!(
            BracketState::new(5, &[2, 0, 1, 4, 3], &[10, 9, 10, 7, 8]).unwrap(),
            BracketState {
                matching_by_id: vec![None; 5],
                bracket: vec![slot(2, 0), slot(0, 1)],
                next_score_group_iterator: 2,
                matched: vec![false; 5],
                score_group_begin: 0,
                score_group_begin_vertex: 0,
            }
        );
    }

    #[test]
    fn unmatched_players_after_the_last_score_group_do_not_repeat_an_empty_iteration() {
        let mut state = BracketState::new(2, &[0, 1], &[2, 2]).unwrap();
        // Simulate both players being carried after their final group was processed.
        state.score_group_begin = 2;
        assert!(matches!(
            extend_next_score_group(&mut state, &[0, 1], &[2, 2]),
            Err(BracketStatus::EmptyScoreGroup),
        ));
    }

    #[test]
    fn advancing_score_groups_is_progress_even_when_players_are_carried() {
        let mut state = BracketState::new(4, &[0, 1, 2, 3], &[4, 3, 2, 1]).unwrap();
        let first = extend_next_score_group(&mut state, &[0, 1, 2, 3], &[4, 3, 2, 1]).unwrap();
        // Keep every player but mark the old resident as a carried downfloater.
        state.score_group_begin = 1;
        state.score_group_begin_vertex = first.next_score_group_begin_vertex;
        let next = extend_next_score_group(&mut state, &[0, 1, 2, 3], &[4, 3, 2, 1]).unwrap();
        assert_eq!(next.score_group.begin, 1);
        assert_eq!(next.score_group.end, 2);
        assert_eq!(state.next_score_group_iterator, 3);
    }

    #[test]
    fn first_remainder_group_end_returns_boundary_index() {
        assert_eq!(first_remainder_group_end(&[2, 4, 6, 8], 2), Some(6));
    }

    #[test]
    fn first_remainder_group_end_returns_none_when_no_boundary_exists() {
        assert_eq!(first_remainder_group_end(&[2, 4, 6, 8], 4), None);
    }

    #[test]
    fn build_remainder_skips_backmatched_players_and_counts_pairs() {
        let mut matching = vec![0; 15];
        matching[10] = 8;
        matching[11] = 10;
        matching[12] = 13;
        matching[13] = 12;

        assert_eq!(
            build_remainder(
                &matching,
                &[
                    slot(0, 0),
                    slot(1, 10),
                    slot(2, 11),
                    slot(3, 12),
                    slot(4, 13),
                    slot(5, 14),
                ],
                ScoreGroupRange { begin: 1, end: 5 },
                VertexRange { begin: 10, end: 14 },
            )
            .unwrap(),
            RemainderGroup {
                players: vec![2, 3, 4],
                pairs: 2,
                first_group_end: Some(4),
            }
        );
    }

    #[test]
    fn exchange_plan_counts_exchanges_before_first_group_boundary() {
        let mut plan = build_exchange_plan(RemainderGroup {
            players: vec![1, 2, 3, 4],
            pairs: 2,
            first_group_end: Some(3),
        });
        let mut matching = vec![0; 14];
        matching[10] = 11;
        matching[11] = 8;
        matching[12] = 13;
        matching[13] = 12;

        plan.exchange_count = count_remainder_exchanges(
            &matching,
            &[
                slot(0, 0),
                slot(1, 10),
                slot(2, 11),
                slot(3, 12),
                slot(4, 13),
            ],
            &plan.remainder,
            plan.first_group_end,
            VertexRange { begin: 10, end: 14 },
        );

        assert_eq!(
            plan,
            ExchangePlan {
                remainder: vec![1, 2, 3, 4],
                pairs: 2,
                first_group_end: Some(3),
                exchange_count: 1,
            }
        );
    }

    #[test]
    fn apply_exchange_search_finalizes_remainder_pair() {
        let mut solver = fixed_solver(2);
        let mut matched = vec![false; 2];
        let mut base_edge_weights = vec![vec![], vec![BigWeight::from_usize(7)]];

        apply_exchange_search(
            &mut solver,
            &mut matched,
            &mut base_edge_weights,
            ExchangePlan {
                remainder: vec![0, 1],
                pairs: 0,
                first_group_end: None,
                exchange_count: 0,
            },
            ExchangeSearchInput {
                bracket: &[slot(0, 0), slot(1, 1)],
                next_score_group_begin: 2,
                next_score_group_begin_vertex: 2,
                player_count: 2,
                score_group_vertices: VertexRange { begin: 0, end: 2 },
                default_edge_weight: &BigWeight::from_usize(100),
            },
            |base_edge_weights, smaller_player_index, larger_player_index, _, _| {
                base_edge_weights[larger_player_index][smaller_player_index].clone()
            },
        )
        .unwrap();
        solver.compute_matching();

        assert_eq!(matched, vec![true, true]);
        assert_eq!(
            solver.current_matching().expect("matching was computed"),
            &[1, 0]
        );
        assert_eq!(base_edge_weights[1][0], BigWeight::from_usize(7));
    }

    #[test]
    fn extend_score_group_appends_same_score_players_and_vertices() {
        let mut bracket = vec![slot(0, 0)];
        let mut next_score_group_iterator = 1usize;

        let score_group_iterator = extend_score_group(
            &mut bracket,
            &[0, 1, 2, 3],
            &[10, 9, 9, 8],
            &mut next_score_group_iterator,
        );

        assert_eq!(score_group_iterator, 1);
        assert_eq!(next_score_group_iterator, 3);
        assert_eq!(bracket, vec![slot(0, 0), slot(1, 1), slot(2, 2)]);
    }

    #[test]
    fn carry_unmatched_players_records_pairs_and_carries_remaining_players() {
        let mut matching_by_id = vec![None; 5];

        let carried = carry_unmatched_players(
            &mut matching_by_id,
            CarryUnmatchedPlayersInput {
                matching: &[2, 4, 0, 3, 1],
                matched: &[true, false, false, false, false],
                player_ids_by_index: &[2, 0, 4, 1, 3],
                sorted_players: &[0, 1, 2, 3, 4],
                current_scores: &[10, 9, 8, 8, 7],
                bracket: &[slot(0, 0), slot(1, 1), slot(2, 2), slot(3, 3)],
                next_score_group_begin: 2,
                score_group_iterator: 2,
                bye_assignee_score: 8,
            },
        )
        .unwrap();

        assert_eq!(matching_by_id, vec![None, None, Some(2), None, Some(0)]);
        assert_eq!(
            carried,
            CarryUnmatchedPlayersOutput {
                bracket: vec![slot(1, 1), slot(2, 2), slot(3, 3)],
                score_group_begin: 1,
                is_single_downfloater_the_bye_assignee: false,
            }
        );
    }

    #[test]
    fn install_base_edge_weights_uses_bracket_vertices() {
        let mut solver = fixed_solver(4);
        let base_edge_weights = vec![
            vec![],
            vec![BigWeight::from_usize(100)],
            vec![BigWeight::from_usize(1), BigWeight::from_usize(1)],
            vec![
                BigWeight::from_usize(1),
                BigWeight::from_usize(1),
                BigWeight::from_usize(90),
            ],
        ];

        install_base_edge_weights(
            &mut solver,
            &[slot(0, 2), slot(1, 0), slot(2, 3), slot(3, 1)],
            &base_edge_weights,
        )
        .unwrap();
        solver.compute_matching();

        assert_eq!(
            solver.current_matching().expect("matching was computed"),
            &[2, 3, 0, 1]
        );
    }

    #[test]
    fn finalize_confirmed_pairs_marks_match_and_fixes_edge() {
        let mut solver = fixed_solver(4);
        let base_edge_weights = vec![
            vec![],
            vec![BigWeight::from_usize(1)],
            vec![BigWeight::from_usize(10), BigWeight::zero()],
            vec![
                BigWeight::zero(),
                BigWeight::from_usize(8),
                BigWeight::zero(),
            ],
        ];
        install_base_edge_weights(
            &mut solver,
            &[slot(0, 0), slot(1, 1), slot(2, 2), slot(3, 3)],
            &base_edge_weights,
        )
        .unwrap();
        let mut matched = vec![true, false, false, false];

        finalize_confirmed_pairs(
            &mut solver,
            &mut matched,
            ConfirmedPairFinalizationInput {
                bracket: &[slot(0, 0), slot(1, 1), slot(2, 2), slot(3, 3)],
                base_edge_weights: &base_edge_weights,
                score_group: ScoreGroupRange { begin: 1, end: 3 },
                default_edge_weight: &BigWeight::from_usize(100),
            },
        )
        .unwrap();

        solver.compute_matching();

        assert!(matched[0]);
        assert!(matched[2]);
        let matching = solver.current_matching().expect("matching was computed");
        assert_eq!(matching[0], 2);
        assert_eq!(matching[2], 0);
    }

    #[test]
    fn match_moved_down_players_marks_already_current_bracket_match() {
        let mut solver = fixed_solver(4);
        assert!(
            solver
                .set_big_edge_weight(0, 1, BigWeight::from_usize(4))
                .is_ok()
        );
        assert!(
            solver
                .set_big_edge_weight(2, 3, BigWeight::from_usize(3))
                .is_ok()
        );
        solver.compute_matching();

        let base_edge_weights = vec![
            vec![],
            vec![BigWeight::from_usize(4)],
            vec![BigWeight::zero(), BigWeight::zero()],
            vec![
                BigWeight::zero(),
                BigWeight::zero(),
                BigWeight::from_usize(3),
            ],
        ];
        let mut matched = vec![false; 4];

        match_moved_down_players(
            &mut solver,
            &mut matched,
            MovedDownMatchingInput {
                current_scores: &[10, 9, 9, 8],
                bracket: &[slot(0, 0), slot(1, 1), slot(2, 2), slot(3, 3)],
                base_edge_weights: &base_edge_weights,
                score_group: ScoreGroupRange { begin: 1, end: 3 },
                vertices: VertexRange { begin: 1, end: 3 },
            },
        )
        .unwrap();

        assert!(matched[0]);
        assert!(!matched[1]);
        assert!(!matched[2]);
        assert!(!matched[3]);
    }
}
