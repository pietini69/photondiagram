pub mod export;
pub mod photoncam;
pub mod vsd;
pub mod vsdx;
pub mod zip_archive;

use crate::model::DiagramDocument;
use crate::stencils::StencilItem;
use std::fs;
use std::path::Path;

/// Opens a diagram document from disk based on its file extension (`.pvis`, `.vsdx`, `.vdx`, `.vsd`).
pub fn load_document_from_path(path: &Path) -> Result<DiagramDocument, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let bytes = fs::read(path).map_err(|e| format!("Tiedoston lukuvirhe ({:?}): {}", path, e))?;

    match ext.as_str() {
        "pvis" | "json" => serde_json::from_slice::<DiagramDocument>(&bytes)
            .map_err(|e| format!("PhotonVision JSON -virhe: {}", e)),
        "lbrn2" | "lbrn" => photoncam::import_lbrn2_bytes(&bytes),
        "vsdx" => vsdx::read_vsdx_bytes(&bytes),
        "vdx" | "xml" => {
            let text = String::from_utf8_lossy(&bytes);
            if text.contains("<LightBurnProject") {
                photoncam::import_lbrn2_bytes(&bytes)
            } else {
                vsdx::read_vdx_str(&text)
            }
        }
        "vsd" => vsd::read_vsd_bytes(&bytes),
        _ => {
            // Auto-detect by content header
            if bytes.starts_with(b"PK\x03\x04") {
                vsdx::read_vsdx_bytes(&bytes)
            } else if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
                vsd::read_vsd_bytes(&bytes)
            } else if let Ok(doc) = serde_json::from_slice::<DiagramDocument>(&bytes) {
                Ok(doc)
            } else {
                let text = String::from_utf8_lossy(&bytes);
                if text.contains("<LightBurnProject") {
                    photoncam::import_lbrn2_bytes(&bytes)
                } else {
                    vsdx::read_vdx_str(&text)
                }
            }
        }
    }
}

/// Saves a diagram document to disk based on its file extension (`.pvis`, `.vsdx`, `.vdx`, `.svg`, `.dxf`, `.csv`).
pub fn save_document_to_path(doc: &DiagramDocument, path: &Path) -> Result<(), String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("pvis")
        .to_lowercase();

    match ext.as_str() {
        "pvis" | "json" => {
            let json = serde_json::to_vec_pretty(doc)
                .map_err(|e| format!("JSON-tallennusvirhe: {}", e))?;
            fs::write(path, json).map_err(|e| format!("Kirjoitusvirhe: {}", e))
        }
        "vsdx" => {
            let bytes = vsdx::write_vsdx_bytes(doc)?;
            fs::write(path, bytes).map_err(|e| format!("VSDX-kirjoitusvirhe: {}", e))
        }
        "vdx" => {
            let xml = vsdx::write_vdx_string(doc);
            fs::write(path, xml.as_bytes()).map_err(|e| format!("VDX-kirjoitusvirhe: {}", e))
        }
        "svg" => {
            let svg = export::export_page_svg(doc.current_page(), doc.show_line_jumps);
            fs::write(path, svg.as_bytes()).map_err(|e| format!("SVG-kirjoitusvirhe: {}", e))
        }
        "dxf" => {
            let dxf = export::export_page_dxf(doc.current_page());
            fs::write(path, dxf.as_bytes()).map_err(|e| format!("DXF-kirjoitusvirhe: {}", e))
        }
        "csv" => {
            let csv = export::export_bom_csv(doc);
            fs::write(path, csv.as_bytes()).map_err(|e| format!("CSV-kirjoitusvirhe: {}", e))
        }
        "lbrn2" | "lbrn" => {
            let xml = photoncam::export_document_lbrn2(doc);
            fs::write(path, xml.as_bytes()).map_err(|e| format!("PhotonCAM LBRN2 -kirjoitusvirhe: {}", e))
        }
        _ => {
            let bytes = vsdx::write_vsdx_bytes(doc)?;
            fs::write(path, bytes).map_err(|e| format!("Kirjoitusvirhe: {}", e))
        }
    }
}

/// Imports a `.vssx` stencil library file from disk.
pub fn load_stencil_library(path: &Path) -> Result<Vec<StencilItem>, String> {
    let bytes =
        fs::read(path).map_err(|e| format!("Kirjastotiedoston lukuvirhe ({:?}): {}", path, e))?;
    let lib_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Visio-kirjasto");
    vsdx::import_vssx_stencils(&bytes, lib_name)
}
