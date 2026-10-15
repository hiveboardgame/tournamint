use super::super::weight::BigWeight;
use super::model::{Blossom, BlossomLabel, EdmondsSolver, Node};

impl EdmondsSolver<'_> {
    pub(super) fn invariant<T>(value: Option<T>, description: &'static str) -> T {
        match value {
            Some(value) => value,
            None => unreachable!("matching solver invariant violated: {description}"),
        }
    }

    pub(super) fn node_index(&self, node: Node) -> usize {
        node.index(self.n)
    }

    pub(super) fn node_storage_len(&self) -> usize {
        self.label.len()
    }

    fn ensure_node_storage_index(&mut self, index: usize) {
        let len = index + 1;
        if self.label.len() < len {
            self.label.resize(len, None);
            self.labeledge.resize(len, None);
            self.blossomparent.resize(len, None);
            self.blossombase.resize(len, None);
            self.bestedge.resize(len, None);
            self.bestedge_slack.resize_with(len, || None);
            self.bestedge_slack_epoch.resize(len, 0);
        }
    }

    fn ensure_node_storage(&mut self, node: Node) {
        self.ensure_node_storage_index(self.node_index(node));
    }

    pub(super) fn clear_node_storage(&mut self, node: Node) {
        let index = self.node_index(node);
        if index < self.label.len() {
            self.label[index] = None;
            self.labeledge[index] = None;
            self.blossomparent[index] = None;
            self.blossombase[index] = None;
            self.bestedge[index] = None;
            self.bestedge_slack[index] = None;
            self.bestedge_slack_epoch[index] = 0;
        }
    }

    pub(super) fn label_of(&self, node: Node) -> Option<BlossomLabel> {
        let index = self.node_index(node);
        if index < self.label.len() {
            self.label[index]
        } else {
            None
        }
    }

    pub(super) fn set_label(&mut self, node: Node, label: Option<BlossomLabel>) {
        let index = self.node_index(node);
        if label.is_some() {
            self.ensure_node_storage(node);
            self.label[index] = label;
        } else if index < self.label.len() {
            self.label[index] = None;
        }
    }

    pub(super) fn labeledge_of(&self, node: Node) -> Option<(usize, usize)> {
        let index = self.node_index(node);
        if index < self.labeledge.len() {
            self.labeledge[index]
        } else {
            None
        }
    }

    pub(super) fn set_labeledge(&mut self, node: Node, edge: Option<(usize, usize)>) {
        let index = self.node_index(node);
        if edge.is_some() {
            self.ensure_node_storage(node);
            self.labeledge[index] = edge;
        } else if index < self.labeledge.len() {
            self.labeledge[index] = None;
        }
    }

    pub(super) fn parent_of(&self, node: Node) -> Option<Node> {
        let index = self.node_index(node);
        if index < self.blossomparent.len() {
            self.blossomparent[index]
        } else {
            None
        }
    }

    pub(super) fn set_parent(&mut self, node: Node, parent: Option<Node>) {
        let index = self.node_index(node);
        if parent.is_some() {
            self.ensure_node_storage(node);
            self.blossomparent[index] = parent;
        } else if index < self.blossomparent.len() {
            self.blossomparent[index] = None;
        }
    }

    pub(super) fn base_of(&self, node: Node) -> usize {
        Self::invariant(
            self.blossombase
                .get(self.node_index(node))
                .copied()
                .flatten(),
            "node base",
        )
    }

    pub(super) fn set_base(&mut self, node: Node, base: usize) {
        let index = self.node_index(node);
        self.ensure_node_storage(node);
        self.blossombase[index] = Some(base);
    }

    pub(super) fn bestedge_at(&self, index: usize) -> Option<(usize, usize)> {
        if index < self.bestedge.len() {
            self.bestedge[index]
        } else {
            None
        }
    }

    pub(super) fn set_bestedge(&mut self, node: Node, edge: Option<(usize, usize)>) {
        let index = self.node_index(node);
        if edge.is_some() {
            self.ensure_node_storage(node);
            self.bestedge[index] = edge;
            self.bestedge_slack[index] = None;
            self.bestedge_slack_epoch[index] = 0;
        } else if index < self.bestedge.len() {
            self.bestedge[index] = None;
            self.bestedge_slack[index] = None;
            self.bestedge_slack_epoch[index] = 0;
        }
    }

    pub(super) fn set_bestedge_with_slack_at(
        &mut self,
        index: usize,
        edge: (usize, usize),
        slack: &mut BigWeight,
    ) {
        self.ensure_node_storage_index(index);
        self.bestedge[index] = Some(edge);
        let slack_slot = self.bestedge_slack[index].get_or_insert_with(BigWeight::zero);
        std::mem::swap(slack_slot, slack);
        self.bestedge_slack_epoch[index] = self.current_slack_epoch;
    }

    pub(super) fn bestedge_slack_at(&self, index: usize) -> Option<&BigWeight> {
        if index < self.bestedge_slack.len()
            && self.bestedge_slack_epoch[index] == self.current_slack_epoch
        {
            self.bestedge_slack[index].as_ref()
        } else {
            None
        }
    }

    pub(super) fn invalidate_bestedge_slacks(&mut self) {
        if self.current_slack_epoch == u32::MAX {
            self.bestedge_slack_epoch.fill(0);
            self.current_slack_epoch = 1;
        } else {
            self.current_slack_epoch += 1;
        }
    }

    pub(super) fn blossomdual_of(&self, blossom_id: usize) -> Option<&BigWeight> {
        self.blossomdual.get(blossom_id).and_then(Option::as_ref)
    }

    pub(super) fn blossomdual_mut(&mut self, blossom_id: usize) -> &mut BigWeight {
        if self.blossomdual.len() <= blossom_id {
            self.blossomdual.resize_with(blossom_id + 1, || None);
        }
        self.blossomdual[blossom_id].get_or_insert_with(BigWeight::zero)
    }

    pub(super) fn set_blossomdual(&mut self, blossom_id: usize, value: Option<BigWeight>) {
        if self.blossomdual.len() <= blossom_id {
            self.blossomdual.resize_with(blossom_id + 1, || None);
        }
        self.blossomdual[blossom_id] = value;
    }

    pub(super) fn clear_blossomdual(&mut self, blossom_id: usize) {
        if blossom_id < self.blossomdual.len() {
            self.blossomdual[blossom_id] = None;
        }
    }

    fn allowedge_index(&self, vertex0: usize, vertex1: usize) -> usize {
        vertex0 * self.n + vertex1
    }

    pub(super) fn allow_edge(&mut self, vertex0: usize, vertex1: usize) {
        let epoch = self.allowedge_epoch;
        let edge0 = self.allowedge_index(vertex0, vertex1);
        let edge1 = self.allowedge_index(vertex1, vertex0);
        self.allowedge[edge0] = epoch;
        self.allowedge[edge1] = epoch;
    }

    pub(super) fn allowedge_clear(&mut self) {
        if self.allowedge_epoch == u32::MAX {
            self.allowedge.fill(0);
            self.allowedge_epoch = 1;
        } else {
            self.allowedge_epoch += 1;
        }
    }

    pub(super) fn slack_into(&self, vertex0: usize, vertex1: usize, result: &mut BigWeight) {
        let edge_weight = Self::invariant(self.graph.edge_weight(vertex0, vertex1), "edge weight");
        self.slack_weight_into(vertex0, vertex1, edge_weight, result);
    }

    pub(super) fn slack_weight_into(
        &self,
        vertex0: usize,
        vertex1: usize,
        edge_weight: &BigWeight,
        result: &mut BigWeight,
    ) {
        let vertex0_dual = &self.dualvar[vertex0];
        let vertex1_dual = &self.dualvar[vertex1];
        result.assign_slack(vertex0_dual, vertex1_dual, edge_weight);
    }

    pub(super) fn slack_weight_is_zero(
        &self,
        vertex0: usize,
        vertex1: usize,
        edge_weight: &BigWeight,
    ) -> bool {
        let vertex0_dual = &self.dualvar[vertex0];
        let vertex1_dual = &self.dualvar[vertex1];
        BigWeight::slack_is_zero(vertex0_dual, vertex1_dual, edge_weight)
    }

    pub(super) fn extend_leaves(&self, node: Node, result: &mut Vec<usize>) {
        Self::extend_leaves_from(&self.blossoms, node, result);
    }

    fn extend_leaves_from(blossoms: &[Option<Blossom>], node: Node, result: &mut Vec<usize>) {
        match node {
            Node::Vertex(vertex) => result.push(vertex),
            Node::Blossom(blossom_id) => {
                if let Some(blossom) = &blossoms[blossom_id] {
                    for child in &blossom.childs {
                        Self::extend_leaves_from(blossoms, *child, result);
                    }
                }
            }
        }
    }

    pub(super) fn assign_label(
        &mut self,
        vertex: usize,
        label: BlossomLabel,
        from_vertex: Option<usize>,
    ) {
        let blossom = self.inblossom[vertex];
        debug_assert!(self.label_of(Node::Vertex(vertex)).is_none());
        debug_assert!(self.label_of(blossom).is_none());
        self.set_label(Node::Vertex(vertex), Some(label));
        self.set_label(blossom, Some(label));
        let edge = from_vertex.map(|from_vertex| (from_vertex, vertex));
        self.set_labeledge(Node::Vertex(vertex), edge);
        self.set_labeledge(blossom, edge);
        self.set_bestedge(Node::Vertex(vertex), None);
        self.set_bestedge(blossom, None);
        if label == BlossomLabel::Outer {
            let workspace = &mut self.workspace;
            Self::extend_leaves_from(&workspace.blossoms, blossom, &mut workspace.queue);
        } else {
            let base = self.base_of(blossom);
            let mate = Self::invariant(self.mate[base], "T blossom base mate");
            self.assign_label(mate, BlossomLabel::Outer, Some(base));
        }
    }

    pub(super) fn scan_blossom(&mut self, mut vertex: usize, neighbor: usize) -> Option<usize> {
        let mut path = std::mem::take(&mut self.scan_path_scratch);
        let mut vertex_opt = Some(vertex);
        let mut neighbor_opt = Some(neighbor);
        let mut base = None;

        while let Some(current_vertex) = vertex_opt {
            vertex = current_vertex;
            let mut blossom = self.inblossom[vertex];
            if self
                .label_of(blossom)
                .is_some_and(BlossomLabel::is_scanned_outer)
            {
                base = Some(self.base_of(blossom));
                break;
            }
            debug_assert_eq!(self.label_of(blossom), Some(BlossomLabel::Outer));
            path.push(blossom);
            self.set_label(blossom, Some(BlossomLabel::ScannedOuter));
            if let Some(edge) = self.labeledge_of(blossom) {
                debug_assert_eq!(Some(edge.0), self.mate[self.base_of(blossom)]);
                vertex = edge.0;
                blossom = self.inblossom[vertex];
                debug_assert_eq!(self.label_of(blossom), Some(BlossomLabel::Inner));
                vertex_opt = self.labeledge_of(blossom).map(|edge| edge.0);
            } else {
                debug_assert!(self.mate[self.base_of(blossom)].is_none());
                vertex_opt = None;
            }
            if neighbor_opt.is_some() {
                std::mem::swap(&mut vertex_opt, &mut neighbor_opt);
            }
        }

        for &blossom in &path {
            self.set_label(blossom, Some(BlossomLabel::Outer));
        }
        path.clear();
        self.scan_path_scratch = path;
        base
    }
}
