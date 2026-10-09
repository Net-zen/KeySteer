//! Deterministic overlap planning; storage is carried by the API cache.
use crate::api::geometry::Rect;
use crate::api::presentation::hint_cache::*;
use smallvec::SmallVec;

// Inline capacity is 512 labels (INLINE_LABELS). 128 below is the bit width
// of one u128 row: small batches use a single word, 129..=512 use multiple
// words, and only batches above 512 require dynamic graph storage.
const INLINE_GRAPH_WORDS: usize = INLINE_LABELS * INLINE_LABELS.div_ceil(128);

// Select only the live inline row representation. The dynamic path does not
// zero an unused 32 KiB inline matrix on every Partial. The first real edge
// initializes inline rows; disjoint labels need none. All variants stay inline.
#[allow(clippy::large_enum_variant)]
enum InlineRows {
    Single([u128; 128]),
    Multi([u128; INLINE_GRAPH_WORDS]),
    Unused,
    Uninitialized,
}

struct ConflictGraph {
    // Fixed storage avoids a SmallVec representation check on each edge visit.
    inline_rows: InlineRows,
    inline_degrees: [u16; INLINE_LABELS],
    dynamic_rows: Vec<u64>,
    dynamic_degrees: Vec<GraphRowInfo>,
    len: usize,
    words: usize,
}

impl ConflictGraph {
    #[inline(always)]
    fn new(len: usize) -> Self {
        let words = if len <= INLINE_LABELS {
            0
        } else {
            len.div_ceil(64)
        };
        Self {
            inline_rows: if words != 0 {
                InlineRows::Unused
            } else {
                InlineRows::Uninitialized
            },
            inline_degrees: [0; INLINE_LABELS],
            dynamic_rows: vec![0; len.saturating_mul(words)],
            dynamic_degrees: if words == 0 {
                Vec::new()
            } else {
                vec![GraphRowInfo::default(); len]
            },
            len,
            words,
        }
    }

    #[inline(always)]
    fn new_wide(len: usize, mut rows: Vec<u64>, mut degrees: Vec<GraphRowInfo>) -> Self {
        let words = len.div_ceil(64);
        rows.resize(len.saturating_mul(words), 0);
        rows.fill(0);
        degrees.resize(len, GraphRowInfo::default());
        degrees.fill(GraphRowInfo::default());
        Self {
            inline_rows: InlineRows::Unused,
            inline_degrees: [0; INLINE_LABELS],
            dynamic_rows: rows,
            dynamic_degrees: degrees,
            len,
            words,
        }
    }

    fn recycle(self, wide: &mut WideVisualLayerWorkspace) {
        wide.graph_rows = self.dynamic_rows;
        wide.degrees = self.dynamic_degrees;
    }

    #[cold]
    #[inline(never)]
    fn initialize_inline_rows(&mut self) {
        self.inline_rows = if self.len <= 128 {
            InlineRows::Single([0; 128])
        } else {
            InlineRows::Multi([0; INLINE_GRAPH_WORDS])
        };
    }

    fn add_edge(&mut self, left: usize, right: usize) {
        if matches!(self.inline_rows, InlineRows::Uninitialized) {
            self.initialize_inline_rows();
        }
        match &mut self.inline_rows {
            InlineRows::Uninitialized => unreachable!(),
            InlineRows::Single(rows) => {
                rows[left] |= 1u128 << right;
                rows[right] |= 1u128 << left;
                self.inline_degrees[left] += 1;
                self.inline_degrees[right] += 1;
            }
            InlineRows::Multi(rows) => {
                let words = self.len.div_ceil(128);
                rows[left * words + right / 128] |= 1u128 << (right % 128);
                rows[right * words + left / 128] |= 1u128 << (left % 128);
                self.inline_degrees[left] += 1;
                self.inline_degrees[right] += 1;
            }
            InlineRows::Unused => {
                self.dynamic_rows[left * self.words + right / 64] |= 1u64 << (right % 64);
                self.dynamic_rows[right * self.words + left / 64] |= 1u64 << (left % 64);
                Self::extend_row(&mut self.dynamic_degrees[left], right / 64);
                Self::extend_row(&mut self.dynamic_degrees[right], left / 64);
            }
        }
    }

    fn extend_row(row: &mut GraphRowInfo, word: usize) {
        debug_assert!(word < u16::MAX as usize);
        let word = word as u16;
        if row.degree == 0 {
            row.first_word = word;
            row.end_word = word + 1;
        } else {
            row.first_word = row.first_word.min(word);
            row.end_word = row.end_word.max(word + 1);
        }
        row.degree += 1;
    }

    fn len(&self) -> usize {
        self.len
    }

    fn degree(&self, vertex: usize) -> u16 {
        if self.words == 0 {
            self.inline_degrees[vertex]
        } else {
            self.dynamic_degrees[vertex].degree
        }
    }

    fn for_each_neighbor(&self, vertex: usize, mut visit: impl FnMut(usize)) {
        match &self.inline_rows {
            InlineRows::Uninitialized => {}
            InlineRows::Single(rows) => {
                let mut neighbors = rows[vertex];
                while neighbors != 0 {
                    let neighbor = neighbors.trailing_zeros() as usize;
                    neighbors &= neighbors - 1;
                    visit(neighbor);
                }
            }
            InlineRows::Multi(rows) => {
                let words = self.len.div_ceil(128);
                for (word_index, &word) in rows[vertex * words..(vertex + 1) * words]
                    .iter()
                    .enumerate()
                {
                    let mut neighbors = word;
                    while neighbors != 0 {
                        let neighbor = word_index * 128 + neighbors.trailing_zeros() as usize;
                        neighbors &= neighbors - 1;
                        visit(neighbor);
                    }
                }
            }
            InlineRows::Unused => {
                let info = self.dynamic_degrees[vertex];
                let first = usize::from(info.first_word);
                let end = usize::from(info.end_word);
                let start = vertex * self.words;
                for (offset, &word) in self.dynamic_rows[start + first..start + end]
                    .iter()
                    .enumerate()
                {
                    let mut neighbors = word;
                    while neighbors != 0 {
                        let neighbor = (first + offset) * 64 + neighbors.trailing_zeros() as usize;
                        neighbors &= neighbors - 1;
                        if neighbor < self.len {
                            visit(neighbor);
                        }
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
enum CandidateOrder {
    FrontToBack,
    BackToFront,
    Degree,
    Rows,
    Columns,
}

const CANDIDATE_ORDERS: [CandidateOrder; 5] = [
    CandidateOrder::FrontToBack,
    CandidateOrder::BackToFront,
    CandidateOrder::Degree,
    CandidateOrder::Rows,
    CandidateOrder::Columns,
];

/// Build deterministic global layers without changing label geometry.
///
/// `placements` must follow final draw order and carries the corresponding
/// full Hint index. Disconnected overlap components are colored independently
/// and then share the same global depth numbers.
pub(crate) fn build_visual_layer_plan(
    placements: &[(usize, Rect)],
    hint_count: usize,
    uniform_size: bool,
    visually_stacked: impl Fn(Rect, Rect) -> bool,
    plan: &mut VisualLayerPlan,
) {
    plan.clear();
    if placements.len() < 2 || separated_in_order(placements) {
        plan.finish_unstacked();
        return;
    }

    // The retained label plan tells us whether equal dimensions are possible.
    // Equal code lengths alone do not imply equal final placement or overlap.
    let rect = placements[0].1;
    if uniform_size
        && placements[1..].iter().all(|(_, other)| *other == rect)
        && visually_stacked(rect, rect)
    {
        plan.finish_stacked(placements, hint_count);
        return;
    }

    // Mixed one/two-character plans can still be one complete component.
    // Alignment rejects ordinary layouts cheaply; every real edge is checked
    // before skipping the graph, so differing widths retain exact semantics.
    if !uniform_size && aligned_clique(placements, &visually_stacked) {
        plan.finish_stacked(placements, hint_count);
        return;
    }

    if uniform_size && let Some(depth) = separated_stack_depth(placements, &visually_stacked) {
        plan.finish_separated_stacks(placements, hint_count, depth);
        return;
    }

    build_general_plan(placements, hint_count, visually_stacked, plan);
}

fn aligned_clique(
    placements: &[(usize, Rect)],
    visually_stacked: &impl Fn(Rect, Rect) -> bool,
) -> bool {
    let reference = placements[0].1;
    let mut same_left = true;
    let mut same_right = true;
    let mut same_center = true;
    for (_, rect) in placements {
        if !(rect.x.is_finite()
            && rect.right().is_finite()
            && rect.y.is_finite()
            && rect.bottom().is_finite()
            && rect.width > 0.0
            && rect.height > 0.0
            && rect.y == reference.y
            && rect.height == reference.height)
        {
            return false;
        }
        same_left &= rect.x == reference.x;
        same_right &= rect.right() == reference.right();
        same_center &= rect.center().x == reference.center().x;
        if !(same_left || same_right || same_center) {
            return false;
        }
    }
    for (right, (_, rect)) in placements.iter().enumerate() {
        if placements[..right]
            .iter()
            .any(|(_, left)| !visually_stacked(*left, *rect))
        {
            return false;
        }
    }
    true
}

/// Identical consecutive rectangles form cliques. Only accept them when their
/// distinct rectangles pass the same conservative geometric separation proof.
fn separated_stack_depth(
    placements: &[(usize, Rect)],
    visually_stacked: &impl Fn(Rect, Rect) -> bool,
) -> Option<usize> {
    let groups = || placements.chunk_by(|left, right| left.1 == right.1);
    if !separated_rects_in_order(groups().map(|group| group[0].1)) {
        return None;
    }
    let mut depth = 0;
    for group in groups().filter(|group| group.len() > 1) {
        if !visually_stacked(group[0].1, group[0].1) {
            return None;
        }
        depth = depth.max(group.len());
    }
    Some(depth)
}

/// Prove separation for rectangles arriving left-to-right in vertical bands.
/// Each band starts below all earlier bands. Within it, each rectangle starts
/// right of the preceding ones and cannot reach back into an earlier band.
/// This works with unequal label sizes and does not depend on a batch count.
fn separated_in_order(placements: &[(usize, Rect)]) -> bool {
    separated_rects_in_order(placements.iter().map(|(_, rect)| *rect))
}

fn separated_rects_in_order(rects: impl Iterator<Item = Rect>) -> bool {
    let mut previous_bottom = f64::NEG_INFINITY;
    let mut band_bottom = f64::NEG_INFINITY;
    let mut band_right = f64::NEG_INFINITY;
    for rect in rects {
        let right = rect.right();
        let bottom = rect.bottom();
        if !(rect.x.is_finite()
            && rect.y.is_finite()
            && right.is_finite()
            && bottom.is_finite()
            && rect.width >= 0.0
            && rect.height >= 0.0)
        {
            return false;
        }
        if rect.y >= band_bottom {
            previous_bottom = band_bottom;
            band_bottom = bottom;
        } else if rect.y >= previous_bottom && rect.x >= band_right {
            band_bottom = band_bottom.max(bottom);
        } else {
            return false;
        }
        band_right = right;
    }
    true
}

// Keep graph/coloring workspaces off the proven-disjoint first-frame path.
#[inline(never)]
fn build_general_plan(
    placements: &[(usize, Rect)],
    hint_count: usize,
    visually_stacked: impl Fn(Rect, Rect) -> bool,
    plan: &mut VisualLayerPlan,
) {
    if placements.len() > INLINE_LABELS {
        let mut wide = plan.wide.take().unwrap_or_default();
        build_wide_plan(placements, hint_count, &visually_stacked, plan, &mut wide);
        plan.wide = Some(wide);
        return;
    }

    let mut graph = ConflictGraph::new(placements.len());
    let swept =
        placements.len() >= 16 && inline_sweep_edges(placements, &visually_stacked, &mut graph);
    if !swept {
        for right in 1..placements.len() {
            for left in 0..right {
                if visually_stacked(placements[left].1, placements[right].1) {
                    graph.add_edge(left, right);
                }
            }
        }
    }

    if graph.inline_degrees[..placements.len()]
        .iter()
        .all(|&degree| degree == 0)
    {
        plan.finish_unstacked();
        return;
    }

    let mut visited = [false; INLINE_LABELS];
    let mut packed_component_layers = [WIDE_UNSTACKED; INLINE_LABELS];
    let mut global_layer_count = 0usize;
    let mut component = [0; INLINE_LABELS];
    let mut pending = [0; INLINE_LABELS];
    let mut coloring = InlineColorWorkspace::default();

    for root in 0..graph.len() {
        if visited[root] || graph.degree(root) == 0 {
            visited[root] = true;
            continue;
        }
        let mut component_len = 0;
        let mut pending_len = 1;
        visited[root] = true;
        pending[0] = root;
        while pending_len > 0 {
            pending_len -= 1;
            let vertex = pending[pending_len];
            component[component_len] = vertex;
            component_len += 1;
            graph.for_each_neighbor(vertex, |neighbor| {
                if !visited[neighbor] {
                    visited[neighbor] = true;
                    pending[pending_len] = neighbor;
                    pending_len += 1;
                }
            });
        }
        let component = &mut component[..component_len];
        component.sort_unstable();

        // In a clique every label needs its own layer. Canonicalization orders
        // those layers front-to-back regardless of the coloring candidate.
        if component
            .iter()
            .all(|&vertex| usize::from(graph.degree(vertex)) == component.len() - 1)
        {
            for (layer, &vertex) in component.iter().rev().enumerate() {
                packed_component_layers[vertex] =
                    ((component.len() as u32) << u16::BITS) | layer as u32;
            }
            global_layer_count = global_layer_count.max(component.len());
            continue;
        }

        let layer_count = color_component_inline(&graph, placements, component, &mut coloring);
        global_layer_count = global_layer_count.max(layer_count);
        for &vertex in component.iter() {
            packed_component_layers[vertex] =
                ((layer_count as u32) << u16::BITS) | u32::from(coloring.best[vertex]);
        }
    }

    plan.finish(
        placements,
        hint_count,
        &packed_component_layers[..placements.len()],
        global_layer_count,
    );
}

/// Use the same X-axis rejection as the wide plan, with inline 16-bit indices (including index 511).
/// Invalid geometry keeps the exhaustive predicate path unchanged.
fn inline_sweep_edges(
    placements: &[(usize, Rect)],
    visually_stacked: &impl Fn(Rect, Rect) -> bool,
    graph: &mut ConflictGraph,
) -> bool {
    if !placements.iter().all(|(_, rect)| {
        rect.x.is_finite()
            && rect.y.is_finite()
            && rect.width.is_finite()
            && rect.height.is_finite()
            && rect.width >= 0.0
            && rect.height >= 0.0
    }) {
        return false;
    }
    debug_assert!(placements.len() <= INLINE_LABELS);
    let mut order = [0u16; INLINE_LABELS];
    let order = &mut order[..placements.len()];
    for (i, slot) in order.iter_mut().enumerate() {
        *slot = i as u16;
    }
    order.sort_unstable_by(|left, right| {
        placements[usize::from(*left)]
            .1
            .x
            .total_cmp(&placements[usize::from(*right)].1.x)
            .then_with(|| left.cmp(right))
    });
    let mut active = [0u16; INLINE_LABELS];
    let mut active_len = 0;
    for &right in order.iter() {
        let right_rect = placements[usize::from(right)].1;
        let mut kept = 0;
        for i in 0..active_len {
            let left = active[i];
            let left_rect = placements[usize::from(left)].1;
            if left_rect.right() >= right_rect.x {
                active[kept] = left;
                kept += 1;
                if left_rect.intersect(&right_rect).is_some()
                    && visually_stacked(left_rect, right_rect)
                {
                    graph.add_edge(usize::from(left), usize::from(right));
                }
            }
        }
        active[kept] = right;
        active_len = kept + 1;
    }
    true
}

fn build_wide_plan(
    placements: &[(usize, Rect)],
    hint_count: usize,
    visually_stacked: &impl Fn(Rect, Rect) -> bool,
    plan: &mut VisualLayerPlan,
    wide: &mut WideVisualLayerWorkspace,
) {
    let len = placements.len();
    let valid_sweep = prepare_wide_sweep(placements, &mut wide.sweep_order);
    if valid_sweep
        && !wide_sweep_has_edge(
            placements,
            visually_stacked,
            &wide.sweep_order,
            &mut wide.sweep_active,
        )
    {
        plan.finish_unstacked();
        return;
    }
    let mut graph = ConflictGraph::new_wide(
        len,
        std::mem::take(&mut wide.graph_rows),
        std::mem::take(&mut wide.degrees),
    );
    build_wide_edges(
        placements,
        visually_stacked,
        &mut graph,
        &wide.sweep_order,
        &mut wide.sweep_active,
        valid_sweep,
    );
    wide.visited.resize(len, false);
    wide.visited.fill(false);
    wide.packed.resize(len, WIDE_UNSTACKED);
    wide.packed.fill(WIDE_UNSTACKED);
    wide.best.resize(len, UNCOLORED);
    wide.colors.resize(len, UNCOLORED);
    wide.component.clear();
    wide.pending.clear();
    wide.order.clear();
    let mut global = 0usize;
    for root in 0..len {
        if wide.visited[root] || graph.degree(root) == 0 {
            wide.visited[root] = true;
            continue;
        }
        wide.component.clear();
        wide.pending.clear();
        wide.visited[root] = true;
        wide.pending.push(root);
        while let Some(vertex) = wide.pending.pop() {
            wide.component.push(vertex);
            graph.for_each_neighbor(vertex, |neighbor| {
                if !wide.visited[neighbor] {
                    wide.visited[neighbor] = true;
                    wide.pending.push(neighbor);
                }
            });
        }
        wide.component.sort_unstable();
        let depth = if wide
            .component
            .iter()
            .all(|&vertex| usize::from(graph.degree(vertex)) == wide.component.len() - 1)
        {
            for (layer, &vertex) in wide.component.iter().rev().enumerate() {
                wide.best[vertex] = layer as u16;
            }
            wide.component.len()
        } else {
            color_component_wide(&graph, placements, wide)
        };
        global = global.max(depth);
        for &vertex in &wide.component {
            wide.packed[vertex] = (depth as u32) << u16::BITS | u32::from(wide.best[vertex]);
        }
    }
    plan.finish(placements, hint_count, &wide.packed, global);
    graph.recycle(wide);
}

fn build_wide_edges(
    placements: &[(usize, Rect)],
    visually_stacked: &impl Fn(Rect, Rect) -> bool,
    graph: &mut ConflictGraph,
    order: &[usize],
    active: &mut Vec<usize>,
    prepared_sweep: bool,
) {
    if !prepared_sweep {
        for right in 1..placements.len() {
            for left in 0..right {
                if visually_stacked(placements[left].1, placements[right].1) {
                    graph.add_edge(left, right);
                }
            }
        }
        return;
    }
    active.clear();
    for &right in order.iter() {
        let right_rect = placements[right].1;
        active.retain(|left| placements[*left].1.right() >= right_rect.x);
        for &left in active.iter() {
            let left_rect = placements[left].1;
            if left_rect.intersect(&right_rect).is_some() && visually_stacked(left_rect, right_rect)
            {
                graph.add_edge(left, right);
            }
        }
        active.push(right);
    }
}

fn prepare_wide_sweep(placements: &[(usize, Rect)], order: &mut Vec<usize>) -> bool {
    let valid = placements.iter().all(|(_, rect)| {
        rect.x.is_finite()
            && rect.y.is_finite()
            && rect.width.is_finite()
            && rect.height.is_finite()
            && rect.width >= 0.0
            && rect.height >= 0.0
    });
    if !valid {
        return false;
    }
    order.clear();
    order.extend(0..placements.len());
    order.sort_unstable_by(|left, right| {
        placements[*left]
            .1
            .x
            .total_cmp(&placements[*right].1.x)
            .then_with(|| left.cmp(right))
    });
    true
}

fn wide_sweep_has_edge(
    placements: &[(usize, Rect)],
    visually_stacked: &impl Fn(Rect, Rect) -> bool,
    order: &[usize],
    active: &mut Vec<usize>,
) -> bool {
    active.clear();
    if let [left, right, ..] = order {
        let left_rect = placements[*left].1;
        let right_rect = placements[*right].1;
        if left_rect.intersect(&right_rect).is_some() && visually_stacked(left_rect, right_rect) {
            return true;
        }
    }
    let mut next_expiry = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_bottom = f64::NEG_INFINITY;
    for &right in order.iter() {
        let right_rect = placements[right].1;
        // Nothing can expire until the sweep passes the earliest right edge.
        if next_expiry < right_rect.x {
            next_expiry = f64::INFINITY;
            min_y = f64::INFINITY;
            max_bottom = f64::NEG_INFINITY;
            active.retain(|left| {
                let rect = placements[*left].1;
                let end = rect.right();
                if end < right_rect.x {
                    false
                } else {
                    next_expiry = next_expiry.min(end);
                    min_y = min_y.min(rect.y);
                    max_bottom = max_bottom.max(rect.bottom());
                    true
                }
            });
        }
        // The union of active vertical intervals is a conservative bound.
        // Only a rectangle reaching it can conflict with an active label.
        if right_rect.y < max_bottom && right_rect.bottom() > min_y {
            for &left in active.iter() {
                let left_rect = placements[left].1;
                if left_rect.intersect(&right_rect).is_some()
                    && visually_stacked(left_rect, right_rect)
                {
                    return true;
                }
            }
        }
        active.push(right);
        next_expiry = next_expiry.min(right_rect.right());
        min_y = min_y.min(right_rect.y);
        max_bottom = max_bottom.max(right_rect.bottom());
    }
    false
}

fn color_component_wide(
    graph: &ConflictGraph,
    placements: &[(usize, Rect)],
    wide: &mut WideVisualLayerWorkspace,
) -> usize {
    let component = wide.component.as_slice();
    for &vertex in component {
        wide.best[vertex] = UNCOLORED;
        wide.colors[vertex] = UNCOLORED;
    }
    wide.occupied.resize(component.len().div_ceil(64), 0);
    let mut best_depth = usize::MAX;
    let mut best_agreement = 0usize;
    for candidate in CANDIDATE_ORDERS {
        wide.order.clear();
        wide.order.extend_from_slice(component);
        prepare_order_slice(candidate, placements, graph, &mut wide.order);
        for &vertex in component {
            wide.colors[vertex] = UNCOLORED;
        }
        greedy_color(graph, &wide.order, &mut wide.colors, &mut wide.occupied);
        for _ in 0..2 {
            compact_colors(graph, component, &mut wide.colors, &mut wide.occupied);
        }
        let depth = canonicalize_colors_wide(
            component,
            &mut wide.colors,
            &mut wide.frontmost,
            &mut wide.classes,
            &mut wide.remap,
        );
        let agreement = visual_agreement(graph, component, &wide.colors);
        let better = depth < best_depth
            || (depth == best_depth
                && (agreement > best_agreement
                    || (agreement == best_agreement
                        && lexicographically_better(component, &wide.colors, &wide.best))));
        if better {
            best_depth = depth;
            best_agreement = agreement;
            for &vertex in component {
                wide.best[vertex] = wide.colors[vertex];
            }
        }
    }
    best_depth
}

fn canonicalize_colors_wide(
    component: &[usize],
    colors: &mut [u16],
    frontmost: &mut Vec<usize>,
    classes: &mut Vec<(u16, usize)>,
    remap: &mut Vec<u16>,
) -> usize {
    let count = component
        .iter()
        .map(|vertex| usize::from(colors[*vertex]))
        .max()
        .map_or(0, |color| color + 1);
    frontmost.resize(count, usize::MAX);
    frontmost.fill(usize::MAX);
    for &vertex in component {
        let slot = &mut frontmost[usize::from(colors[vertex])];
        *slot = if *slot == usize::MAX {
            vertex
        } else {
            (*slot).max(vertex)
        };
    }
    classes.clear();
    classes.extend(
        frontmost
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, front)| *front != usize::MAX)
            .map(|(color, front)| (color as u16, front)),
    );
    classes.sort_unstable_by(|(lc, lf), (rc, rf)| rf.cmp(lf).then_with(|| lc.cmp(rc)));
    remap.resize(count, UNCOLORED);
    remap.fill(UNCOLORED);
    for (new, (old, _)) in classes.iter().enumerate() {
        remap[usize::from(*old)] = new as u16;
    }
    for &vertex in component {
        colors[vertex] = remap[usize::from(colors[vertex])];
    }
    classes.len()
}

#[derive(Default)]
struct InlineColorWorkspace {
    best: SmallVec<[u16; INLINE_LABELS]>,
    colors: SmallVec<[u16; INLINE_LABELS]>,
    order: SmallVec<[usize; INLINE_LABELS]>,
    occupied: SmallVec<[u64; INLINE_LABELS.div_ceil(64)]>,
    frontmost: SmallVec<[Option<usize>; INLINE_LABELS]>,
    classes: SmallVec<[(u16, usize); INLINE_LABELS]>,
    remap: SmallVec<[u16; INLINE_LABELS]>,
}

#[cfg(test)]
fn color_component(
    graph: &ConflictGraph,
    placements: &[(usize, Rect)],
    component: &[usize],
) -> (SmallVec<[u16; INLINE_LABELS]>, usize) {
    let mut workspace = InlineColorWorkspace::default();
    let depth = color_component_inline(graph, placements, component, &mut workspace);
    (workspace.best, depth)
}

fn color_component_inline(
    graph: &ConflictGraph,
    placements: &[(usize, Rect)],
    component: &[usize],
    w: &mut InlineColorWorkspace,
) -> usize {
    // Initialize each scan-sized array only once; later disconnected groups
    // reset their own vertices, just like the dynamic workspace does.
    w.best.resize(graph.len(), UNCOLORED);
    w.colors.resize(graph.len(), UNCOLORED);
    w.occupied.resize(component.len().div_ceil(64), 0);
    let mut best_layer_count = usize::MAX;
    let mut best_agreement = 0usize;
    for candidate in CANDIDATE_ORDERS {
        prepare_order(candidate, placements, graph, component, &mut w.order);
        for &vertex in component {
            w.colors[vertex] = UNCOLORED;
        }
        greedy_color(graph, &w.order, &mut w.colors, &mut w.occupied);
        for _ in 0..2 {
            compact_colors(graph, component, &mut w.colors, &mut w.occupied);
        }
        let layer_count = canonicalize_colors(
            component,
            &mut w.colors,
            &mut w.frontmost,
            &mut w.classes,
            &mut w.remap,
        );
        let agreement = visual_agreement(graph, component, &w.colors);
        let better = layer_count < best_layer_count
            || (layer_count == best_layer_count
                && (agreement > best_agreement
                    || (agreement == best_agreement
                        && lexicographically_better(component, &w.colors, &w.best))));
        if better {
            best_layer_count = layer_count;
            best_agreement = agreement;
            for &vertex in component {
                w.best[vertex] = w.colors[vertex];
            }
        }
    }
    best_layer_count
}

fn prepare_order(
    candidate: CandidateOrder,
    placements: &[(usize, Rect)],
    graph: &ConflictGraph,
    component: &[usize],
    order: &mut SmallVec<[usize; INLINE_LABELS]>,
) {
    order.clear();
    order.extend_from_slice(component);
    prepare_order_slice(candidate, placements, graph, order);
}

fn prepare_order_slice(
    candidate: CandidateOrder,
    placements: &[(usize, Rect)],
    graph: &ConflictGraph,
    order: &mut [usize],
) {
    match candidate {
        CandidateOrder::FrontToBack => order.reverse(),
        CandidateOrder::BackToFront => {}
        CandidateOrder::Degree => order.sort_unstable_by(|left, right| {
            graph
                .degree(*right)
                .cmp(&graph.degree(*left))
                .then_with(|| right.cmp(left))
        }),
        CandidateOrder::Rows => order.sort_unstable_by(|left, right| {
            let left_rect = placements[*left].1;
            let right_rect = placements[*right].1;
            left_rect
                .y
                .total_cmp(&right_rect.y)
                .then_with(|| left_rect.x.total_cmp(&right_rect.x))
                .then_with(|| left.cmp(right))
        }),
        CandidateOrder::Columns => order.sort_unstable_by(|left, right| {
            let left_rect = placements[*left].1;
            let right_rect = placements[*right].1;
            left_rect
                .x
                .total_cmp(&right_rect.x)
                .then_with(|| left_rect.y.total_cmp(&right_rect.y))
                .then_with(|| left.cmp(right))
        }),
    }
}

fn greedy_color(graph: &ConflictGraph, order: &[usize], colors: &mut [u16], occupied: &mut [u64]) {
    for &vertex in order {
        occupied.fill(0);
        graph.for_each_neighbor(vertex, |neighbor| {
            let color = colors[neighbor];
            if color != UNCOLORED {
                let color = usize::from(color);
                occupied[color / 64] |= 1u64 << (color % 64);
            }
        });
        colors[vertex] = first_free_color(occupied) as u16;
    }
}

fn compact_colors(
    graph: &ConflictGraph,
    component: &[usize],
    colors: &mut [u16],
    occupied: &mut [u64],
) {
    for &vertex in component.iter().rev() {
        occupied.fill(0);
        graph.for_each_neighbor(vertex, |neighbor| {
            let color = colors[neighbor];
            if color != UNCOLORED {
                let color = usize::from(color);
                occupied[color / 64] |= 1u64 << (color % 64);
            }
        });
        let first_free = first_free_color(occupied) as u16;
        if first_free < colors[vertex] {
            colors[vertex] = first_free;
        }
    }
}

fn first_free_color(occupied: &[u64]) -> usize {
    occupied
        .iter()
        .enumerate()
        .find_map(|(word_index, word)| {
            (*word != u64::MAX).then(|| word_index * 64 + (!word).trailing_zeros() as usize)
        })
        .unwrap_or(occupied.len() * 64)
}

fn canonicalize_colors(
    component: &[usize],
    colors: &mut [u16],
    frontmost: &mut SmallVec<[Option<usize>; INLINE_LABELS]>,
    classes: &mut SmallVec<[(u16, usize); INLINE_LABELS]>,
    remap: &mut SmallVec<[u16; INLINE_LABELS]>,
) -> usize {
    let max_color = component
        .iter()
        .map(|vertex| colors[*vertex])
        .max()
        .unwrap_or(0) as usize;
    frontmost.resize(max_color + 1, None);
    frontmost.fill(None);
    for &vertex in component {
        let entry = &mut frontmost[usize::from(colors[vertex])];
        *entry = Some(entry.map_or(vertex, |current| current.max(vertex)));
    }
    classes.clear();
    classes.extend(
        frontmost
            .iter()
            .enumerate()
            .filter_map(|(color, front)| front.map(|front| (color as u16, front))),
    );
    classes.sort_unstable_by(|(left_color, left_front), (right_color, right_front)| {
        right_front
            .cmp(left_front)
            .then_with(|| left_color.cmp(right_color))
    });
    remap.resize(max_color + 1, UNCOLORED);
    for (new_color, (old_color, _)) in classes.iter().enumerate() {
        remap[usize::from(*old_color)] = new_color as u16;
    }
    for &vertex in component {
        colors[vertex] = remap[usize::from(colors[vertex])];
    }
    classes.len()
}

fn visual_agreement(graph: &ConflictGraph, component: &[usize], colors: &[u16]) -> usize {
    let mut agreement = 0usize;
    for &back in component {
        graph.for_each_neighbor(back, |front| {
            if front > back && colors[front] < colors[back] {
                agreement += 1;
            }
        });
    }
    agreement
}

fn lexicographically_better(component: &[usize], candidate: &[u16], current: &[u16]) -> bool {
    component
        .iter()
        .map(|vertex| candidate[*vertex])
        .cmp(component.iter().map(|vertex| current[*vertex]))
        .is_lt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_visual_layer_plan(
        placements: &[(usize, Rect)],
        hint_count: usize,
        visually_stacked: impl Fn(Rect, Rect) -> bool,
        plan: &mut VisualLayerPlan,
    ) {
        super::build_visual_layer_plan(placements, hint_count, true, visually_stacked, plan);
    }

    fn overlap(left: Rect, right: Rect) -> bool {
        left.intersect(&right).is_some_and(|intersection| {
            let area = intersection.width * intersection.height;
            let smaller = (left.width * left.height).min(right.width * right.height);
            smaller > 0.0 && area >= smaller * 0.20
        })
    }

    fn quadratic_reference(placements: &[(usize, Rect)], plan: &mut VisualLayerPlan) {
        plan.clear();
        let mut graph = ConflictGraph::new(placements.len());
        for right in 1..placements.len() {
            for left in 0..right {
                if overlap(placements[left].1, placements[right].1) {
                    graph.add_edge(left, right);
                }
            }
        }
        let mut visited = vec![false; graph.len()];
        let mut packed = vec![WIDE_UNSTACKED; graph.len()];
        let mut global = 0usize;
        for root in 0..graph.len() {
            if visited[root] || graph.degree(root) == 0 {
                visited[root] = true;
                continue;
            }
            let mut component = Vec::new();
            let mut pending = vec![root];
            visited[root] = true;
            while let Some(vertex) = pending.pop() {
                component.push(vertex);
                graph.for_each_neighbor(vertex, |neighbor| {
                    if !visited[neighbor] {
                        visited[neighbor] = true;
                        pending.push(neighbor);
                    }
                });
            }
            component.sort_unstable();
            let (colors, depth) = color_component(&graph, placements, &component);
            global = global.max(depth);
            for &vertex in &component {
                packed[vertex] = (depth as u32) << u16::BITS | u32::from(colors[vertex]);
            }
        }
        plan.finish(placements, placements.len(), &packed, global);
    }

    fn assert_matches_quadratic_reference(placements: &[(usize, Rect)]) {
        let mut optimized = VisualLayerPlan::default();
        build_visual_layer_plan(placements, placements.len(), overlap, &mut optimized);
        let mut reference = VisualLayerPlan::default();
        quadratic_reference(placements, &mut reference);

        assert_eq!(optimized.layer_count(), reference.layer_count());
        assert!(optimized.is_ready());
        for hint_index in 0..placements.len() {
            assert_eq!(
                optimized.layer_info(hint_index),
                reference.layer_info(hint_index),
                "hint {hint_index}"
            );
            for selected_layer in 0..=optimized.layer_count().saturating_add(1) {
                assert_eq!(
                    optimized.draw_rank(hint_index, selected_layer),
                    reference.draw_rank(hint_index, selected_layer),
                    "hint {hint_index}, selected layer {selected_layer}"
                );
            }
        }
    }

    #[test]
    fn mixed_width_cliques_verify_all_edges_before_direct_layer_assignment() {
        for align in 0..3 {
            let placements = (0..10)
                .map(|i| {
                    let width = 10.0 + (i % 3) as f64 * 10.0;
                    let x = match align {
                        0 => 0.0,
                        1 => 30.0 - width,
                        _ => 15.0 - width / 2.0,
                    };
                    (i, Rect::new(x, 0.0, width, 20.0))
                })
                .collect::<Vec<_>>();
            assert!(aligned_clique(&placements, &overlap));
            let mut plan = VisualLayerPlan::default();
            super::build_visual_layer_plan(&placements, 10, false, overlap, &mut plan);
            let mut reference = VisualLayerPlan::default();
            quadratic_reference(&placements, &mut reference);
            for index in 0..10 {
                assert_eq!(plan.layer_info(index), reference.layer_info(index));
                for layer in 0..=10 {
                    assert_eq!(
                        plan.draw_rank(index, layer),
                        reference.draw_rank(index, layer)
                    );
                }
            }
            // Alignment alone cannot imply the caller's overlap predicate.
            assert!(!aligned_clique(&placements, &|left, right| {
                left.width == right.width && overlap(left, right)
            }));
        }
        let mixed_alignment = [
            (0, Rect::new(0.0, 0.0, 100.0, 20.0)),
            (1, Rect::new(0.0, 0.0, 10.0, 20.0)),
            (2, Rect::new(90.0, 0.0, 10.0, 20.0)),
        ];
        assert!(!aligned_clique(&mixed_alignment, &overlap));
    }

    #[test]
    fn separated_stacks_keep_component_depths_and_canonical_draw_order() {
        for sizes in [&[1, 2, 3, 4, 9][..], &[1, 2, 254, 255][..]] {
            let mut placements = Vec::new();
            for (group, &size) in sizes.iter().enumerate() {
                let rect = Rect::new(group as f64 * 100.0, 0.0, 20.0, 20.0);
                for _ in 0..size {
                    placements.push((placements.len(), rect));
                }
            }
            assert_eq!(
                separated_stack_depth(&placements, &overlap),
                sizes.iter().copied().max()
            );
            assert_matches_quadratic_reference(&placements);
            // Moving a distinct run into another one must use the full graph.
            placements[0].1.x = 95.0;
            assert!(separated_stack_depth(&placements, &overlap).is_none());
            assert_matches_quadratic_reference(&placements);
        }

        for count in [2, 24, 64, 128, 512, 513, 2_000] {
            let placements = (0..count)
                .map(|i| {
                    let group = i / 2;
                    (
                        i * 2,
                        Rect::new(
                            (group % 37) as f64 * 50.0,
                            (group / 37) as f64 * 30.0,
                            20.0,
                            20.0,
                        ),
                    )
                })
                .collect::<Vec<_>>();
            let mut plan = VisualLayerPlan::default();
            build_visual_layer_plan(&placements, count * 2, overlap, &mut plan);
            assert_eq!(plan.layer_count(), 2);
            assert!(plan.wide.is_none(), "no graph workspace for {count} labels");
            for i in 0..count {
                assert_eq!(plan.layer_info(i * 2 + 1), None);
                assert_eq!(
                    plan.layer_info(i * 2),
                    if i + 1 == count && count % 2 == 1 {
                        None
                    } else {
                        Some((1 - i % 2, 2))
                    }
                );
            }
        }
    }

    #[test]
    fn ordered_separation_is_conservative_for_rows_heights_and_input_order() {
        let rows = [
            (0, Rect::new(-20.0, -10.0, 10.0, 20.0)),
            (1, Rect::new(-10.0, -5.0, 30.0, 10.0)),
            (2, Rect::new(20.0, -8.0, 5.0, 25.0)),
            (3, Rect::new(-30.0, 17.0, 10.0, 12.0)),
        ];
        assert!(separated_in_order(&rows));
        assert_matches_quadratic_reference(&rows);
        let mut crossing = rows;
        // This rightmost rectangle reaches back into the preceding band.
        crossing[3] = (3, Rect::new(19.0, 16.0, 10.0, 12.0));
        assert!(!separated_in_order(&crossing));
        assert_matches_quadratic_reference(&crossing);

        let mut seed = 17u64;
        for _ in 0..20_000 {
            let placements: Vec<_> = (0..6)
                .map(|index| {
                    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                    (
                        index,
                        Rect::new(
                            ((seed >> 32) % 17) as f64 - 8.0,
                            ((seed >> 24) % 17) as f64 - 8.0,
                            ((seed >> 16) % 7) as f64,
                            ((seed >> 8) % 7) as f64,
                        ),
                    )
                })
                .collect();
            if separated_in_order(&placements) {
                for (right, (_, rect)) in placements.iter().enumerate() {
                    assert!(
                        placements[..right]
                            .iter()
                            .all(|(_, left)| left.intersect(rect).is_none())
                    );
                }
            }
        }
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
            let mut invalid = rows;
            invalid[1].1.width = bad;
            assert!(!separated_in_order(&invalid));
            assert_matches_quadratic_reference(&invalid);
        }
    }

    #[test]
    fn disjoint_plan_resets_dense_layers_and_preserves_filtered_hint_indices() {
        let dense = (0..300)
            .map(|i| (i, Rect::new(0.0, 0.0, 20.0, 20.0)))
            .collect::<Vec<_>>();
        let mut plan = VisualLayerPlan::default();
        build_visual_layer_plan(&dense, 300, overlap, &mut plan);
        assert_eq!(plan.layer_count(), 300);
        for count in [0, 1, 9, 64, 128, 241, 512, 513, 2_000] {
            let placements = (0..count)
                .map(|i| {
                    (
                        i * 2,
                        Rect::new(
                            (i % 37) as f64 * 50.0,
                            (i / 37) as f64 * 30.0,
                            20.0 + (i % 3) as f64,
                            20.0,
                        ),
                    )
                })
                .collect::<Vec<_>>();
            super::build_visual_layer_plan(
                &placements,
                count * 2,
                false,
                |_, _| panic!("disjoint geometry should not build edges"),
                &mut plan,
            );
            assert!(plan.is_ready());
            assert_eq!(plan.len(), 0);
            assert_eq!(plan.layer_count(), 0);
            assert!(plan.wide.is_none());
            for i in 0..count * 2 {
                assert_eq!(plan.layer_info(i), None);
            }
        }
    }

    #[test]
    fn ten_thousand_dynamic_rows_skip_empty_words_and_reset_on_reuse() {
        let mut graph = ConflictGraph::new_wide(10_000, Vec::new(), Vec::new());
        for (a, b) in [(0, 9999), (0, 127), (0, 129), (4999, 5000), (9998, 9999)] {
            graph.add_edge(a, b);
        }
        for (vertex, expected) in [
            (0, vec![127, 129, 9999]),
            (127, vec![0]),
            (4999, vec![5000]),
            (9999, vec![0, 9998]),
            (1234, vec![]),
        ] {
            let mut actual = Vec::new();
            graph.for_each_neighbor(vertex, |v| actual.push(v));
            assert_eq!(actual, expected);
            assert_eq!(usize::from(graph.degree(vertex)), expected.len());
        }
        let mut wide = WideVisualLayerWorkspace::default();
        graph.recycle(&mut wide);
        let mut reused = ConflictGraph::new_wide(513, wide.graph_rows, wide.degrees);
        reused.add_edge(128, 512);
        let mut actual = Vec::new();
        reused.for_each_neighbor(512, |v| actual.push(v));
        assert_eq!(actual, vec![128]);
        reused.for_each_neighbor(0, |_| panic!("stale word interval"));
        assert_eq!(reused.degree(0), 0);
    }

    #[test]
    fn path_counterexample_uses_two_layers_in_every_draw_order() {
        let source = [
            Rect::new(0.0, 0.0, 20.0, 20.0),
            Rect::new(15.0, 0.0, 20.0, 20.0),
            Rect::new(30.0, 0.0, 20.0, 20.0),
            Rect::new(45.0, 0.0, 20.0, 20.0),
        ];
        let permutations = [
            [0, 1, 2, 3],
            [0, 1, 3, 2],
            [0, 2, 1, 3],
            [0, 2, 3, 1],
            [0, 3, 1, 2],
            [0, 3, 2, 1],
            [1, 0, 2, 3],
            [1, 0, 3, 2],
            [1, 2, 0, 3],
            [1, 2, 3, 0],
            [1, 3, 0, 2],
            [1, 3, 2, 0],
            [2, 0, 1, 3],
            [2, 0, 3, 1],
            [2, 1, 0, 3],
            [2, 1, 3, 0],
            [2, 3, 0, 1],
            [2, 3, 1, 0],
            [3, 0, 1, 2],
            [3, 0, 2, 1],
            [3, 1, 0, 2],
            [3, 1, 2, 0],
            [3, 2, 0, 1],
            [3, 2, 1, 0],
        ];
        for permutation in permutations {
            let placements: Vec<_> = permutation
                .into_iter()
                .enumerate()
                .map(|(draw_index, source_index)| (draw_index, source[source_index]))
                .collect();
            let mut plan = VisualLayerPlan::default();
            build_visual_layer_plan(&placements, placements.len(), overlap, &mut plan);
            assert_eq!(plan.layer_count(), 2, "permutation {permutation:?}");
        }
    }

    #[test]
    fn plan_is_indexed_by_full_hint_list() {
        let placements = [
            (1, Rect::new(0.0, 0.0, 20.0, 20.0)),
            (3, Rect::new(0.0, 0.0, 20.0, 20.0)),
        ];
        let mut plan = VisualLayerPlan::default();
        build_visual_layer_plan(&placements, 5, overlap, &mut plan);

        assert_eq!(plan.len(), 5);
        assert_eq!(plan.layer(0), None);
        assert_eq!(plan.layer(2), None);
        assert!(plan.layer(1).is_some());
        assert!(plan.layer(3).is_some());
        assert_eq!(plan.component_layer_count(1), Some(2));
        assert_eq!(plan.component_layer_count(3), Some(2));
        assert_eq!(plan.component_layer_count(4), None);

        // Equal geometry is not sufficient when the conflict predicate rejects it.
        build_visual_layer_plan(&placements, 5, |_, _| false, &mut plan);
        assert_eq!(plan.layer_count(), 0);
        for index in 0..5 {
            assert_eq!(plan.layer_info(index), None);
        }
    }

    #[test]
    fn global_selection_wraps_each_components_non_default_layers() {
        let placements = [
            (0, Rect::new(0.0, 0.0, 20.0, 20.0)),
            (1, Rect::new(0.0, 0.0, 20.0, 20.0)),
            (2, Rect::new(100.0, 0.0, 20.0, 20.0)),
            (3, Rect::new(100.0, 0.0, 20.0, 20.0)),
            (4, Rect::new(100.0, 0.0, 20.0, 20.0)),
        ];
        let mut plan = VisualLayerPlan::default();
        build_visual_layer_plan(&placements, placements.len(), overlap, &mut plan);

        assert_eq!(plan.layer_count(), 3);
        for selected_layer in 1..=4 {
            assert_eq!(
                (0..2)
                    .filter(|index| plan.is_selected(*index, selected_layer))
                    .count(),
                1,
                "the shallow component must be switchable at global selection {selected_layer}"
            );
            assert_eq!(
                (2..5)
                    .filter(|index| plan.is_selected(*index, selected_layer))
                    .count(),
                1,
                "the deep component must select one local layer at {selected_layer}"
            );
        }
    }

    #[test]
    fn every_component_layer_has_a_distinct_rank_and_selection_is_topmost() {
        let placements = [
            (0, Rect::new(0.0, 0.0, 20.0, 20.0)),
            (1, Rect::new(0.0, 0.0, 20.0, 20.0)),
            (2, Rect::new(100.0, 0.0, 20.0, 20.0)),
            (3, Rect::new(100.0, 0.0, 20.0, 20.0)),
            (4, Rect::new(100.0, 0.0, 20.0, 20.0)),
        ];
        let mut plan = VisualLayerPlan::default();
        build_visual_layer_plan(&placements, placements.len(), overlap, &mut plan);

        for selected in 0..plan.layer_count() {
            let global_top = plan.layer_count();
            for component in [&[0, 1][..], &[2, 3, 4][..]] {
                let mut ranks = component
                    .iter()
                    .map(|index| plan.draw_rank(*index, selected).unwrap())
                    .collect::<Vec<_>>();
                assert!(ranks.contains(&global_top));
                ranks.sort_unstable();
                ranks.dedup();
                assert_eq!(ranks.len(), component.len());
            }
        }
    }

    #[test]
    fn wide_boundaries_cover_sparse_pairs_dense_and_dynamic_fallback() {
        for count in [127, 128, 129, 192, 255, 256, 257, 511, 512, 513] {
            let placements = (0..count)
                .map(|index| (index, Rect::new(index as f64 * 100.0, 0.0, 20.0, 20.0)))
                .collect::<Vec<_>>();
            let mut plan = VisualLayerPlan::default();
            build_visual_layer_plan(&placements, count, overlap, &mut plan);
            assert_eq!(plan.layer_count(), 0, "count={count}");
        }
        let pairs = (0..128)
            .flat_map(|pair| {
                let rect = Rect::new(pair as f64 * 100.0, 0.0, 20.0, 20.0);
                [(pair * 2, rect), (pair * 2 + 1, rect)]
            })
            .collect::<Vec<_>>();
        let mut plan = VisualLayerPlan::default();
        build_visual_layer_plan(&pairs, pairs.len(), overlap, &mut plan);
        assert_eq!(plan.layer_count(), 2);
        for pair in 0..128 {
            assert_eq!(plan.layer(pair * 2), Some(1));
            assert_eq!(plan.layer(pair * 2 + 1), Some(0));
        }
        plan.release_retained();

        for count in [3, 128, 256, 512, 513] {
            let dense = (0..count)
                .map(|index| (index, Rect::new(0.0, 0.0, 20.0, 20.0)))
                .collect::<Vec<_>>();
            assert_matches_quadratic_reference(&dense);
            build_visual_layer_plan(&dense, dense.len(), overlap, &mut plan);
            assert_eq!(plan.layer_count(), count);
            for index in 0..count {
                assert_eq!(plan.layer(index), Some(count - index - 1));
            }
        }

        let dynamic = (0..2_000)
            .map(|index| (index, Rect::new(index as f64 * 100.0, 0.0, 20.0, 20.0)))
            .collect::<Vec<_>>();
        let mut sparse_plan = VisualLayerPlan::default();
        build_visual_layer_plan(&dynamic, dynamic.len(), overlap, &mut sparse_plan);
        assert_eq!(sparse_plan.layer_count(), 0);
        assert_eq!(sparse_plan.retained_graph_words(), 0);
    }

    #[test]
    fn wide_sweep_preserves_varying_heights_and_numeric_boundaries() {
        fn verify(placements: &[(usize, Rect)], active: &mut Vec<usize>) {
            let mut order = Vec::new();
            assert!(prepare_wide_sweep(placements, &mut order));
            let predicates: [fn(Rect, Rect) -> bool; 2] = [overlap, |left, right| {
                super::super::visually_stacked(left, right, 4.0, 3.0)
            }];
            for predicate in predicates {
                let expected = (0..placements.len()).any(|right| {
                    (0..right).any(|left| predicate(placements[left].1, placements[right].1))
                });
                assert_eq!(
                    wide_sweep_has_edge(placements, &predicate, &order, active),
                    expected,
                    "{placements:?}"
                );
            }
        }

        // Reuse the workspace across early hits and empty/disjoint results.
        let mut active = vec![999; 300];
        let mut state = 3u64;
        for count in 0..65 {
            for pattern in 0..64 {
                let placements = (0..count)
                    .map(|index| {
                        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                        let x = ((state >> 32) % 101) as f64;
                        let y = if pattern % 2 == 0 {
                            ((index * 37) % count * 100) as f64
                        } else {
                            ((state >> 40) % 101) as f64
                        };
                        let width = if pattern % 3 == 0 {
                            200.0
                        } else {
                            ((state >> 20) % 31) as f64
                        };
                        let height = if pattern % 5 == 0 {
                            ((state >> 16) % 300) as f64
                        } else {
                            ((state >> 8) % 21) as f64
                        };
                        (index, Rect::new(x, y, width, height))
                    })
                    .collect::<Vec<_>>();
                verify(&placements, &mut active);
            }
        }
        let coordinates = [
            -f64::MAX,
            -100.0,
            -f64::MIN_POSITIVE,
            -0.0,
            0.0,
            f64::MIN_POSITIVE,
            100.0,
            f64::MAX,
        ];
        let heights = [0.0, f64::from_bits(1), 1.0, 100.0, f64::MAX];
        for y1 in coordinates {
            for y2 in coordinates {
                for height1 in heights {
                    for height2 in heights {
                        verify(
                            &[
                                (0, Rect::new(0.0, y1, 100.0, height1)),
                                (1, Rect::new(1.0, y2, 100.0, height2)),
                            ],
                            &mut active,
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn wide_fast_path_preserves_overlap_and_shift_layer_semantics() {
        let sparse = (0..2_000)
            .map(|index| (index, Rect::new(index as f64 * 100.0, 0.0, 20.0, 20.0)))
            .collect::<Vec<_>>();
        assert_matches_quadratic_reference(&sparse);

        let pairs = (0..250)
            .flat_map(|pair| {
                let rect = Rect::new(pair as f64 * 100.0, 0.0, 20.0, 20.0);
                [(pair * 2, rect), (pair * 2 + 1, rect)]
            })
            .collect::<Vec<_>>();
        assert_matches_quadratic_reference(&pairs);

        let chain = (0..256)
            .map(|index| (index, Rect::new(index as f64 * 15.0, 0.0, 20.0, 20.0)))
            .collect::<Vec<_>>();
        assert_matches_quadratic_reference(&chain);

        let dense = (0..129)
            .map(|index| (index, Rect::new(0.0, 0.0, 20.0, 20.0)))
            .collect::<Vec<_>>();
        assert_matches_quadratic_reference(&dense);

        let mut invalid = sparse[..129].to_vec();
        invalid[64].1.x = f64::NAN;
        assert_matches_quadratic_reference(&invalid);
    }

    #[test]
    fn inline_sweep_preserves_layers_for_sparse_dense_and_invalid_geometry() {
        for count in [15, 16, 24, 31, 32, 64, 100, 128] {
            for pattern in 0..5 {
                let placements: Vec<_> = (0..count)
                    .map(|index| {
                        let rect = match pattern {
                            0 => Rect::new(index as f64 * 100.0, 0.0, 20.0, 20.0),
                            1 => Rect::new((index / 2) as f64 * 100.0, 0.0, 20.0, 20.0),
                            2 => Rect::new(index as f64 * 15.0, 0.0, 20.0, 20.0),
                            3 => Rect::new(0.0, 0.0, 20.0, 20.0),
                            _ => Rect::new(
                                ((index * 73) % 101) as f64,
                                ((index * 31) % 79) as f64,
                                (index % 23) as f64,
                                (index % 19) as f64,
                            ),
                        };
                        (index, rect)
                    })
                    .collect();
                assert_matches_quadratic_reference(&placements);
            }
            for bad in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
                let mut placements: Vec<_> = (0..count)
                    .map(|index| (index, Rect::new(index as f64 * 15.0, 0.0, 20.0, 20.0)))
                    .collect();
                placements[count / 2].1.width = bad;
                assert_matches_quadratic_reference(&placements);
                let identical = (0..count)
                    .map(|index| (index, Rect::new(0.0, 0.0, bad, 20.0)))
                    .collect::<Vec<_>>();
                assert_matches_quadratic_reference(&identical);
            }
        }
    }

    #[test]
    fn inline_graph_crosses_word_and_512_boundaries_without_losing_edges() {
        for count in [128, 129, 256, 257, 511, 512, 513, 2000] {
            let mut graph = ConflictGraph::new(count);
            graph.add_edge(0, count - 1);
            graph.add_edge(count / 2, count - 1);
            let mut actual = Vec::new();
            graph.for_each_neighbor(count - 1, |n| actual.push(n));
            assert_eq!(actual, vec![0, count / 2]);
            assert_eq!(graph.degree(count - 1), 2);
            assert_eq!(graph.words == 0, count <= INLINE_LABELS);
        }
        for count in [255, 256, 257, 511, 512, 513] {
            let placements: Vec<_> = (0..count)
                .map(|i| (i, Rect::new((i / 2) as f64 * 100.0, 0.0, 20.0, 20.0)))
                .collect();
            assert_matches_quadratic_reference(&placements);
        }
        // More than 255 layers must still use the 16-bit packed representation.
        let placements: Vec<_> = (0..512)
            .map(|i| (i, Rect::new(0.0, 0.0, 20.0, 20.0)))
            .collect();
        let mut plan = VisualLayerPlan::default();
        build_visual_layer_plan(&placements, 512, overlap, &mut plan);
        assert_eq!(plan.layer_count(), 512);
        for i in 0..512 {
            assert_eq!(plan.layer(i), Some(511 - i));
        }
        assert!(plan.wide.is_none());
        let LayerStorage::Wide(layers) = &plan.layers else {
            panic!("deep plan must use wide packing")
        };
        assert!(!layers.spilled());
    }

    #[test]
    #[ignore = "allocation probe; run alone with --test-threads=1"]
    fn up_to_512_labels_build_without_heap_allocations() {
        for count in [129, 256, 257, 384, 511, 512] {
            let placements: Vec<_> = (0..count)
                .map(|i| (i, Rect::new((i / 2) as f64 * 100.0, 0.0, 20.0, 20.0)))
                .collect();
            let mut plan = VisualLayerPlan::default();
            let region = stats_alloc::Region::new(crate::TEST_ALLOCATOR);
            build_visual_layer_plan(&placements, count, overlap, &mut plan);
            let change = region.change();
            assert_eq!(change.allocations, 0, "count={count}: {change:?}");
            assert_eq!(change.reallocations, 0, "count={count}: {change:?}");
            assert!(plan.wide.is_none());
        }
    }

    #[test]
    #[ignore = "allocation probe; run alone with --test-threads=1"]
    fn inline_sweep_does_not_allocate() {
        let placements: Vec<_> = (0..128)
            .map(|index| (index, Rect::new(index as f64 * 15.0, 0.0, 20.0, 20.0)))
            .collect();
        let mut plan = VisualLayerPlan::default();
        build_visual_layer_plan(&placements, placements.len(), overlap, &mut plan);
        let region = stats_alloc::Region::new(crate::TEST_ALLOCATOR);
        for _ in 0..1_000 {
            build_visual_layer_plan(&placements, placements.len(), overlap, &mut plan);
            std::hint::black_box(&plan);
        }
        let change = region.change();
        assert_eq!(change.allocations, 0, "{change:?}");
        assert_eq!(change.reallocations, 0, "{change:?}");
    }

    #[test]
    #[ignore = "microbenchmark probe; run in release with --test-threads=1"]
    fn common_128_label_performance_probe() {
        const WARMUP: usize = 2_000;
        const SAMPLES: usize = 20_000;
        let placements: Vec<_> = (0..128)
            .map(|index| {
                let column = index % 16;
                let row = index / 16;
                (
                    index,
                    Rect::new(column as f64 * 16.0, row as f64 * 16.0, 20.0, 20.0),
                )
            })
            .collect();
        let mut plan = VisualLayerPlan::default();
        for _ in 0..WARMUP {
            build_visual_layer_plan(&placements, placements.len(), overlap, &mut plan);
        }
        let mut samples = Vec::with_capacity(SAMPLES);
        for _ in 0..SAMPLES {
            let started = std::time::Instant::now();
            build_visual_layer_plan(&placements, placements.len(), overlap, &mut plan);
            samples.push(started.elapsed().as_nanos());
        }
        samples.sort_unstable();
        let p50 = samples[(SAMPLES - 1) * 50 / 100];
        let p95 = samples[(SAMPLES - 1) * 95 / 100];
        let p99 = samples[(SAMPLES - 1) * 99 / 100];
        println!("visual_layer_128 samples={SAMPLES} p50={p50}ns p95={p95}ns p99={p99}ns");
        assert!(p99 < 100_000, "128-label visual-layer p99 was {p99}ns");
    }

    #[test]
    #[ignore = "microbenchmark probe; run in release with --test-threads=1"]
    fn common_256_pair_performance_probe() {
        const WARMUP: usize = 2_000;
        const SAMPLES: usize = 20_000;
        let placements = (0..128)
            .flat_map(|pair| {
                let rect = Rect::new(pair as f64 * 100.0, 0.0, 20.0, 20.0);
                [(pair * 2, rect), (pair * 2 + 1, rect)]
            })
            .collect::<Vec<_>>();
        let mut plan = VisualLayerPlan::default();
        for _ in 0..WARMUP {
            build_visual_layer_plan(&placements, placements.len(), overlap, &mut plan);
        }
        let mut samples = Vec::with_capacity(SAMPLES);
        for _ in 0..SAMPLES {
            let started = std::time::Instant::now();
            build_visual_layer_plan(&placements, placements.len(), overlap, &mut plan);
            samples.push(started.elapsed().as_nanos());
        }
        samples.sort_unstable();
        let mut reference = VisualLayerPlan::default();
        let mut reference_samples = Vec::with_capacity(SAMPLES);
        for _ in 0..SAMPLES {
            let started = std::time::Instant::now();
            quadratic_reference(&placements, &mut reference);
            reference_samples.push(started.elapsed().as_nanos());
        }
        reference_samples.sort_unstable();
        println!(
            "visual_layer_256_pairs samples={SAMPLES} optimized_p50={}ns optimized_p95={}ns optimized_p99={}ns reference_p50={}ns reference_p95={}ns reference_p99={}ns",
            samples[(SAMPLES - 1) * 50 / 100],
            samples[(SAMPLES - 1) * 95 / 100],
            samples[(SAMPLES - 1) * 99 / 100],
            reference_samples[(SAMPLES - 1) * 50 / 100],
            reference_samples[(SAMPLES - 1) * 95 / 100],
            reference_samples[(SAMPLES - 1) * 99 / 100],
        );
    }
}
