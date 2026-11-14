use crate::formats::zip_archive::ZipArchive;
use crate::model::{
    ArrowType, ConnectionPort, ConnectorStyle, DiagramConnector, DiagramDocument, DiagramPage,
    DiagramShape, DrawingScale, Point, PortDir, PrimOp,
};
use crate::routing::update_all_connectors;
use crate::stencils::{built_in_stencils, StencilCategory, StencilItem};
use roxmltree::{Document, Node};

const INCH_TO_MM: f64 = 25.4;

/// Reads a `.vsdx` (Visio 2013+ Open Packaging Conventions ZIP) file into a `DiagramDocument`.
pub fn read_vsdx_bytes(bytes: &[u8]) -> Result<DiagramDocument, String> {
    let archive = ZipArchive::from_bytes(bytes)?;

    // Fast path: if this .vsdx was saved by PhotonVision and includes our lossless metadata entry,
    // restore the full document directly while remaining 100% compatible with MS Visio!
    if let Some(json_str) = archive.get_text("visio/photon_vision.json") {
        if let Ok(doc) = serde_json::from_str::<DiagramDocument>(&json_str) {
            return Ok(doc);
        }
    }

    let mut doc = DiagramDocument::default();
    doc.pages.clear();

    // Collect all page XML files in `visio/pages/page*.xml`
    let mut page_files: Vec<String> = archive
        .files
        .keys()
        .filter(|k| {
            k.starts_with("visio/pages/page")
                && k.ends_with(".xml")
                && *k != "visio/pages/pages.xml"
        })
        .cloned()
        .collect();
    page_files.sort();

    if page_files.is_empty() {
        // Fallback: maybe single XML or masters
        if let Some(pages_xml) = archive.get_text("visio/pages/pages.xml") {
            let page = parse_visio_page_xml(&pages_xml, "Sivu 1", &mut doc.next_id)?;
            doc.pages.push(page);
        }
    } else {
        for (idx, path) in page_files.iter().enumerate() {
            if let Some(xml_text) = archive.get_text(path) {
                let page_name = format!("Sivu {}", idx + 1);
                let page = parse_visio_page_xml(&xml_text, &page_name, &mut doc.next_id)?;
                doc.pages.push(page);
            }
        }
    }

    if doc.pages.is_empty() {
        doc.pages.push(DiagramPage::default());
    }

    Ok(doc)
}

/// Reads a `.vdx` (Visio 2003–2010 XML Drawing) file into a `DiagramDocument`.
pub fn read_vdx_str(xml_text: &str) -> Result<DiagramDocument, String> {
    let xml = Document::parse(xml_text).map_err(|e| format!("VDX XML -jäsennysvirhe: {}", e))?;
    let mut doc = DiagramDocument::default();
    doc.pages.clear();

    for node in xml.descendants() {
        if node.has_tag_name("Page") {
            let name = node.attribute("NameU").or_else(|| node.attribute("Name")).unwrap_or("Sivu 1");
            let page = parse_visio_page_node(node, name, &mut doc.next_id);
            doc.pages.push(page);
        }
    }

    if doc.pages.is_empty() {
        let page = parse_visio_page_xml(xml_text, "Sivu 1", &mut doc.next_id)?;
        doc.pages.push(page);
    }

    Ok(doc)
}

/// Parses Visio color representations (Hex strings `#50e6ff`, `RGB(...)`, or standard palette indices 0..24).
pub fn parse_visio_color(s: &str) -> Option<[u8; 4]> {
    let trimmed = s.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
        return None;
    }
    if let Some(hex) = trimmed.strip_prefix('#') {
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some([r, g, b, 255]);
        } else if hex.len() == 8 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            return Some([r, g, b, a]);
        } else if hex.len() == 3 {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
            return Some([r, g, b, 255]);
        }
    }
    if trimmed.to_ascii_uppercase().starts_with("RGB(") && trimmed.ends_with(')') {
        let inside = &trimmed[4..trimmed.len() - 1];
        let parts: Vec<&str> = inside.split(',').map(|p| p.trim()).collect();
        if parts.len() >= 3 {
            let r = parts[0].parse::<u8>().ok()?;
            let g = parts[1].parse::<u8>().ok()?;
            let b = parts[2].parse::<u8>().ok()?;
            return Some([r, g, b, 255]);
        }
    }
    if let Ok(idx) = trimmed.parse::<u32>() {
        let c = match idx {
            0 => [0, 0, 0, 255],          // Black
            1 => [255, 255, 255, 255],  // White
            2 => [239, 68, 68, 255],    // Red
            3 => [34, 197, 94, 255],    // Green
            4 => [59, 130, 246, 255],   // Blue
            5 => [234, 179, 8, 255],    // Yellow
            6 => [217, 70, 239, 255],   // Magenta
            7 => [6, 182, 212, 255],    // Cyan
            8 => [153, 27, 27, 255],    // Dark Red
            9 => [22, 101, 52, 255],    // Dark Green
            10 => [30, 64, 175, 255],   // Dark Blue
            11 => [161, 98, 7, 255],    // Dark Yellow
            12 => [134, 25, 143, 255],  // Purple
            13 => [17, 94, 89, 255],    // Dark Cyan
            14 => [203, 213, 225, 255], // Gray
            15 => [100, 116, 139, 255], // Dark Gray
            16 => [148, 163, 184, 255], // Gray
            17 => [226, 232, 240, 255], // Light Gray
            24 => [0, 0, 0, 0],         // Transparent
            _ => [59, 130, 246, 255],
        };
        return Some(c);
    }
    None
}

fn get_cell_val<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
    for c in node.children() {
        if c.has_tag_name("Cell") && c.attribute("N") == Some(name) {
            return c.attribute("V");
        }
    }
    None
}

fn get_cell_f64(node: Node, name: &str, default: f64) -> f64 {
    get_cell_val(node, name)
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(default)
}

fn get_row_cell_f64(row: Node, name: &str, default: f64) -> f64 {
    for c in row.children() {
        if c.has_tag_name("Cell") && c.attribute("N") == Some(name) {
            if let Some(v) = c.attribute("V").and_then(|s| s.parse::<f64>().ok()) {
                return v;
            }
        }
    }
    default
}

/// Generates 1 to 3 uppercase monogram initials from a shape/master name.
fn generate_initials(name: &str) -> String {
    let clean = name.trim();
    let words: Vec<&str> = clean
        .split(|c: char| c.is_whitespace() || c == '_' || c == '-' || c == '.')
        .filter(|w| !w.is_empty())
        .collect();

    if words.is_empty() {
        return "SYM".to_string();
    }

    if words.len() >= 2 {
        let mut s = String::new();
        for w in words.iter().take(3) {
            if let Some(ch) = w.chars().next() {
                s.extend(ch.to_uppercase());
            }
        }
        if !s.is_empty() {
            return s;
        }
    }

    let word = words[0];
    let upper: String = word.chars().take(3).flat_map(|c| c.to_uppercase()).collect();
    if upper.is_empty() {
        "SYM".to_string()
    } else {
        upper
    }
}

/// Generates a clean, modern card with a centered monogram emblem chip when a stencil
/// has no vector shapes (e.g. placeholder masters or corrupted shape exports in older stencil packs).
fn generate_placeholder_geometry(
    name: &str,
    dominant_fill: Option<[u8; 4]>,
    dominant_stroke: Option<[u8; 4]>,
) -> Vec<PrimOp> {
    let initials = generate_initials(name);
    let border_col = dominant_stroke.unwrap_or([59, 130, 246, 255]);
    let chip_col = dominant_fill.unwrap_or([59, 130, 246, 255]);

    vec![
        // Outer rounded card
        PrimOp::Path {
            points: vec![
                (0.12, 0.08),
                (0.88, 0.08),
                (0.92, 0.12),
                (0.92, 0.88),
                (0.88, 0.92),
                (0.12, 0.92),
                (0.08, 0.88),
                (0.08, 0.12),
            ],
            closed: true,
            filled: true,
            fill_color: Some([241, 245, 249, 255]), // Light slate card bg
            stroke_color: Some(border_col),
            stroke_width: Some(1.5),
        },
        // Central circular emblem / chip
        PrimOp::Ellipse {
            cx: 0.5,
            cy: 0.46,
            rx: 0.28,
            ry: 0.28,
            filled: true,
            fill_color: Some([255, 255, 255, 255]),
            stroke_color: Some(chip_col),
        },
        // Initials text centered in the emblem
        PrimOp::Text {
            x: 0.5,
            y: 0.46,
            text: initials,
            size_rel: 0.26,
        },
        // Subtle accent bottom bar
        PrimOp::Line {
            x1: 0.25,
            y1: 0.80,
            x2: 0.75,
            y2: 0.80,
        },
    ]
}

/// Recursively collects all geometry primitives from a shape and its sub-shapes,
/// transforming their coordinates into normalized (0..=1, 0..=1) space of the master root.
fn collect_shape_geometry(
    shape: Node,
    parent_x: f64,
    parent_y: f64,
    root_w: f64,
    root_h: f64,
    is_root: bool,
    ops: &mut Vec<PrimOp>,
    dominant_fill: &mut Option<[u8; 4]>,
    dominant_stroke: &mut Option<[u8; 4]>,
) {
    let sw = get_cell_f64(shape, "Width", root_w);
    let sh = get_cell_f64(shape, "Height", root_h);
    let spx = get_cell_f64(shape, "PinX", sw * 0.5);
    let spy = get_cell_f64(shape, "PinY", sh * 0.5);
    let loc_x = get_cell_f64(shape, "LocPinX", sw * 0.5);
    let loc_y = get_cell_f64(shape, "LocPinY", sh * 0.5);

    let (x_base, y_base) = if is_root {
        (0.0, 0.0)
    } else {
        (parent_x + (spx - loc_x), parent_y + (spy - loc_y))
    };

    let no_show = get_cell_f64(shape, "NoShow", 0.0) >= 0.5;
    let no_fill = get_cell_f64(shape, "NoFill", 0.0) >= 0.5;
    let no_line = get_cell_f64(shape, "NoLine", 0.0) >= 0.5;
    let fill_pattern = get_cell_f64(shape, "FillPattern", 1.0);
    let line_pattern = get_cell_f64(shape, "LinePattern", 1.0);

    let has_explicit_no_fill =
        no_fill || (get_cell_val(shape, "FillPattern").is_some() && fill_pattern == 0.0);
    let has_explicit_no_line =
        no_line || (get_cell_val(shape, "LinePattern").is_some() && line_pattern == 0.0);

    let cur_fill = if has_explicit_no_fill {
        None
    } else {
        get_cell_val(shape, "FillForegnd")
            .and_then(parse_visio_color)
            .or_else(|| {
                // Check FillGradient section
                for sec in shape.children().filter(|n| {
                    n.has_tag_name("Section") && n.attribute("N") == Some("FillGradient")
                }) {
                    for row in sec.children().filter(|n| n.has_tag_name("Row")) {
                        if let Some(col_str) = get_cell_val(row, "GradientStopColor") {
                            if let Some(c) = parse_visio_color(col_str) {
                                return Some(c);
                            }
                        }
                    }
                }
                None
            })
    };

    let line_color = get_cell_val(shape, "LineColor").and_then(parse_visio_color);
    let cur_stroke = if has_explicit_no_line {
        None
    } else if let Some(lc) = line_color {
        Some(lc)
    } else if cur_fill.is_some() {
        // Filled vector icon shapes don't get artificial dark borders unless LinePattern is explicitly set > 0
        if get_cell_val(shape, "LinePattern").is_some() && line_pattern > 0.0 {
            Some([30, 41, 59, 255])
        } else {
            None
        }
    } else if get_cell_val(shape, "FillPattern").is_some() && fill_pattern == 0.0 {
        // Invisible dummy bounding box: no fill and no stroke
        None
    } else {
        // Unfilled shape (line/curve/wire): default stroke
        Some([30, 41, 59, 255])
    };

    if dominant_fill.is_none() && cur_fill.is_some() {
        *dominant_fill = cur_fill;
    }
    if dominant_stroke.is_none() && cur_stroke.is_some() {
        *dominant_stroke = cur_stroke;
    }

    if !no_show {
        for sec in shape.children().filter(|n| {
            n.has_tag_name("Section") && n.attribute("N") == Some("Geometry")
                || n.has_tag_name("Geom")
        }) {
            let sec_no_show = get_cell_f64(sec, "NoShow", 0.0) >= 0.5;
            if sec_no_show {
                continue;
            }
            let sec_no_fill = get_cell_f64(sec, "NoFill", 0.0) >= 0.5 || cur_fill.is_none();
            let sec_no_line = get_cell_f64(sec, "NoLine", 0.0) >= 0.5 || cur_stroke.is_none();
            if sec_no_fill && sec_no_line {
                // Completely invisible geometry section (e.g. invisible bounding box) - skip!
                continue;
            }
            let is_filled = !sec_no_fill && cur_fill.is_some();

            let to_norm_rel = |rx: f64, ry: f64| -> (f64, f64) {
                let nx = ((x_base + rx * sw) / root_w).clamp(0.0, 1.0);
                let ny = (1.0 - ((y_base + ry * sh) / root_h)).clamp(0.0, 1.0);
                (nx, ny)
            };
            let to_norm_abs = |ax: f64, ay: f64| -> (f64, f64) {
                let nx = ((x_base + ax) / root_w).clamp(0.0, 1.0);
                let ny = (1.0 - ((y_base + ay) / root_h)).clamp(0.0, 1.0);
                (nx, ny)
            };

            let mut cur_path: Vec<(f64, f64)> = Vec::new();

            for row in sec.children().filter(|n| n.has_tag_name("Row")) {
                let row_type = row.attribute("T").unwrap_or_else(|| row.tag_name().name());

                match row_type {
                    "RelMoveTo" => {
                        if cur_path.len() >= 2 {
                            ops.push(PrimOp::Path {
                                points: std::mem::take(&mut cur_path),
                                closed: true,
                                filled: is_filled,
                                fill_color: cur_fill,
                                stroke_color: cur_stroke,
                                stroke_width: Some(1.0),
                            });
                        } else {
                            cur_path.clear();
                        }
                        let rx = get_row_cell_f64(row, "X", 0.0);
                        let ry = get_row_cell_f64(row, "Y", 0.0);
                        cur_path.push(to_norm_rel(rx, ry));
                    }
                    "MoveTo" => {
                        if cur_path.len() >= 2 {
                            ops.push(PrimOp::Path {
                                points: std::mem::take(&mut cur_path),
                                closed: true,
                                filled: is_filled,
                                fill_color: cur_fill,
                                stroke_color: cur_stroke,
                                stroke_width: Some(1.0),
                            });
                        } else {
                            cur_path.clear();
                        }
                        let ax = get_row_cell_f64(row, "X", 0.0);
                        let ay = get_row_cell_f64(row, "Y", 0.0);
                        cur_path.push(to_norm_abs(ax, ay));
                    }
                    "RelLineTo" | "RelEllipticalArcTo" => {
                        let rx = get_row_cell_f64(row, "X", 0.0);
                        let ry = get_row_cell_f64(row, "Y", 0.0);
                        cur_path.push(to_norm_rel(rx, ry));
                    }
                    "LineTo" | "EllipticalArcTo" => {
                        let ax = get_row_cell_f64(row, "X", 0.0);
                        let ay = get_row_cell_f64(row, "Y", 0.0);
                        cur_path.push(to_norm_abs(ax, ay));
                    }
                    "RelCubBezTo" => {
                        let p0 = cur_path.last().copied().unwrap_or((0.0, 0.0));
                        let c1 = to_norm_rel(get_row_cell_f64(row, "A", 0.0), get_row_cell_f64(row, "B", 0.0));
                        let c2 = to_norm_rel(get_row_cell_f64(row, "C", 0.0), get_row_cell_f64(row, "D", 0.0));
                        let p1 = to_norm_rel(get_row_cell_f64(row, "X", 0.0), get_row_cell_f64(row, "Y", 0.0));
                        for step in 1..=4 {
                            let t = step as f64 / 4.0;
                            let it = 1.0 - t;
                            let x = it.powi(3) * p0.0 + 3.0 * it.powi(2) * t * c1.0 + 3.0 * it * t.powi(2) * c2.0 + t.powi(3) * p1.0;
                            let y = it.powi(3) * p0.1 + 3.0 * it.powi(2) * t * c1.1 + 3.0 * it * t.powi(2) * c2.1 + t.powi(3) * p1.1;
                            cur_path.push((x, y));
                        }
                    }
                    "RelQuadBezTo" => {
                        let p0 = cur_path.last().copied().unwrap_or((0.0, 0.0));
                        let c = to_norm_rel(get_row_cell_f64(row, "A", 0.0), get_row_cell_f64(row, "B", 0.0));
                        let p1 = to_norm_rel(get_row_cell_f64(row, "X", 0.0), get_row_cell_f64(row, "Y", 0.0));
                        for step in 1..=3 {
                            let t = step as f64 / 3.0;
                            let it = 1.0 - t;
                            let x = it.powi(2) * p0.0 + 2.0 * it * t * c.0 + t.powi(2) * p1.0;
                            let y = it.powi(2) * p0.1 + 2.0 * it * t * c.1 + t.powi(2) * p1.1;
                            cur_path.push((x, y));
                        }
                    }
                    "Ellipse" => {
                        let cx_abs = x_base + get_row_cell_f64(row, "X", sw * 0.5);
                        let cy_abs = y_base + get_row_cell_f64(row, "Y", sh * 0.5);
                        let a_abs = get_row_cell_f64(row, "A", 0.0);
                        let d_abs = get_row_cell_f64(row, "D", 0.0);
                        let cx = (cx_abs / root_w).clamp(0.0, 1.0);
                        let cy = (1.0 - (cy_abs / root_h)).clamp(0.0, 1.0);
                        let rx = if a_abs.abs() > 0.0001 {
                            ((a_abs - get_row_cell_f64(row, "X", 0.0)).abs() / root_w).clamp(0.001, 0.5)
                        } else {
                            ((sw * 0.5) / root_w).clamp(0.001, 0.5)
                        };
                        let ry = if d_abs.abs() > 0.0001 {
                            ((d_abs - get_row_cell_f64(row, "Y", 0.0)).abs() / root_h).clamp(0.001, 0.5)
                        } else {
                            ((sh * 0.5) / root_h).clamp(0.001, 0.5)
                        };
                        ops.push(PrimOp::Ellipse {
                            cx,
                            cy,
                            rx,
                            ry,
                            filled: is_filled,
                            fill_color: cur_fill,
                            stroke_color: cur_stroke,
                        });
                    }
                    _ => {}
                }
            }

            if cur_path.len() >= 2 {
                ops.push(PrimOp::Path {
                    points: cur_path,
                    closed: true,
                    filled: is_filled,
                    fill_color: cur_fill,
                    stroke_color: cur_stroke,
                    stroke_width: Some(1.0),
                });
            }
        }
    }

    // Recursively process child shapes
    for child in shape.children().filter(|n| n.has_tag_name("Shape")) {
        collect_shape_geometry(
            child,
            x_base,
            y_base,
            root_w,
            root_h,
            false,
            ops,
            dominant_fill,
            dominant_stroke,
        );
    }
    for shapes_container in shape.children().filter(|n| n.has_tag_name("Shapes")) {
        for child in shapes_container.children().filter(|n| n.has_tag_name("Shape")) {
            collect_shape_geometry(
                child,
                x_base,
                y_base,
                root_w,
                root_h,
                false,
                ops,
                dominant_fill,
                dominant_stroke,
            );
        }
    }
}

/// Imports a `.vssx` (Visio Stencil Library) file and returns extracted `StencilItem`s
/// with their real master names, categories, and high-fidelity vector geometry.
pub fn import_vssx_stencils(bytes: &[u8], library_name: &str) -> Result<Vec<StencilItem>, String> {
    let archive = ZipArchive::from_bytes(bytes)?;
    let mut stencils = Vec::new();

    // 1. Map relationships from `visio/masters/_rels/masters.xml.rels`
    let mut rel_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    if let Some(rels_text) = archive.get_text("visio/masters/_rels/masters.xml.rels") {
        if let Ok(rels_doc) = Document::parse(&rels_text) {
            for r in rels_doc.descendants().filter(|n| n.has_tag_name("Relationship")) {
                if let (Some(id), Some(target)) = (r.attribute("Id"), r.attribute("Target")) {
                    let mut norm_target = target.replace('\\', "/");
                    norm_target = norm_target.trim_start_matches('/').to_string();
                    if !norm_target.starts_with("visio/") {
                        if norm_target.starts_with("masters/") {
                            norm_target = format!("visio/{}", norm_target);
                        } else {
                            norm_target = format!("visio/masters/{}", norm_target);
                        }
                    }
                    rel_map.insert(id.to_string(), norm_target);
                }
            }
        }
    }

    // 2. Parse masters from `visio/masters/masters.xml`
    let mut master_entries: Vec<(String, String, String)> = Vec::new();
    if let Some(masters_text) = archive.get_text("visio/masters/masters.xml") {
        if let Ok(masters_doc) = Document::parse(&masters_text) {
            for m in masters_doc.descendants().filter(|n| n.has_tag_name("Master")) {
                let mid = m.attribute("ID").unwrap_or("0").to_string();
                let name = m
                    .attribute("Name")
                    .or_else(|| m.attribute("NameU"))
                    .unwrap_or("Symboli")
                    .trim()
                    .to_string();

                let rel_id = m
                    .children()
                    .find(|n| n.has_tag_name("Rel"))
                    .and_then(|n| {
                        n.attribute((
                            "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
                            "id",
                        ))
                        .or_else(|| n.attribute("r:id"))
                        .or_else(|| n.attribute("id"))
                    });

                let target_file = rel_id
                    .and_then(|rid| rel_map.get(rid).cloned())
                    .unwrap_or_else(|| format!("visio/masters/master{}.xml", mid));

                master_entries.push((mid, name, target_file));
            }
        }
    }

    // Fallback: If masters.xml was not present or empty, discover all master*.xml files
    if master_entries.is_empty() {
        let mut files: Vec<String> = archive
            .files
            .keys()
            .filter(|k| {
                k.starts_with("visio/masters/master")
                    && k.ends_with(".xml")
                    && !k.ends_with("masters.xml")
            })
            .cloned()
            .collect();
        files.sort();
        for (idx, f) in files.into_iter().enumerate() {
            master_entries.push((format!("{}", idx + 1), format!("Symboli {}", idx + 1), f));
        }
    }

    for (mid, master_name, master_file) in master_entries {
        let xml_text = match archive.get_text(&master_file) {
            Some(t) => t,
            None => continue,
        };

        let master_doc = match Document::parse(&xml_text) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let top_shape = match master_doc.descendants().find(|n| n.has_tag_name("Shape")) {
            Some(s) => s,
            None => continue,
        };

        let resolved_name = if master_name.starts_with("Symboli") {
            top_shape
                .attribute("NameU")
                .or_else(|| top_shape.attribute("Name"))
                .unwrap_or(&master_name)
                .trim()
                .to_string()
        } else {
            master_name
        };

        let mut root_w = get_cell_f64(top_shape, "Width", 1.0);
        let mut root_h = get_cell_f64(top_shape, "Height", 1.0);
        if root_w <= 0.001 {
            root_w = 1.0;
        }
        if root_h <= 0.001 {
            root_h = 1.0;
        }

        // Parse connection points from root shape
        let mut ports = Vec::new();
        for sec in top_shape.children().filter(|n| {
            n.has_tag_name("Section") && n.attribute("N") == Some("Connection")
        }) {
            for row in sec.children().filter(|n| n.has_tag_name("Row")) {
                let x_in = get_row_cell_f64(row, "X", root_w * 0.5);
                let y_in = get_row_cell_f64(row, "Y", root_h * 0.5);
                let rx = (x_in / root_w).clamp(0.0, 1.0);
                let ry = (1.0 - (y_in / root_h)).clamp(0.0, 1.0);
                let dir = if rx <= 0.05 {
                    PortDir::Left
                } else if rx >= 0.95 {
                    PortDir::Right
                } else if ry <= 0.05 {
                    PortDir::Up
                } else if ry >= 0.95 {
                    PortDir::Down
                } else {
                    PortDir::Any
                };
                let pname = row
                    .attribute("N")
                    .or_else(|| row.attribute("IX"))
                    .unwrap_or("P")
                    .to_string();
                ports.push(ConnectionPort::new(&pname, rx, ry, dir));
            }
        }

        if ports.is_empty() {
            ports.push(ConnectionPort::new("T", 0.5, 0.0, PortDir::Up));
            ports.push(ConnectionPort::new("B", 0.5, 1.0, PortDir::Down));
            ports.push(ConnectionPort::new("L", 0.0, 0.5, PortDir::Left));
            ports.push(ConnectionPort::new("R", 1.0, 0.5, PortDir::Right));
        }

        // Recursively extract all vector geometry
        let mut geometry = Vec::new();
        let mut dominant_fill = None;
        let mut dominant_stroke = None;
        collect_shape_geometry(
            top_shape,
            0.0,
            0.0,
            root_w,
            root_h,
            true,
            &mut geometry,
            &mut dominant_fill,
            &mut dominant_stroke,
        );

        if geometry.is_empty() {
            geometry = generate_placeholder_geometry(&resolved_name, dominant_fill, dominant_stroke);
        }

        let raw_w_mm = (root_w * INCH_TO_MM).max(4.0);
        let raw_h_mm = (root_h * INCH_TO_MM).max(4.0);
        let ar = raw_w_mm / raw_h_mm;
        let (w_mm, h_mm) = if raw_w_mm >= raw_h_mm {
            let w = 24.0_f64;
            (w, (w / ar).clamp(8.0, 50.0))
        } else {
            let h = 24.0_f64;
            ((h * ar).clamp(8.0, 50.0), h)
        };

        let stroke_color = dominant_stroke.unwrap_or([30, 41, 59, 255]);
        let fill_color = dominant_fill.unwrap_or([59, 130, 246, 255]);

        let id = format!(
            "vssx_{}_{}",
            library_name
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect::<String>(),
            mid
        );

        stencils.push(StencilItem {
            id,
            name: resolved_name.clone(),
            category: StencilCategory::ImportedVisio(library_name.to_string()),
            default_label: resolved_name,
            default_value: String::new(),
            default_width_mm: w_mm,
            default_height_mm: h_mm,
            default_layer: "Oletus".to_string(),
            stroke_color,
            fill_color,
            stroke_width: 1.5,
            ports,
            geometry,
            is_dimension: false,
            is_room_area: false,
        });
    }

    if stencils.is_empty() {
        return Err("VSSX-tiedostosta ei löytynyt Visio Master -symboleita".to_string());
    }

    Ok(stencils)
}

fn parse_visio_page_xml(
    xml_text: &str,
    default_name: &str,
    next_id: &mut u64,
) -> Result<DiagramPage, String> {
    let xml = Document::parse(xml_text).map_err(|e| format!("Visio XML -virhe: {}", e))?;
    Ok(parse_visio_page_node(xml.root_element(), default_name, next_id))
}

fn parse_visio_page_node(root: Node, default_name: &str, next_id: &mut u64) -> DiagramPage {
    let mut page = DiagramPage {
        name: default_name.to_string(),
        width_mm: 297.0,
        height_mm: 210.0,
        grid_spacing_mm: 5.0,
        scale: DrawingScale::Scale1To1,
        ..Default::default()
    };

    // Inspect PageSheet cells for PageWidth / PageHeight (Visio stores dimensions in inches)
    for node in root.descendants() {
        if node.has_tag_name("Cell") {
            match node.attribute("N") {
                Some("PageWidth") => {
                    if let Some(v) = node.attribute("V").and_then(|s| s.parse::<f64>().ok()) {
                        page.width_mm = (v * INCH_TO_MM).clamp(50.0, 5000.0);
                    }
                }
                Some("PageHeight") => {
                    if let Some(v) = node.attribute("V").and_then(|s| s.parse::<f64>().ok()) {
                        page.height_mm = (v * INCH_TO_MM).clamp(50.0, 5000.0);
                    }
                }
                _ => {}
            }
        }
    }

    let builtins = built_in_stencils();

    // Parse all `<Shape>` elements
    for shapes_container in root.descendants().filter(|n| n.has_tag_name("Shapes")) {
        for shape_node in shapes_container.children().filter(|n| n.has_tag_name("Shape")) {
            parse_visio_shape(shape_node, &mut page, &builtins, next_id);
        }
    }

    // Parse `<Connects>` elements to link connectors to shapes
    for conn_node in root.descendants().filter(|n| n.has_tag_name("Connect")) {
        let from_sheet = conn_node
            .attribute("FromSheet")
            .and_then(|s| s.parse::<u64>().ok());
        let to_sheet = conn_node
            .attribute("ToSheet")
            .and_then(|s| s.parse::<u64>().ok());
        let from_cell = conn_node.attribute("FromCell").unwrap_or("");

        if let (Some(c_id), Some(target_id)) = (from_sheet, to_sheet) {
            if let Some(conn) = page.connectors.iter_mut().find(|c| c.id == c_id) {
                if from_cell.contains("Begin") {
                    conn.from_shape = Some(target_id);
                    conn.from_port = 0;
                } else if from_cell.contains("End") {
                    conn.to_shape = Some(target_id);
                    conn.to_port = 0;
                }
            }
        }
    }

    update_all_connectors(&mut page.connectors, &page.shapes);
    page
}

fn parse_visio_shape(
    shape_node: Node,
    page: &mut DiagramPage,
    builtins: &[StencilItem],
    next_id: &mut u64,
) {
    let raw_id = shape_node
        .attribute("ID")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or_else(|| {
            let id = *next_id;
            *next_id += 1;
            id
        });
    if raw_id >= *next_id {
        *next_id = raw_id + 1;
    }

    let name_u = shape_node
        .attribute("NameU")
        .or_else(|| shape_node.attribute("Name"))
        .unwrap_or("Shape")
        .to_string();

    let mut pin_x_in: f64 = 2.0;
    let mut pin_y_in: f64 = 2.0;
    let mut width_in: f64 = 1.0;
    let mut height_in: f64 = 0.6;
    let mut angle_rad: f64 = 0.0;
    let mut begin_x_in: Option<f64> = None;
    let mut begin_y_in: Option<f64> = None;
    let mut end_x_in: Option<f64> = None;
    let mut end_y_in: Option<f64> = None;
    let mut stencil_hint = String::new();
    let mut value_hint = String::new();

    // Direct child Cells of this Shape
    for child in shape_node.children() {
        if child.has_tag_name("Cell") {
            let val = child.attribute("V").unwrap_or("");
            match child.attribute("N") {
                Some("PinX") => pin_x_in = val.parse().unwrap_or(pin_x_in),
                Some("PinY") => pin_y_in = val.parse().unwrap_or(pin_y_in),
                Some("Width") => width_in = val.parse().unwrap_or(width_in),
                Some("Height") => height_in = val.parse().unwrap_or(height_in),
                Some("Angle") => angle_rad = val.parse().unwrap_or(0.0),
                Some("BeginX") => begin_x_in = val.parse().ok(),
                Some("BeginY") => begin_y_in = val.parse().ok(),
                Some("EndX") => end_x_in = val.parse().ok(),
                Some("EndY") => end_y_in = val.parse().ok(),
                Some("PhotonStencil") => stencil_hint = val.to_string(),
                Some("PhotonValue") => value_hint = val.to_string(),
                _ => {}
            }
        }
        // Visio 2003-2010 VDX XForm element
        if child.has_tag_name("XForm") {
            for xf in child.children() {
                let txt = xf.text().unwrap_or("").trim();
                match xf.tag_name().name() {
                    "PinX" => pin_x_in = txt.parse().unwrap_or(pin_x_in),
                    "PinY" => pin_y_in = txt.parse().unwrap_or(pin_y_in),
                    "Width" => width_in = txt.parse().unwrap_or(width_in),
                    "Height" => height_in = txt.parse().unwrap_or(height_in),
                    "Angle" => angle_rad = txt.parse().unwrap_or(0.0),
                    _ => {}
                }
            }
        }
        if child.has_tag_name("XForm1D") {
            for xf in child.children() {
                let txt = xf.text().unwrap_or("").trim();
                match xf.tag_name().name() {
                    "BeginX" => begin_x_in = txt.parse().ok(),
                    "BeginY" => begin_y_in = txt.parse().ok(),
                    "EndX" => end_x_in = txt.parse().ok(),
                    "EndY" => end_y_in = txt.parse().ok(),
                    _ => {}
                }
            }
        }
    }

    // Extract text content from `<Text>`
    let mut text_content = String::new();
    for child in shape_node.children().filter(|n| n.has_tag_name("Text")) {
        for desc in child.descendants() {
            if desc.is_text() {
                if let Some(t) = desc.text() {
                    text_content.push_str(t);
                }
            }
        }
    }
    let text_content = text_content.trim().to_string();

    // Check if this is a 1D connector (has BeginX/EndX or Dynamic connector name)
    if let (Some(bx), Some(by), Some(ex), Some(ey)) = (begin_x_in, begin_y_in, end_x_in, end_y_in) {
        let from_pos = Point::new(bx * INCH_TO_MM, page.height_mm - by * INCH_TO_MM);
        let to_pos = Point::new(ex * INCH_TO_MM, page.height_mm - ey * INCH_TO_MM);
        page.connectors.push(DiagramConnector {
            id: raw_id,
            from_shape: None,
            from_port: 0,
            from_pos,
            to_shape: None,
            to_port: 0,
            to_pos,
            style: ConnectorStyle::Orthogonal,
            start_arrow: ArrowType::None,
            end_arrow: ArrowType::None,
            label: text_content,
            stroke_color: [30, 41, 59, 255],
            stroke_width: 1.8,
            dashed: false,
            layer: "Sähkö & Kytkennät".to_string(),
            waypoints: vec![from_pos, to_pos],
        });
        return;
    }

    let w_mm = (width_in.abs() * INCH_TO_MM).max(2.0);
    let h_mm = (height_in.abs() * INCH_TO_MM).max(2.0);
    // Visio Y axis points UP from bottom-left; PhotonVision Y axis points DOWN from top-left
    let cx_mm = pin_x_in * INCH_TO_MM;
    let cy_mm = (page.height_mm - pin_y_in * INCH_TO_MM).clamp(0.0, page.height_mm);
    let rot_deg = (-angle_rad.to_degrees()).rem_euclid(360.0);

    // Match built-in stencil if PhotonStencil or NameU matches
    if let Some(st) = builtins
        .iter()
        .find(|s| s.id == stencil_hint || s.id.eq_ignore_ascii_case(&name_u) || s.name.eq_ignore_ascii_case(&name_u))
    {
        let mut shape = st.instantiate(raw_id, cx_mm, cy_mm, 1);
        shape.width = w_mm;
        shape.height = h_mm;
        shape.rotation_deg = rot_deg;
        if !text_content.is_empty() {
            shape.label = text_content;
        }
        if !value_hint.is_empty() {
            shape.value = value_hint;
        }
        page.shapes.push(shape);
        return;
    }

    // Parse custom Visio Geometry sections (`MoveTo`, `LineTo`, `PolylineTo`)
    let mut geometry = Vec::new();
    for sec in shape_node.children().filter(|n| {
        n.has_tag_name("Section") && n.attribute("N") == Some("Geometry")
            || n.has_tag_name("Geom")
    }) {
        let mut cur_x = 0.0;
        let mut cur_y = 0.0;
        for row in sec.children().filter(|n| n.has_tag_name("Row") || n.has_tag_name("MoveTo") || n.has_tag_name("LineTo")) {
            let row_type = row.attribute("T").unwrap_or_else(|| row.tag_name().name());
            let mut rx = None;
            let mut ry = None;
            for c in row.children() {
                if c.has_tag_name("Cell") {
                    match c.attribute("N") {
                        Some("X") => rx = c.attribute("V").and_then(|v| v.parse::<f64>().ok()),
                        Some("Y") => ry = c.attribute("V").and_then(|v| v.parse::<f64>().ok()),
                        _ => {}
                    }
                } else if c.has_tag_name("X") {
                    rx = c.text().and_then(|v| v.trim().parse::<f64>().ok());
                } else if c.has_tag_name("Y") {
                    ry = c.text().and_then(|v| v.trim().parse::<f64>().ok());
                }
            }
            if let (Some(x_in), Some(y_in)) = (rx, ry) {
                let nx = if width_in.abs() > 1e-6 {
                    (x_in / width_in).clamp(0.0, 1.0)
                } else {
                    x_in.clamp(0.0, 1.0)
                };
                let ny = if height_in.abs() > 1e-6 {
                    (1.0 - (y_in / height_in)).clamp(0.0, 1.0)
                } else {
                    (1.0 - y_in).clamp(0.0, 1.0)
                };
                if row_type == "MoveTo" {
                    cur_x = nx;
                    cur_y = ny;
                } else if row_type == "LineTo" {
                    geometry.push(PrimOp::Line {
                        x1: cur_x,
                        y1: cur_y,
                        x2: nx,
                        y2: ny,
                    });
                    cur_x = nx;
                    cur_y = ny;
                }
            }
        }
    }

    if geometry.is_empty() {
        geometry.push(PrimOp::Rect {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
            filled: true,
        });
    }

    page.shapes.push(DiagramShape {
        id: raw_id,
        stencil_id: "visio_shape".to_string(),
        name: name_u,
        label: text_content,
        value: value_hint,
        x: cx_mm,
        y: cy_mm,
        width: w_mm,
        height: h_mm,
        rotation_deg: rot_deg,
        flip_x: false,
        flip_y: false,
        stroke_color: [30, 41, 59, 255],
        fill_color: [241, 245, 249, 255],
        stroke_width: 1.8,
        dashed: false,
        layer: "Oletus".to_string(),
        ports: vec![
            ConnectionPort::new("T", 0.5, 0.0, PortDir::Up),
            ConnectionPort::new("B", 0.5, 1.0, PortDir::Down),
            ConnectionPort::new("L", 0.0, 0.5, PortDir::Left),
            ConnectionPort::new("R", 1.0, 0.5, PortDir::Right),
        ],
        geometry,
        properties: Vec::new(),
        is_dimension: false,
        is_room_area: false,
        locked: false,
    });
}

/// Writes a `DiagramDocument` into a valid `.vsdx` (Visio 2013+ Open Packaging Conventions ZIP) byte buffer.
pub fn write_vsdx_bytes(doc: &DiagramDocument) -> Result<Vec<u8>, String> {
    let mut zip = ZipArchive::new();

    // 1. [Content_Types].xml
    let mut content_types = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Default Extension="json" ContentType="application/json"/>
  <Override PartName="/visio/document.xml" ContentType="application/vnd.ms-visio.drawing.main+xml"/>
  <Override PartName="/visio/pages/pages.xml" ContentType="application/vnd.ms-visio.pages+xml"/>
"#,
    );
    for i in 0..doc.pages.len() {
        content_types.push_str(&format!(
            "  <Override PartName=\"/visio/pages/page{}.xml\" ContentType=\"application/vnd.ms-visio.page+xml\"/>\n",
            i + 1
        ));
    }
    content_types.push_str("</Types>");
    zip.insert("[Content_Types].xml", content_types.into_bytes());

    // 2. _rels/.rels
    let root_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.microsoft.com/visio/2010/relationships/document" Target="visio/document.xml"/>
</Relationships>"#;
    zip.insert("_rels/.rels", root_rels.as_bytes().to_vec());

    // 3. visio/document.xml
    let document_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<VisioDocument xmlns="http://schemas.microsoft.com/office/visio/2012/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <DocumentSettings/>
</VisioDocument>"#;
    zip.insert("visio/document.xml", document_xml.as_bytes().to_vec());

    // 4. visio/_rels/document.xml.rels
    let doc_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.microsoft.com/visio/2010/relationships/pages" Target="pages/pages.xml"/>
</Relationships>"#;
    zip.insert("visio/_rels/document.xml.rels", doc_rels.as_bytes().to_vec());

    // 5. visio/pages/pages.xml and visio/pages/_rels/pages.xml.rels
    let mut pages_xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Pages xmlns="http://schemas.microsoft.com/office/visio/2012/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
"#,
    );
    let mut pages_rels = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
"#,
    );

    for (i, page) in doc.pages.iter().enumerate() {
        let p_num = i + 1;
        let pw_in = page.width_mm / INCH_TO_MM;
        let ph_in = page.height_mm / INCH_TO_MM;
        pages_xml.push_str(&format!(
            "  <Page ID=\"{}\" NameU=\"{}\" Name=\"{}\">\n    <PageSheet>\n      <Cell N=\"PageWidth\" V=\"{:.4}\"/>\n      <Cell N=\"PageHeight\" V=\"{:.4}\"/>\n    </PageSheet>\n    <Rel r:id=\"rId{}\"/>\n  </Page>\n",
            p_num,
            escape_xml(&page.name),
            escape_xml(&page.name),
            pw_in,
            ph_in,
            p_num
        ));
        pages_rels.push_str(&format!(
            "  <Relationship Id=\"rId{}\" Type=\"http://schemas.microsoft.com/visio/2010/relationships/page\" Target=\"page{}.xml\"/>\n",
            p_num, p_num
        ));

        let page_xml = build_visio_page_xml(page);
        zip.insert(&format!("visio/pages/page{}.xml", p_num), page_xml.into_bytes());
    }

    pages_xml.push_str("</Pages>");
    pages_rels.push_str("</Relationships>");
    zip.insert("visio/pages/pages.xml", pages_xml.into_bytes());
    zip.insert("visio/pages/_rels/pages.xml.rels", pages_rels.into_bytes());

    // 6. Also embed lossless PhotonVision JSON inside the package so reopening in PhotonVision
    // restores all custom SFS/IEC stencils, architectural scales, and layers 100% intact.
    if let Ok(json_bytes) = serde_json::to_vec_pretty(doc) {
        zip.insert("visio/photon_vision.json", json_bytes);
    }

    Ok(zip.to_bytes())
}

/// Writes a `DiagramDocument` into a `.vdx` (Visio 2003–2010 XML Drawing) string.
pub fn write_vdx_string(doc: &DiagramDocument) -> String {
    let mut out = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<VisioDocument xmlns="http://schemas.microsoft.com/visio/2003/core">
  <Pages>
"#,
    );
    for (i, page) in doc.pages.iter().enumerate() {
        let pw_in = page.width_mm / INCH_TO_MM;
        let ph_in = page.height_mm / INCH_TO_MM;
        out.push_str(&format!(
            "    <Page ID=\"{}\" NameU=\"{}\" Name=\"{}\">\n      <PageSheet>\n        <Cell N=\"PageWidth\" V=\"{:.4}\"/>\n        <Cell N=\"PageHeight\" V=\"{:.4}\"/>\n      </PageSheet>\n",
            i + 1,
            escape_xml(&page.name),
            escape_xml(&page.name),
            pw_in,
            ph_in
        ));
        out.push_str(&build_visio_shapes_and_connects_xml(page));
        out.push_str("    </Page>\n");
    }
    out.push_str("  </Pages>\n</VisioDocument>\n");
    out
}

fn build_visio_page_xml(page: &DiagramPage) -> String {
    let pw_in = page.width_mm / INCH_TO_MM;
    let ph_in = page.height_mm / INCH_TO_MM;
    let mut out = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<PageContents xmlns="http://schemas.microsoft.com/office/visio/2012/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <PageSheet>
    <Cell N="PageWidth" V="{:.4}"/>
    <Cell N="PageHeight" V="{:.4}"/>
  </PageSheet>
"#,
        pw_in, ph_in
    );
    out.push_str(&build_visio_shapes_and_connects_xml(page));
    out.push_str("</PageContents>\n");
    out
}

fn build_visio_shapes_and_connects_xml(page: &DiagramPage) -> String {
    let mut out = String::from("  <Shapes>\n");

    for s in &page.shapes {
        let pin_x = s.x / INCH_TO_MM;
        let pin_y = (page.height_mm - s.y) / INCH_TO_MM;
        let w_in = s.width / INCH_TO_MM;
        let h_in = s.height / INCH_TO_MM;
        let angle_rad = -s.rotation_deg.to_radians();

        let text_display = if s.value.is_empty() {
            s.label.clone()
        } else if s.label.is_empty() {
            s.value.clone()
        } else {
            format!("{} ({})", s.label, s.value)
        };

        out.push_str(&format!(
            "    <Shape ID=\"{}\" NameU=\"{}\" Name=\"{}\" Type=\"Shape\">\n",
            s.id,
            escape_xml(&s.stencil_id),
            escape_xml(&s.name)
        ));
        out.push_str(&format!(
            "      <Cell N=\"PinX\" V=\"{:.4}\"/>\n      <Cell N=\"PinY\" V=\"{:.4}\"/>\n      <Cell N=\"Width\" V=\"{:.4}\"/>\n      <Cell N=\"Height\" V=\"{:.4}\"/>\n      <Cell N=\"Angle\" V=\"{:.4}\"/>\n      <Cell N=\"PhotonStencil\" V=\"{}\"/>\n      <Cell N=\"PhotonValue\" V=\"{}\"/>\n",
            pin_x,
            pin_y,
            w_in,
            h_in,
            angle_rad,
            escape_xml(&s.stencil_id),
            escape_xml(&s.value)
        ));

        // Export normalized geometry into Visio Geometry section
        out.push_str("      <Section N=\"Geometry\" IX=\"0\">\n");
        let mut row_ix = 1;
        for op in &s.geometry {
            match op {
                PrimOp::Line { x1, y1, x2, y2 } => {
                    out.push_str(&format!(
                        "        <Row T=\"MoveTo\" IX=\"{}\"><Cell N=\"X\" V=\"{:.4}\"/><Cell N=\"Y\" V=\"{:.4}\"/></Row>\n",
                        row_ix,
                        x1 * w_in,
                        (1.0 - y1) * h_in
                    ));
                    row_ix += 1;
                    out.push_str(&format!(
                        "        <Row T=\"LineTo\" IX=\"{}\"><Cell N=\"X\" V=\"{:.4}\"/><Cell N=\"Y\" V=\"{:.4}\"/></Row>\n",
                        row_ix,
                        x2 * w_in,
                        (1.0 - y2) * h_in
                    ));
                    row_ix += 1;
                }
                PrimOp::Rect { x, y, w, h, .. } => {
                    let x0 = x * w_in;
                    let y0 = (1.0 - y) * h_in;
                    let x1 = (x + w) * w_in;
                    let y1 = (1.0 - (y + h)) * h_in;
                    out.push_str(&format!(
                        "        <Row T=\"MoveTo\" IX=\"{}\"><Cell N=\"X\" V=\"{:.4}\"/><Cell N=\"Y\" V=\"{:.4}\"/></Row>\n",
                        row_ix, x0, y0
                    ));
                    row_ix += 1;
                    for (px, py) in [(x1, y0), (x1, y1), (x0, y1), (x0, y0)] {
                        out.push_str(&format!(
                            "        <Row T=\"LineTo\" IX=\"{}\"><Cell N=\"X\" V=\"{:.4}\"/><Cell N=\"Y\" V=\"{:.4}\"/></Row>\n",
                            row_ix, px, py
                        ));
                        row_ix += 1;
                    }
                }
                PrimOp::Path { points, .. } | PrimOp::Polyline { points, .. } => {
                    if let Some(first) = points.first() {
                        out.push_str(&format!(
                            "        <Row T=\"MoveTo\" IX=\"{}\"><Cell N=\"X\" V=\"{:.4}\"/><Cell N=\"Y\" V=\"{:.4}\"/></Row>\n",
                            row_ix,
                            first.0 * w_in,
                            (1.0 - first.1) * h_in
                        ));
                        row_ix += 1;
                        for pt in &points[1..] {
                            out.push_str(&format!(
                                "        <Row T=\"LineTo\" IX=\"{}\"><Cell N=\"X\" V=\"{:.4}\"/><Cell N=\"Y\" V=\"{:.4}\"/></Row>\n",
                                row_ix,
                                pt.0 * w_in,
                                (1.0 - pt.1) * h_in
                            ));
                            row_ix += 1;
                        }
                    }
                }
                PrimOp::Ellipse { cx, cy, rx, ry, .. } => {
                    let ecx = cx * w_in;
                    let ecy = (1.0 - cy) * h_in;
                    let erx = rx * w_in;
                    let ery = ry * h_in;
                    out.push_str(&format!(
                        "        <Row T=\"Ellipse\" IX=\"{}\"><Cell N=\"X\" V=\"{:.4}\"/><Cell N=\"Y\" V=\"{:.4}\"/><Cell N=\"A\" V=\"{:.4}\"/><Cell N=\"B\" V=\"{:.4}\"/><Cell N=\"C\" V=\"{:.4}\"/><Cell N=\"D\" V=\"{:.4}\"/></Row>\n",
                        row_ix, ecx, ecy, ecx + erx, ecy, ecx, ecy + ery
                    ));
                    row_ix += 1;
                }
                _ => {}
            }
        }
        out.push_str("      </Section>\n");

        if !text_display.is_empty() {
            out.push_str(&format!(
                "      <Text>{}</Text>\n",
                escape_xml(&text_display)
            ));
        }
        out.push_str("    </Shape>\n");
    }

    // Export 1D Connectors as Visio Dynamic Connector shapes
    for c in &page.connectors {
        let bx = c.from_pos.x / INCH_TO_MM;
        let by = (page.height_mm - c.from_pos.y) / INCH_TO_MM;
        let ex = c.to_pos.x / INCH_TO_MM;
        let ey = (page.height_mm - c.to_pos.y) / INCH_TO_MM;

        out.push_str(&format!(
            "    <Shape ID=\"{}\" NameU=\"Dynamic connector\" Type=\"Shape\">\n      <Cell N=\"BeginX\" V=\"{:.4}\"/>\n      <Cell N=\"BeginY\" V=\"{:.4}\"/>\n      <Cell N=\"EndX\" V=\"{:.4}\"/>\n      <Cell N=\"EndY\" V=\"{:.4}\"/>\n",
            c.id, bx, by, ex, ey
        ));
        if !c.label.is_empty() {
            out.push_str(&format!("      <Text>{}</Text>\n", escape_xml(&c.label)));
        }
        out.push_str("    </Shape>\n");
    }

    out.push_str("  </Shapes>\n  <Connects>\n");
    for c in &page.connectors {
        if let Some(fid) = c.from_shape {
            out.push_str(&format!(
                "    <Connect FromSheet=\"{}\" FromCell=\"BeginX\" ToSheet=\"{}\" ToCell=\"PinX\"/>\n",
                c.id, fid
            ));
        }
        if let Some(tid) = c.to_shape {
            out.push_str(&format!(
                "    <Connect FromSheet=\"{}\" FromCell=\"EndX\" ToSheet=\"{}\" ToCell=\"PinX\"/>\n",
                c.id, tid
            ));
        }
    }
    out.push_str("  </Connects>\n");
    out
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
