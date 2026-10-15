use super::super::weight::BigWeight;
use super::model::{
    BlossomBestEdgeScratch, BlossomLabel, DeltaKind, EdmondsSolver, EdmondsWorkspace, Node,
};
use crate::matching::graph::MatchingGraph;

struct DeltaStep {
    kind: DeltaKind,
    value: BigWeight,
    edge: Option<(usize, usize)>,
    blossom: Option<Node>,
}

enum QueueAllowedEdgeResult {
    Continue,
    BlossomAdded,
    Augmented,
}

enum QueueEdgeEvaluation {
    SkipCandidate,
    Ready { allowed: bool, slack_computed: bool },
}

#[derive(Clone, Copy)]
struct QueueNeighborState {
    blossom: Node,
    blossom_label: Option<BlossomLabel>,
    vertex_label: Option<BlossomLabel>,
    vertex_label_loaded: bool,
}

struct QueueVertexScanState {
    vertex_blossom: Node,
    vertex_blossom_index: usize,
    edge_slack: BigWeight,
    current_slack: BigWeight,
}

impl QueueVertexScanState {
    fn new(vertex_blossom: Node, vertex_blossom_index: usize) -> Self {
        Self {
            vertex_blossom,
            vertex_blossom_index,
            edge_slack: BigWeight::zero(),
            current_slack: BigWeight::zero(),
        }
    }
}

impl EdmondsWorkspace {
    // Blossom maps, labels, and mate links are solver-owned invariants updated
    // in lockstep. Caller input errors are handled before this point by
    // zero/non-edge filtering and public solver methods returning
    // `false`/`Option`.
    pub(in crate::matching) fn new(capacity: usize) -> Self {
        let node_capacity = capacity
            .checked_mul(2)
            .expect("matching node dimensions fit addressable memory");
        let edge_slots = capacity
            .checked_mul(capacity)
            .expect("matching edge dimensions fit addressable memory");

        Self {
            n: 0,
            mate: Vec::with_capacity(capacity),
            label: Vec::with_capacity(node_capacity),
            labeledge: Vec::with_capacity(node_capacity),
            inblossom: Vec::with_capacity(capacity),
            blossomparent: Vec::with_capacity(node_capacity),
            blossombase: Vec::with_capacity(node_capacity),
            bestedge: Vec::with_capacity(node_capacity),
            bestedge_slack: Vec::with_capacity(node_capacity),
            bestedge_slack_epoch: Vec::with_capacity(node_capacity),
            current_slack_epoch: 1,
            dualvar: Vec::with_capacity(capacity),
            blossomdual: Vec::with_capacity(capacity),
            allowedge: Vec::with_capacity(edge_slots),
            allowedge_epoch: 1,
            queue: Vec::with_capacity(capacity),
            blossoms: Vec::with_capacity(capacity),
            blossom_best_edge_scratch: BlossomBestEdgeScratch::with_capacity(
                node_capacity,
                capacity,
            ),
            scan_path_scratch: Vec::with_capacity(node_capacity),
            expand_leaf_scratch: Vec::with_capacity(capacity),
            active_vertices: Vec::with_capacity(capacity),
        }
    }

    pub(in crate::matching) fn solve_into(
        &mut self,
        graph: &MatchingGraph,
        matching: &mut Vec<usize>,
    ) {
        let mut solver = EdmondsSolver {
            graph,
            workspace: self,
        };
        solver.solve_into(matching);
    }
}

impl EdmondsSolver<'_> {
    fn reset_active_vertices(&mut self) {
        let n = self.n;
        self.mate.resize(n, None);
        self.mate.fill(None);
        self.active_vertices.clear();
        for vertex in 0..n {
            if self.mate[vertex].is_some() {
                continue;
            }
            let isolated_neighbor = match self.graph.neighbors(vertex) {
                [neighbor] if self.mate[*neighbor].is_none() => {
                    match self.graph.neighbors(*neighbor) {
                        [reverse_neighbor] if *reverse_neighbor == vertex => Some(*neighbor),
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some(neighbor) = isolated_neighbor {
                self.mate[vertex] = Some(neighbor);
                self.mate[neighbor] = Some(vertex);
            }
        }
        for vertex in 0..n {
            if self.mate[vertex].is_none() {
                self.active_vertices.push(vertex);
            }
        }
    }

    fn initial_dual_value(&self) -> BigWeight {
        let mut max_weight = None;
        for &vertex0 in &self.active_vertices {
            for &vertex1 in self.graph.neighbors(vertex0) {
                let weight =
                    Self::invariant(self.graph.edge_weight(vertex0, vertex1), "edge weight");
                if max_weight.is_none_or(|current| current < weight) {
                    max_weight = Some(weight);
                }
            }
        }
        let mut max_weight = max_weight.cloned().unwrap_or_else(BigWeight::zero);
        max_weight.shr_assign(1);

        max_weight
    }

    fn reset_solver_storage(&mut self, initial_dual: &BigWeight) {
        let n = self.n;
        self.label.resize(n, None);
        self.label.fill(None);
        self.labeledge.resize(n, None);
        self.labeledge.fill(None);
        self.inblossom.clear();
        self.inblossom.extend((0..n).map(Node::Vertex));
        self.blossomparent.resize(n, None);
        self.blossomparent.fill(None);
        self.blossombase.clear();
        self.blossombase.extend((0..n).map(Some));
        self.bestedge.resize(n, None);
        self.bestedge.fill(None);
        self.bestedge_slack.resize_with(n, || None);
        self.bestedge_slack.fill(None);
        self.bestedge_slack_epoch.resize(n, 0);
        self.bestedge_slack_epoch.fill(0);
        self.current_slack_epoch = 1;
        self.dualvar.resize_with(n, BigWeight::zero);
        for dual in &mut self.dualvar {
            dual.clone_from(initial_dual);
        }
        self.blossomdual.clear();
        let edge_slots = n * n;
        if self.allowedge.len() != edge_slots {
            self.allowedge.resize(edge_slots, 0);
            self.allowedge.fill(0);
            self.allowedge_epoch = 1;
        }
        self.queue.clear();
        self.blossoms.clear();
        self.scan_path_scratch.clear();
        self.expand_leaf_scratch.clear();
    }

    fn reset(&mut self) {
        self.n = self.graph.size();
        self.reset_active_vertices();
        let initial_dual = self.initial_dual_value();
        self.reset_solver_storage(&initial_dual);
    }

    fn solve_into(&mut self, matching: &mut Vec<usize>) {
        self.reset();
        matching.clear();
        if self.n == 0 {
            return;
        }
        self.solve(matching)
    }

    fn scan_delta_unlabeled_vertices(&self, step: &mut DeltaStep, slack: &mut BigWeight) {
        for active_index in 0..self.active_vertices.len() {
            let vertex = self.active_vertices[active_index];
            if self.label_of(self.inblossom[vertex]).is_none() {
                self.consider_delta_unlabeled_vertex_edge(vertex, step, slack);
            }
        }
    }

    fn consider_delta_unlabeled_vertex_edge(
        &self,
        vertex: usize,
        step: &mut DeltaStep,
        slack: &mut BigWeight,
    ) {
        if let Some(edge) = self.bestedge_at(vertex) {
            if let Some(cached_slack) = self.bestedge_slack_at(vertex) {
                if cached_slack < &step.value {
                    step.value.clone_from(cached_slack);
                    step.kind = DeltaKind::UnlabeledVertex;
                    step.edge = Some(edge);
                }
            } else {
                self.slack_into(edge.0, edge.1, &mut *slack);
                if *slack < step.value {
                    std::mem::swap(&mut step.value, &mut *slack);
                    step.kind = DeltaKind::UnlabeledVertex;
                    step.edge = Some(edge);
                }
            }
        }
    }

    fn consider_delta_top_edge(
        &self,
        node_index: usize,
        step: &mut DeltaStep,
        slack: &mut BigWeight,
    ) {
        if let Some(edge) = self.bestedge_at(node_index) {
            if let Some(cached_slack) = self.bestedge_slack_at(node_index) {
                slack.clone_from(cached_slack);
            } else {
                self.slack_into(edge.0, edge.1, &mut *slack);
            }
            debug_assert!(slack.is_even());
            slack.shr_assign(1);
            if *slack < step.value {
                std::mem::swap(&mut step.value, &mut *slack);
                step.kind = DeltaKind::LabeledEdge;
                step.edge = Some(edge);
            }
        }
    }

    fn scan_delta_top_vertices(&self, step: &mut DeltaStep, slack: &mut BigWeight) {
        for active_index in 0..self.active_vertices.len() {
            let vertex = self.active_vertices[active_index];
            if self.blossomparent[vertex].is_some() {
                continue;
            }
            if self.label[vertex] == Some(BlossomLabel::Outer) {
                self.consider_delta_top_edge(vertex, step, slack);
            }
        }
    }

    fn scan_delta_top_blossoms(&self, step: &mut DeltaStep, slack: &mut BigWeight) {
        for blossom_id in 0..self.blossoms.len() {
            if self.blossoms[blossom_id].is_none() {
                continue;
            }
            let node_index = self.n + blossom_id;
            if self
                .blossomparent
                .get(node_index)
                .copied()
                .flatten()
                .is_some()
            {
                continue;
            }
            if self.label.get(node_index).copied().flatten() == Some(BlossomLabel::Outer) {
                self.consider_delta_top_edge(node_index, step, slack);
            }
        }
    }

    fn scan_delta_blossom_duals(&self, step: &mut DeltaStep) {
        for blossom_id in 0..self.blossoms.len() {
            if self.blossoms[blossom_id].is_none() {
                continue;
            }
            let node_index = self.n + blossom_id;
            if self
                .blossomparent
                .get(node_index)
                .copied()
                .flatten()
                .is_none()
                && self.label.get(node_index).copied().flatten() == Some(BlossomLabel::Inner)
            {
                if let Some(blossom_delta) = self.blossomdual_of(blossom_id) {
                    if blossom_delta < &step.value {
                        step.value.clone_from(blossom_delta);
                        step.kind = DeltaKind::BlossomExpansion;
                        step.blossom = Some(Node::Blossom(blossom_id));
                    }
                } else if !step.value.is_zero() {
                    step.value = BigWeight::zero();
                    step.kind = DeltaKind::BlossomExpansion;
                    step.blossom = Some(Node::Blossom(blossom_id));
                }
            }
        }
    }

    fn find_delta(&self) -> DeltaStep {
        let mut step = DeltaStep {
            kind: DeltaKind::NoImprovement,
            value: self
                .active_vertices
                .iter()
                .map(|&vertex| &self.dualvar[vertex])
                .min()
                .cloned()
                .unwrap_or_else(BigWeight::zero),
            edge: None,
            blossom: None,
        };
        let mut slack = BigWeight::zero();

        self.scan_delta_unlabeled_vertices(&mut step, &mut slack);
        self.scan_delta_top_vertices(&mut step, &mut slack);
        self.scan_delta_top_blossoms(&mut step, &mut slack);
        self.scan_delta_blossom_duals(&mut step);

        step
    }

    fn apply_delta_to_vertex_duals(&mut self, delta: &BigWeight) {
        for active_index in 0..self.active_vertices.len() {
            let vertex = self.active_vertices[active_index];
            match self.label_of(self.inblossom[vertex]) {
                Some(BlossomLabel::Outer) => self.dualvar[vertex].sub_assign(delta),
                Some(BlossomLabel::Inner) => self.dualvar[vertex].add_assign(delta),
                _ => {}
            }
        }
    }

    fn apply_delta_to_blossom_duals(&mut self, delta: &BigWeight) {
        for blossom_id in 0..self.blossoms.len() {
            if self.blossoms[blossom_id].is_none() {
                continue;
            }
            let node = Node::Blossom(blossom_id);
            if self.parent_of(node).is_none() {
                match self.label_of(node) {
                    Some(BlossomLabel::Outer) => {
                        self.blossomdual_mut(blossom_id).add_assign(delta);
                    }
                    Some(BlossomLabel::Inner) => {
                        self.blossomdual_mut(blossom_id).sub_assign(delta);
                    }
                    _ => {}
                }
            }
        }
    }

    fn apply_delta(&mut self, delta: &BigWeight) {
        self.apply_delta_to_vertex_duals(delta);
        self.apply_delta_to_blossom_duals(delta);
        if !delta.is_zero() {
            self.invalidate_bestedge_slacks();
        }
    }

    fn update_queue_bestedge(
        &mut self,
        target_index: usize,
        candidate: (usize, usize),
        edge_slack: &mut BigWeight,
        current_slack: &mut BigWeight,
        edge_slack_computed: bool,
    ) {
        let current = self.bestedge_at(target_index);
        let mut refreshed_current_slack = None;
        let update = if let Some(edge) = current {
            debug_assert!(edge_slack_computed);
            if let Some(best_slack) = self.bestedge_slack_at(target_index) {
                &*edge_slack < best_slack
            } else {
                self.slack_into(edge.0, edge.1, current_slack);
                refreshed_current_slack = Some(edge);
                *edge_slack < *current_slack
            }
        } else {
            true
        };
        if update {
            self.set_bestedge_with_slack_at(target_index, candidate, edge_slack);
        } else if let Some(edge) = refreshed_current_slack {
            self.set_bestedge_with_slack_at(target_index, edge, current_slack);
        }
    }

    fn process_allowed_queue_edge(
        &mut self,
        vertex: usize,
        neighbor: usize,
        neighbor_state: QueueNeighborState,
    ) -> QueueAllowedEdgeResult {
        if neighbor_state.blossom_label.is_none() {
            self.assign_label(neighbor, BlossomLabel::Inner, Some(vertex));
            return QueueAllowedEdgeResult::Continue;
        }

        if neighbor_state.blossom_label == Some(BlossomLabel::Outer) {
            if let Some(base) = self.scan_blossom(vertex, neighbor) {
                self.add_blossom(base, vertex, neighbor);
                QueueAllowedEdgeResult::BlossomAdded
            } else {
                self.augment_matching(vertex, neighbor);
                QueueAllowedEdgeResult::Augmented
            }
        } else {
            let neighbor_vertex_unlabeled = if neighbor_state.vertex_label_loaded {
                neighbor_state.vertex_label.is_none()
            } else {
                self.label_of(Node::Vertex(neighbor)).is_none()
            };
            if neighbor_vertex_unlabeled {
                debug_assert_eq!(
                    self.label_of(neighbor_state.blossom),
                    Some(BlossomLabel::Inner)
                );
                self.set_label(Node::Vertex(neighbor), Some(BlossomLabel::Inner));
                self.set_labeledge(Node::Vertex(neighbor), Some((vertex, neighbor)));
            }
            QueueAllowedEdgeResult::Continue
        }
    }

    fn process_blocked_queue_edge(
        &mut self,
        edge: (usize, usize),
        vertex_blossom_index: usize,
        neighbor_state: QueueNeighborState,
        edge_slack: &mut BigWeight,
        current_slack: &mut BigWeight,
        edge_slack_computed: bool,
    ) {
        let (_, neighbor) = edge;
        if neighbor_state.blossom_label == Some(BlossomLabel::Outer) {
            self.update_queue_bestedge(
                vertex_blossom_index,
                edge,
                edge_slack,
                current_slack,
                edge_slack_computed,
            );
            return;
        }

        let neighbor_vertex_unlabeled = neighbor_state.blossom_label.is_none()
            || if neighbor_state.vertex_label_loaded {
                neighbor_state.vertex_label.is_none()
            } else {
                self.label_of(Node::Vertex(neighbor)).is_none()
            };
        if neighbor_vertex_unlabeled {
            debug_assert!(
                neighbor_state.blossom_label.is_some()
                    || self.label_of(Node::Vertex(neighbor)).is_none()
            );
            self.update_queue_bestedge(
                neighbor,
                edge,
                edge_slack,
                current_slack,
                edge_slack_computed,
            );
        }
    }

    fn queue_edge_uses_zero_only_slack(
        &self,
        neighbor: usize,
        neighbor_state: &mut QueueNeighborState,
    ) -> bool {
        if neighbor_state.blossom_label.is_some()
            && neighbor_state.blossom_label != Some(BlossomLabel::Outer)
        {
            neighbor_state.vertex_label = self.label_of(Node::Vertex(neighbor));
            neighbor_state.vertex_label_loaded = true;
            neighbor_state.vertex_label.is_some()
        } else {
            false
        }
    }

    fn queue_edge_bestedge_target(
        &self,
        neighbor: usize,
        vertex_blossom_index: usize,
        neighbor_state: &mut QueueNeighborState,
    ) -> Option<usize> {
        if neighbor_state.blossom_label == Some(BlossomLabel::Outer) {
            Some(vertex_blossom_index)
        } else if neighbor_state.blossom_label.is_none() {
            Some(neighbor)
        } else {
            neighbor_state.vertex_label = self.label_of(Node::Vertex(neighbor));
            neighbor_state.vertex_label_loaded = true;
            neighbor_state.vertex_label.is_none().then_some(neighbor)
        }
    }

    fn cached_queue_bestedge_slack(&self, target: Option<usize>) -> Option<&BigWeight> {
        target
            .filter(|&target| self.bestedge_at(target).is_some())
            .and_then(|target| self.bestedge_slack_at(target))
            .filter(|best_slack| !best_slack.is_zero())
    }

    fn queue_candidate_can_improve_cached_best(
        &self,
        vertex: usize,
        neighbor: usize,
        edge_weight: &BigWeight,
        best_slack: &BigWeight,
    ) -> bool {
        BigWeight::slack_less_than(
            &self.dualvar[vertex],
            &self.dualvar[neighbor],
            edge_weight,
            best_slack,
        )
    }

    fn allow_queue_edge_if_zero(
        &mut self,
        vertex: usize,
        neighbor: usize,
        edge_weight: &BigWeight,
    ) -> bool {
        if self.slack_weight_is_zero(vertex, neighbor, edge_weight) {
            self.allow_edge(vertex, neighbor);
            true
        } else {
            false
        }
    }

    fn evaluate_zero_only_queue_edge(
        &mut self,
        vertex: usize,
        neighbor: usize,
        edge_weight: &BigWeight,
    ) -> QueueEdgeEvaluation {
        QueueEdgeEvaluation::Ready {
            allowed: self.allow_queue_edge_if_zero(vertex, neighbor, edge_weight),
            slack_computed: false,
        }
    }

    fn materialize_queue_edge_slack(
        &mut self,
        vertex: usize,
        neighbor: usize,
        edge_weight: &BigWeight,
        edge_slack: &mut BigWeight,
    ) -> bool {
        self.slack_weight_into(vertex, neighbor, edge_weight, edge_slack);
        if edge_slack.is_zero() {
            self.allow_edge(vertex, neighbor);
            true
        } else {
            false
        }
    }

    fn evaluate_queue_edge(
        &mut self,
        edge: (usize, usize),
        edge_weight: &BigWeight,
        vertex_blossom_index: usize,
        neighbor_state: &mut QueueNeighborState,
        edge_slack: &mut BigWeight,
    ) -> QueueEdgeEvaluation {
        let (vertex, neighbor) = edge;
        let mut edge_allowed = self.allowedge[vertex * self.n + neighbor] == self.allowedge_epoch;
        if edge_allowed {
            return QueueEdgeEvaluation::Ready {
                allowed: true,
                slack_computed: false,
            };
        }

        let zero_only_slack = self.queue_edge_uses_zero_only_slack(neighbor, neighbor_state);
        if zero_only_slack {
            return self.evaluate_zero_only_queue_edge(vertex, neighbor, edge_weight);
        }

        let bestedge_target =
            self.queue_edge_bestedge_target(neighbor, vertex_blossom_index, neighbor_state);
        if let Some(best_slack) = self.cached_queue_bestedge_slack(bestedge_target)
            && !self.queue_candidate_can_improve_cached_best(
                vertex,
                neighbor,
                edge_weight,
                best_slack,
            )
        {
            return QueueEdgeEvaluation::SkipCandidate;
        }

        if self.allow_queue_edge_if_zero(vertex, neighbor, edge_weight) {
            return QueueEdgeEvaluation::Ready {
                allowed: true,
                slack_computed: false,
            };
        }

        edge_allowed = self.materialize_queue_edge_slack(vertex, neighbor, edge_weight, edge_slack);
        QueueEdgeEvaluation::Ready {
            allowed: edge_allowed,
            slack_computed: true,
        }
    }

    fn queue_edge_action_from_evaluation(evaluation: QueueEdgeEvaluation) -> Option<(bool, bool)> {
        match evaluation {
            QueueEdgeEvaluation::SkipCandidate => None,
            QueueEdgeEvaluation::Ready {
                allowed,
                slack_computed,
            } => Some((allowed, slack_computed)),
        }
    }

    fn process_allowed_queue_neighbor(
        &mut self,
        vertex: usize,
        neighbor: usize,
        neighbor_state: QueueNeighborState,
        scan: &mut QueueVertexScanState,
    ) -> bool {
        match self.process_allowed_queue_edge(vertex, neighbor, neighbor_state) {
            QueueAllowedEdgeResult::Continue => false,
            QueueAllowedEdgeResult::BlossomAdded => {
                scan.vertex_blossom = self.inblossom[vertex];
                scan.vertex_blossom_index = self.node_index(scan.vertex_blossom);
                false
            }
            QueueAllowedEdgeResult::Augmented => true,
        }
    }

    fn process_ready_queue_edge(
        &mut self,
        vertex: usize,
        neighbor: usize,
        neighbor_state: QueueNeighborState,
        scan: &mut QueueVertexScanState,
        edge_allowed: bool,
        edge_slack_computed: bool,
    ) -> bool {
        if edge_allowed {
            self.process_allowed_queue_neighbor(vertex, neighbor, neighbor_state, scan)
        } else {
            self.process_blocked_queue_edge(
                (vertex, neighbor),
                scan.vertex_blossom_index,
                neighbor_state,
                &mut scan.edge_slack,
                &mut scan.current_slack,
                edge_slack_computed,
            );
            false
        }
    }

    fn process_queue_neighbor(
        &mut self,
        vertex: usize,
        neighbor: usize,
        edge_weight: &BigWeight,
        scan: &mut QueueVertexScanState,
    ) -> bool {
        let neighbor_blossom = self.inblossom[neighbor];
        if scan.vertex_blossom == neighbor_blossom {
            return false;
        }

        let mut neighbor_state = QueueNeighborState {
            blossom: neighbor_blossom,
            blossom_label: self.label_of(neighbor_blossom),
            vertex_label: None,
            vertex_label_loaded: false,
        };

        let evaluation = self.evaluate_queue_edge(
            (vertex, neighbor),
            edge_weight,
            scan.vertex_blossom_index,
            &mut neighbor_state,
            &mut scan.edge_slack,
        );
        let Some((edge_allowed, edge_slack_computed)) =
            Self::queue_edge_action_from_evaluation(evaluation)
        else {
            return false;
        };

        self.process_ready_queue_edge(
            vertex,
            neighbor,
            neighbor_state,
            scan,
            edge_allowed,
            edge_slack_computed,
        )
    }

    fn initial_queue_vertex_scan(&self, vertex: usize) -> QueueVertexScanState {
        let vertex_blossom = self.inblossom[vertex];
        let vertex_blossom_index = self.node_index(vertex_blossom);
        QueueVertexScanState::new(vertex_blossom, vertex_blossom_index)
    }

    fn scan_queue_vertex_neighbors(
        &mut self,
        graph: &MatchingGraph,
        vertex: usize,
        edge_weight_row: &[BigWeight],
        scan: &mut QueueVertexScanState,
    ) -> bool {
        for &neighbor in graph.neighbors(vertex) {
            if self.process_queue_neighbor(vertex, neighbor, &edge_weight_row[neighbor], scan) {
                return true;
            }
        }

        false
    }

    fn process_queue_vertex(&mut self, vertex: usize) -> bool {
        let graph = self.graph;
        debug_assert_eq!(
            self.label_of(self.inblossom[vertex]),
            Some(BlossomLabel::Outer)
        );

        let edge_weight_row = graph.edge_weight_row(vertex);
        let mut scan = self.initial_queue_vertex_scan(vertex);
        self.scan_queue_vertex_neighbors(graph, vertex, edge_weight_row, &mut scan)
    }

    fn clear_stage_state(&mut self) {
        self.label.fill(None);
        self.labeledge.fill(None);
        self.bestedge.fill(None);
        for blossom in self.blossoms.iter_mut().flatten() {
            blossom.my_bestedges = None;
        }
        self.allowedge_clear();
        self.queue.clear();
    }

    fn label_unmatched_stage_roots(&mut self) {
        for active_index in 0..self.active_vertices.len() {
            let vertex = self.active_vertices[active_index];
            if self.mate[vertex].is_none() && self.label_of(self.inblossom[vertex]).is_none() {
                self.assign_label(vertex, BlossomLabel::Outer, None);
            }
        }
    }

    fn initialize_stage(&mut self) {
        self.clear_stage_state();
        self.label_unmatched_stage_roots();
    }

    fn drain_stage_queue(&mut self) -> bool {
        while let Some(vertex) = self.queue.pop() {
            if self.process_queue_vertex(vertex) {
                return true;
            }
        }

        false
    }

    fn activate_delta_edge(&mut self, edge: (usize, usize)) {
        self.allow_edge(edge.0, edge.1);
        self.queue.push(edge.0);
    }

    fn expand_delta_blossom(&mut self, blossom: Node) {
        self.expand_blossom(blossom, false);
    }

    fn apply_stage_delta_step(&mut self, delta: DeltaStep) -> bool {
        self.apply_delta(&delta.value);

        match delta.kind {
            DeltaKind::NoImprovement => false,
            DeltaKind::UnlabeledVertex | DeltaKind::LabeledEdge => {
                self.activate_delta_edge(Self::invariant(delta.edge, "delta edge"));
                true
            }
            DeltaKind::BlossomExpansion => {
                self.expand_delta_blossom(Self::invariant(delta.blossom, "delta blossom"));
                true
            }
        }
    }

    fn advance_stage_by_delta(&mut self) -> bool {
        let delta = self.find_delta();
        self.apply_stage_delta_step(delta)
    }

    fn run_stage(&mut self) -> bool {
        loop {
            if self.drain_stage_queue() {
                return true;
            }

            if !self.advance_stage_by_delta() {
                return false;
            }
        }
    }

    fn debug_assert_matching_symmetry(&self) {
        for vertex in 0..self.n {
            if let Some(mate) = self.mate[vertex] {
                debug_assert_eq!(self.mate[mate], Some(vertex));
            }
        }
    }

    fn expand_endstage_blossoms(&mut self) {
        let blossom_count = self.blossoms.len();
        for blossom_id in 0..blossom_count {
            if !matches!(self.blossoms.get(blossom_id), Some(Some(_))) {
                continue;
            }
            let node = Node::Blossom(blossom_id);
            if self.parent_of(node).is_none()
                && self.label_of(node) == Some(BlossomLabel::Outer)
                && self
                    .blossomdual_of(blossom_id)
                    .is_none_or(BigWeight::is_zero)
            {
                self.expand_blossom(node, true);
            }
        }
    }

    fn run_augmenting_stages(&mut self) {
        loop {
            self.initialize_stage();
            let augmented = self.run_stage();
            self.debug_assert_matching_symmetry();

            if !augmented {
                break;
            }

            self.expand_endstage_blossoms();
        }
    }

    fn write_matching_result(&self, matching: &mut Vec<usize>) {
        matching.extend((0..self.n).map(|vertex| self.mate[vertex].unwrap_or(vertex)));
    }

    fn solve(&mut self, matching: &mut Vec<usize>) {
        self.run_augmenting_stages();
        self.write_matching_result(matching);
    }
}
