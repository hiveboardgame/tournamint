//! Exact Double-Swiss selection by ordered scoregroup stages.
//!
//! For one top scoregroup, a legal perfect matching of the whole remaining
//! field simultaneously witnesses a pairable current bracket and a complete
//! lower remainder. In a C4-minimal bracket no two selected upfloaters can be
//! paired together: moving such a pair unchanged into the remainder would
//! reduce the upfloater count by two. The lower endpoints matched to current
//! residents are therefore exactly the selected upfloaters.
//!
//! C4 through the TPN set tie-break are optimized in cumulative matching
//! stages. Once that set is fixed, C8 and the two parts of the FIDE pairing
//! identifier are optimized in three bracket-local stages. This removes the
//! former arbitrary-subset recursion. The remaining worst case is polynomial:
//! several dense blossom solves per scoregroup, with up to `O(n^4)` aggregate
//! matching work when a field contains many tiny scoregroups.

use crate::domain::{Pairing, PlayerId};
use crate::limits::bits_to_represent;
use crate::matching::{BigWeight, MatchingSolver};
use crate::swiss::{ByeAssignment, ByeKind, MatchDisposition, RoundPairings};
use crate::swisssystems::bracketing::{
    BracketPlayer, BracketStatus, ComputeFailure, build_bracket_players, compute_failure,
    set_edge_weight,
};
use crate::swisssystems::common::pairing_from_player_color;

use super::colors::choose_double_swiss_player_color;
use super::forbidden::{
    ForbiddenPairs, build_forbidden_pairs, compatible, eligible_for_bye, empty_forbidden_pairs,
};
use super::input::DoubleSwissInput;
use super::scores::{
    compute_current_scores, player_score_with_acceleration, sorted_pairable_players,
};

struct PreparedDoubleSwissInput<'a> {
    players: Vec<BracketPlayer<'a>>,
    current_scores: Vec<usize>,
    sorted_players: Vec<usize>,
    forbidden_pairs: ForbiddenPairs,
    eligible_for_bye: Vec<bool>,
    previous_floaters: Vec<bool>,
    is_final_round: bool,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct MatchingDiagnostics {
    pub(crate) matching_calls: usize,
    pub(crate) total_vertices: usize,
    pub(crate) submitted_compatible_edges: usize,
    pub(crate) max_weight_limb_len: usize,
}

#[derive(Default)]
struct MatchingInstrumentation {
    #[cfg(test)]
    diagnostics: MatchingDiagnostics,
}

impl MatchingInstrumentation {
    fn record_matching_call(&mut self, vertex_count: usize) -> Result<(), BracketStatus> {
        #[cfg(test)]
        {
            self.diagnostics.matching_calls = self
                .diagnostics
                .matching_calls
                .checked_add(1)
                .expect("matching diagnostics counter fits usize");
            self.diagnostics.total_vertices = self
                .diagnostics
                .total_vertices
                .checked_add(vertex_count)
                .expect("matching diagnostics counter fits usize");
        }
        #[cfg(not(test))]
        let _ = vertex_count;
        Ok(())
    }

    #[cfg(test)]
    fn record_submitted_compatible_edge(
        &mut self,
        weight_limb_len: usize,
    ) -> Result<(), BracketStatus> {
        self.diagnostics.submitted_compatible_edges = self
            .diagnostics
            .submitted_compatible_edges
            .checked_add(1)
            .expect("matching diagnostics counter fits usize");
        self.diagnostics.max_weight_limb_len =
            self.diagnostics.max_weight_limb_len.max(weight_limb_len);
        Ok(())
    }
}

fn copied_vec<T: Copy>(values: &[T]) -> Result<Vec<T>, BracketStatus> {
    Ok(values.to_vec())
}

fn build_player_indices_by_id(
    players: &[BracketPlayer<'_>],
) -> Result<Vec<Option<usize>>, BracketStatus> {
    let mut player_indices = Vec::with_capacity(players.len());
    player_indices.resize(players.len(), None);
    for (player_index, player) in players.iter().enumerate() {
        let index = player_indices
            .get_mut(player.id)
            .ok_or(BracketStatus::MalformedTournament)?;
        if index.is_none() {
            *index = Some(player_index);
        }
    }
    Ok(player_indices)
}

fn player_index_by_id(
    player_indices: &[Option<usize>],
    player_id: usize,
) -> Result<usize, BracketStatus> {
    player_indices
        .get(player_id)
        .copied()
        .flatten()
        .ok_or(BracketStatus::MalformedTournament)
}

fn is_final_round(input: &DoubleSwissInput<'_>) -> Result<bool, BracketStatus> {
    let current_round = input
        .pairing
        .played_rounds
        .checked_add(1)
        .ok_or(BracketStatus::MalformedTournament)?;
    if current_round > input.expected_rounds {
        return Err(BracketStatus::MalformedTournament);
    }
    Ok(current_round == input.expected_rounds)
}

fn build_previous_floaters(
    input: &DoubleSwissInput<'_>,
    players: &[BracketPlayer<'_>],
    final_round: bool,
) -> Result<Vec<bool>, BracketStatus> {
    let mut result = Vec::with_capacity(players.len());
    if final_round || input.pairing.played_rounds == 0 {
        result.resize(players.len(), false);
        return Ok(result);
    }

    let player_indices = build_player_indices_by_id(players)?;
    let previous_round = input
        .pairing
        .played_rounds
        .checked_sub(1)
        .ok_or(BracketStatus::MalformedTournament)?;
    for player in players {
        let Some(record) = player.records.get(previous_round).copied() else {
            result.push(false);
            continue;
        };
        let crate::swiss::PlayerRoundRecord::Match {
            opponent,
            state:
                crate::swiss::SwissMatchRecord::Complete {
                    disposition: MatchDisposition::Regular,
                    ..
                },
            ..
        } = record
        else {
            result.push(false);
            continue;
        };
        let opponent_index = player_index_by_id(&player_indices, opponent.index())?;
        let opponent = players
            .get(opponent_index)
            .ok_or(BracketStatus::MalformedTournament)?;
        let player_score = player_score_with_acceleration(input, player, 1)?;
        let opponent_score = player_score_with_acceleration(input, opponent, 1)?;
        result.push(player_score != opponent_score);
    }
    Ok(result)
}

fn prepare_double_swiss_input<'a>(
    input: &DoubleSwissInput<'a>,
) -> Result<PreparedDoubleSwissInput<'a>, ComputeFailure> {
    let players = build_bracket_players(input.pairing.tournament).map_err(compute_failure)?;
    let current_scores = compute_current_scores(input, &players).map_err(compute_failure)?;
    let sorted_players =
        sorted_pairable_players(input, &players, &current_scores).map_err(compute_failure)?;
    let forbidden_pairs = if sorted_players.is_empty() {
        empty_forbidden_pairs(players.len()).map_err(compute_failure)?
    } else {
        build_forbidden_pairs(input, &players).map_err(compute_failure)?
    };
    let mut eligible_for_bye_players = Vec::with_capacity(players.len());
    eligible_for_bye_players.extend(players.iter().map(eligible_for_bye));
    let final_round = is_final_round(input).map_err(compute_failure)?;
    let previous_floaters =
        build_previous_floaters(input, &players, final_round).map_err(compute_failure)?;

    Ok(PreparedDoubleSwissInput {
        players,
        current_scores,
        sorted_players,
        forbidden_pairs,
        eligible_for_bye: eligible_for_bye_players,
        previous_floaters,
        is_final_round: final_round,
    })
}

/// Appends one lexicographic field below every field already present.
///
/// `matching_sum_bound` is an inclusive bound on the sum of this field over
/// any matching, rather than merely a per-edge bound. Consequently its binary
/// width leaves no path for a carry into the preceding priority field.
fn append_field(
    weight: &mut BigWeight,
    matching_sum_bound: usize,
    edge_digit: usize,
) -> Result<(), BracketStatus> {
    if edge_digit > matching_sum_bound {
        return Err(BracketStatus::MalformedTournament);
    }
    weight.shl_assign(bits_to_represent(matching_sum_bound));
    weight.add_assign_usize(edge_digit);
    Ok(())
}

fn perfect_matching(mut solver: MatchingSolver) -> Result<Option<Vec<usize>>, BracketStatus> {
    solver.compute_matching();
    let matching = solver
        .current_matching()
        .ok_or(BracketStatus::MalformedTournament)?;
    if matching
        .iter()
        .enumerate()
        .any(|(vertex, opponent)| vertex == *opponent)
    {
        return Ok(None);
    }
    copied_vec(matching).map(Some)
}

fn pab_edge_weight(
    candidate: Option<usize>,
    prepared: &PreparedDoubleSwissInput<'_>,
    maximum_score: usize,
    maximum_played_matches: usize,
    maximum_rank: usize,
) -> Result<BigWeight, BracketStatus> {
    // The leading one is the maximum-cardinality digit. Every lower field is
    // nonzero on at most the single edge incident with the dummy vertex.
    let mut weight = BigWeight::from_usize(1);
    let score_digit = candidate
        .map(|player| maximum_score - prepared.current_scores[player])
        .unwrap_or(0);
    append_field(&mut weight, maximum_score, score_digit)?;
    let played_matches_digit = candidate
        .map(|player| prepared.players[player].played_matches)
        .unwrap_or(0);
    append_field(&mut weight, maximum_played_matches, played_matches_digit)?;
    let rank_digit = candidate
        .map(|player| prepared.players[player].rank_index)
        .unwrap_or(0);
    append_field(&mut weight, maximum_rank, rank_digit)?;
    Ok(weight)
}

fn choose_bye(
    prepared: &PreparedDoubleSwissInput<'_>,
    diagnostics: &mut MatchingInstrumentation,
) -> Result<Option<usize>, BracketStatus> {
    if prepared.sorted_players.len() & 1 == 0 {
        return Ok(None);
    }

    let maximum_score = prepared
        .sorted_players
        .iter()
        .map(|player| prepared.current_scores[*player])
        .max()
        .ok_or(BracketStatus::MalformedTournament)?;
    let maximum_played_matches = prepared
        .sorted_players
        .iter()
        .map(|player| prepared.players[*player].played_matches)
        .max()
        .ok_or(BracketStatus::MalformedTournament)?;
    let maximum_rank = prepared
        .sorted_players
        .iter()
        .map(|player| prepared.players[*player].rank_index)
        .max()
        .ok_or(BracketStatus::MalformedTournament)?;
    let dummy = prepared.sorted_players.len();
    let vertex_count = dummy
        .checked_add(1)
        .expect("a pairable Double-Swiss field leaves room for its dummy vertex");
    diagnostics.record_matching_call(vertex_count)?;
    let mut solver = MatchingSolver::with_vertices(vertex_count);

    for right in 0..prepared.sorted_players.len() {
        for left in 0..right {
            if compatible(
                &prepared.players,
                &prepared.forbidden_pairs,
                prepared.sorted_players[left],
                prepared.sorted_players[right],
            ) {
                let weight = pab_edge_weight(
                    None,
                    prepared,
                    maximum_score,
                    maximum_played_matches,
                    maximum_rank,
                )?;
                #[cfg(test)]
                let weight_limb_len = weight.limb_len();
                set_edge_weight(&mut solver, left, right, weight)?;
                #[cfg(test)]
                diagnostics.record_submitted_compatible_edge(weight_limb_len)?;
            }
        }
    }
    for vertex in 0..prepared.sorted_players.len() {
        let player = prepared.sorted_players[vertex];
        if prepared.eligible_for_bye[player] {
            let weight = pab_edge_weight(
                Some(player),
                prepared,
                maximum_score,
                maximum_played_matches,
                maximum_rank,
            )?;
            #[cfg(test)]
            let weight_limb_len = weight.limb_len();
            set_edge_weight(&mut solver, vertex, dummy, weight)?;
            #[cfg(test)]
            diagnostics.record_submitted_compatible_edge(weight_limb_len)?;
        }
    }

    let matching = perfect_matching(solver)?.ok_or(BracketStatus::NoValidPairing)?;
    let bye_vertex = *matching
        .get(dummy)
        .ok_or(BracketStatus::MalformedTournament)?;
    prepared
        .sorted_players
        .get(bye_vertex)
        .copied()
        .map(Some)
        .ok_or(BracketStatus::MalformedTournament)
}

fn distinct_lower_score_thresholds(
    prepared: &PreparedDoubleSwissInput<'_>,
    lower: &[usize],
) -> Result<Vec<usize>, BracketStatus> {
    let mut thresholds = Vec::with_capacity(lower.len());
    thresholds.extend(lower.iter().map(|player| prepared.current_scores[*player]));
    thresholds.sort_unstable();
    thresholds.dedup();
    // The count strictly above the largest score is always zero. At every
    // earlier ascending threshold, maximizing the count above it minimizes
    // the multiplicity of that low score. With C4 fixing the vector length,
    // these fields therefore maximize its ascending score multiset.
    thresholds.pop();
    Ok(thresholds)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SelectionStage {
    C4,
    C5,
    C6,
    C7,
    TpnSet,
}

impl SelectionStage {
    fn includes_c5(self) -> bool {
        !matches!(self, Self::C4)
    }

    fn includes_c6(self) -> bool {
        matches!(self, Self::C6 | Self::C7 | Self::TpnSet)
    }

    fn includes_c7(self) -> bool {
        matches!(self, Self::C7 | Self::TpnSet)
    }

    fn includes_tpn_set(self) -> bool {
        matches!(self, Self::TpnSet)
    }
}

struct SelectionContext<'a, 'input> {
    prepared: &'a PreparedDoubleSwissInput<'input>,
    remaining: &'a [usize],
    resident_count: usize,
    following_group_end: usize,
    score_thresholds: &'a [usize],
}

fn selection_edge_weight(
    context: &SelectionContext<'_, '_>,
    stage: SelectionStage,
    left: usize,
    right: usize,
) -> Result<BigWeight, BracketStatus> {
    let edge_count_bound = context.remaining.len() / 2;
    let is_resident_cross = left < context.resident_count && right >= context.resident_count;
    let is_following_group_cross = left >= context.resident_count
        && left < context.following_group_end
        && right >= context.following_group_end;

    // C4: fixed-cardinality matchings maximize non-cross edges, equivalently
    // minimizing the number of R-to-lower edges.
    let mut weight = BigWeight::from_usize(1);
    append_field(
        &mut weight,
        edge_count_bound,
        usize::from(!is_resident_cross),
    )?;

    if stage.includes_c5() {
        // C5: compare selected lower endpoints by ascending score multiset.
        for threshold in context.score_thresholds {
            let digit = usize::from(
                is_resident_cross
                    && context.prepared.current_scores[context.remaining[right]] > *threshold,
            );
            append_field(&mut weight, edge_count_bound, digit)?;
        }
    }

    if stage.includes_c6() {
        // C6: R-to-following-group edges remove those players before the next
        // bracket. Only an edge from a remaining member of that group to a
        // still lower player is an outgoing upfloat there. If the group is
        // emptied, this count is structurally zero.
        append_field(
            &mut weight,
            edge_count_bound,
            usize::from(!is_following_group_cross),
        )?;
    }

    // C7 is absent in the final round. Its selected-set cardinality is fixed
    // by C4, so rewarding non-previous floaters minimizes previous ones.
    if stage.includes_c7() && !context.prepared.is_final_round {
        let digit = usize::from(
            is_resident_cross && !context.prepared.previous_floaters[context.remaining[right]],
        );
        append_field(&mut weight, edge_count_bound, digit)?;
    }

    // The remaining C5 tie-break is lexicographic U membership in the same
    // descending-score/ascending-TPN order already carried by `remaining`.
    // Each vertex can contribute to its own field at most once.
    if stage.includes_tpn_set() {
        for candidate in context.resident_count..context.remaining.len() {
            let digit = usize::from(is_resident_cross && right == candidate);
            append_field(&mut weight, 1, digit)?;
        }
    }
    Ok(weight)
}

fn solve_selection_stage(
    context: &SelectionContext<'_, '_>,
    stage: SelectionStage,
    diagnostics: &mut MatchingInstrumentation,
) -> Result<Option<Vec<usize>>, BracketStatus> {
    diagnostics.record_matching_call(context.remaining.len())?;
    let mut solver = MatchingSolver::with_vertices(context.remaining.len());
    for right in 0..context.remaining.len() {
        for left in 0..right {
            if compatible(
                &context.prepared.players,
                &context.prepared.forbidden_pairs,
                context.remaining[left],
                context.remaining[right],
            ) {
                let weight = selection_edge_weight(context, stage, left, right)?;
                #[cfg(test)]
                let weight_limb_len = weight.limb_len();
                set_edge_weight(&mut solver, left, right, weight)?;
                #[cfg(test)]
                diagnostics.record_submitted_compatible_edge(weight_limb_len)?;
            }
        }
    }
    perfect_matching(solver)
}

fn selection_cross_count(matching: &[usize], resident_count: usize) -> usize {
    matching
        .iter()
        .copied()
        .enumerate()
        .filter(|(left, right)| *left < resident_count && *right >= resident_count)
        .count()
}

fn selection_above_threshold_count(
    prepared: &PreparedDoubleSwissInput<'_>,
    remaining: &[usize],
    matching: &[usize],
    resident_count: usize,
    threshold: usize,
) -> usize {
    matching
        .iter()
        .copied()
        .enumerate()
        .filter(|(left, right)| {
            *left < resident_count
                && *right >= resident_count
                && prepared.current_scores[remaining[*right]] > threshold
        })
        .count()
}

fn selection_following_cross_count(
    matching: &[usize],
    resident_count: usize,
    following_group_end: usize,
) -> usize {
    matching
        .iter()
        .copied()
        .enumerate()
        .filter(|(left, right)| {
            *left >= resident_count && *left < following_group_end && *right >= following_group_end
        })
        .count()
}

fn selection_previous_floater_count(
    prepared: &PreparedDoubleSwissInput<'_>,
    remaining: &[usize],
    matching: &[usize],
    resident_count: usize,
) -> usize {
    matching
        .iter()
        .copied()
        .enumerate()
        .filter(|(left, right)| {
            *left < resident_count
                && *right >= resident_count
                && prepared.previous_floaters[remaining[*right]]
        })
        .count()
}

fn debug_assert_selection_prefix_retained(
    context: &SelectionContext<'_, '_>,
    retained_stage: SelectionStage,
    previous: &[usize],
    current: &[usize],
) {
    debug_assert_eq!(
        selection_cross_count(previous, context.resident_count),
        selection_cross_count(current, context.resident_count)
    );
    if retained_stage.includes_c5() {
        for threshold in context.score_thresholds {
            debug_assert_eq!(
                selection_above_threshold_count(
                    context.prepared,
                    context.remaining,
                    previous,
                    context.resident_count,
                    *threshold,
                ),
                selection_above_threshold_count(
                    context.prepared,
                    context.remaining,
                    current,
                    context.resident_count,
                    *threshold,
                )
            );
        }
    }
    if retained_stage.includes_c6() {
        debug_assert_eq!(
            selection_following_cross_count(
                previous,
                context.resident_count,
                context.following_group_end,
            ),
            selection_following_cross_count(
                current,
                context.resident_count,
                context.following_group_end,
            )
        );
    }
    if retained_stage.includes_c7() && !context.prepared.is_final_round {
        debug_assert_eq!(
            selection_previous_floater_count(
                context.prepared,
                context.remaining,
                previous,
                context.resident_count,
            ),
            selection_previous_floater_count(
                context.prepared,
                context.remaining,
                current,
                context.resident_count,
            )
        );
    }
}

fn select_upfloaters(
    prepared: &PreparedDoubleSwissInput<'_>,
    remaining: &[usize],
    resident_count: usize,
    diagnostics: &mut MatchingInstrumentation,
) -> Result<Option<Vec<usize>>, BracketStatus> {
    let lower = &remaining[resident_count..];
    let score_thresholds = distinct_lower_score_thresholds(prepared, lower)?;
    let following_group_end = lower.first().map_or(resident_count, |first| {
        resident_count
            + lower
                .iter()
                .take_while(|player| {
                    prepared.current_scores[**player] == prepared.current_scores[*first]
                })
                .count()
    });
    let context = SelectionContext {
        prepared,
        remaining,
        resident_count,
        following_group_end,
        score_thresholds: &score_thresholds,
    };
    let stages: &[SelectionStage] = if prepared.is_final_round {
        &[
            SelectionStage::C4,
            SelectionStage::C5,
            SelectionStage::C6,
            SelectionStage::TpnSet,
        ]
    } else {
        &[
            SelectionStage::C4,
            SelectionStage::C5,
            SelectionStage::C6,
            SelectionStage::C7,
            SelectionStage::TpnSet,
        ]
    };
    let mut previous_stage = None;
    let mut final_matching = None;
    for stage in stages {
        let Some(matching) = solve_selection_stage(&context, *stage, diagnostics)? else {
            return Ok(None);
        };
        if let (Some(retained_stage), Some(previous)) = (previous_stage, final_matching.as_deref())
        {
            debug_assert_selection_prefix_retained(&context, retained_stage, previous, &matching);
        }
        previous_stage = Some(*stage);
        final_matching = Some(matching);
    }
    let matching = final_matching.ok_or(BracketStatus::MalformedTournament)?;

    let mut upfloaters = Vec::with_capacity(resident_count.min(lower.len()));
    for (left, right) in matching.iter().copied().enumerate() {
        if left < resident_count && right >= resident_count {
            upfloaters.push(remaining[right]);
        }
    }
    Ok(Some(upfloaters))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BracketStage {
    C8,
    TopSet,
    BottomVector,
}

fn bracket_edge_weight(
    prepared: &PreparedDoubleSwissInput<'_>,
    ordered: &[usize],
    is_upfloater: &[bool],
    ordered_top_vertices: &[usize],
    stage: BracketStage,
    left: usize,
    right: usize,
) -> Result<BigWeight, BracketStatus> {
    let edge_count_bound = ordered.len() / 2;
    let c8_sum_bound = edge_count_bound
        .checked_mul(2)
        .expect("twice half a slice length fits usize");
    let left_player = ordered[left];
    let right_player = ordered[right];
    let c8_cost =
        usize::from(is_upfloater[left_player] && prepared.previous_floaters[right_player])
            + usize::from(is_upfloater[right_player] && prepared.previous_floaters[left_player]);
    let c8_digit = 2usize
        .checked_sub(c8_cost)
        .ok_or(BracketStatus::MalformedTournament)?;

    let mut weight = BigWeight::from_usize(1);
    append_field(&mut weight, c8_sum_bound, c8_digit)?;

    if matches!(stage, BracketStage::TopSet) {
        // Every matched edge contributes its earlier TPN endpoint to the top
        // set. One one-bit field per endpoint reproduces lexicographically
        // smallest top-set selection without an edge-sized bit universe.
        for candidate_top in 0..ordered.len() {
            append_field(&mut weight, 1, usize::from(left == candidate_top))?;
        }
    }

    if matches!(stage, BracketStage::BottomVector) {
        // The graph for this stage already fixes the top/bottom partition.
        // One log(n)-width field per ordered top endpoint now minimizes its
        // corresponding bottom endpoint. At most one edge contributes to each
        // field.
        for candidate_top in ordered_top_vertices {
            let bottom_digit = if left == *candidate_top {
                ordered
                    .len()
                    .checked_sub(right)
                    .ok_or(BracketStatus::MalformedTournament)?
            } else {
                0
            };
            append_field(&mut weight, ordered.len(), bottom_digit)?;
        }
    }
    Ok(weight)
}

fn solve_bracket_stage(
    prepared: &PreparedDoubleSwissInput<'_>,
    ordered: &[usize],
    is_upfloater: &[bool],
    fixed_top_vertices: Option<&[bool]>,
    ordered_top_vertices: &[usize],
    stage: BracketStage,
    diagnostics: &mut MatchingInstrumentation,
) -> Result<Option<Vec<usize>>, BracketStatus> {
    diagnostics.record_matching_call(ordered.len())?;
    let mut solver = MatchingSolver::with_vertices(ordered.len());
    for right in 0..ordered.len() {
        for left in 0..right {
            if let Some(is_top) = fixed_top_vertices {
                let left_is_top = *is_top.get(left).ok_or(BracketStatus::MalformedTournament)?;
                let right_is_top = *is_top
                    .get(right)
                    .ok_or(BracketStatus::MalformedTournament)?;
                if !left_is_top || right_is_top {
                    continue;
                }
            }
            if compatible(
                &prepared.players,
                &prepared.forbidden_pairs,
                ordered[left],
                ordered[right],
            ) {
                let weight = bracket_edge_weight(
                    prepared,
                    ordered,
                    is_upfloater,
                    ordered_top_vertices,
                    stage,
                    left,
                    right,
                )?;
                #[cfg(test)]
                let weight_limb_len = weight.limb_len();
                set_edge_weight(&mut solver, left, right, weight)?;
                #[cfg(test)]
                diagnostics.record_submitted_compatible_edge(weight_limb_len)?;
            }
        }
    }
    perfect_matching(solver)
}

fn matching_c8_cost(
    prepared: &PreparedDoubleSwissInput<'_>,
    ordered: &[usize],
    is_upfloater: &[bool],
    matching: &[usize],
) -> usize {
    matching
        .iter()
        .copied()
        .enumerate()
        .filter(|(left, right)| left < right)
        .map(|(left, right)| {
            let left_player = ordered[left];
            let right_player = ordered[right];
            usize::from(is_upfloater[left_player] && prepared.previous_floaters[right_player])
                + usize::from(is_upfloater[right_player] && prepared.previous_floaters[left_player])
        })
        .sum()
}

fn fully_compatible_zero_upfloater_pairs(
    prepared: &PreparedDoubleSwissInput<'_>,
    ordered: &[usize],
    upfloaters: &[usize],
) -> Result<Option<Vec<(usize, usize)>>, BracketStatus> {
    if !upfloaters.is_empty() || ordered.len() & 1 != 0 {
        return Ok(None);
    }
    for right in 0..ordered.len() {
        for left in 0..right {
            if !compatible(
                &prepared.players,
                &prepared.forbidden_pairs,
                ordered[left],
                ordered[right],
            ) {
                return Ok(None);
            }
        }
    }

    // C8 is identically zero without upfloaters. In a complete compatibility
    // graph, the lexicographically smallest top set is therefore the first
    // half of the TPN order. With that partition fixed, the smallest bottom
    // vector pairs each top player with the corresponding second-half player.
    let half = ordered.len() / 2;
    let mut pairs = Vec::with_capacity(half);
    for top in 0..half {
        let bottom = top
            .checked_add(half)
            .expect("two half-slice indices fit usize");
        pairs.push((
            *ordered.get(top).ok_or(BracketStatus::MalformedTournament)?,
            *ordered
                .get(bottom)
                .ok_or(BracketStatus::MalformedTournament)?,
        ));
    }
    Ok(Some(pairs))
}

fn pair_bracket(
    prepared: &PreparedDoubleSwissInput<'_>,
    bracket: &[usize],
    upfloaters: &[usize],
    diagnostics: &mut MatchingInstrumentation,
) -> Result<Option<Vec<(usize, usize)>>, BracketStatus> {
    if bracket.len() & 1 != 0 {
        return Ok(None);
    }
    let mut ordered = copied_vec(bracket)?;
    ordered.sort_unstable_by_key(|player| (prepared.players[*player].rank_index, *player));
    if let Some(pairs) = fully_compatible_zero_upfloater_pairs(prepared, &ordered, upfloaters)? {
        return Ok(Some(pairs));
    }
    let mut is_upfloater = Vec::with_capacity(prepared.players.len());
    is_upfloater.resize(prepared.players.len(), false);
    for player in upfloaters {
        *is_upfloater
            .get_mut(*player)
            .ok_or(BracketStatus::MalformedTournament)? = true;
    }

    let Some(c8_matching) = solve_bracket_stage(
        prepared,
        &ordered,
        &is_upfloater,
        None,
        &[],
        BracketStage::C8,
        diagnostics,
    )?
    else {
        return Ok(None);
    };
    let Some(top_matching) = solve_bracket_stage(
        prepared,
        &ordered,
        &is_upfloater,
        None,
        &[],
        BracketStage::TopSet,
        diagnostics,
    )?
    else {
        return Ok(None);
    };
    debug_assert_eq!(
        matching_c8_cost(prepared, &ordered, &is_upfloater, &c8_matching),
        matching_c8_cost(prepared, &ordered, &is_upfloater, &top_matching)
    );

    let mut fixed_top_vertices = Vec::with_capacity(ordered.len());
    fixed_top_vertices.resize(ordered.len(), false);
    let mut ordered_top_vertices = Vec::with_capacity(ordered.len() / 2);
    for (left, right) in top_matching.iter().copied().enumerate() {
        if left < right {
            fixed_top_vertices[left] = true;
            ordered_top_vertices.push(left);
        }
    }

    let Some(matching) = solve_bracket_stage(
        prepared,
        &ordered,
        &is_upfloater,
        Some(&fixed_top_vertices),
        &ordered_top_vertices,
        BracketStage::BottomVector,
        diagnostics,
    )?
    else {
        return Ok(None);
    };
    debug_assert_eq!(
        matching_c8_cost(prepared, &ordered, &is_upfloater, &top_matching),
        matching_c8_cost(prepared, &ordered, &is_upfloater, &matching)
    );
    debug_assert!(matching.iter().copied().enumerate().all(|(left, right)| {
        left >= right || (fixed_top_vertices[left] && !fixed_top_vertices[right])
    }));

    let mut pairs = Vec::with_capacity(ordered.len() / 2);
    for (left, right) in matching.iter().copied().enumerate() {
        if left < right {
            pairs.push((ordered[left], ordered[right]));
        }
    }
    Ok(Some(pairs))
}

fn pair_remaining(
    prepared: &PreparedDoubleSwissInput<'_>,
    initial_remaining: &[usize],
    diagnostics: &mut MatchingInstrumentation,
) -> Result<Option<Vec<(usize, usize)>>, BracketStatus> {
    if initial_remaining.is_empty() {
        return Ok(Some(Vec::new()));
    }
    if initial_remaining.len() & 1 != 0 {
        return Ok(None);
    }

    let mut remaining = copied_vec(initial_remaining)?;
    let mut pairs = Vec::with_capacity(initial_remaining.len() / 2);
    let mut selected = Vec::with_capacity(prepared.players.len());
    selected.resize(prepared.players.len(), false);

    while !remaining.is_empty() {
        let resident_score = prepared.current_scores[remaining[0]];
        let resident_count = remaining
            .iter()
            .take_while(|player| prepared.current_scores[**player] == resident_score)
            .count();
        if resident_count == remaining.len() {
            let Some(mut bracket_pairs) = pair_bracket(prepared, &remaining, &[], diagnostics)?
            else {
                return Ok(None);
            };
            pairs.append(&mut bracket_pairs);
            break;
        }
        let Some(upfloaters) =
            select_upfloaters(prepared, &remaining, resident_count, diagnostics)?
        else {
            return Ok(None);
        };

        let bracket_len = resident_count
            .checked_add(upfloaters.len())
            .expect("selected bracket is a subset of the remaining field");
        let mut bracket = Vec::with_capacity(bracket_len);
        bracket.extend_from_slice(&remaining[..resident_count]);
        bracket.extend_from_slice(&upfloaters);

        selected.fill(false);
        for player in &upfloaters {
            *selected
                .get_mut(*player)
                .ok_or(BracketStatus::MalformedTournament)? = true;
        }
        let mut lower_remaining = Vec::with_capacity(
            remaining
                .len()
                .checked_sub(bracket_len)
                .ok_or(BracketStatus::MalformedTournament)?,
        );
        lower_remaining.extend(
            remaining[resident_count..]
                .iter()
                .copied()
                .filter(|player| !selected[*player]),
        );

        // The final selection matching decomposes into perfect matchings of
        // R+U and of the lower remainder. A U-U edge in the former would
        // combine with the latter into a full matching with two fewer C4
        // cross edges, contradicting the first selection stage.
        let mut bracket_pairs = pair_bracket(prepared, &bracket, &upfloaters, diagnostics)?
            .ok_or(BracketStatus::MalformedTournament)?;
        pairs.append(&mut bracket_pairs);
        remaining = lower_remaining;
    }
    Ok(Some(pairs))
}

fn build_result(
    input: &DoubleSwissInput<'_>,
    prepared: &PreparedDoubleSwissInput<'_>,
    pairs: Vec<(usize, usize)>,
    bye: Option<usize>,
) -> Result<RoundPairings, BracketStatus> {
    let mut result = RoundPairings::default();
    result.games.reserve(pairs.len());
    result.byes.reserve(usize::from(bye.is_some()));
    for (player, opponent) in pairs {
        let color = choose_double_swiss_player_color(
            input,
            &prepared.players,
            &prepared.current_scores,
            player,
            opponent,
        );
        let players = pairing_from_player_color(
            prepared.players[player].id,
            prepared.players[opponent].id,
            color,
        );
        result.games.push(Pairing::new(
            PlayerId::new(players.white),
            PlayerId::new(players.black),
        ));
    }
    if let Some(bye_player) = bye {
        result.byes.push(ByeAssignment {
            player: PlayerId::new(prepared.players[bye_player].id),
            kind: ByeKind::PairingAllocated,
        });
    }
    Ok(result)
}

fn compute_prepared_matching(
    input: &DoubleSwissInput<'_>,
    prepared: &PreparedDoubleSwissInput<'_>,
    diagnostics: &mut MatchingInstrumentation,
) -> Result<RoundPairings, ComputeFailure> {
    if prepared.sorted_players.is_empty() {
        return Ok(RoundPairings::default());
    }

    let bye = choose_bye(prepared, diagnostics).map_err(compute_failure)?;
    let mut remaining = copied_vec(&prepared.sorted_players).map_err(compute_failure)?;
    if let Some(bye_player) = bye {
        remaining.retain(|player| *player != bye_player);
    }
    let pairs = pair_remaining(prepared, &remaining, diagnostics)
        .map_err(compute_failure)?
        .ok_or_else(|| compute_failure(BracketStatus::NoValidPairing))?;
    build_result(input, prepared, pairs, bye).map_err(compute_failure)
}

pub(super) fn compute_matching_impl(
    input: &DoubleSwissInput<'_>,
) -> Result<RoundPairings, ComputeFailure> {
    let prepared = prepare_double_swiss_input(input)?;
    compute_prepared_matching(input, &prepared, &mut MatchingInstrumentation::default())
}

#[cfg(test)]
pub(super) fn compute_matching_impl_with_diagnostics(
    input: &DoubleSwissInput<'_>,
) -> Result<(RoundPairings, MatchingDiagnostics), ComputeFailure> {
    let prepared = prepare_double_swiss_input(input)?;
    let mut instrumentation = MatchingInstrumentation::default();
    let round = compute_prepared_matching(input, &prepared, &mut instrumentation)?;
    Ok((round, instrumentation.diagnostics))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Color, ForbiddenPairEntry, PlayerColorData, PointSystem, Score};
    use crate::swiss::{SwissPairingPlayerEntry, SwissPairingSnapshot};
    use crate::swisssystems::common::PairingContext;

    fn flat_tournament(
        rank_by_player: &[usize],
        forbidden_pair: Option<(usize, usize)>,
    ) -> SwissPairingSnapshot {
        let mut tournament = SwissPairingSnapshot::new();
        for (player, rank_index) in rank_by_player.iter().copied().enumerate() {
            tournament.push_player(SwissPairingPlayerEntry {
                id: PlayerId::new(player),
                is_valid: true,
                rank_index,
                score_without_acceleration: Score::new(0),
                played_games: 0,
                played_matches: 0,
                color_data: PlayerColorData::no_played_color_history(),
                records: &[],
                accelerations: &[],
            });
        }
        if let Some((left, right)) = forbidden_pair {
            let players = [PlayerId::new(left), PlayerId::new(right)];
            tournament.push_forbidden_pair(ForbiddenPairEntry {
                round_range: 0..1,
                players: &players,
            });
        }
        tournament
    }

    fn flat_input(tournament: &SwissPairingSnapshot) -> DoubleSwissInput<'_> {
        DoubleSwissInput {
            pairing: PairingContext {
                tournament,
                played_rounds: 0,
                point_system: PointSystem::STANDARD,
                initial_color: Color::White,
            },
            expected_rounds: 1,
            points_type_max: usize::MAX,
        }
    }

    fn prepare(tournament: &SwissPairingSnapshot) -> PreparedDoubleSwissInput<'_> {
        let input = flat_input(tournament);
        prepare_double_swiss_input(&input).expect("valid flat Double-Swiss input")
    }

    #[test]
    fn complete_zero_upfloater_bracket_has_positional_fide_identifier() {
        let tournament = flat_tournament(&[4, 0, 5, 1, 6, 2, 7, 3], None);
        let prepared = prepare(&tournament);
        let bracket = [6, 5, 4, 3, 2, 1, 0, 7];
        let mut ordered = bracket;
        ordered.sort_unstable_by_key(|player| (prepared.players[*player].rank_index, *player));
        assert_eq!(ordered, [1, 3, 5, 7, 0, 2, 4, 6]);

        let expected = vec![(1, 0), (3, 2), (5, 4), (7, 6)];
        assert_eq!(
            fully_compatible_zero_upfloater_pairs(&prepared, &ordered, &[]).unwrap(),
            Some(expected.clone())
        );
        let mut instrumentation = MatchingInstrumentation::default();
        assert_eq!(
            pair_bracket(&prepared, &bracket, &[], &mut instrumentation).unwrap(),
            Some(expected.clone())
        );
        assert_eq!(
            pair_remaining(&prepared, &ordered, &mut instrumentation).unwrap(),
            Some(expected)
        );
        assert_eq!(instrumentation.diagnostics.matching_calls, 0);

        let (round, diagnostics) =
            compute_matching_impl_with_diagnostics(&flat_input(&tournament)).unwrap();
        assert_eq!(round.games.len(), 4);
        assert_eq!(diagnostics.matching_calls, 0);
    }

    #[test]
    fn complete_bracket_fast_path_rejects_upfloaters_and_missing_edges() {
        let tournament = flat_tournament(&[0, 1, 2, 3], None);
        let prepared = prepare(&tournament);
        let ordered = [0, 1, 2, 3];
        assert_eq!(
            fully_compatible_zero_upfloater_pairs(&prepared, &ordered, &[2]).unwrap(),
            None
        );

        let tournament = flat_tournament(&[0, 1, 2, 3], Some((0, 3)));
        let prepared = prepare(&tournament);
        assert_eq!(
            fully_compatible_zero_upfloater_pairs(&prepared, &ordered, &[]).unwrap(),
            None
        );
    }

    #[test]
    fn restricted_bracket_reports_test_only_matching_diagnostics() {
        let tournament = flat_tournament(&[0, 1, 2, 3], Some((0, 3)));
        let input = flat_input(&tournament);

        let (round, diagnostics) = compute_matching_impl_with_diagnostics(&input).unwrap();
        assert_eq!(round.games.len(), 2);
        assert_eq!(diagnostics.matching_calls, 3);
        assert_eq!(diagnostics.total_vertices, 12);
        assert_eq!(diagnostics.submitted_compatible_edges, 13);
        assert!(diagnostics.max_weight_limb_len > 0);
    }

    #[test]
    fn pab_and_complete_remainder_report_test_only_matching_diagnostics() {
        let tournament = flat_tournament(&[0, 1, 2], None);
        let input = flat_input(&tournament);

        let (round, diagnostics) = compute_matching_impl_with_diagnostics(&input).unwrap();
        assert_eq!(round.games.len(), 1);
        assert_eq!(round.byes.len(), 1);
        assert_eq!(diagnostics.matching_calls, 1);
        assert_eq!(diagnostics.total_vertices, 4);
        assert_eq!(diagnostics.submitted_compatible_edges, 6);
    }
}
