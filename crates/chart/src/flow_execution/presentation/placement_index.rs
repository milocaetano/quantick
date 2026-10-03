//! Fixed balanced topology; activation expands only final, already placed bounds.
use super::{
    CIRCLE_INDEX_LEAF_CAPACITY, FlowDisc, FlowRegionVisual, intersects, intersects_bounds,
};

const EMPTY_BOUNDS: [[f64; 2]; 2] = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];

pub(super) struct PlacedIndex {
    nodes: Vec<Node>,
    leaves: Vec<usize>,
    active: Vec<bool>,
}

struct Node {
    bounds: [[f64; 2]; 2],
    parent: Option<usize>,
    children: Children,
}

enum Children {
    Leaf(Vec<usize>),
    Split(usize, usize),
}

impl PlacedIndex {
    pub(super) fn new(regions: &[FlowRegionVisual], peaks: &[usize]) -> Self {
        let mut index = Self {
            nodes: Vec::new(),
            leaves: vec![usize::MAX; regions.len()],
            active: vec![false; regions.len()],
        };
        if !peaks.is_empty() {
            // Partition a copy: quantity priority remains untouched in the caller.
            index.build(regions, &mut peaks.to_vec(), None);
        }
        index
    }

    fn build(
        &mut self,
        regions: &[FlowRegionVisual],
        indices: &mut [usize],
        parent: Option<usize>,
    ) -> usize {
        let node = self.nodes.len();
        self.nodes.push(Node {
            bounds: EMPTY_BOUNDS,
            parent,
            children: Children::Leaf(Vec::new()),
        });
        let children = if indices.len() <= CIRCLE_INDEX_LEAF_CAPACITY {
            for &index in indices.iter() {
                self.leaves[index] = node;
            }
            Children::Leaf(indices.to_vec())
        } else {
            let mut centers = EMPTY_BOUNDS;
            for &index in indices.iter() {
                for (axis, coordinate) in regions[index].source_disc.center.into_iter().enumerate()
                {
                    let value = f64::from(coordinate);
                    centers[0][axis] = centers[0][axis].min(value);
                    centers[1][axis] = centers[1][axis].max(value);
                }
            }
            let axis = usize::from(centers[1][1] - centers[0][1] > centers[1][0] - centers[0][0]);
            let middle = indices.len() / 2;
            indices.select_nth_unstable_by(middle, |&a, &b| {
                regions[a].source_disc.center[axis]
                    .total_cmp(&regions[b].source_disc.center[axis])
                    .then_with(|| a.cmp(&b))
            });
            let (left, right) = indices.split_at_mut(middle);
            Children::Split(
                self.build(regions, left, Some(node)),
                self.build(regions, right, Some(node)),
            )
        };
        self.nodes[node].children = children;
        node
    }

    pub(super) fn overlaps(&self, disc: FlowDisc, regions: &[FlowRegionVisual], gap: f32) -> bool {
        !self.nodes.is_empty() && self.overlaps_node(0, disc, regions, gap)
    }

    fn overlaps_node(
        &self,
        node: usize,
        disc: FlowDisc,
        regions: &[FlowRegionVisual],
        gap: f32,
    ) -> bool {
        let node = &self.nodes[node];
        if !intersects_bounds(disc, node.bounds, gap) {
            return false;
        }
        match &node.children {
            Children::Leaf(indices) => indices
                .iter()
                .any(|&index| self.active[index] && intersects(disc, regions[index].disc, gap)),
            Children::Split(left, right) => {
                self.overlaps_node(*left, disc, regions, gap)
                    || self.overlaps_node(*right, disc, regions, gap)
            }
        }
    }

    pub(super) fn insert(&mut self, index: usize, regions: &[FlowRegionVisual]) {
        self.active[index] = true;
        let disc = regions[index].disc;
        let mut ancestor = Some(self.leaves[index]);
        while let Some(index) = ancestor {
            let node = &mut self.nodes[index];
            for (axis, coordinate) in disc.center.into_iter().enumerate() {
                let center = f64::from(coordinate);
                node.bounds[0][axis] = node.bounds[0][axis].min(center - f64::from(disc.radius));
                node.bounds[1][axis] = node.bounds[1][axis].max(center + f64::from(disc.radius));
            }
            ancestor = node.parent;
        }
    }
}
