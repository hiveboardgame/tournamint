use super::super::weight::BigWeight;
use super::model::{
    Blossom, BlossomBestEdgeScratch, BlossomChildren, BlossomEdges, BlossomLabel, EdmondsSolver,
    Node,
};
use crate::matching::graph::MatchingGraph;

struct BlossomContractionPath {
    childs: BlossomChildren,
    edges: BlossomEdges,
}

impl EdmondsSolver<'_> {
    pub(super) fn add_blossom(&mut self, base: usize, vertex: usize, neighbor: usize) {
        let graph = self.graph;
        let base_blossom = self.inblossom[base];

        let blossom_id = self.blossoms.len();
        let blossom_node = Node::Blossom(blossom_id);
        self.blossoms.push(Some(Blossom {
            childs: BlossomChildren::new(),
            edges: BlossomEdges::new(),
            my_bestedges: None,
        }));
        self.set_base(blossom_node, base);
        self.set_parent(blossom_node, None);
        self.set_parent(base_blossom, Some(blossom_node));

        let contraction_path =
            self.build_blossom_contraction_path(blossom_node, base_blossom, vertex, neighbor);

        debug_assert_eq!(self.label_of(base_blossom), Some(BlossomLabel::Outer));
        let base_edge = self.labeledge_of(base_blossom);
        self.set_label(blossom_node, Some(BlossomLabel::Outer));
        self.set_labeledge(blossom_node, base_edge);
        self.set_blossomdual(blossom_id, Some(BigWeight::zero()));

        let mut scratch = std::mem::replace(
            &mut self.blossom_best_edge_scratch,
            BlossomBestEdgeScratch::empty(),
        );
        scratch.reset(self.node_storage_len());

        self.assign_contraction_path_leaves(blossom_node, &contraction_path.childs, &mut scratch);
        self.rebuild_contraction_best_edges(
            graph,
            blossom_node,
            &contraction_path.childs,
            &mut scratch,
        );
        self.install_contracted_blossom(blossom_id, blossom_node, contraction_path, &scratch);
        self.blossom_best_edge_scratch = scratch;
    }

    fn build_blossom_contraction_path(
        &mut self,
        blossom_node: Node,
        base_blossom: Node,
        mut vertex: usize,
        mut neighbor: usize,
    ) -> BlossomContractionPath {
        let mut vertex_blossom = self.inblossom[vertex];
        let mut neighbor_blossom = self.inblossom[neighbor];
        let mut childs = BlossomChildren::new();
        let mut edges = BlossomEdges::new();
        edges.push((vertex, neighbor));

        while vertex_blossom != base_blossom {
            self.set_parent(vertex_blossom, Some(blossom_node));
            childs.push(vertex_blossom);
            let edge = Self::invariant(self.labeledge_of(vertex_blossom), "vertex label edge");
            edges.push(edge);
            debug_assert!(
                self.label_of(vertex_blossom) == Some(BlossomLabel::Inner)
                    || (self.label_of(vertex_blossom) == Some(BlossomLabel::Outer)
                        && Some(edge.0) == self.mate[self.base_of(vertex_blossom)])
            );
            vertex = edge.0;
            vertex_blossom = self.inblossom[vertex];
        }
        childs.push(base_blossom);
        childs.reverse();
        edges.reverse();

        while neighbor_blossom != base_blossom {
            self.set_parent(neighbor_blossom, Some(blossom_node));
            childs.push(neighbor_blossom);
            let edge = Self::invariant(self.labeledge_of(neighbor_blossom), "neighbor label edge");
            edges.push((edge.1, edge.0));
            debug_assert!(
                self.label_of(neighbor_blossom) == Some(BlossomLabel::Inner)
                    || (self.label_of(neighbor_blossom) == Some(BlossomLabel::Outer)
                        && Some(edge.0) == self.mate[self.base_of(neighbor_blossom)])
            );
            neighbor = edge.0;
            neighbor_blossom = self.inblossom[neighbor];
        }

        BlossomContractionPath { childs, edges }
    }

    fn assign_contraction_path_leaves(
        &mut self,
        blossom_node: Node,
        childs: &BlossomChildren,
        scratch: &mut BlossomBestEdgeScratch,
    ) {
        for &child in childs {
            scratch.leaf_scratch.clear();
            self.extend_leaves(child, &mut scratch.leaf_scratch);
            for leaf_index in 0..scratch.leaf_scratch.len() {
                let leaf = scratch.leaf_scratch[leaf_index];
                if self.label_of(self.inblossom[leaf]) == Some(BlossomLabel::Inner) {
                    self.queue.push(leaf);
                }
                self.inblossom[leaf] = blossom_node;
            }
        }
    }

    fn rebuild_contraction_best_edges(
        &mut self,
        graph: &MatchingGraph,
        blossom_node: Node,
        childs: &BlossomChildren,
        scratch: &mut BlossomBestEdgeScratch,
    ) {
        for &child in childs {
            self.rebuild_contraction_child_best_edges(graph, blossom_node, child, scratch);
            self.set_bestedge(child, None);
        }
    }

    fn rebuild_contraction_child_best_edges(
        &mut self,
        graph: &MatchingGraph,
        blossom_node: Node,
        child: Node,
        scratch: &mut BlossomBestEdgeScratch,
    ) {
        match child {
            Node::Blossom(child_id) => {
                if let Some(edges) = self.blossoms[child_id]
                    .as_mut()
                    .and_then(|blossom| blossom.my_bestedges.take())
                {
                    for (endpoint0, endpoint1) in edges {
                        self.update_blossom_best_edge(
                            blossom_node,
                            scratch,
                            (endpoint0, endpoint1),
                        );
                    }
                } else {
                    scratch.leaf_scratch.clear();
                    self.extend_leaves(child, &mut scratch.leaf_scratch);
                    for leaf_index in 0..scratch.leaf_scratch.len() {
                        let leaf = scratch.leaf_scratch[leaf_index];
                        self.scan_contraction_leaf_best_edges(graph, blossom_node, leaf, scratch);
                    }
                }
            }
            Node::Vertex(child_vertex) => {
                self.scan_contraction_leaf_best_edges(graph, blossom_node, child_vertex, scratch);
            }
        }
    }

    fn scan_contraction_leaf_best_edges(
        &mut self,
        graph: &MatchingGraph,
        blossom_node: Node,
        leaf: usize,
        scratch: &mut BlossomBestEdgeScratch,
    ) {
        let edge_weight_row = graph.edge_weight_row(leaf);
        for &neighbor in graph.neighbors(leaf) {
            self.update_blossom_best_edge_with_weight(
                blossom_node,
                scratch,
                (leaf, neighbor),
                &edge_weight_row[neighbor],
            );
        }
    }

    fn install_contracted_blossom(
        &mut self,
        blossom_id: usize,
        blossom_node: Node,
        contraction_path: BlossomContractionPath,
        scratch: &BlossomBestEdgeScratch,
    ) {
        let bestedge_count = scratch.edge_to.iter().filter(|edge| edge.is_some()).count();
        let mut my_bestedges = Vec::with_capacity(bestedge_count);
        let mut my_best_edge = None;
        let mut my_best_slack_index = None;
        for (index, edge) in scratch.edge_to.iter().enumerate() {
            if let Some(edge) = edge {
                my_bestedges.push(*edge);
                if my_best_slack_index.is_none_or(|best_index| {
                    scratch.best_slack_to[index] < scratch.best_slack_to[best_index]
                }) {
                    my_best_edge = Some(*edge);
                    my_best_slack_index = Some(index);
                }
            }
        }
        self.set_bestedge(blossom_node, my_best_edge);
        if let Some(blossom) = &mut self.blossoms[blossom_id] {
            blossom.childs = contraction_path.childs;
            blossom.edges = contraction_path.edges;
            blossom.my_bestedges = Some(my_bestedges);
        }
    }

    fn update_blossom_best_edge(
        &mut self,
        blossom_node: Node,
        scratch: &mut BlossomBestEdgeScratch,
        edge: (usize, usize),
    ) {
        let graph = self.graph;
        let edge_weight = Self::invariant(graph.edge_weight(edge.0, edge.1), "edge weight");
        self.update_blossom_best_edge_with_weight(blossom_node, scratch, edge, edge_weight);
    }

    fn update_blossom_best_edge_with_weight(
        &mut self,
        blossom_node: Node,
        scratch: &mut BlossomBestEdgeScratch,
        edge: (usize, usize),
        edge_weight: &BigWeight,
    ) {
        let (mut endpoint0, mut endpoint1) = edge;
        if self.inblossom[endpoint1] == blossom_node {
            std::mem::swap(&mut endpoint0, &mut endpoint1);
        }
        let neighbor_blossom = self.inblossom[endpoint1];
        if neighbor_blossom == blossom_node
            || self.label_of(neighbor_blossom) != Some(BlossomLabel::Outer)
        {
            return;
        }

        let neighbor_blossom_index = self.node_index(neighbor_blossom);
        if scratch.edge_to.len() <= neighbor_blossom_index {
            scratch.edge_to.resize(neighbor_blossom_index + 1, None);
            scratch
                .zero_slack_to
                .resize(neighbor_blossom_index + 1, false);
            scratch
                .best_slack_to
                .resize_with(neighbor_blossom_index + 1, BigWeight::zero);
        }
        if scratch.zero_slack_to[neighbor_blossom_index] {
            return;
        }
        if scratch.edge_to[neighbor_blossom_index].is_some()
            && !scratch.best_slack_to[neighbor_blossom_index].is_zero()
        {
            let candidate_can_improve = BigWeight::slack_less_than(
                &self.dualvar[endpoint0],
                &self.dualvar[endpoint1],
                edge_weight,
                &scratch.best_slack_to[neighbor_blossom_index],
            );
            if !candidate_can_improve {
                return;
            }
        }
        if self.slack_weight_is_zero(endpoint0, endpoint1, edge_weight) {
            scratch.zero_slack_to[neighbor_blossom_index] = true;
            scratch.edge_to[neighbor_blossom_index] = Some((endpoint0, endpoint1));
            scratch.best_slack_to[neighbor_blossom_index].set_zero();
            return;
        }
        self.slack_weight_into(endpoint0, endpoint1, edge_weight, &mut scratch.edge_slack);
        if scratch.edge_slack.is_zero() {
            scratch.zero_slack_to[neighbor_blossom_index] = true;
        }
        let update = if scratch.edge_to[neighbor_blossom_index].is_some() {
            scratch.edge_slack < scratch.best_slack_to[neighbor_blossom_index]
        } else {
            true
        };
        if update {
            scratch.edge_to[neighbor_blossom_index] = Some((endpoint0, endpoint1));
            std::mem::swap(
                &mut scratch.best_slack_to[neighbor_blossom_index],
                &mut scratch.edge_slack,
            );
        }
    }

    pub(super) fn expand_blossom(&mut self, blossom_node: Node, endstage: bool) {
        let Node::Blossom(blossom_id) = blossom_node else {
            return;
        };
        let Some(blossom) = self.blossoms[blossom_id].take() else {
            return;
        };
        let childs = blossom.childs;
        let edges = blossom.edges;
        let mut leaf_scratch = std::mem::take(&mut self.expand_leaf_scratch);

        self.restore_expanded_child_ownership(&childs, endstage, &mut leaf_scratch);
        self.relabel_inner_expanded_blossom(
            blossom_node,
            endstage,
            &childs,
            &edges,
            &mut leaf_scratch,
        );

        self.clear_node_storage(blossom_node);
        self.clear_blossomdual(blossom_id);
        leaf_scratch.clear();
        self.expand_leaf_scratch = leaf_scratch;
    }

    fn restore_expanded_child_ownership(
        &mut self,
        childs: &BlossomChildren,
        endstage: bool,
        leaf_scratch: &mut Vec<usize>,
    ) {
        for child in childs {
            self.set_parent(*child, None);
            match *child {
                Node::Blossom(child_id)
                    if endstage && self.blossomdual_of(child_id).is_none_or(BigWeight::is_zero) =>
                {
                    self.expand_blossom(*child, endstage);
                }
                Node::Blossom(_) => {
                    leaf_scratch.clear();
                    self.extend_leaves(*child, leaf_scratch);
                    for &leaf in leaf_scratch.iter() {
                        self.inblossom[leaf] = *child;
                    }
                }
                Node::Vertex(vertex) => self.inblossom[vertex] = *child,
            }
        }
    }

    fn relabel_inner_expanded_blossom(
        &mut self,
        blossom_node: Node,
        endstage: bool,
        childs: &BlossomChildren,
        edges: &BlossomEdges,
        leaf_scratch: &mut Vec<usize>,
    ) {
        if endstage || self.label_of(blossom_node) != Some(BlossomLabel::Inner) {
            return;
        }

        let entry_edge = Self::invariant(self.labeledge_of(blossom_node), "entry edge");
        let entry_child = self.inblossom[entry_edge.1];
        let mut index = Self::invariant(
            childs.iter().position(|child| *child == entry_child),
            "entry child",
        )
        .cast_signed();
        let step = if index & 1 != 0 {
            index -= childs.len().cast_signed();
            1
        } else {
            -1
        };
        let (mut vertex, mut neighbor) =
            Self::invariant(self.labeledge_of(blossom_node), "entry edge");
        while index != 0 {
            let (previous, next) = if step == 1 {
                edges[Self::wrap_index(index, edges.len())]
            } else {
                let edge = edges[Self::wrap_index(index - 1, edges.len())];
                (edge.1, edge.0)
            };
            self.set_label(Node::Vertex(neighbor), None);
            self.set_label(Node::Vertex(next), None);
            self.assign_label(neighbor, BlossomLabel::Inner, Some(vertex));
            self.allow_edge(previous, next);
            index += step;
            if step == 1 {
                (vertex, neighbor) = edges[Self::wrap_index(index, edges.len())];
            } else {
                let edge = edges[Self::wrap_index(index - 1, edges.len())];
                (neighbor, vertex) = edge;
            }
            self.allow_edge(vertex, neighbor);
            index += step;
        }
        let base_child = childs[Self::wrap_index(index, childs.len())];
        self.set_label(Node::Vertex(neighbor), Some(BlossomLabel::Inner));
        self.set_label(base_child, Some(BlossomLabel::Inner));
        self.set_labeledge(Node::Vertex(neighbor), Some((vertex, neighbor)));
        self.set_labeledge(base_child, Some((vertex, neighbor)));
        self.set_bestedge(base_child, None);

        index += step;
        while childs[Self::wrap_index(index, childs.len())] != entry_child {
            let child = childs[Self::wrap_index(index, childs.len())];
            if self.label_of(child) == Some(BlossomLabel::Outer) {
                index += step;
                continue;
            }
            let mut reachable_vertex = None;
            match child {
                Node::Blossom(_) => {
                    leaf_scratch.clear();
                    self.extend_leaves(child, leaf_scratch);
                    for &leaf in leaf_scratch.iter() {
                        if self.label_of(Node::Vertex(leaf)).is_some() {
                            reachable_vertex = Some(leaf);
                            break;
                        }
                    }
                }
                Node::Vertex(vertex) => reachable_vertex = Some(vertex),
            }
            if let Some(reachable_vertex) = reachable_vertex
                && self.label_of(Node::Vertex(reachable_vertex)).is_some()
            {
                debug_assert_eq!(
                    self.label_of(Node::Vertex(reachable_vertex)),
                    Some(BlossomLabel::Inner)
                );
                debug_assert_eq!(self.inblossom[reachable_vertex], child);
                self.set_label(Node::Vertex(reachable_vertex), None);
                let mate = Self::invariant(self.mate[self.base_of(child)], "base mate");
                self.set_label(Node::Vertex(mate), None);
                let label_edge = Self::invariant(
                    self.labeledge_of(Node::Vertex(reachable_vertex)),
                    "reachable label edge",
                );
                self.assign_label(reachable_vertex, BlossomLabel::Inner, Some(label_edge.0));
            }
            index += step;
        }
    }
}
