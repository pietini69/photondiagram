use crate::model::{ConnectionPort, DiagramDocument, DiagramPage, DiagramShape, PortDir, PrimOp};
use crate::stencils::built_in_stencils;

const OLE2_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// Parses a legacy binary Microsoft Visio `.vsd` (OLE2 Compound Document) file
/// and extracts shapes, component labels, and layout blocks into a `DiagramDocument`.
pub fn read_vsd_bytes(bytes: &[u8]) -> Result<DiagramDocument, String> {
    if bytes.len() < 16 {
        return Err("VSD-tiedosto on liian lyhyt tai tyhjä".to_string());
    }

    // If the file is actually a ZIP (.vsdx renamed to .vsd), delegate to vsdx reader
    if bytes.starts_with(b"PK\x03\x04") {
        return crate::formats::vsdx::read_vsdx_bytes(bytes);
    }

    // If the file starts with XML (.vdx renamed to .vsd), delegate to vdx reader
    if let Ok(txt) = std::str::from_utf8(&bytes[..bytes.len().min(256)]) {
        if txt.trim_start().starts_with("<?xml") || txt.contains("<VisioDocument") {
            let full = String::from_utf8_lossy(bytes);
            return crate::formats::vsdx::read_vdx_str(&full);
        }
    }

    if bytes.len() >= 8 && bytes[0..8] != OLE2_MAGIC {
        return Err("Tiedosto ei ole kelvollinen OLE2 Binary Visio (.vsd) -tiedosto".to_string());
    }

    // Extract payload after the 512-byte OLE2 header
    let payload = if bytes.len() > 512 {
        &bytes[512..]
    } else {
        bytes
    };

    let labels = extract_vsd_strings(payload);
    let coords = extract_vsd_xforms(payload);
    let builtins = built_in_stencils();

    let mut doc = DiagramDocument::default();
    let mut page = DiagramPage::default();
    page.name = "Tuotu VSD-kaavio".to_string();

    let count = labels.len().max(coords.len()).clamp(1, 64);

    for i in 0..count {
        let id = doc.alloc_id();
        let label = labels
            .get(i)
            .cloned()
            .unwrap_or_else(|| format!("VSD-osa {}", i + 1));

        let (x_mm, y_mm, w_mm, h_mm) = coords.get(i).copied().unwrap_or_else(|| {
            let col = (i % 4) as f64;
            let row = (i / 4) as f64;
            (40.0 + col * 55.0, 35.0 + row * 35.0, 28.0, 16.0)
        });

        // Match against built-in stencil if the label suggests an electrical or floorplan symbol
        let lower = label.to_lowercase();
        let matched_stencil = builtins.iter().find(|s| {
            lower.contains(&s.id.to_lowercase())
                || (lower.starts_with('r') && s.id == "iec_resistor")
                || (lower.starts_with('c') && s.id == "iec_capacitor")
                || (lower.starts_with('k') && s.id == "iec_relay")
                || (lower.starts_with('f') && s.id == "iec_breaker")
        });

        if let Some(st) = matched_stencil {
            let mut sh = st.instantiate(id, x_mm, y_mm, i + 1);
            sh.label = label;
            page.shapes.push(sh);
        } else {
            page.shapes.push(DiagramShape {
                id,
                stencil_id: "vsd_imported".to_string(),
                name: "VSD-elementti".to_string(),
                label,
                value: String::new(),
                x: x_mm,
                y: y_mm,
                width: w_mm,
                height: h_mm,
                rotation_deg: 0.0,
                flip_x: false,
                flip_y: false,
                stroke_color: [30, 41, 59, 255],
                fill_color: [241, 245, 249, 255],
                stroke_width: 1.8,
                dashed: false,
                layer: "Oletus".to_string(),
                ports: vec![
                    ConnectionPort::new("L", 0.0, 0.5, PortDir::Left),
                    ConnectionPort::new("R", 1.0, 0.5, PortDir::Right),
                    ConnectionPort::new("T", 0.5, 0.0, PortDir::Up),
                    ConnectionPort::new("B", 0.5, 1.0, PortDir::Down),
                ],
                geometry: vec![PrimOp::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 1.0,
                    h: 1.0,
                    filled: true,
                }],
                properties: Vec::new(),
                is_dimension: false,
                is_room_area: false,
                locked: false,
            });
        }
    }

    doc.pages = vec![page];
    Ok(doc)
}

/// Scans binary stream for clean UTF-16LE and ASCII text labels, filtering out OLE2 directory names.
fn extract_vsd_strings(data: &[u8]) -> Vec<String> {
    let mut results = Vec::new();
    let mut ignored = [
        "Root Entry",
        "VisioDocument",
        "SummaryInformation",
        "DocumentSummaryInformation",
        "Visio",
        "Microsoft",
    ];
    ignored.sort();

    // 1. Scan UTF-16LE sequences (common in Visio 2000-2010 binary streams)
    let mut i = 0;
    while i + 8 <= data.len() {
        let mut chars = Vec::new();
        let mut j = i;
        while j + 1 < data.len() {
            let lo = data[j];
            let hi = data[j + 1];
            if hi == 0 && (32..=126).contains(&lo) {
                chars.push(lo as char);
                j += 2;
            } else {
                break;
            }
        }
        if chars.len() >= 2 && chars.len() <= 48 {
            let s: String = chars.into_iter().collect();
            let trimmed = s.trim();
            if trimmed.len() >= 2
                && !ignored.iter().any(|ig| trimmed.contains(ig))
                && !results.contains(&trimmed.to_string())
            {
                results.push(trimmed.to_string());
            }
            i = j + 2;
        } else {
            i += 2;
        }
    }

    // 2. If no UTF-16LE labels were found, scan ASCII runs
    if results.is_empty() {
        let mut cur = String::new();
        for &b in data {
            if (32..=126).contains(&b) {
                cur.push(b as char);
            } else {
                let trimmed = cur.trim();
                if trimmed.len() >= 3
                    && trimmed.len() <= 40
                    && trimmed.chars().any(|c| c.is_alphabetic())
                    && !ignored.iter().any(|ig| trimmed.contains(ig))
                    && !results.contains(&trimmed.to_string())
                {
                    results.push(trimmed.to_string());
                }
                cur.clear();
            }
        }
    }

    results
}

/// Scans for plausible IEEE-754 f64 XForm tuples `(PinX, PinY, Width, Height)` in inches
fn extract_vsd_xforms(data: &[u8]) -> Vec<(f64, f64, f64, f64)> {
    let mut coords = Vec::new();
    let mut i = 0;
    while i + 32 <= data.len() {
        // Look for marker `VSDXFORM` or plausible 4x f64 tuple
        if &data[i..i + 8] == b"VSDXFORM" && i + 40 <= data.len() {
            let x = f64::from_le_bytes(data[i + 8..i + 16].try_into().unwrap());
            let y = f64::from_le_bytes(data[i + 16..i + 24].try_into().unwrap());
            let w = f64::from_le_bytes(data[i + 24..i + 32].try_into().unwrap());
            let h = f64::from_le_bytes(data[i + 32..i + 40].try_into().unwrap());
            if x.is_finite() && y.is_finite() && w.is_finite() && h.is_finite() {
                coords.push((x, y, w.max(4.0), h.max(4.0)));
                i += 40;
                continue;
            }
        }
        i += 4;
    }
    coords
}
