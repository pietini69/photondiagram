//! 2D polygon triangulation and robust path drawing routines.
//!
//! Provides ear-clipping triangulation into `egui::Mesh`, eliminating all
//! tessellator miter division-by-zero explosions for concave and complex shapes.

use eframe::egui::{Color32, Mesh, Painter, Pos2, Shape, Stroke};
use eframe::epaint::{Vertex, WHITE_UV};

/// Triangulates a 2D polygon into a list of triangle index triples `[i0, i1, i2]`.
///
/// Uses ear-clipping with robust fallbacks.
/// Guaranteed to terminate and strictly references indices in `0..pts.len()`.
pub fn triangulate_polygon(pts: &[Pos2]) -> Vec<[u32; 3]> {
    let n = pts.len();
    if n < 3 {
        return Vec::new();
    }
    if n == 3 {
        return vec![[0, 1, 2]];
    }

    // Calculate signed area:
    // signed_area = 0.5 * sum(x_i * y_{i+1} - x_{i+1} * y_i)
    let mut double_area = 0.0_f32;
    for i in 0..n {
        let j = (i + 1) % n;
        double_area += pts[i].x * pts[j].y - pts[j].x * pts[i].y;
    }

    if double_area.abs() < 1e-7 {
        // Degenerate polygon (collinear or zero area)
        return Vec::new();
    }

    let mut remaining: Vec<usize> = (0..n).collect();
    let mut triangles = Vec::with_capacity(n - 2);

    let cross_2d = |a: Pos2, b: Pos2, c: Pos2| -> f32 {
        (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
    };

    let is_point_in_triangle = |p: Pos2, a: Pos2, b: Pos2, c: Pos2, sign: f32| -> bool {
        let c1 = cross_2d(a, b, p) * sign;
        let c2 = cross_2d(b, c, p) * sign;
        let c3 = cross_2d(c, a, p) * sign;
        c1 >= -1e-5 && c2 >= -1e-5 && c3 >= -1e-5
    };

    let sign = if double_area > 0.0 { 1.0_f32 } else { -1.0_f32 };
    let mut max_iters = remaining.len() * 4;

    while remaining.len() > 3 && max_iters > 0 {
        max_iters -= 1;
        let rem_len = remaining.len();
        let mut ear_found = false;

        for i in 0..rem_len {
            let prev = remaining[(i + rem_len - 1) % rem_len];
            let curr = remaining[i];
            let next = remaining[(i + 1) % rem_len];

            let a = pts[prev];
            let b = pts[curr];
            let c = pts[next];

            // Convex check
            if cross_2d(a, b, c) * sign <= 1e-7 {
                continue;
            }

            // Check if any other remaining vertex lies inside this triangle
            let mut has_point_inside = false;
            for j in 0..rem_len {
                let p_idx = remaining[j];
                if p_idx == prev || p_idx == curr || p_idx == next {
                    continue;
                }
                if is_point_in_triangle(pts[p_idx], a, b, c, sign) {
                    has_point_inside = true;
                    break;
                }
            }

            if !has_point_inside {
                triangles.push([prev as u32, curr as u32, next as u32]);
                remaining.remove(i);
                ear_found = true;
                break;
            }
        }

        if !ear_found {
            // Fallback: clip any convex corner, or if none, clip remaining[0]
            let mut clipped = false;
            for i in 0..rem_len {
                let prev = remaining[(i + rem_len - 1) % rem_len];
                let curr = remaining[i];
                let next = remaining[(i + 1) % rem_len];
                if cross_2d(pts[prev], pts[curr], pts[next]) * sign > 0.0 {
                    triangles.push([prev as u32, curr as u32, next as u32]);
                    remaining.remove(i);
                    clipped = true;
                    break;
                }
            }
            if !clipped && !remaining.is_empty() {
                let prev = remaining[remaining.len() - 1];
                let curr = remaining[0];
                let next = remaining[1];
                triangles.push([prev as u32, curr as u32, next as u32]);
                remaining.remove(0);
            }
        }
    }

    if remaining.len() == 3 {
        triangles.push([remaining[0] as u32, remaining[1] as u32, remaining[2] as u32]);
    }

    triangles
}

/// Renders a 2D path or polygon safely and robustly.
///
/// Fills are rendered using an explicit ear-clipped `egui::Mesh` so no normal / miter
/// calculations occur and coordinates stay strictly bounded.
/// Strokes with sharp hairpin turns are drawn as individual segments to prevent
/// tessellator extrusion spikes.
pub fn draw_robust_path(
    painter: &Painter,
    pts: &[Pos2],
    closed: bool,
    fill_color: Color32,
    stroke: Stroke,
) {
    if pts.len() < 2 {
        return;
    }

    // Filter out non-finite and consecutive duplicate coordinates
    let mut clean_pts: Vec<Pos2> = Vec::with_capacity(pts.len());
    for &p in pts {
        if !p.x.is_finite() || !p.y.is_finite() {
            continue;
        }
        if let Some(&last) = clean_pts.last() {
            if (p.x - last.x).abs() < 0.001 && (p.y - last.y).abs() < 0.001 {
                continue;
            }
        }
        clean_pts.push(p);
    }

    // Remove closing duplicate vertex if closed
    if closed && clean_pts.len() >= 3 {
        let p0 = clean_pts[0];
        if let Some(&plast) = clean_pts.last() {
            if (plast.x - p0.x).abs() < 0.001 && (plast.y - p0.y).abs() < 0.001 {
                clean_pts.pop();
            }
        }
    }

    if clean_pts.len() < 2 {
        return;
    }

    // 1. Draw fill via ear-clipping Mesh
    if closed && fill_color.a() > 0 && clean_pts.len() >= 3 {
        let tris = triangulate_polygon(&clean_pts);
        if !tris.is_empty() {
            let mut mesh = Mesh::default();
            mesh.vertices = clean_pts
                .iter()
                .map(|&pos| Vertex {
                    pos,
                    uv: WHITE_UV,
                    color: fill_color,
                })
                .collect();
            mesh.indices.reserve(tris.len() * 3);
            for tri in tris {
                mesh.indices.push(tri[0]);
                mesh.indices.push(tri[1]);
                mesh.indices.push(tri[2]);
            }
            painter.add(Shape::Mesh(mesh.into()));
        }
    }

    // 2. Draw stroke
    if stroke.width > 0.0 && stroke.color.a() > 0 {
        if closed && clean_pts.len() >= 3 {
            // Check for hairpin turns
            let mut has_hairpin = false;
            let n = clean_pts.len();
            for i in 0..n {
                let p_prev = clean_pts[(i + n - 1) % n];
                let p_curr = clean_pts[i];
                let p_next = clean_pts[(i + 1) % n];
                let d0 = p_curr - p_prev;
                let d1 = p_next - p_curr;
                let len0 = (d0.x * d0.x + d0.y * d0.y).sqrt();
                let len1 = (d1.x * d1.x + d1.y * d1.y).sqrt();
                if len0 > 1e-4 && len1 > 1e-4 {
                    let dot = (d0.x * d1.x + d0.y * d1.y) / (len0 * len1);
                    if dot < -0.92 {
                        has_hairpin = true;
                        break;
                    }
                }
            }

            if has_hairpin {
                // Render as separate segments to avoid miter explosion
                for i in 0..n {
                    let p0 = clean_pts[i];
                    let p1 = clean_pts[(i + 1) % n];
                    painter.line_segment([p0, p1], stroke);
                }
            } else {
                let mut loop_pts = clean_pts.clone();
                loop_pts.push(clean_pts[0]);
                painter.add(Shape::line(loop_pts, stroke));
            }
        } else if clean_pts.len() >= 2 {
            painter.add(Shape::line(clean_pts, stroke));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_triangulate_square() {
        let pts = vec![
            Pos2::new(0.0, 0.0),
            Pos2::new(10.0, 0.0),
            Pos2::new(10.0, 10.0),
            Pos2::new(0.0, 10.0),
        ];
        let tris = triangulate_polygon(&pts);
        assert_eq!(tris.len(), 2, "Square should be triangulated into 2 triangles");
        for tri in &tris {
            for &idx in tri {
                assert!((idx as usize) < pts.len());
            }
        }
    }

    #[test]
    fn test_triangulate_concave_arrow() {
        // Arrow pointing right:
        // (0, 2), (5, 2), (5, 0), (10, 4), (5, 8), (5, 6), (0, 6)
        let pts = vec![
            Pos2::new(0.0, 2.0),
            Pos2::new(5.0, 2.0),
            Pos2::new(5.0, 0.0),
            Pos2::new(10.0, 4.0),
            Pos2::new(5.0, 8.0),
            Pos2::new(5.0, 6.0),
            Pos2::new(0.0, 6.0),
        ];
        let tris = triangulate_polygon(&pts);
        assert_eq!(tris.len(), 5, "7-vertex polygon triangulates into 5 triangles");
        for tri in &tris {
            for &idx in tri {
                assert!((idx as usize) < pts.len());
            }
        }
    }

    #[test]
    fn test_triangulate_hairpin_ribbon() {
        // A folded ribbon shape with sharp angles
        let pts = vec![
            Pos2::new(10.0, 10.0),
            Pos2::new(50.0, 10.0),
            Pos2::new(50.001, 10.001),
            Pos2::new(15.0, 12.0),
            Pos2::new(10.0, 10.001),
        ];
        let tris = triangulate_polygon(&pts);
        assert!(!tris.is_empty());
        for tri in &tris {
            for &idx in tri {
                assert!((idx as usize) < pts.len());
            }
        }
    }
}

