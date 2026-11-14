use crate::model::{BoundingBox, ConnectorStyle, DiagramConnector, DiagramShape, Point};

/// Updates all connectors on a page so their endpoints stay glued to their attached shape ports
/// and recalculates orthogonal paths (`waypoints`).
pub fn update_all_connectors(connectors: &mut [DiagramConnector], shapes: &[DiagramShape]) {
    for conn in connectors.iter_mut() {
        let mut start_dir = (0.0, 0.0);
        let mut end_dir = (0.0, 0.0);

        if let Some(fid) = conn.from_shape {
            if let Some(shape) = shapes.iter().find(|s| s.id == fid) {
                let (pos, dir) = shape.port_world_pos_and_dir(conn.from_port);
                conn.from_pos = pos;
                start_dir = dir;
            }
        }
        if let Some(tid) = conn.to_shape {
            if let Some(shape) = shapes.iter().find(|s| s.id == tid) {
                let (pos, dir) = shape.port_world_pos_and_dir(conn.to_port);
                conn.to_pos = pos;
                end_dir = dir;
            }
        }

        conn.waypoints = compute_connector_path(conn, start_dir, end_dir, shapes);
    }
}

/// Computes the polyline path for a single connector, including start and end points.
pub fn compute_connector_path(
    conn: &DiagramConnector,
    start_dir: (f64, f64),
    end_dir: (f64, f64),
    shapes: &[DiagramShape],
) -> Vec<Point> {
    let p0 = conn.from_pos;
    let p1 = conn.to_pos;

    match conn.style {
        ConnectorStyle::Straight => vec![p0, p1],
        ConnectorStyle::Curved => {
            // Sample a smooth cubic Bezier curve into a polyline
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
            let stub = 4.0; // 4 mm lead-out stub from connection port
            let s_out = Point::new(p0.x + start_dir.0 * stub, p0.y + start_dir.1 * stub);
            let e_out = Point::new(p1.x + end_dir.0 * stub, p1.y + end_dir.1 * stub);

            // Collect obstacle bounding boxes (excluding attached shapes and room backgrounds)
            let obstacles: Vec<BoundingBox> = shapes
                .iter()
                .filter(|s| {
                    !s.is_room_area
                        && !s.is_dimension
                        && Some(s.id) != conn.from_shape
                        && Some(s.id) != conn.to_shape
                })
                .map(|s| s.bounding_box().expand(2.5))
                .collect();

            let mut pts = Vec::with_capacity(8);
            pts.push(p0);
            if start_dir != (0.0, 0.0) {
                pts.push(s_out);
            }

            // Decide primary routing orientation
            let horizontal_first = if start_dir.0.abs() > 0.5 && end_dir.0.abs() > 0.5 {
                true
            } else if start_dir.1.abs() > 0.5 && end_dir.1.abs() > 0.5 {
                false
            } else {
                (e_out.x - s_out.x).abs() >= (e_out.y - s_out.y).abs()
            };

            if horizontal_first {
                let mut mid_x = (s_out.x + e_out.x) * 0.5;
                // If the vertical segment at mid_x intersects an obstacle, shift mid_x around it
                for obs in &obstacles {
                    if segment_intersects_box(
                        Point::new(mid_x, s_out.y),
                        Point::new(mid_x, e_out.y),
                        obs,
                    ) {
                        let left_cand = obs.min_x - 3.0;
                        let right_cand = obs.max_x + 3.0;
                        mid_x = if (left_cand - mid_x).abs() < (right_cand - mid_x).abs() {
                            left_cand
                        } else {
                            right_cand
                        };
                    }
                }

                // Check if end port faces the wrong way (U-turn needed)
                if start_dir.0 > 0.5 && end_dir.0 > 0.5 && e_out.x < s_out.x {
                    mid_x = s_out.x.max(e_out.x) + 6.0;
                } else if start_dir.0 < -0.5 && end_dir.0 < -0.5 && e_out.x > s_out.x {
                    mid_x = s_out.x.min(e_out.x) - 6.0;
                }

                if (s_out.y - e_out.y).abs() > 0.05 {
                    pts.push(Point::new(mid_x, s_out.y));
                    pts.push(Point::new(mid_x, e_out.y));
                }
            } else {
                let mut mid_y = (s_out.y + e_out.y) * 0.5;
                for obs in &obstacles {
                    if segment_intersects_box(
                        Point::new(s_out.x, mid_y),
                        Point::new(e_out.x, mid_y),
                        obs,
                    ) {
                        let top_cand = obs.min_y - 3.0;
                        let bot_cand = obs.max_y + 3.0;
                        mid_y = if (top_cand - mid_y).abs() < (bot_cand - mid_y).abs() {
                            top_cand
                        } else {
                            bot_cand
                        };
                    }
                }

                if (s_out.x - e_out.x).abs() > 0.05 {
                    pts.push(Point::new(s_out.x, mid_y));
                    pts.push(Point::new(e_out.x, mid_y));
                }
            }

            if end_dir != (0.0, 0.0) {
                pts.push(e_out);
            }
            pts.push(p1);

            simplify_orthogonal_path(pts)
        }
    }
}

fn segment_intersects_box(a: Point, b: Point, bbox: &BoundingBox) -> bool {
    let seg_box = BoundingBox::new(a.x, a.y, b.x, b.y);
    seg_box.intersects(bbox)
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
            continue;
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
                // If both connectors share the exact same source port, don't draw a jump at their shared branch
                let pts_b = if conn_b.waypoints.len() >= 2 {
                    &conn_b.waypoints
                } else {
                    continue;
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
