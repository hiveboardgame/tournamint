use super::model::{BlossomLabel, EdmondsSolver, Node};

impl EdmondsSolver<'_> {
    pub(super) fn augment_blossom(&mut self, blossom_node: Node, vertex: usize) {
        let Node::Blossom(blossom_id) = blossom_node else {
            return;
        };
        let mut child = Node::Vertex(vertex);
        while self.parent_of(child) != Some(blossom_node) {
            child = Self::invariant(self.parent_of(child), "ancestor parent");
        }
        if matches!(child, Node::Blossom(_)) {
            self.augment_blossom(child, vertex);
        }

        let (mut childs, mut edges) = {
            let blossom = Self::invariant(self.blossoms[blossom_id].as_mut(), "augment blossom");
            (
                std::mem::take(&mut blossom.childs),
                std::mem::take(&mut blossom.edges),
            )
        };
        let rotate_index = Self::invariant(
            childs.iter().position(|candidate| *candidate == child),
            "child in blossom",
        );
        let mut index = rotate_index.cast_signed();
        let step = if index & 1 != 0 {
            index -= childs.len().cast_signed();
            1
        } else {
            -1
        };
        while index != 0 {
            index += step;
            let child = childs[Self::wrap_index(index, childs.len())];
            let (matched_vertex, next_vertex) = if step == 1 {
                edges[Self::wrap_index(index, edges.len())]
            } else {
                let edge = edges[Self::wrap_index(index - 1, edges.len())];
                (edge.1, edge.0)
            };
            if matches!(child, Node::Blossom(_)) {
                self.augment_blossom(child, matched_vertex);
            }
            index += step;
            let child = childs[Self::wrap_index(index, childs.len())];
            if matches!(child, Node::Blossom(_)) {
                self.augment_blossom(child, next_vertex);
            }
            self.mate[matched_vertex] = Some(next_vertex);
            self.mate[next_vertex] = Some(matched_vertex);
        }

        childs.rotate_left(rotate_index);
        edges.rotate_left(rotate_index);
        let rotated_base_child = childs[0];
        if let Some(blossom) = &mut self.blossoms[blossom_id] {
            blossom.childs = childs;
            blossom.edges = edges;
        }
        let base = self.base_of(rotated_base_child);
        self.set_base(blossom_node, base);
        debug_assert_eq!(base, vertex);
    }

    pub(super) fn augment_matching(&mut self, vertex: usize, neighbor: usize) {
        for (mut path_vertex, mut match_vertex) in [(vertex, neighbor), (neighbor, vertex)] {
            loop {
                let path_blossom = self.inblossom[path_vertex];
                debug_assert_eq!(self.label_of(path_blossom), Some(BlossomLabel::Outer));
                debug_assert!(
                    (self.labeledge_of(path_blossom).is_none()
                        && self.mate[self.base_of(path_blossom)].is_none())
                        || self
                            .labeledge_of(path_blossom)
                            .map(|edge| Some(edge.0) == self.mate[self.base_of(path_blossom)])
                            .unwrap_or(false)
                );
                if matches!(path_blossom, Node::Blossom(_)) {
                    self.augment_blossom(path_blossom, path_vertex);
                }
                self.mate[path_vertex] = Some(match_vertex);
                let Some(label_edge) = self.labeledge_of(path_blossom) else {
                    break;
                };
                let t_vertex = label_edge.0;
                let t_blossom = self.inblossom[t_vertex];
                debug_assert_eq!(self.label_of(t_blossom), Some(BlossomLabel::Inner));
                let next_edge = Self::invariant(self.labeledge_of(t_blossom), "T blossom edge");
                path_vertex = next_edge.0;
                match_vertex = next_edge.1;
                debug_assert_eq!(self.base_of(t_blossom), t_vertex);
                if matches!(t_blossom, Node::Blossom(_)) {
                    self.augment_blossom(t_blossom, match_vertex);
                }
                self.mate[match_vertex] = Some(path_vertex);
            }
        }
    }

    pub(super) fn wrap_index(index: isize, len: usize) -> usize {
        let len = len.cast_signed();
        index.rem_euclid(len).cast_unsigned()
    }
}
