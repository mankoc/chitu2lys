//! Rebuilding CHITUBOX's support trees from its parts, as Lychee supports.
//!
//! CHITUBOX supports are trees. Tips (type 1) lead through straight columns (type 3) to a node where the tips of a
//! tree meet, and from there one column runs down to a foot (types 4 and 5), to a bottom tip standing on the model
//! (type 2), or onto another tree's column. Crossing pairs of columns are braces.
//!
//! A Lychee support draws, besides its tip cone, at most two straight pieces: a plate support its vertical column
//! and a middle; a support on the model or on another support its base piece and a middle. So each tree becomes:
//!
//! - one support carrying the tree: on the foot, on the model (at the bottom tip's contact), or on the support of
//!   the tree it leans on;
//! - one branch support per other tip, resting on that support where the tips meet.
//!
//! Each support's joins are chosen among the bends of its CHITUBOX path so that its drawing strays least from it.
//! Each brace whose ends lie on two of these supports becomes a Lychee brace. Tips whose path cannot be followed
//! stand straight below their tip, on the plate or on the model.
//!
//! Some CHITUBOX trunks carry no tip of their own: their branches join them a little below their top. Such a
//! branch is followed on down the trunk to its foot.

use std::collections::{HashMap, HashSet};

use crate::chitubox::Part;
use crate::ground::Ground;
use glam::{DVec2, DVec3};

/// Part types (`CHITUBOX_FILE_FORMAT.md`, 6.5).
const TOP_TIP: u32 = 1;
const BOTTOM_TIP: u32 = 2;
const COLUMN: u32 = 3;
const FOOT_CONE: u32 = 4;
const FOOT_PAD: u32 = 5;

/// How close (mm) two part ends must be to count as the same node.
const NODE: f64 = 0.02;
/// How close (mm) a point must be to a segment to lie on it.
const SAME: f64 = 0.05;
/// Shortest base piece: Lychee thins pieces shorter than about their diameter / 1.75.
const MIN_PIECE: f64 = 0.7;
/// How far (mm) the end of a tree or a brace may be from the support it rests on (where a support cannot follow
/// CHITUBOX's path exactly, its column may be off by about a millimetre).
const NEAR: f64 = 1.5;

/// Where a Lychee support's base goes. Every support also has a middle from the base's `join` to its tip join.
#[derive(Debug, Clone, PartialEq)]
pub enum Base {
    /// On the plate at `foot` (model-frame X/Y); its vertical column rises to the height `join` (model frame).
    Plate { foot: DVec2, join: f64 },
    /// On the model at `contact`, `depth` deep and `point` wide there; its base piece runs to `join`.
    Model { contact: DVec3, depth: f64, point: f64, join: DVec3 },
    /// On support `parent` (index in the plan) at `point`; its base piece runs to `join`.
    Branch { parent: usize, point: DVec3, join: DVec3 },
}

/// One Lychee support: a CHITUBOX tip, where its tip cone ends, its base, and the diameter of its column or base
/// piece.
pub struct Planned<'a> {
    pub tip: &'a Part,
    /// End of the tip cone, where the middle starts (usually the tip part's lower end).
    pub tip_join: DVec3,
    pub base: Base,
    pub column: f64,
}

impl Planned<'_> {
    /// The straight pieces this support draws (model frame), with `plate` the plate's height: its column or base
    /// piece, its middle and its tip.
    pub fn pieces(&self, plate: f64) -> [(DVec3, DVec3); 3] {
        let (start, join) = match self.base {
            Base::Plate { foot, join } => (foot.extend(plate), foot.extend(join)),
            Base::Model { contact, join, .. } => (contact, join),
            Base::Branch { point, join, .. } => (point, join),
        };
        [(start, join), (join, self.tip_join), (self.tip_join, self.tip.a)]
    }
}

/// One Lychee brace between the supports `lower.0` and `upper.0` (indices in the plan), from `lower.1` up to
/// `upper.1`.
pub struct Brace {
    pub lower: (usize, DVec3),
    pub upper: (usize, DVec3),
    pub diameter: f64,
}

/// An exact key for a point: part ends are first snapped to their node (`Nodes`), so equal nodes have equal keys.
fn key(p: DVec3) -> [u64; 3] {
    p.to_array().map(f64::to_bits)
}

/// The nodes of a support tree: part ends closer than `NODE` are one node, at the first of them.
#[derive(Default)]
struct Nodes {
    cells: HashMap<[i64; 3], Vec<DVec3>>,
}

impl Nodes {
    /// The nodes of the parts' ends.
    fn of(parts: &[Part]) -> Self {
        let mut nodes = Self::default();
        for part in parts.iter().filter(|p| matches!(p.kind, TOP_TIP | BOTTOM_TIP | COLUMN | FOOT_CONE | FOOT_PAD)) {
            nodes.snap(part.a);
            nodes.snap(part.b);
        }
        nodes
    }

    fn cell(p: DVec3) -> [i64; 3] {
        p.to_array().map(|v| (v / NODE).floor() as i64)
    }

    /// The node at `p`, if there is one.
    fn find(&self, p: DVec3) -> Option<DVec3> {
        let [x, y, z] = Self::cell(p);
        (-1..=1)
            .flat_map(|dx| (-1..=1).flat_map(move |dy| (-1..=1).map(move |dz| [x + dx, y + dy, z + dz])))
            .filter_map(|cell| self.cells.get(&cell))
            .flatten()
            .copied()
            .find(|q| q.distance(p) < NODE)
    }

    /// The node at `p`, made if there is none.
    fn snap(&mut self, p: DVec3) -> DVec3 {
        self.find(p).unwrap_or_else(|| {
            self.cells.entry(Self::cell(p)).or_default().push(p);
            p
        })
    }
}

/// The point of the segment `a b` nearest to `p`.
fn nearest_on_segment(p: DVec3, (a, b): (DVec3, DVec3)) -> DVec3 {
    let ab = b - a;
    let t = if ab.length_squared() > 1e-12 { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
    a + ab * t
}

/// Distance from `p` to the segment `a b`.
fn distance_to_segment(p: DVec3, segment: (DVec3, DVec3)) -> f64 {
    p.distance(nearest_on_segment(p, segment))
}

/// How far apart two polylines are: the largest distance from a point of either (its corners and points along its
/// pieces) to the other.
fn deviation(a: &[DVec3], b: &[DVec3]) -> f64 {
    let one_way = |from: &[DVec3], to: &[DVec3]| {
        let samples = from.windows(2).flat_map(|w| (0..4).map(move |k| w[0].lerp(w[1], k as f64 / 4.0)));
        samples
            .chain(from.last().copied())
            .map(|p| to.windows(2).map(|w| distance_to_segment(p, (w[0], w[1]))).fold(f64::INFINITY, f64::min))
            .fold(0.0, f64::max)
    };
    one_way(a, b).max(one_way(b, a))
}

/// The columns of the support trees (not braces), from their upper to their lower end, found by place.
struct Columns {
    list: Vec<(DVec3, DVec3)>,
    /// The columns crossing each `CELL`-wide square of the plate.
    grid: HashMap<[i64; 2], Vec<usize>>,
    /// The columns ending at each node, from above.
    ending_at: HashMap<[u64; 3], Vec<usize>>,
}

impl Columns {
    /// Size (mm) of the squares the columns are found by.
    const CELL: f64 = 2.0;

    fn new(list: Vec<(DVec3, DVec3)>) -> Self {
        let (mut grid, mut ending_at) = (HashMap::<_, Vec<usize>>::new(), HashMap::<_, Vec<usize>>::new());
        let cell = |v: f64| (v / Self::CELL).floor() as i64;
        for (i, &(hi, lo)) in list.iter().enumerate() {
            let (min, max) = (hi.min(lo) - SAME, hi.max(lo) + SAME);
            for x in cell(min.x)..=cell(max.x) {
                for y in cell(min.y)..=cell(max.y) {
                    grid.entry([x, y]).or_default().push(i);
                }
            }
            ending_at.entry(key(lo)).or_default().push(i);
        }
        Self { list, grid, ending_at }
    }

    /// The columns passing through `p` (their ends included).
    fn through(&self, p: DVec3) -> impl Iterator<Item = usize> + '_ {
        let cell = [p.x, p.y].map(|v| (v / Self::CELL).floor() as i64);
        let found = self.grid.get(&cell).map_or(&[][..], |v| &v[..]);
        found.iter().copied().filter(move |&i| distance_to_segment(p, self.list[i]) < SAME)
    }

    /// The column a path ending at `end` lands on: one passing through it between its ends.
    fn landing(&self, end: DVec3) -> Option<usize> {
        self.through(end).find(|&i| key(self.list[i].0) != key(end) && key(self.list[i].1) != key(end))
    }
}

/// Whether column part `p` is one of a crossing pair (a brace): the same two points with the heights swapped.
/// `nodes` holds the snapped part ends.
fn braces_of(parts: &[Part], nodes: &Nodes) -> Vec<bool> {
    let snapped = |p: DVec3| nodes.find(p).map(key);
    let ends: HashSet<_> = parts.iter().filter(|p| p.kind == COLUMN).map(|p| (snapped(p.a), snapped(p.b))).collect();
    parts
        .iter()
        .map(|p| {
            p.kind == COLUMN && (p.a.z - p.b.z).abs() > 0.5 && {
                let (a, b) = (DVec3::new(p.a.x, p.a.y, p.b.z), DVec3::new(p.b.x, p.b.y, p.a.z));
                let (a, b) = (snapped(a), snapped(b));
                a.is_some() && b.is_some() && ends.contains(&(a, b))
            }
        })
        .collect()
}

/// The points of `path` where it bends: its first point, the ends of its straight pieces, and its last point (at
/// least two points, the same twice for a path that goes nowhere).
fn corners(path: &[DVec3]) -> Vec<DVec3> {
    let mut out = vec![path[0]];
    for window in path.windows(3) {
        let (a, b, c) = (window[0], window[1], window[2]);
        if (b - a).normalize_or_zero().dot((c - b).normalize_or_zero()) < 0.9995 {
            out.push(b);
        }
    }
    out.push(*path.last().unwrap());
    out.dedup_by(|a, b| a.distance(*b) < 1e-9);
    if out.len() == 1 {
        out.push(out[0]);
    }
    out
}

/// A point `length` along the way from `from` to `to`, not past half of it.
fn along(from: DVec3, to: DVec3, length: f64) -> DVec3 {
    let span = from.distance(to);
    from + (to - from).normalize_or_zero() * length.min(span / 2.0)
}

/// A cone drawn from radius `point` to `radius` over `length`: how much thinner it is than CHITUBOX's cone of the
/// same radii that is only `natural` long and then goes on as a column.
fn thinning(point: f64, radius: f64, natural: f64, length: f64) -> f64 {
    if length <= natural { 0.0 } else { (radius - point).max(0.0) * (1.0 - natural / length) }
}

/// The Lychee drawing that follows `path` best. `path` runs from where the support starts (its foot, its contact on
/// the model, or its point on the parent) through its bends up to the tip's lower end. The support draws its column
/// or base piece from the start to one of `joins`, its middle from there to a tip join (the tip's lower end, or a
/// bend above the join with the tip cone stretched down to it) and the tip cone up to the contact. Returns the
/// join, the tip join and how far the drawing strays from CHITUBOX's: the distance between the two paths, plus how
/// much a stretched tip cone gets thinner.
fn fit(path: &[DVec3], tip: &Part, joins: &[DVec3]) -> (DVec3, DVec3, f64) {
    let mut chitubox = path.to_vec();
    chitubox.push(tip.a);
    let mut best = (path[0], tip.b, f64::INFINITY);
    for &join in joins {
        // tip joins: the bends above the join, up to the tip's lower end
        let above = path.iter().position(|p| p.distance(join) < 1e-9).map_or(1, |i| i + 1);
        // (the tip's own lower end first, so that ties keep the tip as it is)
        for &tip_join in path[above.min(path.len() - 1)..].iter().rev() {
            let drawn = [path[0], join, tip_join, tip.a];
            let stretch = thinning(tip.size_a, tip.size_b, tip.a.distance(tip.b), tip.a.distance(tip_join));
            let stray = deviation(&chitubox, &drawn) + stretch;
            if stray < best.2 - 1e-9 {
                best = (join, tip_join, stray);
            }
        }
    }
    best
}

/// Where the base piece of a support starting at `path[0]` may end: at any bend of `path` before the tip's lower
/// end, or (last, for ties) a short way towards the first bend.
fn base_joins(path: &[DVec3]) -> Vec<DVec3> {
    let mut joins = path[1..path.len() - 1].to_vec();
    joins.push(along(path[0], path[1], MIN_PIECE));
    joins
}

/// How a tree ends: on a foot, on the model (a bottom tip), or on another tree's column.
#[derive(Clone, Copy)]
enum Root {
    Foot(DVec3),
    /// A bottom tip: its upper end `top`, its contact on the model, how deep it goes in, and its diameters at the
    /// contact and at the top.
    Model {
        top: DVec3,
        contact: DVec3,
        depth: f64,
        point: f64,
        diameter: f64,
    },
    OnColumn(DVec3),
}

/// One tip's path: the tip, and the points from its lower end down to where its tree ends.
type Chain<'a> = (&'a Part, Vec<DVec3>);

/// The planner's working state for one object.
struct Planner<'a> {
    planned: Vec<Planned<'a>>,
    plate: f64,
    /// Diameter of the column below each node.
    diameter_below: HashMap<[u64; 3], f64>,
    columns: Columns,
    /// The support standing for each column, once planned.
    owner: Vec<Option<usize>>,
}

impl<'a> Planner<'a> {
    /// Note that support `index` stands for the columns along CHITUBOX's `path`.
    fn claim(&mut self, index: usize, path: &[DVec3]) {
        for pair in path.windows(2) {
            let (upper, lower) = if pair[0].z >= pair[1].z { (pair[0], pair[1]) } else { (pair[1], pair[0]) };
            for &i in self.columns.ending_at.get(&key(lower)).map_or(&[][..], |v| &v[..]) {
                if distance_to_segment(upper, self.columns.list[i]) < SAME {
                    self.owner[i].get_or_insert(index);
                }
            }
        }
    }

    /// The support standing for a column through `p` (with `landing`, one that `p` lies between the ends of), and
    /// the point of its drawing nearest `p`.
    fn owner_at(&self, p: DVec3, landing: bool) -> Option<(usize, DVec3)> {
        let owner = if landing {
            self.columns.landing(p).and_then(|i| self.owner[i])
        } else {
            self.columns.through(p).find_map(|i| self.owner[i])
        }?;
        let (_, point) = nearest(&self.planned[owner..=owner], self.plate, p, f64::INFINITY)?;
        Some((owner, point))
    }

    /// Diameter of the column below `node`, or `fallback`.
    fn diameter(&self, node: DVec3, fallback: f64) -> f64 {
        self.diameter_below.get(&key(node)).copied().unwrap_or(fallback)
    }

    /// Plan one tree: `chains` are its tips' paths, all ending where the tree ends (`root`); `parent` is the
    /// support the tree leans on.
    fn tree(&mut self, root: Root, chains: Vec<Chain<'a>>, parent: Option<usize>) {
        // the merge node: the highest point all chains share
        let shared: HashSet<[u64; 3]> = chains
            .iter()
            .map(|(_, c)| c.iter().map(|p| key(*p)).collect::<HashSet<_>>())
            .reduce(|a, b| a.intersection(&b).copied().collect())
            .unwrap_or_default();
        let merge =
            chains[0].1.iter().copied().filter(|p| shared.contains(&key(*p))).max_by(|a, b| a.z.total_cmp(&b.z));
        let Some(merge) = merge else { return };
        // path from the end of the tree up to the merge node (shared by all), then each tip's own path up to its tip
        let first = &chains[0].1;
        let at = first.iter().position(|p| key(*p) == key(merge)).unwrap_or(0);
        let trunk: Vec<DVec3> = first[at..].iter().rev().copied().collect(); // end of the tree .. merge node
        let own = |chain: &Vec<DVec3>| -> Vec<DVec3> {
            let at = chain.iter().position(|p| key(*p) == key(merge)).unwrap_or(chain.len() - 1);
            chain[..=at].iter().rev().copied().collect() // merge node .. tip's lower end
        };

        // the carrier: the tip whose path, joined to the trunk, a support follows best
        let (best, planned) = chains
            .iter()
            .enumerate()
            .map(|(i, (tip, chain))| (i, self.carrier(root, parent, &trunk, &own(chain), tip)))
            .min_by(|a, b| a.1.1.total_cmp(&b.1.1))
            .map(|(i, (planned, _))| (i, planned))
            .expect("a tree has tips");
        let carrier = self.planned.len();
        self.planned.push(planned);
        let mut path = trunk.clone();
        path.extend(own(&chains[best].1).iter().skip(1));
        self.claim(carrier, &path);
        // the other tips branch off where the carrier's drawing passes nearest the merge node
        let point = nearest(&self.planned[carrier..], self.plate, merge, f64::INFINITY).map_or(merge, |(_, p)| p);
        for (i, (tip, chain)) in chains.iter().enumerate() {
            if i == best {
                continue;
            }
            let own = own(chain);
            let mut path = vec![point];
            path.extend(&own[1..]);
            let path = corners(&path);
            let (join, tip_join, _) = fit(&path, tip, &base_joins(&path));
            let column = self.diameter(own.get(1).copied().unwrap_or(tip.b), tip.size_b * 2.0);
            let base = Base::Branch { parent: carrier, point, join };
            self.claim(self.planned.len(), &own);
            self.planned.push(Planned { tip, tip_join, base, column });
        }
    }

    /// The support carrying a tree, and how far its drawing strays from CHITUBOX's path (mm).
    fn carrier(
        &self,
        root: Root,
        parent: Option<usize>,
        trunk: &[DVec3],
        own: &[DVec3],
        tip: &'a Part,
    ) -> (Planned<'a>, f64) {
        // the whole path from the end of the tree up to the tip's lower end
        let mut path: Vec<DVec3> = trunk.to_vec();
        path.extend(own.iter().skip(1));
        if let Root::OnColumn(point) = root {
            path[0] = point; // where the parent's drawing passes
        }
        let bends = corners(&path);
        let column = self.diameter(bends.get(1).copied().unwrap_or(bends[0]), tip.size_b * 2.0);
        match root {
            Root::Foot(foot) => {
                // the column rises from the foot to a bend straight above it
                let above: Vec<DVec3> = bends[1..bends.len() - 1]
                    .iter()
                    .copied()
                    .filter(|p| p.truncate().distance(foot.truncate()) < SAME)
                    .collect();
                let joins = if above.is_empty() { vec![foot] } else { above };
                let (join, tip_join, stray) = fit(&bends, tip, &joins);
                let base = Base::Plate { foot: foot.truncate(), join: join.z };
                (Planned { tip, tip_join, base, column }, stray)
            }
            Root::Model { top, contact, depth, point, diameter } => {
                // the base piece is the bottom tip (a long cone up the trunk would make the trunk thin)
                let mut path = vec![contact];
                path.extend(&bends);
                let path = corners(&path);
                let (join, tip_join, stray) = fit(&path, tip, &[top]);
                let base = Base::Model { contact, depth, point, join };
                (Planned { tip, tip_join, base, column: diameter }, stray)
            }
            Root::OnColumn(point) => {
                let (join, tip_join, stray) = fit(&bends, tip, &base_joins(&bends));
                let base = Base::Branch { parent: parent.unwrap_or(0), point, join };
                (Planned { tip, tip_join, base, column }, stray)
            }
        }
    }
}

/// Among `planned`, the support whose column (or base piece) or middle passes nearest `p`, within `within` mm, and
/// the nearest point of it (indices count from the start of `planned`).
fn nearest(planned: &[Planned], plate: f64, p: DVec3, within: f64) -> Option<(usize, DVec3)> {
    planned
        .iter()
        .enumerate()
        .flat_map(|(i, s)| s.pieces(plate).into_iter().take(2).map(move |piece| (i, piece)))
        .map(|(i, piece)| {
            let q = nearest_on_segment(p, piece);
            (p.distance(q), i, q)
        })
        .filter(|(d, _, _)| *d < within)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, i, q)| (i, q))
}

/// Plan the Lychee supports and braces for an object's CHITUBOX parts. `plate` is the plate's height in the model
/// frame; `ground` finds the model below a point for tips whose path cannot be followed.
pub fn plan<'a>(parts: &'a [Part], plate: f64, ground: Option<&Ground>) -> (Vec<Planned<'a>>, Vec<Brace>) {
    let nodes = Nodes::of(parts);
    let snap = |p: DVec3| nodes.find(p).unwrap_or(p);
    let is_brace = braces_of(parts, &nodes);
    // every column part (not a brace) leads down from its higher end to its lower end
    let mut down: HashMap<[u64; 3], DVec3> = HashMap::new();
    let mut diameter_below: HashMap<[u64; 3], f64> = HashMap::new();
    let mut columns: Vec<(DVec3, DVec3)> = Vec::new();
    for (part, brace) in parts.iter().zip(&is_brace) {
        if part.kind == COLUMN && !brace {
            let (a, b) = (snap(part.a), snap(part.b));
            let (hi, lo) = if a.z >= b.z { (a, b) } else { (b, a) };
            let lower = down.entry(key(hi)).or_insert(lo);
            if lo.z <= lower.z {
                *lower = lo;
                diameter_below.insert(key(hi), part.size_a + part.size_b);
            }
            columns.push((hi, lo));
        }
    }
    let mut roots: HashMap<[u64; 3], Root> = HashMap::new();
    for part in parts {
        match part.kind {
            FOOT_CONE => {
                roots.insert(key(snap(part.a)), Root::Foot(snap(part.a)));
            }
            BOTTOM_TIP => {
                let root = Root::Model {
                    top: snap(part.b),
                    contact: part.a,
                    depth: part.depth_a,
                    point: part.size_a * 2.0,
                    diameter: part.size_b * 2.0,
                };
                roots.insert(key(snap(part.b)), root);
            }
            _ => {}
        }
    }
    // a short column standing straight on its pad, without a foot cone
    for part in parts.iter().filter(|p| p.kind == FOOT_PAD) {
        let top = snap(if part.a.z >= part.b.z { part.a } else { part.b });
        roots.entry(key(top)).or_insert(Root::Foot(top));
    }
    // a column dipping just below the top of its foot cone and back up to it
    for &(hi, lo) in &columns {
        if hi.distance(lo) < 1.0 && !roots.contains_key(&key(lo)) {
            if let Some(&root @ Root::Foot(_)) = roots.get(&key(hi)) {
                roots.insert(key(lo), root);
            }
        }
    }

    // follow a path down its columns until it reaches a foot or a bottom tip, or stops
    let follow = |chain: &mut Vec<DVec3>| {
        while !roots.contains_key(&key(*chain.last().unwrap())) {
            match down.get(&key(*chain.last().unwrap())) {
                Some(&next) if chain.len() < 500 => chain.push(next),
                _ => break,
            }
        }
    };
    let columns = Columns::new(columns);

    // follow every tip down; group the tips by where their path ends
    let mut groups: Vec<([u64; 3], Vec<Chain>)> = Vec::new();
    let mut group_of: HashMap<[u64; 3], usize> = HashMap::new();
    let mut loose = Vec::new();
    for tip in parts.iter().filter(|p| p.kind == TOP_TIP) {
        let mut chain = vec![snap(tip.b)];
        follow(&mut chain);
        let end = *chain.last().unwrap();
        if !roots.contains_key(&key(end)) && columns.landing(end).is_none() {
            loose.push(tip);
            continue;
        }
        let group = *group_of.entry(key(end)).or_insert_with(|| {
            groups.push((key(end), Vec::new()));
            groups.len() - 1
        });
        groups[group].1.push((tip, chain));
    }

    let owner = vec![None; columns.list.len()];
    let mut planner = Planner { planned: Vec::new(), plate, diameter_below, columns, owner };
    // trees on a foot or on the model first, then trees leaning on other trees once those are planned
    let mut waiting: Vec<Vec<Chain>> = Vec::new();
    for (end, chains) in groups {
        match roots.get(&end) {
            Some(&root) => planner.tree(root, chains, None),
            None => waiting.push(chains),
        }
    }
    while !waiting.is_empty() {
        let before = waiting.len();
        let mut still = Vec::new();
        for chains in waiting {
            // lean on the support standing for the column, where its drawing passes nearest the CHITUBOX end
            let end = *chains[0].1.last().unwrap();
            match planner.owner_at(end, true) {
                Some((parent, point)) => planner.tree(Root::OnColumn(point), chains, Some(parent)),
                None => still.push(chains),
            }
        }
        if still.len() == before {
            // Nothing planned carries the columns these lean on: such a column is a trunk with no tip of its own.
            // The highest of them goes on down that trunk to where it stands (the others may then lean on it).
            let highest = (0..still.len()).max_by(|&a, &b| {
                let z = |i: usize| still[i][0].1.last().unwrap().z;
                z(a).total_cmp(&z(b))
            });
            let Some(mut chains) = highest.map(|i| still.swap_remove(i)) else { break };
            let end = *chains[0].1.last().unwrap();
            let Some(column) = planner.columns.landing(end) else { unreachable!("waiting paths end on a column") };
            let lo = planner.columns.list[column].1;
            for (_, chain) in &mut chains {
                chain.push(lo);
                follow(chain);
            }
            let end = *chains[0].1.last().unwrap();
            match roots.get(&key(end)) {
                Some(&root) => planner.tree(root, chains, None),
                None if planner.columns.landing(end).is_some() => still.push(chains),
                None => loose.extend(chains.into_iter().map(|(tip, _)| tip)),
            }
        }
        waiting = still;
    }
    for tip in loose {
        let base = match ground.and_then(|g| g.below(tip.b, 0.05)) {
            Some((point, _)) => Base::Model {
                contact: point,
                depth: 0.0,
                point: tip.size_a * 2.0,
                join: along(point, tip.b, MIN_PIECE),
            },
            None => Base::Plate { foot: tip.b.truncate(), join: tip.b.z },
        };
        planner.planned.push(Planned { tip, tip_join: tip.b, base, column: tip.size_b * 2.0 });
    }

    // braces whose two ends lie on supports
    let braces = parts
        .iter()
        .zip(&is_brace)
        .filter(|(_, brace)| **brace)
        .filter_map(|(part, _)| {
            let (lo, hi) = if part.a.z <= part.b.z { (part.a, part.b) } else { (part.b, part.a) };
            // on the supports standing for the columns the brace ends on, or failing that the nearest ones
            let rest = |p: DVec3| planner.owner_at(p, false).or_else(|| nearest(&planner.planned, plate, p, NEAR));
            let ((from, lo), (to, hi)) = (rest(lo)?, rest(hi)?);
            (from != to).then_some(Brace { lower: (from, lo), upper: (to, hi), diameter: part.size_a * 2.0 })
        })
        .collect();
    (planner.planned, braces)
}

/// How closely the plan follows CHITUBOX: for points every 0.25 mm along each tip and column, the distance to the
/// nearest piece the Lychee supports draw. With `braces` `None` (braces not converted), CHITUBOX's braces are left
/// out of the comparison. Returns (points within `SAME`, all points, largest distance).
pub fn fidelity(parts: &[Part], planned: &[Planned], braces: Option<&[Brace]>, plate: f64) -> (usize, usize, f64) {
    let mut pieces: Vec<(DVec3, DVec3)> = planned.iter().flat_map(|s| s.pieces(plate)).collect();
    pieces.extend(braces.unwrap_or_default().iter().map(|b| (b.lower.1, b.upper.1)));
    let is_brace = braces_of(parts, &Nodes::of(parts));
    let compared = parts
        .iter()
        .zip(&is_brace)
        .filter(|(p, brace)| matches!(p.kind, TOP_TIP | BOTTOM_TIP | COLUMN) && (braces.is_some() || !**brace));
    let (mut close, mut total, mut worst) = (0, 0, 0.0f64);
    for (part, _) in compared {
        let samples = (part.a.distance(part.b) / 0.25).ceil().max(1.0) as usize;
        for k in 0..=samples {
            let p = part.a.lerp(part.b, k as f64 / samples as f64);
            let d = pieces.iter().map(|&piece| distance_to_segment(p, piece)).fold(f64::INFINITY, f64::min);
            total += 1;
            if d < SAME {
                close += 1;
            }
            worst = worst.max(d);
        }
    }
    (close, total, worst)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(kind: u32, a: [f64; 3], b: [f64; 3], size: f64) -> Part {
        Part { kind, a: DVec3::from(a), b: DVec3::from(b), size_a: size, size_b: size, depth_a: 0.0, mesh: (0, 0) }
    }

    #[test]
    fn tips_meeting_above_a_foot_become_a_trunk_and_branches() {
        // two tips meet at (0, 0, 10), straight above the foot at z = -40
        let parts = [
            part(TOP_TIP, [-3.0, 0.0, 16.0], [-3.0, 0.0, 14.0], 0.2),
            part(TOP_TIP, [3.0, 0.0, 16.0], [3.0, 0.0, 14.0], 0.2),
            part(COLUMN, [-3.0, 0.0, 14.0], [0.0, 0.0, 10.0], 0.6),
            part(COLUMN, [3.0, 0.0, 14.0], [0.0, 0.0, 10.0], 0.6),
            part(COLUMN, [0.0, 0.0, 10.0], [0.0, 0.0, -40.0], 0.6),
            part(FOOT_CONE, [0.0, 0.0, -40.0], [0.0, 0.0, -41.0], 1.0),
        ];
        let (planned, braces) = plan(&parts, -41.8, None);
        assert_eq!(planned.len(), 2);
        assert!(braces.is_empty());
        assert_eq!(planned[0].base, Base::Plate { foot: DVec2::ZERO, join: 10.0 });
        assert!((planned[0].column - 1.2).abs() < 1e-9);
        assert!(
            matches!(planned[1].base, Base::Branch { parent: 0, point, .. } if point == DVec3::new(0.0, 0.0, 10.0))
        );
        let (close, total, worst) = fidelity(&parts, &planned, Some(&braces), -41.8);
        assert_eq!(close, total, "every point of the CHITUBOX supports is drawn (worst {worst})");
    }

    #[test]
    fn a_bent_branch_keeps_its_bend() {
        // the second tip's branch leaves the trunk at (0, 0, 10), bends at (2, 0, 12) and rises to its tip
        let parts = [
            part(TOP_TIP, [0.0, 0.0, 22.0], [0.0, 0.0, 20.0], 0.2),
            part(TOP_TIP, [2.0, 0.0, 18.0], [2.0, 0.0, 16.0], 0.2),
            part(COLUMN, [0.0, 0.0, 20.0], [0.0, 0.0, 10.0], 0.6),
            part(COLUMN, [2.0, 0.0, 16.0], [2.0, 0.0, 12.0], 0.6),
            part(COLUMN, [2.0, 0.0, 12.0], [0.0, 0.0, 10.0], 0.6),
            part(COLUMN, [0.0, 0.0, 10.0], [0.0, 0.0, -40.0], 0.6),
            part(FOOT_CONE, [0.0, 0.0, -40.0], [0.0, 0.0, -41.0], 1.0),
        ];
        let (planned, braces) = plan(&parts, -41.8, None);
        assert_eq!(planned.len(), 2);
        let (close, total, worst) = fidelity(&parts, &planned, Some(&braces), -41.8);
        assert_eq!(close, total, "every point of the CHITUBOX supports is drawn (worst {worst})");
    }

    #[test]
    fn a_tree_on_the_model_starts_at_the_bottom_tip() {
        let parts = [
            part(TOP_TIP, [0.0, 0.0, 22.0], [0.0, 0.0, 20.0], 0.2),
            part(COLUMN, [0.0, 0.0, 20.0], [0.0, 0.0, 6.0], 0.6),
            part(BOTTOM_TIP, [1.0, 0.0, 4.0], [0.0, 0.0, 6.0], 0.2),
        ];
        let (planned, braces) = plan(&parts, -41.8, None);
        assert_eq!(planned.len(), 1);
        assert!(matches!(planned[0].base, Base::Model { contact, join, .. }
            if contact == DVec3::new(1.0, 0.0, 4.0) && join == DVec3::new(0.0, 0.0, 6.0)));
        let (close, total, worst) = fidelity(&parts, &planned, Some(&braces), -41.8);
        assert_eq!(close, total, "every point of the CHITUBOX supports is drawn (worst {worst})");
    }

    #[test]
    fn a_tree_leaning_on_another_trunk_becomes_a_branch_of_it() {
        let parts = [
            part(TOP_TIP, [0.0, 0.0, 22.0], [0.0, 0.0, 20.0], 0.2),
            part(COLUMN, [0.0, 0.0, 20.0], [0.0, 0.0, -40.0], 0.6),
            part(FOOT_CONE, [0.0, 0.0, -40.0], [0.0, 0.0, -41.0], 1.0),
            // a tip whose column ends on the trunk above at z = 0 (not at a node of the trunk)
            part(TOP_TIP, [5.0, 0.0, 9.0], [5.0, 0.0, 7.0], 0.2),
            part(COLUMN, [5.0, 0.0, 7.0], [0.0, 0.0, 0.0], 0.6),
        ];
        let (planned, _) = plan(&parts, -41.8, None);
        assert_eq!(planned.len(), 2);
        assert!(matches!(planned[1].base, Base::Branch { parent: 0, point, .. } if point == DVec3::ZERO));
    }

    #[test]
    fn a_branch_on_a_trunk_without_tips_stands_on_its_foot() {
        let parts = [
            // the trunk rises to z = 2 but its only branch joins it at z = 0
            part(COLUMN, [0.0, 0.0, 2.0], [0.0, 0.0, -40.0], 0.6),
            part(FOOT_CONE, [0.0, 0.0, -40.0], [0.0, 0.0, -41.0], 1.0),
            part(TOP_TIP, [5.0, 0.0, 9.0], [5.0, 0.0, 7.0], 0.2),
            part(COLUMN, [5.0, 0.0, 7.0], [0.0, 0.0, 0.0], 0.6),
        ];
        let (planned, _) = plan(&parts, -41.8, None);
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].base, Base::Plate { foot: DVec2::ZERO, join: 0.0 });
        assert_eq!(planned[0].tip_join, DVec3::new(5.0, 0.0, 7.0));
    }

    #[test]
    fn braces_between_trunks_are_kept() {
        let mut parts = Vec::new();
        for x in [0.0, 8.0] {
            parts.push(part(TOP_TIP, [x, 0.0, 22.0], [x, 0.0, 20.0], 0.2));
            parts.push(part(COLUMN, [x, 0.0, 20.0], [x, 0.0, -40.0], 0.6));
            parts.push(part(FOOT_CONE, [x, 0.0, -40.0], [x, 0.0, -41.0], 1.0));
        }
        parts.push(part(COLUMN, [0.0, 0.0, -30.0], [8.0, 0.0, -22.0], 0.4));
        parts.push(part(COLUMN, [0.0, 0.0, -22.0], [8.0, 0.0, -30.0], 0.4));
        let (planned, braces) = plan(&parts, -41.8, None);
        assert_eq!(planned.len(), 2);
        assert_eq!(braces.len(), 2);
        assert!(braces.iter().all(|b| b.lower.1.z < b.upper.1.z && b.lower.0 != b.upper.0));
        let (close, total, _) = fidelity(&parts, &planned, Some(&braces), -41.8);
        assert_eq!(close, total);
        // without braces, the braces are left out of the comparison
        let (close, total, _) = fidelity(&parts, &planned, None, -41.8);
        assert_eq!(close, total);
    }
}
