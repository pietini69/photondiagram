use crate::model::{BoundingBox, ConnectorStyle, DiagramConnector, DiagramShape, Point};
use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// Updates all connectors on a page so their endpoints stay glued to their attached shape ports
/// and recalculates orthogonal paths (`waypoints`) with obstacle and wire-overlap avoidance.
pub fn update_all_connectors(connectors: &mut [DiagramConnector], shapes: &[DiagramShape]) {
    // Keep track of already-routed segments: (a, b, from_shape, from_port, to_shape, to_port)
    let mut routed_segments: Vec<(Point, Point, Option<u64>, usize, Option<u64>, usize)> =
        Vec::new();

    for conn in connectors.iter_mut() {
        let mut start_dir = (0.0, 0.0);
        let mut end_dir = (0.0, 0.0);

        if let Some(fid) = conn.from_shape {
            if let Some(shape) = shapes.iter().find(|s| s.id == fid) {
                let (pos, dir) = shape.port_world_pos_and_dir(conn.from_port);
                conn.from_pos = pos;
                start_dir = resolve_port_dir(shape, pos, dir);
            }
        }
        if let Some(tid) = conn.to_shape {
            if let Some(shape) = shapes.iter().find(|s| s.id == tid) {
                let (pos, dir) = shape.port_world_pos_and_dir(conn.to_port);
                conn.to_pos = pos;
                end_dir = resolve_port_dir(shape, pos, dir);
            }
        }

        conn.waypoints =
            compute_connector_path_avoiding(conn, start_dir, end_dir, shapes, &routed_segments);

        for seg in conn.waypoints.windows(2) {
            routed_segments.push((
                seg[0],
                seg[1],
                conn.from_shape,
                conn.from_port,
                conn.to_shape,
                conn.to_port,
            ));
        }
    }
}

/// If a port has `PortDir::Any` (`(0.0, 0.0)`), infer outward normal from the shape center to the port.
fn resolve_port_dir(shape: &DiagramShape, port_pos: Point, dir: (f64, f64)) -> (f64, f64) {
    if dir.0.abs() > 0.01 || dir.1.abs() > 0.01 {
        return dir;
    }
    let dx = port_pos.x - shape.x;
    let dy = port_pos.y - shape.y;
    if dx.abs() >= dy.abs() && dx.abs() > 0.1 {
        (dx.signum(), 0.0)
    } else if dy.abs() > 0.1 {
        (0.0, dy.signum())
    } else {
        (0.0, 0.0)
    }
}

/// Computes the polyline path for a single connector, including start and end points.
pub fn compute_connector_path(
    conn: &DiagramConnector,
    start_dir: (f64, f64),
    end_dir: (f64, f64),
    shapes: &[DiagramShape],
) -> Vec<Point> {
    compute_connector_path_avoiding(conn, start_dir, end_dir, shapes, &[])
}

fn compute_connector_path_avoiding(
    conn: &DiagramConnector,
    start_dir: (f64, f64),
    end_dir: (f64, f64),
    shapes: &[DiagramShape],
    routed_segments: &[(Point, Point, Option<u64>, usize, Option<u64>, usize)],
) -> Vec<Point> {
    let p0 = conn.from_pos;
    let p1 = conn.to_pos;

    match conn.style {
        ConnectorStyle::Straight => vec![p0, p1],
        ConnectorStyle::Curved => {
            let dist = p0.distance_to(p1).max(10.0) * 0.4;
            let (sdx, sdy) = if start_dir == (0.0, 0.0) {
                ((p1.x - p0.x).signum(), 0.0)
            } else {
                start_dir
            };
            let (edx, edy) = if end_dir == (0.0, 0.0) {
                ((p0.x - p1.x).signum(), 0.0)
            } else {
                end_dir
            };
            let c1 = Point::new(p0.x + sdx * dist, p0.y + sdy * dist);
            let c2 = Point::new(p1.x + edx * dist, p1.y + edy * dist);

            let mut pts = Vec::with_capacity(17);
            for i in 0..=16 {
                let t = i as f64 / 16.0;
                let it = 1.0 - t;
                let x = it.powi(3) * p0.x
                    + 3.0 * it.powi(2) * t * c1.x
                    + 3.0 * it * t.powi(2) * c2.x
                    + t.powi(3) * p1.x;
                let y = it.powi(3) * p0.y
                    + 3.0 * it.powi(2) * t * c1.y
                    + 3.0 * it * t.powi(2) * c2.y
                    + t.powi(3) * p1.y;
                pts.push(Point::new(x, y));
            }
            pts
        }
        ConnectorStyle::Orthogonal => {
            let stub = 5.0; // 5 mm outward lead-out stub from connection port
            let s_out = if start_dir != (0.0, 0.0) {
                Point::new(p0.x + start_dir.0 * stub, p0.y + start_dir.1 * stub)
            } else {
                p0
            };
            let e_out = if end_dir != (0.0, 0.0) {
                Point::new(p1.x + end_dir.0 * stub, p1.y + end_dir.1 * stub)
            } else {
                p1
            };

            // Collect obstacle bounding boxes for ALL non-room, non-dimension shapes,
            // INCLUDING from_shape and to_shape so wires NEVER cut through their own component bodies!
            let mut obstacles: Vec<BoundingBox> = Vec::with_capacity(shapes.len());
            for s in shapes {
                if s.is_room_area || s.is_dimension || s.stencil_id.starts_with("note_") {
                    continue;
                }
                let is_endpoint_shape =
                    Some(s.id) == conn.from_shape || Some(s.id) == conn.to_shape;
                let raw_bb = s.bounding_box();
                let mut obs = if is_endpoint_shape {
                    // Tight margin around attached shapes so s_out/e_out (at 5mm) sit outside the obstacle,
                    // while completely blocking any path from cutting through the component or its opposite pins!
                    raw_bb.expand(1.2)
                } else {
                    // Generous margin around intermediate shapes (including top/bottom label clearance)
                    BoundingBox::new(
                        raw_bb.min_x - 3.5,
                        raw_bb.min_y - 5.5,
                        raw_bb.max_x + 3.5,
                        raw_bb.max_y + 4.5,
                    )
                };

                // Ensure s_out and e_out themselves are not trapped inside a third-party obstacle's expanded margin
                if !is_endpoint_shape && (obs.contains(s_out) || obs.contains(e_out)) {
                    obs = raw_bb.expand(0.8);
                }
                obstacles.push(obs);
            }

            // Also add short outward corridors in front of all OTHER ports on all shapes so a wire
            // exiting e.g. Q2 doesn't turn straight down 5mm in front of Q3 and GND!
            let mut all_segments = routed_segments.to_vec();
            for s in shapes {
                if s.is_room_area || s.is_dimension {
                    continue;
                }
                for p_idx in 0..s.ports.len() {
                    if (Some(s.id) == conn.from_shape && p_idx == conn.from_port)
                        || (Some(s.id) == conn.to_shape && p_idx == conn.to_port)
                    {
                        continue;
                    }
                    let (p_pos, p_dir) = s.port_world_pos_and_dir(p_idx);
                    let r_dir = resolve_port_dir(s, p_pos, p_dir);
                    if r_dir != (0.0, 0.0) {
                        let p_ext = Point::new(p_pos.x + r_dir.0 * 9.0, p_pos.y + r_dir.1 * 9.0);
                        all_segments.push((p_pos, p_ext, Some(s.id), p_idx, None, 0));
                    }
                }
            }

            let mid_path = route_orthogonal_astar(
                s_out,
                e_out,
                start_dir,
                end_dir,
                &obstacles,
                &all_segments,
                conn.from_shape,
                conn.from_port,
                conn.to_shape,
                conn.to_port,
            );

            let mut pts = Vec::with_capacity(mid_path.len() + 2);
            pts.push(p0);
            pts.extend(mid_path);
            pts.push(p1);

            simplify_orthogonal_path(pts)
        }
    }
}

#[derive(Clone, Copy)]
struct AStarNode {
    f_cost: f64,
    g_cost: f64,
    idx: usize,
}

impl PartialEq for AStarNode {
    fn eq(&self, other: &Self) -> bool {
        self.f_cost == other.f_cost && self.idx == other.idx
    }
}
impl Eq for AStarNode {}

impl PartialOrd for AStarNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        other.f_cost.partial_cmp(&self.f_cost)
    }
}
impl Ord for AStarNode {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap_or(Ordering::Equal)
    }
}

fn dedup_coords(mut coords: Vec<f64>) -> Vec<f64> {
    coords.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let mut out: Vec<f64> = Vec::with_capacity(coords.len());
    for c in coords {
        if let Some(&last) = out.last() {
            if (c - last).abs() < 0.8 {
                continue;
            }
        }
        out.push(c);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn route_orthogonal_astar(
    s_out: Point,
    e_out: Point,
    start_dir: (f64, f64),
    end_dir: (f64, f64),
    obstacles: &[BoundingBox],
    routed_segments: &[(Point, Point, Option<u64>, usize, Option<u64>, usize)],
    from_shape: Option<u64>,
    from_port: usize,
    to_shape: Option<u64>,
    to_port: usize,
) -> Vec<Point> {
    if s_out.distance_to(e_out) < 0.2 {
        return vec![s_out];
    }

    // Build Hanan candidate grid coordinates from endpoints, midpoints, and obstacle boundaries
    let mut raw_xs = vec![s_out.x, e_out.x, (s_out.x + e_out.x) * 0.5];
    let mut raw_ys = vec![s_out.y, e_out.y, (s_out.y + e_out.y) * 0.5];

    for obs in obstacles {
        raw_xs.push(obs.min_x - 2.0);
        raw_xs.push(obs.min_x - 6.0);
        raw_xs.push(obs.max_x + 2.0);
        raw_xs.push(obs.max_x + 6.0);

        raw_ys.push(obs.min_y - 2.0);
        raw_ys.push(obs.min_y - 6.0);
        raw_ys.push(obs.max_y + 2.0);
        raw_ys.push(obs.max_y + 6.0);
    }

    let base_xs = dedup_coords(raw_xs);
    let base_ys = dedup_coords(raw_ys);

    // Add midpoints between adjacent coordinates so channels between components are always available
    let mut xs = base_xs.clone();
    for w in base_xs.windows(2) {
        if w[1] - w[0] >= 4.0 {
            xs.push((w[0] + w[1]) * 0.5);
        }
    }
    let mut ys = base_ys.clone();
    for w in base_ys.windows(2) {
        if w[1] - w[0] >= 4.0 {
            ys.push((w[0] + w[1]) * 0.5);
        }
    }

    // Ensure exact s_out and e_out coordinates exist in xs and ys
    xs.push(s_out.x);
    xs.push(e_out.x);
    ys.push(s_out.y);
    ys.push(e_out.y);

    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    xs.dedup_by(|a, b| (*a - *b).abs() < 0.05);
    ys.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    ys.dedup_by(|a, b| (*a - *b).abs() < 0.05);

    let nx = xs.len();
    let ny = ys.len();
    let start_xi = xs
        .iter()
        .position(|&x| (x - s_out.x).abs() < 0.1)
        .unwrap_or(0);
    let start_yi = ys
        .iter()
        .position(|&y| (y - s_out.y).abs() < 0.1)
        .unwrap_or(0);
    let goal_xi = xs
        .iter()
        .position(|&x| (x - e_out.x).abs() < 0.1)
        .unwrap_or(0);
    let goal_yi = ys
        .iter()
        .position(|&y| (y - e_out.y).abs() < 0.1)
        .unwrap_or(0);

    // State index: ((yi * nx + xi) * 3 + dir), where dir: 0 = Start, 1 = Horiz, 2 = Vert
    let num_states = nx * ny * 3;
    let mut best_g = vec![f64::INFINITY; num_states];
    let mut parent = vec![usize::MAX; num_states];
    let mut heap = BinaryHeap::new();

    let start_state = (start_yi * nx + start_xi) * 3;
    best_g[start_state] = 0.0;
    let h0 = (s_out.x - e_out.x).abs() + (s_out.y - e_out.y).abs();
    heap.push(AStarNode {
        f_cost: h0,
        g_cost: 0.0,
        idx: start_state,
    });

    let mut reached_goal_state = None;

    while let Some(curr) = heap.pop() {
        if curr.g_cost > best_g[curr.idx] + 1e-6 {
            continue;
        }
        let dir = curr.idx % 3;
        let cell = curr.idx / 3;
        let xi = cell % nx;
        let yi = cell / nx;

        if xi == goal_xi && yi == goal_yi {
            reached_goal_state = Some(curr.idx);
            break;
        }

        let p_curr = Point::new(xs[xi], ys[yi]);

        // Explore 4 cardinal grid neighbors
        let neighbors: [(Option<usize>, Option<usize>, usize); 4] = [
            (xi.checked_sub(1), Some(yi), 1),
            (if xi + 1 < nx { Some(xi + 1) } else { None }, Some(yi), 1),
            (Some(xi), yi.checked_sub(1), 2),
            (Some(xi), if yi + 1 < ny { Some(yi + 1) } else { None }, 2),
        ];

        for (nxi_opt, nyi_opt, ndir) in neighbors {
            let (Some(nxi), Some(nyi)) = (nxi_opt, nyi_opt) else {
                continue;
            };
            let p_next = Point::new(xs[nxi], ys[nyi]);
            let dx = p_next.x - p_curr.x;
            let dy = p_next.y - p_curr.y;
            let seg_len = dx.abs() + dy.abs();
            if seg_len < 1e-4 {
                continue;
            }

            // Prevent immediate reversal back into the start port
            if xi == start_xi && yi == start_yi && start_dir != (0.0, 0.0) {
                let dot = (dx / seg_len) * start_dir.0 + (dy / seg_len) * start_dir.1;
                if dot < -0.5 {
                    continue;
                }
            }

            // Prevent entering goal e_out in the same direction as end_dir (which would fold back over e_out -> p1)
            if nxi == goal_xi && nyi == goal_yi && end_dir != (0.0, 0.0) {
                let dot = (dx / seg_len) * end_dir.0 + (dy / seg_len) * end_dir.1;
                if dot > 0.5 {
                    continue;
                }
            }

            // Check obstacle collision
            let mut obs_penalty = 0.0;
            for obs in obstacles {
                if segment_intersects_box_interior(p_curr, p_next, obs) {
                    obs_penalty += 50_000.0;
                }
            }

            // Check overlap or crossing with already-routed connectors
            let mut wire_penalty = 0.0;
            for &(ra, rb, rf_s, rf_p, rt_s, rt_p) in routed_segments {
                let shares_endpoint = (from_shape.is_some()
                    && ((from_shape == rf_s && from_port == rf_p)
                        || (from_shape == rt_s && from_port == rt_p)))
                    || (to_shape.is_some()
                        && ((to_shape == rf_s && to_port == rf_p)
                            || (to_shape == rt_s && to_port == rt_p)));

                if segments_collinear_overlap(p_curr, p_next, ra, rb, 2.2) {
                    if !shares_endpoint {
                        // Strongly discourage unrelated wires from running on top of each other
                        wire_penalty += 120.0 + seg_len * 3.0;
                    }
                } else if segments_cross_perpendicular(p_curr, p_next, ra, rb) && !shares_endpoint {
                    wire_penalty += 18.0;
                }
            }

            // Bend penalty to minimize corner count
            let mut bend_penalty = 0.0;
            if dir != 0 && dir != ndir {
                bend_penalty += 14.0;
            } else if dir == 0 && start_dir != (0.0, 0.0) {
                // Slight preference to continue straight out of the stub if clean
                let stub_is_horiz = start_dir.0.abs() > 0.5;
                if (stub_is_horiz && ndir != 1) || (!stub_is_horiz && ndir != 2) {
                    bend_penalty += 6.0;
                }
            }
            if nxi == goal_xi && nyi == goal_yi && end_dir != (0.0, 0.0) {
                let end_is_horiz = end_dir.0.abs() > 0.5;
                if (end_is_horiz && ndir != 1) || (!end_is_horiz && ndir != 2) {
                    bend_penalty += 6.0;
                }
            }

            let next_state = (nyi * nx + nxi) * 3 + ndir;
            let new_g = curr.g_cost + seg_len + bend_penalty + wire_penalty + obs_penalty;

            if new_g + 1e-6 < best_g[next_state] {
                best_g[next_state] = new_g;
                parent[next_state] = curr.idx;
                let h = (p_next.x - e_out.x).abs() + (p_next.y - e_out.y).abs();
                heap.push(AStarNode {
                    f_cost: new_g + h,
                    g_cost: new_g,
                    idx: next_state,
                });
            }
        }
    }

    if let Some(goal_state) = reached_goal_state {
        let mut rev_pts = Vec::new();
        let mut st = goal_state;
        while st != usize::MAX {
            let cell = st / 3;
            let xi = cell % nx;
            let yi = cell / nx;
            rev_pts.push(Point::new(xs[xi], ys[yi]));
            if st == start_state {
                break;
            }
            st = parent[st];
        }
        rev_pts.reverse();
        rev_pts
    } else {
        // Fallback L-bend if grid search could not complete
        vec![s_out, Point::new(e_out.x, s_out.y), e_out]
    }
}

/// Checks if orthogonal segment `a -> b` intersects the interior of `bbox`.
fn segment_intersects_box_interior(a: Point, b: Point, bbox: &BoundingBox) -> bool {
    let eps = 0.05;
    let min_x = a.x.min(b.x);
    let max_x = a.x.max(b.x);
    let min_y = a.y.min(b.y);
    let max_y = a.y.max(b.y);

    max_x > bbox.min_x + eps
        && min_x < bbox.max_x - eps
        && max_y > bbox.min_y + eps
        && min_y < bbox.max_y - eps
}

/// Checks if two orthogonal segments run parallel within `tol` mm and overlap along their length.
fn segments_collinear_overlap(a1: Point, a2: Point, b1: Point, b2: Point, tol: f64) -> bool {
    let a_horiz = (a1.y - a2.y).abs() < 0.2;
    let b_horiz = (b1.y - b2.y).abs() < 0.2;
    if a_horiz && b_horiz {
        if (a1.y - b1.y).abs() <= tol {
            let a_min = a1.x.min(a2.x);
            let a_max = a1.x.max(a2.x);
            let b_min = b1.x.min(b2.x);
            let b_max = b1.x.max(b2.x);
            return a_max.min(b_max) - a_min.max(b_min) > 1.0;
        }
    } else if !a_horiz && !b_horiz {
        if (a1.x - b1.x).abs() <= tol {
            let a_min = a1.y.min(a2.y);
            let a_max = a1.y.max(a2.y);
            let b_min = b1.y.min(b2.y);
            let b_max = b1.y.max(b2.y);
            return a_max.min(b_max) - a_min.max(b_min) > 1.0;
        }
    }
    false
}

fn segments_cross_perpendicular(a1: Point, a2: Point, b1: Point, b2: Point) -> bool {
    let a_horiz = (a1.y - a2.y).abs() < 0.2;
    let b_horiz = (b1.y - b2.y).abs() < 0.2;
    if a_horiz == b_horiz {
        return false;
    }
    let (h1, h2, v1, v2) = if a_horiz {
        (a1, a2, b1, b2)
    } else {
        (b1, b2, a1, a2)
    };
    let hx_min = h1.x.min(h2.x) + 0.5;
    let hx_max = h1.x.max(h2.x) - 0.5;
    let vy_min = v1.y.min(v2.y) + 0.5;
    let vy_max = v1.y.max(v2.y) - 0.5;
    v1.x > hx_min && v1.x < hx_max && h1.y > vy_min && h1.y < vy_max
}

/// Removes duplicate or collinear intermediate points from an orthogonal path
fn simplify_orthogonal_path(pts: Vec<Point>) -> Vec<Point> {
    let mut cleaned: Vec<Point> = Vec::with_capacity(pts.len());
    for p in pts {
        if let Some(last) = cleaned.last() {
            if last.distance_to(p) < 0.05 {
                continue;
            }
        }
        cleaned.push(p);
    }

    let mut result: Vec<Point> = Vec::with_capacity(cleaned.len());
    for i in 0..cleaned.len() {
        if i > 0 && i + 1 < cleaned.len() {
            let prev = result.last().copied().unwrap();
            let curr = cleaned[i];
            let next = cleaned[i + 1];
            let same_x = (prev.x - curr.x).abs() < 0.05 && (curr.x - next.x).abs() < 0.05;
            let same_y = (prev.y - curr.y).abs() < 0.05 && (curr.y - next.y).abs() < 0.05;
            if same_x || same_y {
                continue;
            }
        }
        result.push(cleaned[i]);
    }
    result
}

/// Computes the optimal page coordinate (in mm) for displaying a connector's label,
/// placed along its longest clear segment and offset perpendicular to the wire so no wire
/// strikes through the text.
pub fn connector_label_position(waypoints: &[Point]) -> Point {
    connector_label_position_smart(waypoints, &[], &[], 6.0)
}

pub fn connector_label_position_smart(
    waypoints: &[Point],
    all_connectors: &[DiagramConnector],
    shapes: &[DiagramShape],
    half_w_mm: f64,
) -> Point {
    if waypoints.len() < 2 {
        return waypoints.first().copied().unwrap_or(Point::ZERO);
    }

    let max_len = waypoints
        .windows(2)
        .map(|s| s[0].distance_to(s[1]))
        .fold(0.0_f64, f64::max);

    let mut best_pt = waypoints[0].midpoint(waypoints[1]);
    let mut best_score = f64::NEG_INFINITY;

    for seg in waypoints.windows(2) {
        let len = seg[0].distance_to(seg[1]);
        if len < 4.5 && max_len >= 4.5 {
            continue;
        }
        let mid = seg[0].midpoint(seg[1]);
        let dx = (seg[1].x - seg[0].x).abs();
        let dy = (seg[1].y - seg[0].y).abs();

        let candidates: [(Point, f64); 3] = if dx >= dy {
            [
                (Point::new(mid.x, mid.y - 3.2), 4.0),
                (Point::new(mid.x, mid.y + 3.4), 2.0),
                (Point::new(mid.x, mid.y - 5.5), 0.0),
            ]
        } else {
            let off_x = (half_w_mm + 2.2).max(5.5);
            [
                (Point::new(mid.x - off_x, mid.y), 4.0),
                (Point::new(mid.x + off_x, mid.y), 3.0),
                (mid, 0.0),
            ]
        };

        for (cand, pref_bonus) in candidates {
            let tbox = BoundingBox::new(
                cand.x - half_w_mm,
                cand.y - 2.0,
                cand.x + half_w_mm,
                cand.y + 2.0,
            );
            let mut score = len * 0.5 + pref_bonus;

            // Penalize colliding with any segment of this wire
            for s in waypoints.windows(2) {
                if segment_intersects_box_interior(s[0], s[1], &tbox) {
                    score -= 90.0;
                }
            }
            // Penalize colliding with any other connector's segments
            for other in all_connectors {
                for s in other.waypoints.windows(2) {
                    if segment_intersects_box_interior(s[0], s[1], &tbox) {
                        score -= 90.0;
                    }
                }
            }
            // Penalize colliding with any component or its default label zone
            for sh in shapes {
                if sh.is_room_area {
                    continue;
                }
                if sh.bounding_box().expand(2.5).intersects(&tbox) {
                    score -= 140.0;
                }
            }

            if score > best_score {
                best_score = score;
                best_pt = cand;
            }
        }
    }

    best_pt
}

/// Returns a clean world coordinate (in mm) for a shape's external caption (`Top` or `Bottom`),
/// nudging it clear if a wire segment passes directly through the default label position.
pub fn resolve_clean_label_position(
    shape: &DiagramShape,
    connectors: &[DiagramConnector],
    caption_chars: usize,
) -> Point {
    let base = shape.label_position();
    let eff_pos = shape.effective_text_position();
    if !matches!(
        eff_pos,
        crate::model::TextPosition::Top | crate::model::TextPosition::Bottom
    ) {
        return base;
    }

    let half_w = ((caption_chars as f64) * 0.95).clamp(5.0, 22.0);
    let bb = shape.bounding_box();

    let box_for = |pt: Point, is_top: bool| -> BoundingBox {
        if is_top {
            BoundingBox::new(pt.x - half_w, pt.y - 3.6, pt.x + half_w, pt.y + 0.5)
        } else {
            BoundingBox::new(pt.x - half_w, pt.y - 0.5, pt.x + half_w, pt.y + 3.6)
        }
    };

    let is_top = eff_pos == crate::model::TextPosition::Top;
    let count_hits = |tbox: &BoundingBox| -> usize {
        let mut hits = 0;
        for c in connectors {
            for seg in c.waypoints.windows(2) {
                if segment_intersects_box_interior(seg[0], seg[1], tbox) {
                    hits += 1;
                }
            }
        }
        hits
    };

    let base_box = box_for(base, is_top);
    if count_hits(&base_box) == 0 {
        return base;
    }

    // Try flipping to opposite vertical side (Top <-> Bottom) so the label stays strictly
    // aligned with its own component and never jumps horizontally onto an adjacent component!
    let opp_pt = Point::new(
        shape.x,
        if is_top {
            bb.max_y + 2.8
        } else {
            bb.min_y - 2.8
        },
    );
    let opp_box = box_for(opp_pt, !is_top);
    let opp_hits = count_hits(&opp_box);
    if opp_hits == 0 {
        return opp_pt;
    }

    // For vertical-pin voltage sources (DC/AC source where pins are at Top and Bottom),
    // place label cleanly to the left or right of the circle
    if shape.stencil_id.contains("dc_source") || shape.stencil_id.contains("ac_source") {
        let left_pt = Point::new(bb.min_x - half_w - 2.0, shape.y);
        if count_hits(&box_for(left_pt, false)) == 0 {
            return left_pt;
        }
    }

    if opp_hits < count_hits(&base_box) {
        opp_pt
    } else {
        base
    }
}

/// Represents a wire crossing where a horizontal wire segment crosses over a vertical wire segment
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WireJump {
    pub point: Point,
    pub connector_id: u64,
}

/// Finds all intersections between horizontal and vertical segments of different connectors
/// so the renderer/exporter can draw Visio-style line jumps (hyppykaaret) or junction dots.
pub fn find_wire_jumps(connectors: &[DiagramConnector]) -> Vec<WireJump> {
    let mut jumps = Vec::new();

    for (i, conn_a) in connectors.iter().enumerate() {
        let pts_a = if conn_a.waypoints.len() >= 2 {
            &conn_a.waypoints
        } else {
            continue
        };

        for seg_a in pts_a.windows(2) {
            let a1 = seg_a[0];
            let a2 = seg_a[1];
            // Only horizontal segments of conn_a get the bridge jump arc
            if (a1.y - a2.y).abs() > 0.2 {
                continue;
            }
            let min_ax = a1.x.min(a2.x) + 1.5;
            let max_ax = a1.x.max(a2.x) - 1.5;
            if min_ax >= max_ax {
                continue;
            }
            let ay = (a1.y + a2.y) * 0.5;

            for (j, conn_b) in connectors.iter().enumerate() {
                if i == j {
                    continue;
                }
                let pts_b = if conn_b.waypoints.len() >= 2 {
                    &conn_b.waypoints
                } else {
                    continue
                };

                for seg_b in pts_b.windows(2) {
                    let b1 = seg_b[0];
                    let b2 = seg_b[1];
                    // Vertical segment of conn_b
                    if (b1.x - b2.x).abs() > 0.2 {
                        continue;
                    }
                    let min_by = b1.y.min(b2.y) + 1.5;
                    let max_by = b1.y.max(b2.y) - 1.5;
                    if min_by >= max_by {
                        continue;
                    }
                    let bx = (b1.x + b2.x) * 0.5;

                    if bx > min_ax && bx < max_ax && ay > min_by && ay < max_by {
                        jumps.push(WireJump {
                            point: Point::new(bx, ay),
                            connector_id: conn_a.id,
                        });
                    }
                }
            }
        }
    }

    jumps
}

/// Checks if a point `p` is within `tolerance_mm` of any segment of `conn`
pub fn connector_hit_test(conn: &DiagramConnector, p: Point, tolerance_mm: f64) -> bool {
    let pts: Vec<Point> = if conn.waypoints.len() >= 2 {
        conn.waypoints.clone()
    } else {
        vec![conn.from_pos, conn.to_pos]
    };

    for seg in pts.windows(2) {
        if point_to_segment_distance(p, seg[0], seg[1]) <= tolerance_mm {
            return true;
        }
    }
    false
}

pub fn point_to_segment_distance(p: Point, a: Point, b: Point) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len_sq = dx * dx + dy * dy;
    if len_sq < 1e-9 {
        return p.distance_to(a);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len_sq).clamp(0.0, 1.0);
    let proj = Point::new(a.x + t * dx, a.y + t * dy);
    p.distance_to(proj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ArrowType, ConnectorStyle};
    use crate::stencils::instantiate_shape;
    use crate::templates::template_electrical_schematic;

    #[test]
    fn test_capacitor_to_plc_gnd_never_crosses_plc_or_vcc() {
        let mut doc = template_electrical_schematic();
        let cid_shape = doc.alloc_id();
        let cap = instantiate_shape("elec_capacitor", cid_shape, 95.0, 145.0, 12).unwrap();
        let plc_id = doc.pages[0]
            .shapes
            .iter()
            .find(|s| s.stencil_id == "net_plc")
            .unwrap()
            .id;

        doc.pages[0].shapes.push(cap);
        let conn_id = doc.alloc_id();
        doc.pages[0].connectors.push(DiagramConnector {
            id: conn_id,
            from_shape: Some(cid_shape),
            from_port: 1, // Right pin of C12
            from_pos: Point::ZERO,
            to_shape: Some(plc_id),
            to_port: 7, // 0V / GND pin on Right side of PLC1
            to_pos: Point::ZERO,
            style: ConnectorStyle::Orthogonal,
            start_arrow: ArrowType::None,
            end_arrow: ArrowType::None,
            label: String::new(),
            stroke_color: [30, 41, 59, 255],
            stroke_width: 1.8,
            dashed: false,
            layer: "Sähkö & Kytkennät".to_string(),
            waypoints: Vec::new(),
        });

        let page = &mut doc.pages[0];
        update_all_connectors(&mut page.connectors, &page.shapes);

        let plc = page.shapes.iter().find(|s| s.id == plc_id).unwrap();
        let plc_inner = plc.bounding_box().expand(-0.5);
        let (vcc_pos, _) = plc.port_world_pos_and_dir(3); // Port 3 = VCC on left side

        let cap_conn = page.connectors.iter().find(|c| c.id == conn_id).unwrap();
        assert!(cap_conn.waypoints.len() >= 3);

        for seg in cap_conn.waypoints.windows(2) {
            assert!(
                !segment_intersects_box_interior(seg[0], seg[1], &plc_inner),
                "Wire segment {:?} -> {:?} crossed through PLC body {:?}!",
                seg[0],
                seg[1],
                plc_inner
            );
            assert!(
                point_to_segment_distance(vcc_pos, seg[0], seg[1]) > 4.0,
                "Wire passed too close to VCC pin {:?}: seg {:?} -> {:?}",
                vcc_pos,
                seg[0],
                seg[1]
            );
        }

        // Also assert NO wire in the entire schematic crosses through ANY component body
        for conn in &page.connectors {
            for shape in &page.shapes {
                let inner = shape.bounding_box().expand(-0.8);
                for seg in conn.waypoints.windows(2) {
                    assert!(
                        !segment_intersects_box_interior(seg[0], seg[1], &inner),
                        "Wire {} segment {:?}->{:?} crossed inside component {} ({})!",
                        conn.id,
                        seg[0],
                        seg[1],
                        shape.label,
                        shape.stencil_id
                    );
                }
            }
        }
    }
}
