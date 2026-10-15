use super::super::weight::BigWeight;
use crate::matching::graph::MatchingGraph;
use smallvec::SmallVec;
use std::ops::{Deref, DerefMut};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(super) enum Node {
    Vertex(usize),
    Blossom(usize),
}

impl Node {
    pub(super) fn index(self, vertex_count: usize) -> usize {
        match self {
            Self::Vertex(vertex) => vertex,
            Self::Blossom(blossom_id) => vertex_count + blossom_id,
        }
    }
}

pub(super) type BlossomChildren = SmallVec<[Node; 9]>;
pub(super) type BlossomEdges = SmallVec<[(usize, usize); 9]>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BlossomLabel {
    Outer,
    Inner,
    ScannedOuter,
}

impl BlossomLabel {
    pub(super) fn is_scanned_outer(self) -> bool {
        matches!(self, Self::ScannedOuter)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DeltaKind {
    NoImprovement,
    UnlabeledVertex,
    LabeledEdge,
    BlossomExpansion,
}

#[derive(Clone, Debug)]
pub(super) struct Blossom {
    pub(super) childs: BlossomChildren,
    pub(super) edges: BlossomEdges,
    pub(super) my_bestedges: Option<Vec<(usize, usize)>>,
}

#[derive(Clone, Debug)]
pub(super) struct BlossomBestEdgeScratch {
    pub(super) edge_to: Vec<Option<(usize, usize)>>,
    pub(super) zero_slack_to: Vec<bool>,
    pub(super) best_slack_to: Vec<BigWeight>,
    pub(super) edge_slack: BigWeight,
    pub(super) leaf_scratch: Vec<usize>,
}

impl BlossomBestEdgeScratch {
    pub(super) fn with_capacity(node_capacity: usize, vertex_capacity: usize) -> Self {
        Self {
            edge_to: Vec::with_capacity(node_capacity),
            zero_slack_to: Vec::with_capacity(node_capacity),
            best_slack_to: Vec::with_capacity(node_capacity),
            edge_slack: BigWeight::zero(),
            leaf_scratch: Vec::with_capacity(vertex_capacity),
        }
    }

    pub(super) fn empty() -> Self {
        Self {
            edge_to: Vec::new(),
            zero_slack_to: Vec::new(),
            best_slack_to: Vec::new(),
            edge_slack: BigWeight::zero(),
            leaf_scratch: Vec::new(),
        }
    }

    pub(super) fn reset(&mut self, node_storage_len: usize) {
        self.edge_to.clear();
        self.edge_to.resize(node_storage_len, None);
        self.zero_slack_to.clear();
        self.zero_slack_to.resize(node_storage_len, false);
        if self.best_slack_to.len() < node_storage_len {
            self.best_slack_to
                .resize_with(node_storage_len, BigWeight::zero);
        }
        self.leaf_scratch.clear();
    }
}

#[derive(Clone)]
pub(in crate::matching) struct EdmondsWorkspace {
    pub(super) n: usize,
    pub(super) mate: Vec<Option<usize>>,
    pub(super) label: Vec<Option<BlossomLabel>>,
    pub(super) labeledge: Vec<Option<(usize, usize)>>,
    pub(super) inblossom: Vec<Node>,
    pub(super) blossomparent: Vec<Option<Node>>,
    pub(super) blossombase: Vec<Option<usize>>,
    pub(super) bestedge: Vec<Option<(usize, usize)>>,
    pub(super) bestedge_slack: Vec<Option<BigWeight>>,
    pub(super) bestedge_slack_epoch: Vec<u32>,
    pub(super) current_slack_epoch: u32,
    pub(super) dualvar: Vec<BigWeight>,
    pub(super) blossomdual: Vec<Option<BigWeight>>,
    pub(super) allowedge: Vec<u32>,
    pub(super) allowedge_epoch: u32,
    pub(super) queue: Vec<usize>,
    pub(super) blossoms: Vec<Option<Blossom>>,
    pub(super) blossom_best_edge_scratch: BlossomBestEdgeScratch,
    pub(super) scan_path_scratch: Vec<Node>,
    pub(super) expand_leaf_scratch: Vec<usize>,
    pub(super) active_vertices: Vec<usize>,
}

pub(super) struct EdmondsSolver<'a> {
    pub(super) graph: &'a MatchingGraph,
    pub(super) workspace: &'a mut EdmondsWorkspace,
}

impl Deref for EdmondsSolver<'_> {
    type Target = EdmondsWorkspace;

    fn deref(&self) -> &Self::Target {
        &*self.workspace
    }
}

impl DerefMut for EdmondsSolver<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.workspace
    }
}
