use super::blossom::EdmondsWorkspace;
use super::graph::{EdgeUpdate, MatchingGraph};
use super::weight::BigWeight;
use std::cmp::Ordering;

#[derive(Clone)]
pub(crate) struct MatchingSolver {
    graph: MatchingGraph,
    workspace: EdmondsWorkspace,
    matching: Vec<usize>,
    last_solved_graph_state: Option<SolvedGraphState>,
    matching_is_current_optimal: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MatchingError {
    InvalidEdge,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct SolvedGraphState {
    graph_size: usize,
    dirty_edge_count: u64,
}

#[derive(Clone, Copy)]
pub(crate) struct MatchingSnapshot<'a> {
    matching: &'a [usize],
}

impl<'a> MatchingSnapshot<'a> {
    pub(crate) fn as_slice(&self) -> &'a [usize] {
        self.matching
    }
}

impl MatchingSolver {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            graph: MatchingGraph::new(capacity),
            workspace: EdmondsWorkspace::new(capacity),
            matching: Vec::with_capacity(capacity),
            last_solved_graph_state: None,
            matching_is_current_optimal: false,
        }
    }

    pub(crate) fn with_vertices(vertex_count: usize) -> Self {
        let mut solver = Self::new(vertex_count);
        for _ in 0..vertex_count {
            assert!(solver.add_vertex(), "matching capacity equals vertex count");
        }
        solver
    }

    pub(crate) fn size(&self) -> usize {
        self.graph.size()
    }

    pub(crate) fn add_vertex(&mut self) -> bool {
        let added = self.graph.add_vertex();
        if added {
            self.matching_is_current_optimal = false;
        }
        added
    }

    #[cfg(test)]
    pub(super) fn set_edge_weight(
        &mut self,
        vertex0: usize,
        vertex1: usize,
        edge_weight: BigWeight,
    ) -> bool {
        self.try_set_edge_weight(vertex0, vertex1, edge_weight)
            .is_ok()
    }

    fn try_set_edge_weight(
        &mut self,
        vertex0: usize,
        vertex1: usize,
        edge_weight: BigWeight,
    ) -> Result<(), MatchingError> {
        let was_current_optimal = self.matching_is_current_optimal;
        match self
            .graph
            .set_edge_weight(vertex0, vertex1, edge_weight, was_current_optimal)
        {
            EdgeUpdate::Invalid => Err(MatchingError::InvalidEdge),
            EdgeUpdate::Unchanged => Ok(()),
            EdgeUpdate::Changed {
                old_present,
                new_present,
                new_weight_ordering,
                generation_reset,
            } => {
                if generation_reset {
                    self.last_solved_graph_state = None;
                }
                let preserves_current_matching = was_current_optimal
                    && !generation_reset
                    && self.edge_change_preserves_current_matching(
                        vertex0,
                        vertex1,
                        old_present,
                        new_present,
                        new_weight_ordering,
                    );
                if was_current_optimal && !preserves_current_matching {
                    self.matching_is_current_optimal = false;
                }
                Ok(())
            }
        }
    }

    pub(crate) fn set_fixed_edge_weight(
        &mut self,
        vertex0: usize,
        vertex1: usize,
        edge_weight: usize,
    ) -> Result<(), MatchingError> {
        self.try_set_edge_weight(vertex0, vertex1, BigWeight::from_usize(edge_weight))
    }

    pub(crate) fn set_big_edge_weight(
        &mut self,
        vertex0: usize,
        vertex1: usize,
        edge_weight: BigWeight,
    ) -> Result<(), MatchingError> {
        self.try_set_edge_weight(vertex0, vertex1, edge_weight)
    }

    pub(crate) fn compute_matching(&mut self) {
        let current_graph_state = self.current_graph_state();

        let unchanged_since_last_solve = self.last_solved_graph_state == Some(current_graph_state);
        if self.matching.len() == current_graph_state.graph_size
            && (unchanged_since_last_solve || self.matching_is_current_optimal)
        {
            self.last_solved_graph_state = Some(current_graph_state);
            return;
        }

        self.workspace.solve_into(&self.graph, &mut self.matching);
        self.last_solved_graph_state = Some(current_graph_state);
        self.matching_is_current_optimal = true;
    }

    fn current_graph_state(&self) -> SolvedGraphState {
        SolvedGraphState {
            graph_size: self.graph.size(),
            dirty_edge_count: self.graph.dirty_edge_count(),
        }
    }

    #[cfg(test)]
    pub(super) fn solved_graph_state_is_current_for_test(&self) -> bool {
        self.last_solved_graph_state == Some(self.current_graph_state())
    }

    #[cfg(test)]
    pub(super) fn set_dirty_edge_count_for_test(&mut self, value: u64) {
        self.graph.set_dirty_edge_count_for_test(value);
    }

    fn edge_change_preserves_current_matching(
        &self,
        vertex0: usize,
        vertex1: usize,
        old_present: bool,
        new_present: bool,
        new_weight_ordering: Option<Ordering>,
    ) -> bool {
        if self.matching.len() != self.graph.size() {
            return false;
        }

        let edge_is_matched =
            self.matching[vertex0] == vertex1 && self.matching[vertex1] == vertex0;
        match (old_present, new_present) {
            (false, true) => false,
            (true, false) => !edge_is_matched,
            (true, true) => {
                let required = if edge_is_matched {
                    Ordering::Greater
                } else {
                    Ordering::Less
                };
                Self::required_weight_ordering(new_weight_ordering) == required
            }
            (false, false) => true,
        }
    }

    fn required_weight_ordering(ordering: Option<Ordering>) -> Ordering {
        match ordering {
            Some(ordering) => ordering,
            None => unreachable!("matching solver invariant violated: missing weight ordering"),
        }
    }

    pub(crate) fn current_matching(&self) -> Option<&[usize]> {
        if self.matching.len() == self.graph.size()
            && (self.last_solved_graph_state == Some(self.current_graph_state())
                || self.matching_is_current_optimal)
        {
            Some(&self.matching)
        } else {
            None
        }
    }

    pub(crate) fn last_matching_snapshot(&self) -> Option<MatchingSnapshot<'_>> {
        (self.matching.len() == self.graph.size()).then_some(MatchingSnapshot {
            matching: &self.matching,
        })
    }

    pub(crate) fn preserve_current_matching_for_isolated_pair(
        &mut self,
        vertex0: usize,
        vertex1: usize,
    ) {
        if self.matching.len() != self.graph.size()
            || self.matching.get(vertex0).copied() != Some(vertex1)
            || self.matching.get(vertex1).copied() != Some(vertex0)
        {
            return;
        }
        if matches!(self.graph.neighbors(vertex0), [neighbor] if *neighbor == vertex1)
            && matches!(self.graph.neighbors(vertex1), [neighbor] if *neighbor == vertex0)
        {
            self.matching_is_current_optimal = true;
        }
    }
}
