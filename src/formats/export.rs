use crate::model::{ArrowType, DiagramDocument, DiagramPage, DiagramShape, Point, PrimOp};
use crate::routing::find_wire_jumps;
use std::f64::consts::PI;

/// Exports a `DiagramPage` to a standalone, print-ready `.svg` vector document.
pub fn export_page_svg(page: &DiagramPage, show_jumps: bool) -> String {
    let mut svg = format!(
        r##"<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{:.1}mm" height="{:.1}mm" viewBox="0 0 {:.2} {:.2}">
  <rect width="100%" height="100%" fill="#ffffff"/>
"##,
        page.width_mm, page.height_mm, page.width_mm, page.height_mm
    );

    // Render shapes
    for shape in &page.shapes {
        if !is_layer_visible(page, &shape.layer) {
            continue;
        }
        render_shape_svg(page, shape, &mut svg);
    }

    // Render connectors
    let jumps = if show_jumps {
        find_wire_jumps(&page.connectors)
    } else {
        Vec::new()
    };

    for conn in &page.connectors {
        if !is_layer_visible(page, &conn.layer) {
            continue;
        }
        let stroke = rgba_hex(conn.stroke_color);
        let sw = (conn.stroke_width * 0.35).max(0.35);
        let pts = if conn.waypoints.len() >= 2 {
            &conn.waypoints
        } else {
            continue;
        };

        let mut d = format!("M {:.2} {:.2}", pts[0].x, pts[0].y);
        for p in &pts[1..] {
            d.push_str(&format!(" L {:.2} {:.2}", p.x, p.y));
        }
        let dash = if conn.dashed {
            r#" stroke-dasharray="2.5,1.8""#
        } else {
            ""
        };
        svg.push_str(&format!(
            "  <path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{:.2}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"{}/>\n",
            d, stroke, sw, dash
        ));

        // Draw wire jump bridge arcs on this connector
        for j in jumps.iter().filter(|j| j.connector_id == conn.id) {
            let r = 1.6;
            svg.push_str(&format!(
                "  <line x1=\"{:.2}\" y1=\"{:.2}\" x2=\"{:.2}\" y2=\"{:.2}\" stroke=\"#ffffff\" stroke-width=\"{:.2}\"/>\n",
                j.point.x - r,
                j.point.y,
                j.point.x + r,
                j.point.y,
                sw * 2.2
            ));
            svg.push_str(&format!(
                "  <path d=\"M {:.2} {:.2} A {:.2} {:.2} 0 0 1 {:.2} {:.2}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{:.2}\"/>\n",
                j.point.x - r,
                j.point.y,
                r,
                r,
                j.point.x + r,
                j.point.y,
                stroke,
                sw
            ));
        }

        // Draw end/start markers (junction dots or arrows)
        if conn.start_arrow == ArrowType::JunctionDot {
            svg.push_str(&format!(
                "  <circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"1.1\" fill=\"{}\"/>\n",
                pts[0].x, pts[0].y, stroke
            ));
        }
        if conn.end_arrow == ArrowType::JunctionDot {
            let last = pts.last().unwrap();
            svg.push_str(&format!(
                "  <circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"1.1\" fill=\"{}\"/>\n",
                last.x, last.y, stroke
            ));
        } else if conn.end_arrow == ArrowType::Arrow && pts.len() >= 2 {
            let p_prev = pts[pts.len() - 2];
            let p_end = pts[pts.len() - 1];
            let ang = (p_end.y - p_prev.y).atan2(p_end.x - p_prev.x);
            let alen = 2.8;
            let a1 = Point::new(
                p_end.x - alen * (ang - 0.45).cos(),
                p_end.y - alen * (ang - 0.45).sin(),
            );
            let a2 = Point::new(
                p_end.x - alen * (ang + 0.45).cos(),
                p_end.y - alen * (ang + 0.45).sin(),
            );
            svg.push_str(&format!(
                "  <polygon points=\"{:.2},{:.2} {:.2},{:.2} {:.2},{:.2}\" fill=\"{}\"/>\n",
                p_end.x, p_end.y, a1.x, a1.y, a2.x, a2.y, stroke
            ));
        }

        if !conn.label.is_empty() && pts.len() >= 2 {
            let mid = pts[pts.len() / 2 - 1].midpoint(pts[pts.len() / 2]);
            svg.push_str(&format!(
                "  <text x=\"{:.2}\" y=\"{:.2}\" font-family=\"sans-serif\" font-size=\"2.8\" fill=\"{}\" text-anchor=\"middle\">{}</text>\n",
                mid.x,
                mid.y - 1.2,
                stroke,
                escape_xml(&conn.label)
            ));
        }
    }

    svg.push_str("</svg>\n");
    svg
}

fn render_shape_svg(page: &DiagramPage, shape: &DiagramShape, svg: &mut String) {
    let stroke = rgba_hex(shape.stroke_color);
    let fill = rgba_hex(shape.fill_color);
    let fill_alpha = shape.fill_color[3] as f32 / 255.0;
    let sw = (shape.stroke_width * 0.35).max(0.3);

    for op in &shape.geometry {
        match op {
            PrimOp::Line { x1, y1, x2, y2 } => {
                let p1 = shape.local_to_page(*x1, *y1);
                let p2 = shape.local_to_page(*x2, *y2);
                svg.push_str(&format!(
                    "  <line x1=\"{:.2}\" y1=\"{:.2}\" x2=\"{:.2}\" y2=\"{:.2}\" stroke=\"{}\" stroke-width=\"{:.2}\" stroke-linecap=\"round\"/>\n",
                    p1.x, p1.y, p2.x, p2.y, stroke, sw
                ));
            }
            PrimOp::Rect {
                x,
                y,
                w,
                h,
                filled,
            } => {
                let p0 = shape.local_to_page(*x, *y);
                let p1 = shape.local_to_page(*x + *w, *y);
                let p2 = shape.local_to_page(*x + *w, *y + *h);
                let p3 = shape.local_to_page(*x, *y + *h);
                let fill_attr = if *filled && fill_alpha > 0.01 {
                    format!("fill=\"{}\" fill-opacity=\"{:.2}\"", fill, fill_alpha)
                } else {
                    "fill=\"none\"".to_string()
                };
                svg.push_str(&format!(
                    "  <polygon points=\"{:.2},{:.2} {:.2},{:.2} {:.2},{:.2} {:.2},{:.2}\" {} stroke=\"{}\" stroke-width=\"{:.2}\"/>\n",
                    p0.x, p0.y, p1.x, p1.y, p2.x, p2.y, p3.x, p3.y, fill_attr, stroke, sw
                ));
            }
            PrimOp::Circle {
                cx,
                cy,
                r,
                filled,
            } => {
                let c = shape.local_to_page(*cx, *cy);
                let rx = *r * shape.width;
                let ry = *r * shape.height;
                let fill_attr = if *filled && fill_alpha > 0.01 {
                    format!("fill=\"{}\" fill-opacity=\"{:.2}\"", fill, fill_alpha)
                } else {
                    "fill=\"none\"".to_string()
                };
                svg.push_str(&format!(
                    "  <ellipse cx=\"{:.2}\" cy=\"{:.2}\" rx=\"{:.2}\" ry=\"{:.2}\" {} stroke=\"{}\" stroke-width=\"{:.2}\"/>\n",
                    c.x, c.y, rx, ry, fill_attr, stroke, sw
                ));
            }
            PrimOp::Arc {
                cx,
                cy,
                rx,
                ry,
                start_deg,
                sweep_deg,
            } => {
                let steps = 18;
                let mut d = String::new();
                for i in 0..=steps {
                    let t = i as f64 / steps as f64;
                    let ang = (start_deg + sweep_deg * t) * PI / 180.0;
                    let lx = cx + rx * ang.cos();
                    let ly = cy + ry * ang.sin();
                    let p = shape.local_to_page(lx, ly);
                    if i == 0 {
                        d.push_str(&format!("M {:.2} {:.2}", p.x, p.y));
                    } else {
                        d.push_str(&format!(" L {:.2} {:.2}", p.x, p.y));
                    }
                }
                svg.push_str(&format!(
                    "  <path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{:.2}\"/>\n",
                    d, stroke, sw
                ));
            }
            PrimOp::Polyline {
                points,
                closed,
                filled,
            } => {
                if points.is_empty() {
                    continue;
                }
                let mut pts_str = String::new();
                for (lx, ly) in points {
                    let p = shape.local_to_page(*lx, *ly);
                    pts_str.push_str(&format!("{:.2},{:.2} ", p.x, p.y));
                }
                let fill_attr = if *filled && fill_alpha > 0.01 {
                    format!("fill=\"{}\" fill-opacity=\"{:.2}\"", fill, fill_alpha)
                } else {
                    "fill=\"none\"".to_string()
                };
                let tag = if *closed { "polygon" } else { "polyline" };
                svg.push_str(&format!(
                    "  <{} points=\"{}\" {} stroke=\"{}\" stroke-width=\"{:.2}\"/>\n",
                    tag,
                    pts_str.trim(),
                    fill_attr,
                    stroke,
                    sw
                ));
            }
            PrimOp::Path {
                points,
                closed,
                filled,
                fill_color,
                stroke_color,
                stroke_width: sw_opt,
            } => {
                if points.is_empty() {
                    continue;
                }
                let mut pts_str = String::new();
                for (lx, ly) in points {
                    let p = shape.local_to_page(*lx, *ly);
                    pts_str.push_str(&format!("{:.2},{:.2} ", p.x, p.y));
                }
                let f_str = fill_color.map(rgba_hex).unwrap_or_else(|| fill.clone());
                let fill_attr = if *filled {
                    format!("fill=\"{}\"", f_str)
                } else {
                    "fill=\"none\"".to_string()
                };
                let s_str = stroke_color.map(rgba_hex).unwrap_or_else(|| stroke.clone());
                let cur_sw = sw_opt.map(|w| (w * 0.35).max(0.2)).unwrap_or(sw);
                let tag = if *closed { "polygon" } else { "polyline" };
                svg.push_str(&format!(
                    "  <{} points=\"{}\" {} stroke=\"{}\" stroke-width=\"{:.2}\"/>\n",
                    tag,
                    pts_str.trim(),
                    fill_attr,
                    s_str,
                    cur_sw
                ));
            }
            PrimOp::Ellipse {
                cx,
                cy,
                rx,
                ry,
                filled,
                fill_color,
                stroke_color,
            } => {
                let c = shape.local_to_page(*cx, *cy);
                let erx = *rx * shape.width;
                let ery = *ry * shape.height;
                let f_str = fill_color.map(rgba_hex).unwrap_or_else(|| fill.clone());
                let fill_attr = if *filled {
                    format!("fill=\"{}\"", f_str)
                } else {
                    "fill=\"none\"".to_string()
                };
                let s_str = stroke_color.map(rgba_hex).unwrap_or_else(|| stroke.clone());
                svg.push_str(&format!(
                    "  <ellipse cx=\"{:.2}\" cy=\"{:.2}\" rx=\"{:.2}\" ry=\"{:.2}\" {} stroke=\"{}\" stroke-width=\"{:.2}\"/>\n",
                    c.x, c.y, erx, ery, fill_attr, s_str, sw
                ));
            }
            PrimOp::Text { x, y, text, .. } => {
                let p = shape.local_to_page(*x, *y);
                svg.push_str(&format!(
                    "  <text x=\"{:.2}\" y=\"{:.2}\" font-family=\"sans-serif\" font-size=\"2.4\" fill=\"{}\" text-anchor=\"middle\" dominant-baseline=\"central\">{}</text>\n",
                    p.x,
                    p.y,
                    stroke,
                    escape_xml(text)
                ));
            }
        }
    }

    // Label / Dimension / Room area caption
    let caption = shape_display_caption(page, shape);
    if !caption.is_empty() {
        let label_y = if shape.is_room_area || shape.is_dimension {
            shape.y
        } else {
            shape.y - shape.height * 0.5 - 2.0
        };
        svg.push_str(&format!(
            "  <text x=\"{:.2}\" y=\"{:.2}\" font-family=\"sans-serif\" font-size=\"3.0\" font-weight=\"bold\" fill=\"{}\" text-anchor=\"middle\">{}</text>\n",
            shape.x,
            label_y,
            stroke,
            escape_xml(&caption)
        ));
    }
}

/// Exports a `DiagramPage` to AutoCAD `.dxf` (ASCII DXF format in millimeters).
pub fn export_page_dxf(page: &DiagramPage) -> String {
    let mut dxf = String::from("0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n");
    dxf.push_str("0\nSECTION\n2\nENTITIES\n");

    let h_page = page.height_mm;

    for shape in &page.shapes {
        if !is_layer_visible(page, &shape.layer) {
            continue;
        }
        let layer = sanitize_dxf_layer(&shape.layer);

        for op in &shape.geometry {
            match op {
                PrimOp::Line { x1, y1, x2, y2 } => {
                    let p1 = shape.local_to_page(*x1, *y1);
                    let p2 = shape.local_to_page(*x2, *y2);
                    write_dxf_line(&mut dxf, &layer, p1.x, h_page - p1.y, p2.x, h_page - p2.y);
                }
                PrimOp::Rect { x, y, w, h, .. } => {
                    let corners = [
                        shape.local_to_page(*x, *y),
                        shape.local_to_page(*x + *w, *y),
                        shape.local_to_page(*x + *w, *y + *h),
                        shape.local_to_page(*x, *y + *h),
                    ];
                    for i in 0..4 {
                        let a = corners[i];
                        let b = corners[(i + 1) % 4];
                        write_dxf_line(&mut dxf, &layer, a.x, h_page - a.y, b.x, h_page - b.y);
                    }
                }
                PrimOp::Circle { cx, cy, r, .. } => {
                    let c = shape.local_to_page(*cx, *cy);
                    let rad = *r * (shape.width + shape.height) * 0.5;
                    dxf.push_str(&format!(
                        "0\nCIRCLE\n8\n{}\n10\n{:.3}\n20\n{:.3}\n30\n0.0\n40\n{:.3}\n",
                        layer,
                        c.x,
                        h_page - c.y,
                        rad
                    ));
                }
                PrimOp::Polyline { points, closed, .. }
                | PrimOp::Path { points, closed, .. } => {
                    for i in 0..points.len().saturating_sub(1) {
                        let a = shape.local_to_page(points[i].0, points[i].1);
                        let b = shape.local_to_page(points[i + 1].0, points[i + 1].1);
                        write_dxf_line(&mut dxf, &layer, a.x, h_page - a.y, b.x, h_page - b.y);
                    }
                    if *closed && points.len() >= 3 {
                        let a = shape.local_to_page(points.last().unwrap().0, points.last().unwrap().1);
                        let b = shape.local_to_page(points[0].0, points[0].1);
                        write_dxf_line(&mut dxf, &layer, a.x, h_page - a.y, b.x, h_page - b.y);
                    }
                }
                PrimOp::Ellipse { cx, cy, rx, ry, .. } => {
                    let c = shape.local_to_page(*cx, *cy);
                    let rad = (*rx + *ry) * 0.5 * (shape.width + shape.height) * 0.5;
                    dxf.push_str(&format!(
                        "0\nCIRCLE\n8\n{}\n10\n{:.3}\n20\n{:.3}\n30\n0.0\n40\n{:.3}\n",
                        layer,
                        c.x,
                        h_page - c.y,
                        rad
                    ));
                }
                _ => {}
            }
        }

        let caption = shape_display_caption(page, shape);
        if !caption.is_empty() {
            dxf.push_str(&format!(
                "0\nTEXT\n8\n{}\n10\n{:.3}\n20\n{:.3}\n30\n0.0\n40\n2.5\n1\n{}\n",
                layer,
                shape.x,
                h_page - shape.y,
                caption
            ));
        }
    }

    for conn in &page.connectors {
        if !is_layer_visible(page, &conn.layer) {
            continue;
        }
        let layer = sanitize_dxf_layer(&conn.layer);
        for seg in conn.waypoints.windows(2) {
            write_dxf_line(
                &mut dxf,
                &layer,
                seg[0].x,
                h_page - seg[0].y,
                seg[1].x,
                h_page - seg[1].y,
            );
        }
    }

    dxf.push_str("0\nENDSEC\n0\nEOF\n");
    dxf
}

/// Exports a Bill of Materials (Osaluettelo & Huoneluettelo) as UTF-8 CSV.
pub fn export_bom_csv(doc: &DiagramDocument) -> String {
    let mut csv = String::from(
        "Sivu;Tunnus;Komponentti / Symboli;Arvo / Mitoitus;Taso;Todellinen Leveys;Todellinen Korkeus;X (mm);Y (mm)\n",
    );

    for page in &doc.pages {
        for s in &page.shapes {
            let val_or_dim = if s.is_room_area {
                page.scale.format_area(s.width, s.height)
            } else if s.is_dimension {
                page.scale.format_length(s.width.max(s.height))
            } else {
                s.value.clone()
            };
            let real_w = page.scale.format_length(s.width);
            let real_h = page.scale.format_length(s.height);
            csv.push_str(&format!(
                "{};{};{};{};{};{};{};{:.1};{:.1}\n",
                page.name.replace(';', ","),
                s.label.replace(';', ","),
                s.name.replace(';', ","),
                val_or_dim.replace(';', ","),
                s.layer.replace(';', ","),
                real_w,
                real_h,
                s.x,
                s.y
            ));
        }
    }
    csv
}

pub fn shape_display_caption(page: &DiagramPage, shape: &DiagramShape) -> String {
    if shape.is_dimension {
        let len_str = page.scale.format_length(shape.width.max(shape.height));
        if shape.label.is_empty() {
            len_str
        } else {
            format!("{}: {}", shape.label, len_str)
        }
    } else if shape.is_room_area {
        let area_str = page.scale.format_area(shape.width, shape.height);
        let w_m = page.scale.paper_mm_to_real_mm(shape.width) / 1000.0;
        let h_m = page.scale.paper_mm_to_real_mm(shape.height) / 1000.0;
        format!("{} — {} ({:.2}×{:.2} m)", shape.label, area_str, w_m, h_m)
    } else if !shape.label.is_empty() && !shape.value.is_empty() {
        format!("{} ({})", shape.label, shape.value)
    } else if !shape.label.is_empty() {
        shape.label.clone()
    } else if !shape.value.is_empty() {
        shape.value.clone()
    } else if !shape.name.is_empty()
        && !shape.name.starts_with("Ulkoseinä")
        && !shape.name.starts_with("Väliseinä")
        && !shape.name.starts_with("Seinä")
    {
        shape.name.clone()
    } else {
        String::new()
    }
}

fn is_layer_visible(page: &DiagramPage, layer_name: &str) -> bool {
    page.layers
        .iter()
        .find(|l| l.name == layer_name)
        .map(|l| l.visible)
        .unwrap_or(true)
}

fn write_dxf_line(dxf: &mut String, layer: &str, x1: f64, y1: f64, x2: f64, y2: f64) {
    dxf.push_str(&format!(
        "0\nLINE\n8\n{}\n10\n{:.3}\n20\n{:.3}\n30\n0.0\n11\n{:.3}\n21\n{:.3}\n31\n0.0\n",
        layer, x1, y1, x2, y2
    ));
}

fn sanitize_dxf_layer(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}

fn rgba_hex(c: [u8; 4]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
