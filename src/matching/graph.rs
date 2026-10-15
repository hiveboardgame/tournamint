use super::weight::BigWeight;
use std::cmp::Ordering;

#[derive(Clone)]
pub(in crate::matching) struct MatchingGraph {
    capacity: usize,
    size: usize,
    edge_weights: Vec<BigWeight>,
    neighbors: Vec<Vec<usize>>,
    dirty_edge_count: u64,
}

pub(in crate::matching) enum EdgeUpdate {
    Invalid,
    Unchanged,
    Changed {
        old_present: bool,
        new_present: bool,
        new_weight_ordering: Option<Ordering>,
        generation_reset: bool,
    },
}

impl MatchingGraph {
    pub(in crate::matching) fn new(capacity: usize) -> Self {
        let edge_slots = capacity
            .checked_mul(capacity)
            .expect("matching graph dimensions fit addressable memory");
        let mut edge_weights = Vec::with_capacity(edge_slots);
        edge_weights.resize_with(edge_slots, BigWeight::zero);

        let mut neighbors = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            neighbors.push(Vec::new());
        }

        Self {
            capacity,
            size: 0,
            edge_weights,
            neighbors,
            dirty_edge_count: 0,
        }
    }

    pub(in crate::matching) fn size(&self) -> usize {
        self.size
    }

    pub(in crate::matching) fn dirty_edge_count(&self) -> u64 {
        self.dirty_edge_count
    }

    pub(in crate::matching) fn add_vertex(&mut self) -> bool {
        if self.size == self.capacity {
            return false;
        }
        self.size += 1;
        true
    }

    pub(in crate::matching) fn set_edge_weight(
        &mut self,
        vertex0: usize,
        vertex1: usize,
        mut edge_weight: BigWeight,
        capture_weight_ordering: bool,
    ) -> EdgeUpdate {
        if vertex0 == vertex1 || vertex0 >= self.size || vertex1 >= self.size {
            return EdgeUpdate::Invalid;
        }

        let present = !edge_weight.is_zero();
        if present {
            edge_weight.shl_assign(1);
        }

        let index0 = self.edge_index(vertex0, vertex1);
        let index1 = self.edge_index(vertex1, vertex0);
        let was_present = !self.edge_weights[index0].is_zero();
        let changed =
            present != was_present || (present && self.edge_weights[index0] != edge_weight);

        let generation_reset = if changed {
            if let Some(next) = self.dirty_edge_count.checked_add(1) {
                self.dirty_edge_count = next;
                false
            } else {
                self.dirty_edge_count = 0;
                true
            }
        } else {
            return EdgeUpdate::Unchanged;
        };

        let new_weight_ordering = (capture_weight_ordering && was_present && present)
            .then(|| edge_weight.cmp(&self.edge_weights[index0]));

        match (was_present, present) {
            (false, true) => {
                self.add_neighbor(vertex0, vertex1);
                self.add_neighbor(vertex1, vertex0);
            }
            (true, false) => {
                self.remove_neighbor(vertex0, vertex1);
                self.remove_neighbor(vertex1, vertex0);
            }
            _ => {}
        }

        if present {
            self.edge_weights[index0] = edge_weight.clone();
            self.edge_weights[index1] = edge_weight;
        } else {
            self.edge_weights[index0] = BigWeight::zero();
            self.edge_weights[index1] = BigWeight::zero();
        }
        EdgeUpdate::Changed {
            old_present: was_present,
            new_present: present,
            new_weight_ordering,
            generation_reset,
        }
    }

    pub(in crate::matching) fn edge_weight(
        &self,
        vertex0: usize,
        vertex1: usize,
    ) -> Option<&BigWeight> {
        let index = self.edge_index(vertex0, vertex1);
        (!self.edge_weights[index].is_zero()).then_some(&self.edge_weights[index])
    }

    pub(in crate::matching) fn edge_weight_row(&self, vertex: usize) -> &[BigWeight] {
        let start = vertex * self.capacity;
        &self.edge_weights[start..start + self.capacity]
    }

    pub(in crate::matching) fn neighbors(&self, vertex: usize) -> &[usize] {
        &self.neighbors[vertex]
    }

    fn edge_index(&self, vertex0: usize, vertex1: usize) -> usize {
        vertex0 * self.capacity + vertex1
    }

    fn add_neighbor(&mut self, vertex: usize, neighbor: usize) {
        let neighbors = &mut self.neighbors[vertex];
        if let Err(index) = neighbors.binary_search(&neighbor) {
            neighbors.insert(index, neighbor);
        }
    }

    #[cfg(test)]
    pub(in crate::matching) fn set_dirty_edge_count_for_test(&mut self, value: u64) {
        self.dirty_edge_count = value;
    }

    fn remove_neighbor(&mut self, vertex: usize, neighbor: usize) {
        let neighbors = &mut self.neighbors[vertex];
        if let Ok(index) = neighbors.binary_search(&neighbor) {
            neighbors.remove(index);
        }
    }
}
