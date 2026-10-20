//! Navigation over the `.hzx` area node graphs (all units mm).
//!
//! Node lookup follows `FUN_001451B8`/`FUN_00144830`. Paths are computed
//! with Dijkstra over the node links instead of reading the game's
//! precomputed shortest-path matrices, which should give the same routes.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use bevy::math::{Vec2, Vec3};
use mgs2_formats::hzx::{Hzx, NavNode};

pub struct Nav {
    /// (area, node) for every node, flattened.
    nodes: Vec<Node>,
}

struct Node {
    pos: Vec3,
    half: Vec2,
    low: bool,
    links: Vec<usize>,
}

impl Node {
    /// `FUN_00144830`.
    fn contains(&self, p: Vec3) -> bool {
        let (lo, hi) = if self.low { (-10.0, 500.0) } else { (-1000.0, 2000.0) };
        let dy = p.y - self.pos.y;
        dy >= lo
            && dy < hi
            && (p.x - self.pos.x).abs() <= self.half.x
            && (p.z - self.pos.z).abs() <= self.half.y
    }
}

impl Nav {
    pub fn new(hzx: &Hzx) -> Self {
        let mut nodes = Vec::new();
        for area in &hzx.areas {
            let base = nodes.len();
            let to_node = |n: &NavNode| Node {
                pos: Vec3::from(n.position),
                half: Vec2::from(n.half),
                low: n.flags & 0x20 != 0,
                links: n.links.iter().map(|l| base + *l as usize).filter(|i| *i < base + area.nav.len()).collect(),
            };
            nodes.extend(area.nav.iter().map(to_node));
        }
        // Make links symmetric (the data lists them per node).
        for i in 0..nodes.len() {
            for j in nodes[i].links.clone() {
                if !nodes[j].links.contains(&i) {
                    nodes[j].links.push(i);
                }
            }
        }
        Self { nodes }
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Node containing `p`, else the nearest node within 3 m vertically.
    pub fn node_at(&self, p: Vec3) -> Option<usize> {
        if let Some(i) = self.nodes.iter().position(|n| n.contains(p)) {
            return Some(i);
        }
        self.nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| (n.pos.y - p.y).abs() < 3000.0)
            .min_by(|a, b| a.1.pos.distance_squared(p).total_cmp(&b.1.pos.distance_squared(p)))
            .map(|(i, _)| i)
    }

    pub fn node_pos(&self, i: usize) -> Vec3 {
        self.nodes[i].pos
    }

    /// Waypoints from `from` to `to`: node centres along the shortest node
    /// path (excluding the start node), then `to` itself.
    pub fn path(&self, from: Vec3, to: Vec3) -> Vec<Vec3> {
        let (Some(a), Some(b)) = (self.node_at(from), self.node_at(to)) else {
            return vec![to];
        };
        if a == b {
            return vec![to];
        }
        let n = self.nodes.len();
        let mut dist = vec![f32::INFINITY; n];
        let mut prev = vec![usize::MAX; n];
        let mut heap = BinaryHeap::new();
        dist[a] = 0.0;
        heap.push((Reverse(0u32), a));
        while let Some((Reverse(d), i)) = heap.pop() {
            if i == b {
                break;
            }
            if d as f32 > dist[i] {
                continue;
            }
            for &j in &self.nodes[i].links {
                let nd = dist[i] + self.nodes[i].pos.distance(self.nodes[j].pos);
                if nd < dist[j] {
                    dist[j] = nd;
                    prev[j] = i;
                    heap.push((Reverse(nd as u32), j));
                }
            }
        }
        if prev[b] == usize::MAX {
            return vec![to];
        }
        let mut path = vec![to];
        let mut i = prev[b];
        while i != a && i != usize::MAX {
            path.push(self.nodes[i].pos);
            i = prev[i];
        }
        path.reverse();
        path
    }

    /// The node linked to `from`'s that lies most nearly away from
    /// `threat` (`FUN_00253c38`: the bearing to the threat + 0x800, then
    /// the node's links; the game's choice among them is approximated by
    /// the best-aligned link ahead).
    pub fn away(&self, from: Vec3, threat: Vec3) -> Option<Vec3> {
        let a = self.node_at(from)?;
        let dir = Vec2::new(from.x - threat.x, from.z - threat.z).normalize_or_zero();
        self.nodes[a]
            .links
            .iter()
            .map(|&j| self.nodes[j].pos)
            .map(|p| (p, Vec2::new(p.x - from.x, p.z - from.z).normalize_or_zero().dot(dir)))
            .filter(|(_, d)| *d > 0.0)
            .max_by(|x, y| x.1.total_cmp(&y.1))
            .map(|(p, _)| p)
    }

    /// Nodes within `radius` of `around` (for searching).
    pub fn nodes_near(&self, around: Vec3, radius: f32) -> Vec<usize> {
        (0..self.nodes.len()).filter(|i| self.nodes[*i].pos.distance(around) < radius).collect()
    }
}
