//! PhotonCAM / LightBurn (.lbrn2) import and export integration.
//!
//! Enables seamless round-trip CAD/CAM interoperability between PhotonVision
//! and PhotonCAM (laser cutting, CNC routing, engraving, marking, and relief).

use crate::formats::export::shape_display_caption;
use crate::model::{
    DiagramDocument, DiagramPage, DiagramShape, Point, PrimOp,
};
use roxmltree::Node;
use std::collections::HashMap;

/// Helper to escape XML special characters
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// 2D Affine Transform for .lbrn2 `<XForm>` tags
#[derive(Debug, Clone, Copy, PartialEq)]
struct Transform2D {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    tx: f64,
    ty: f64,
}

impl Default for Transform2D {
    fn default() -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            tx: 0.0,
            ty: 0.0,
        }
    }
}

impl Transform2D {
    fn parse(s: &str) -> Self {
        let parts: Vec<f64> = s
            .split_whitespace()
            .filter_map(|p| p.parse::<f64>().ok())
            .collect();
        if parts.len() >= 6 {
            Self {
                a: parts[0],
                b: parts[1],
                c: parts[2],
                d: parts[3],
                tx: parts[4],
                ty: parts[5],
            }
        } else {
            Self::default()
        }
    }

    fn apply(&self, p: Point) -> Point {
        Point::new(
            self.a * p.x + self.c * p.y + self.tx,
            self.b * p.x + self.d * p.y + self.ty,
        )
    }

    fn multiply(&self, other: &Self) -> Self {
        Self {
            a: self.a * other.a + self.c * other.b,
            b: self.b * other.a + self.d * other.b,
            c: self.a * other.c + self.c * other.d,
            d: self.b * other.c + self.d * other.d,
            tx: self.a * other.tx + self.c * other.ty + self.tx,
            ty: self.b * other.tx + self.d * other.ty + self.ty,
        }
    }
}

/// Exports a `DiagramDocument` (its current active page) to standard LightBurn / PhotonCAM `.lbrn2` XML.
pub fn export_document_lbrn2(doc: &DiagramDocument) -> String {
    let page = doc.current_page();
    export_page_lbrn2(page)
}

/// Exports a `DiagramPage` to standard LightBurn / PhotonCAM `.lbrn2` XML.
pub fn export_page_lbrn2(page: &DiagramPage) -> String {
    let mut xml = String::with_capacity(8192);

    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<LightBurnProject AppVersion=\"1.6.00\" FormatVersion=\"1\" MaterialHeight=\"0\">\n");

    // 1. CutSettings (Standard 3 laser layers)
    // Layer 0: Cut (Vector perimeter, outlines, outer walls, openings)
    xml.push_str("    <CutSetting type=\"Cut\">\n");
    xml.push_str("        <index Value=\"0\"/>\n");
    xml.push_str("        <name Value=\"C00 (Cut / Leikkaus)\"/>\n");
    xml.push_str("        <minPower Value=\"20.0\"/>\n");
    xml.push_str("        <maxPower Value=\"80.0\"/>\n");
    xml.push_str("        <speed Value=\"25.0\"/>\n");
    xml.push_str("        <priority Value=\"0\"/>\n");
    xml.push_str("        <numPasses Value=\"1\"/>\n");
    xml.push_str("        <zOffset Value=\"0.00\"/>\n");
    xml.push_str("        <enableAir Value=\"1\"/>\n");
    xml.push_str("        <kerf Value=\"0.000\"/>\n");
    xml.push_str("        <lineInterval Value=\"0.100\"/>\n");
    xml.push_str("    </CutSetting>\n");

    // Layer 1: Scan (Raster / Vector Fill / Engraving / Texts & Logos)
    xml.push_str("    <CutSetting type=\"Scan\">\n");
    xml.push_str("        <index Value=\"1\"/>\n");
    xml.push_str("        <name Value=\"C01 (Scan / Kaiverrus)\"/>\n");
    xml.push_str("        <minPower Value=\"10.0\"/>\n");
    xml.push_str("        <maxPower Value=\"45.0\"/>\n");
    xml.push_str("        <speed Value=\"250.0\"/>\n");
    xml.push_str("        <priority Value=\"1\"/>\n");
    xml.push_str("        <numPasses Value=\"1\"/>\n");
    xml.push_str("        <zOffset Value=\"0.00\"/>\n");
    xml.push_str("        <enableAir Value=\"0\"/>\n");
    xml.push_str("        <kerf Value=\"0.000\"/>\n");
    xml.push_str("        <lineInterval Value=\"0.080\"/>\n");
    xml.push_str("    </CutSetting>\n");

    // Layer 2: Mark / Wires / Dimensions (Low power vector line / scoring)
    xml.push_str("    <CutSetting type=\"Cut\">\n");
    xml.push_str("        <index Value=\"2\"/>\n");
    xml.push_str("        <name Value=\"C02 (Mark / Merkintä &amp; Johdot)\"/>\n");
    xml.push_str("        <minPower Value=\"10.0\"/>\n");
    xml.push_str("        <maxPower Value=\"25.0\"/>\n");
    xml.push_str("        <speed Value=\"100.0\"/>\n");
    xml.push_str("        <priority Value=\"2\"/>\n");
    xml.push_str("        <numPasses Value=\"1\"/>\n");
    xml.push_str("        <zOffset Value=\"0.00\"/>\n");
    xml.push_str("        <enableAir Value=\"1\"/>\n");
    xml.push_str("        <kerf Value=\"0.000\"/>\n");
    xml.push_str("        <lineInterval Value=\"0.100\"/>\n");
    xml.push_str("    </CutSetting>\n");

    // 2. Export Shapes
    for shape in &page.shapes {
        let is_visible = page
            .layers
            .iter()
            .find(|l| l.name == shape.layer)
            .map(|l| l.visible)
            .unwrap_or(true);
        if !is_visible {
            continue;
        }

        export_shape_lbrn2(page, shape, &mut xml);
    }

    // 3. Export Connectors (Wires as vector lines on Layer 2)
    for conn in &page.connectors {
        let is_visible = page
            .layers
            .iter()
            .find(|l| l.name == conn.layer)
            .map(|l| l.visible)
            .unwrap_or(true);
        if !is_visible || conn.waypoints.len() < 2 {
            continue;
        }

        let mut vl = String::new();
        let mut pl = String::new();
        for (i, pt) in conn.waypoints.iter().enumerate() {
            vl.push_str(&format!("V{:.3} {:.3}c0x1c1x1", pt.x, pt.y));
            if i < conn.waypoints.len() - 1 {
                pl.push_str(&format!("L{} {} ", i, i + 1));
            }
        }

        xml.push_str("    <Shape Type=\"Path\" CutIndex=\"2\">\n");
        xml.push_str("        <XForm>1 0 0 1 0 0</XForm>\n");
        xml.push_str(&format!("        <VertList>{}</VertList>\n", vl));
        xml.push_str(&format!("        <PrimList>{}</PrimList>\n", pl.trim()));
        xml.push_str("    </Shape>\n");

        if !conn.label.is_empty() {
            let mid_idx = conn.waypoints.len() / 2;
            let mid = if mid_idx > 0 {
                Point::new(
                    (conn.waypoints[mid_idx - 1].x + conn.waypoints[mid_idx].x) * 0.5,
                    (conn.waypoints[mid_idx - 1].y + conn.waypoints[mid_idx].y) * 0.5 - 2.5,
                )
            } else {
                conn.waypoints[0]
            };
            xml.push_str(&format!(
                "    <Shape Type=\"Text\" CutIndex=\"1\" Font=\"Arial\" Str=\"{}\" H=\"3.00\" Ah=\"1\" Av=\"1\">\n",
                xml_escape(&conn.label)
            ));
            xml.push_str(&format!(
                "        <XForm>1 0 0 1 {:.3} {:.3}</XForm>\n",
                mid.x, mid.y
            ));
            xml.push_str("    </Shape>\n");
        }
    }

    xml.push_str("</LightBurnProject>\n");
    xml
}

fn export_shape_lbrn2(page: &DiagramPage, shape: &DiagramShape, out: &mut String) {
    let mut has_exported_geometry = false;

    for op in &shape.geometry {
        match op {
            PrimOp::Rect {
                x,
                y,
                w,
                h,
                filled,
            } => {
                has_exported_geometry = true;
                let actual_w = (*w * shape.width).abs();
                let actual_h = (*h * shape.height).abs();
                let cx = shape.x + (*x + *w * 0.5 - 0.5) * shape.width;
                let cy = shape.y + (*y + *h * 0.5 - 0.5) * shape.height;
                let cut_idx = if *filled { 1 } else { 0 };

                out.push_str(&format!(
                    "    <Shape Type=\"Rect\" CutIndex=\"{}\" W=\"{:.3}\" H=\"{:.3}\" Cr=\"0.000\">\n",
                    cut_idx, actual_w, actual_h
                ));
                out.push_str(&format!(
                    "        <XForm>1 0 0 1 {:.3} {:.3}</XForm>\n",
                    cx, cy
                ));
                out.push_str("    </Shape>\n");
            }
            PrimOp::Circle { cx, cy, r, filled } => {
                has_exported_geometry = true;
                let center = shape.local_to_page(*cx, *cy);
                let radius = *r * shape.width.min(shape.height);
                let cut_idx = if *filled { 1 } else { 0 };

                out.push_str(&format!(
                    "    <Shape Type=\"Circle\" CutIndex=\"{}\" R=\"{:.3}\">\n",
                    cut_idx, radius
                ));
                out.push_str(&format!(
                    "        <XForm>1 0 0 1 {:.3} {:.3}</XForm>\n",
                    center.x, center.y
                ));
                out.push_str("    </Shape>\n");
            }
            PrimOp::Ellipse {
                cx,
                cy,
                rx,
                ry,
                filled,
                ..
            } => {
                has_exported_geometry = true;
                let center = shape.local_to_page(*cx, *cy);
                let erx = *rx * shape.width;
                let ery = *ry * shape.height;
                let cut_idx = if *filled { 1 } else { 0 };

                out.push_str(&format!(
                    "    <Shape Type=\"Circle\" CutIndex=\"{}\" Rx=\"{:.3}\" Ry=\"{:.3}\">\n",
                    cut_idx, erx, ery
                ));
                out.push_str(&format!(
                    "        <XForm>1 0 0 1 {:.3} {:.3}</XForm>\n",
                    center.x, center.y
                ));
                out.push_str("    </Shape>\n");
            }
            PrimOp::Polyline {
                points,
                closed,
                filled,
            }
            | PrimOp::Path {
                points,
                closed,
                filled,
                ..
            } => {
                if points.len() >= 2 {
                    has_exported_geometry = true;
                    let cut_idx = if *filled { 1 } else { 0 };
                    let mut vl = String::new();
                    let mut pl = String::new();

                    for (i, (lx, ly)) in points.iter().enumerate() {
                        let p = shape.local_to_page(*lx, *ly);
                        vl.push_str(&format!("V{:.3} {:.3}c0x1c1x1", p.x, p.y));
                        if i < points.len() - 1 {
                            pl.push_str(&format!("L{} {} ", i, i + 1));
                        }
                    }
                    if *closed && points.len() > 2 {
                        pl.push_str(&format!("L{} 0", points.len() - 1));
                    }

                    out.push_str(&format!(
                        "    <Shape Type=\"Path\" CutIndex=\"{}\">\n",
                        cut_idx
                    ));
                    out.push_str("        <XForm>1 0 0 1 0 0</XForm>\n");
                    out.push_str(&format!("        <VertList>{}</VertList>\n", vl));
                    out.push_str(&format!("        <PrimList>{}</PrimList>\n", pl.trim()));
                    out.push_str("    </Shape>\n");
                }
            }
            PrimOp::Text {
                x,
                y,
                text,
                size_rel,
            } => {
                has_exported_geometry = true;
                let p = shape.local_to_page(*x, *y);
                let h_mm = (*size_rel * shape.height).clamp(2.0, 50.0);

                out.push_str(&format!(
                    "    <Shape Type=\"Text\" CutIndex=\"1\" Font=\"Arial\" Str=\"{}\" H=\"{:.2}\" Ah=\"1\" Av=\"1\">\n",
                    xml_escape(text), h_mm
                ));
                out.push_str(&format!("        <XForm>1 0 0 1 {:.3} {:.3}</XForm>\n", p.x, p.y));
                out.push_str("    </Shape>\n");
            }
            PrimOp::Line { x1, y1, x2, y2 } => {
                has_exported_geometry = true;
                let p1 = shape.local_to_page(*x1, *y1);
                let p2 = shape.local_to_page(*x2, *y2);
                out.push_str("    <Shape Type=\"Path\" CutIndex=\"0\">\n");
                out.push_str("        <XForm>1 0 0 1 0 0</XForm>\n");
                out.push_str(&format!(
                    "        <VertList>V{:.3} {:.3}c0x1c1x1V{:.3} {:.3}c0x1c1x1</VertList>\n",
                    p1.x, p1.y, p2.x, p2.y
                ));
                out.push_str("        <PrimList>L0 1</PrimList>\n");
                out.push_str("    </Shape>\n");
            }
            PrimOp::Arc { .. } => {}
        }
    }

    // If shape has no vector geometry primitives (standard rectangular component/box)
    if !has_exported_geometry {
        let cut_idx = if shape.fill_color[3] > 0 { 1 } else { 0 };

        out.push_str(&format!(
            "    <Shape Type=\"Rect\" CutIndex=\"{}\" W=\"{:.3}\" H=\"{:.3}\" Cr=\"0.000\">\n",
            cut_idx, shape.width, shape.height
        ));
        out.push_str(&format!(
            "        <XForm>1 0 0 1 {:.3} {:.3}</XForm>\n",
            shape.x, shape.y
        ));
        out.push_str("    </Shape>\n");
    }

    // Export shape label or dimension/room area caption if present
    let caption = shape_display_caption(page, shape);
    if !caption.is_empty() {
        let label_pt = shape.label_position();
        let h_mm = 3.20_f64;

        out.push_str(&format!(
            "    <Shape Type=\"Text\" CutIndex=\"1\" Font=\"Arial\" Str=\"{}\" H=\"{:.2}\" Ah=\"1\" Av=\"1\">\n",
            xml_escape(&caption), h_mm
        ));
        out.push_str(&format!(
            "        <XForm>1 0 0 1 {:.3} {:.3}</XForm>\n",
            label_pt.x, label_pt.y
        ));
        out.push_str("    </Shape>\n");
    }
}

/// Imports a LightBurn / PhotonCAM `.lbrn2` file into a `DiagramDocument`.
pub fn import_lbrn2_bytes(bytes: &[u8]) -> Result<DiagramDocument, String> {
    let xml_str = std::str::from_utf8(bytes)
        .map_err(|e| format!("Virheellinen UTF-8 tiedostosisältö .lbrn2: {}", e))?;

    let doc_tree = roxmltree::Document::parse(xml_str)
        .map_err(|e| format!("XML-jäsennysvirhe .lbrn2: {}", e))?;

    let root = doc_tree.root_element();
    if !root.has_tag_name("LightBurnProject") {
        return Err("Tiedosto ei ole kelvollinen LightBurn/PhotonCAM .lbrn2 -tiedosto".to_string());
    }

    // Collect shared PrimLists if any
    let mut shared_prims: HashMap<String, Vec<(usize, usize, bool)>> = HashMap::new();
    for node in root.children().filter(|n| n.has_tag_name("Shape")) {
        if let Some(pid) = node.attribute("PrimID") {
            if let Some(pl_node) = node.children().find(|n| n.has_tag_name("PrimList")) {
                if let Some(text) = pl_node.text() {
                    shared_prims.insert(pid.to_string(), parse_prim_list_simple(text));
                }
            }
        }
    }

    let mut imported_shapes = Vec::new();
    let mut next_id = 1_u64;

    parse_lbrn2_shapes(
        root,
        Transform2D::default(),
        &shared_prims,
        &mut imported_shapes,
        &mut next_id,
    );

    if imported_shapes.is_empty() {
        return Err("LightBurn-tiedostosta ei löytynyt tuotavia muotoja".to_string());
    }

    // Calculate bounding box to size the page properly
    let mut min_x = f64::MAX;
    let mut min_y = f64::MAX;
    let mut max_x = f64::MIN;
    let mut max_y = f64::MIN;

    for s in &imported_shapes {
        min_x = min_x.min(s.x - s.width * 0.5);
        min_y = min_y.min(s.y - s.height * 0.5);
        max_x = max_x.max(s.x + s.width * 0.5);
        max_y = max_y.max(s.y + s.height * 0.5);
    }

    // Offset shapes so they start at a comfortable margin (20mm, 20mm) if needed
    let margin = 20.0;
    let shift_x = if min_x < margin { margin - min_x } else { 0.0 };
    let shift_y = if min_y < margin { margin - min_y } else { 0.0 };

    if shift_x != 0.0 || shift_y != 0.0 {
        for s in &mut imported_shapes {
            s.x += shift_x;
            s.y += shift_y;
        }
        max_x += shift_x;
        max_y += shift_y;
    }

    let page_w = (max_x + margin).max(297.0); // At least A4
    let page_h = (max_y + margin).max(210.0);

    let mut page = DiagramPage::default();
    page.name = "Laser / CAM".to_string();
    page.width_mm = page_w;
    page.height_mm = page_h;
    page.shapes = imported_shapes;

    let mut doc = DiagramDocument::default();
    doc.pages = vec![page];
    doc.next_id = next_id;

    Ok(doc)
}

fn parse_prim_list_simple(s: &str) -> Vec<(usize, usize, bool)> {
    let mut cmds = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }

        let ch = bytes[i];
        if ch == b'L' || ch == b'B' {
            let is_bezier = ch == b'B';
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            let start_from = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let from: usize = std::str::from_utf8(&bytes[start_from..i])
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);

            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            let start_to = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let to: usize = std::str::from_utf8(&bytes[start_to..i])
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);

            cmds.push((from, to, is_bezier));
        } else {
            i += 1;
        }
    }

    cmds
}

fn parse_vert_list_simple(s: &str) -> Vec<Point> {
    let mut verts = Vec::new();
    for token in s.split('V') {
        let clean = token.trim();
        if clean.is_empty() {
            continue;
        }
        let parts: Vec<&str> = clean.split('c').collect();
        let coords: Vec<&str> = parts[0].split_whitespace().collect();
        if coords.len() >= 2 {
            let x = coords[0].parse::<f64>().unwrap_or(0.0);
            let y = coords[1].parse::<f64>().unwrap_or(0.0);
            verts.push(Point::new(x, y));
        }
    }
    verts
}

fn parse_lbrn2_shapes(
    parent: Node,
    parent_xform: Transform2D,
    shared_prims: &HashMap<String, Vec<(usize, usize, bool)>>,
    out: &mut Vec<DiagramShape>,
    next_id: &mut u64,
) {
    for node in parent.children() {
        if !node.has_tag_name("Shape") {
            if node.tag_name().name().eq_ignore_ascii_case("Children") {
                parse_lbrn2_shapes(node, parent_xform, shared_prims, out, next_id);
            }
            continue;
        }

        let shape_type = node.attribute("Type").unwrap_or("Path");
        let xform_str = node
            .children()
            .find(|n| n.has_tag_name("XForm"))
            .and_then(|n| n.text())
            .unwrap_or("");
        let local_xform = if !xform_str.is_empty() {
            Transform2D::parse(xform_str)
        } else {
            Transform2D::default()
        };
        let cur_xform = parent_xform.multiply(&local_xform);

        match shape_type {
            "Group" => {
                parse_lbrn2_shapes(node, cur_xform, shared_prims, out, next_id);
            }
            "Rect" => {
                let w = node
                    .attribute("W")
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                let h = node
                    .attribute("H")
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                if w > 0.0 && h > 0.0 {
                    let center = cur_xform.apply(Point::ZERO);
                    let s = DiagramShape {
                        id: *next_id,
                        stencil_id: "laser_rect".to_string(),
                        name: "Suorakulmio".to_string(),
                        label: String::new(),
                        value: String::new(),
                        x: center.x,
                        y: center.y,
                        width: w,
                        height: h,
                        rotation_deg: 0.0,
                        flip_x: false,
                        flip_y: false,
                        stroke_color: [30, 41, 59, 255],
                        fill_color: [241, 245, 249, 200],
                        stroke_width: 1.0,
                        dashed: false,
                        layer: "Oletus".to_string(),
                        ports: Vec::new(),
                        geometry: Vec::new(),
                        properties: Vec::new(),
                        is_dimension: false,
                        is_room_area: false,
                        locked: false,
                        text_position: crate::model::TextPosition::Auto,
                    };
                    *next_id += 1;
                    out.push(s);
                }
            }
            "Circle" | "Ellipse" => {
                let r = node
                    .attribute("R")
                    .and_then(|s| s.parse::<f64>().ok())
                    .or_else(|| node.attribute("Rx").and_then(|s| s.parse::<f64>().ok()))
                    .unwrap_or(10.0);
                let ry = node
                    .attribute("Ry")
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(r);
                let center = cur_xform.apply(Point::ZERO);
                let s = DiagramShape {
                    id: *next_id,
                    stencil_id: "laser_circle".to_string(),
                    name: "Ympyrä".to_string(),
                    label: String::new(),
                    value: String::new(),
                    x: center.x,
                    y: center.y,
                    width: r * 2.0,
                    height: ry * 2.0,
                    rotation_deg: 0.0,
                    flip_x: false,
                    flip_y: false,
                    stroke_color: [30, 41, 59, 255],
                    fill_color: [241, 245, 249, 200],
                    stroke_width: 1.0,
                    dashed: false,
                    layer: "Oletus".to_string(),
                    ports: Vec::new(),
                    geometry: vec![PrimOp::Ellipse {
                        cx: 0.5,
                        cy: 0.5,
                        rx: 0.5,
                        ry: 0.5,
                        filled: false,
                        fill_color: None,
                        stroke_color: None,
                    }],
                    properties: Vec::new(),
                    is_dimension: false,
                    is_room_area: false,
                    locked: false,
                    text_position: crate::model::TextPosition::Auto,
                };
                *next_id += 1;
                out.push(s);
            }
            "Text" => {
                let text = node.attribute("Str").unwrap_or("");
                let h = node
                    .attribute("H")
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(5.0);
                if !text.is_empty() {
                    let p = cur_xform.apply(Point::ZERO);
                    let w = (text.len() as f64 * h * 0.7).max(10.0);
                    let s = DiagramShape {
                        id: *next_id,
                        stencil_id: "laser_text".to_string(),
                        name: "Teksti".to_string(),
                        label: text.to_string(),
                        value: String::new(),
                        x: p.x,
                        y: p.y,
                        width: w,
                        height: h * 1.5,
                        rotation_deg: 0.0,
                        flip_x: false,
                        flip_y: false,
                        stroke_color: [30, 41, 59, 255],
                        fill_color: [0, 0, 0, 0],
                        stroke_width: 1.0,
                        dashed: false,
                        layer: "Mitat & Tekstit".to_string(),
                        ports: Vec::new(),
                        geometry: vec![PrimOp::Text {
                            x: 0.5,
                            y: 0.5,
                            text: text.to_string(),
                            size_rel: 0.8,
                        }],
                        properties: Vec::new(),
                        is_dimension: false,
                        is_room_area: false,
                        locked: false,
                        text_position: crate::model::TextPosition::Auto,
                    };
                    *next_id += 1;
                    out.push(s);
                }
            }
            "Path" => {
                if let Some(vl_node) = node.children().find(|n| n.has_tag_name("VertList")) {
                    if let Some(vl_text) = vl_node.text() {
                        let verts = parse_vert_list_simple(vl_text);
                        if verts.len() >= 2 {
                            let world_pts: Vec<Point> = verts.iter().map(|&p| cur_xform.apply(p)).collect();

                            let mut min_x = f64::MAX;
                            let mut min_y = f64::MAX;
                            let mut max_x = f64::MIN;
                            let mut max_y = f64::MIN;
                            for p in &world_pts {
                                min_x = min_x.min(p.x);
                                min_y = min_y.min(p.y);
                                max_x = max_x.max(p.x);
                                max_y = max_y.max(p.y);
                            }
                            let w = (max_x - min_x).max(1.0);
                            let h = (max_y - min_y).max(1.0);

                            // Normalize local coords to 0..1
                            let norm_pts: Vec<(f64, f64)> = world_pts
                                .iter()
                                .map(|p| ((p.x - min_x) / w, (p.y - min_y) / h))
                                .collect();

                            let s = DiagramShape {
                                id: *next_id,
                                stencil_id: "laser_path".to_string(),
                                name: "Polku".to_string(),
                                label: String::new(),
                                value: String::new(),
                                x: min_x + w * 0.5,
                                y: min_y + h * 0.5,
                                width: w,
                                height: h,
                                rotation_deg: 0.0,
                                flip_x: false,
                                flip_y: false,
                                stroke_color: [30, 41, 59, 255],
                                fill_color: [241, 245, 249, 200],
                                stroke_width: 1.0,
                                dashed: false,
                                layer: "Oletus".to_string(),
                                ports: Vec::new(),
                                geometry: vec![PrimOp::Path {
                                    points: norm_pts,
                                    closed: true,
                                    filled: true,
                                    fill_color: Some([226, 232, 240, 220]),
                                    stroke_color: Some([30, 41, 59, 255]),
                                    stroke_width: Some(1.0),
                                }],
                                properties: Vec::new(),
                                is_dimension: false,
                                is_room_area: false,
                                locked: false,
                                text_position: crate::model::TextPosition::Auto,
                            };
                            *next_id += 1;
                            out.push(s);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
