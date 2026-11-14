pub mod formats;
pub mod gui;
pub mod model;
pub mod routing;
pub mod stencils;
pub mod templates;
pub mod triangulate;

#[cfg(test)]
mod tests {
    use crate::formats::export::{export_bom_csv, export_page_dxf, export_page_svg};
    use crate::formats::vsd::read_vsd_bytes;
    use crate::formats::vsdx::{
        import_vssx_stencils, read_vdx_str, read_vsdx_bytes, write_vdx_string, write_vsdx_bytes,
    };
    use crate::formats::zip_archive::ZipArchive;
    use crate::model::{ArrowType, ConnectorStyle, DiagramConnector, DrawingScale, Point};
    use crate::routing::{find_wire_jumps, update_all_connectors};
    use crate::stencils::{built_in_stencils, instantiate_shape, StencilCategory};
    use crate::templates::{template_electrical_schematic, template_floorplan_and_sfs_elec};

    #[test]
    fn test_built_in_stencils_complete() {
        let stencils = built_in_stencils();
        assert!(
            stencils.len() >= 34,
            "Expected at least 34 built-in stencils, got {}",
            stencils.len()
        );
        // Verify electrical & floorplan essentials exist
        for id in [
            "iec_resistor",
            "iec_relay",
            "iec_breaker",
            "sfs_socket_2",
            "sfs_panel",
            "fp_room",
            "fp_door_single",
            "fp_window",
            "fp_sauna_stove",
            "plc_block",
        ] {
            let s = instantiate_shape(id, 1, 50.0, 50.0, 1);
            assert!(s.is_some(), "Missing stencil '{}'", id);
        }
    }

    #[test]
    fn test_orthogonal_routing_and_wire_jumps() {
        let mut doc = template_electrical_schematic();
        let page = doc.current_page_mut();
        update_all_connectors(&mut page.connectors, &page.shapes);
        assert!(page.connectors.iter().all(|c| c.waypoints.len() >= 2));

        // Add two perpendicular crossing connectors to verify wire jump detection
        let c1 = DiagramConnector {
            id: 901,
            from_shape: None,
            from_port: 0,
            from_pos: Point::new(20.0, 50.0),
            to_shape: None,
            to_port: 0,
            to_pos: Point::new(80.0, 50.0),
            style: ConnectorStyle::Straight,
            start_arrow: ArrowType::None,
            end_arrow: ArrowType::None,
            label: String::new(),
            stroke_color: [0, 0, 0, 255],
            stroke_width: 1.5,
            dashed: false,
            layer: "Oletus".to_string(),
            waypoints: vec![Point::new(20.0, 50.0), Point::new(80.0, 50.0)],
        };
        let c2 = DiagramConnector {
            id: 902,
            from_shape: None,
            from_port: 0,
            from_pos: Point::new(50.0, 20.0),
            to_shape: None,
            to_port: 0,
            to_pos: Point::new(50.0, 80.0),
            style: ConnectorStyle::Straight,
            start_arrow: ArrowType::None,
            end_arrow: ArrowType::None,
            label: String::new(),
            stroke_color: [0, 0, 0, 255],
            stroke_width: 1.5,
            dashed: false,
            layer: "Oletus".to_string(),
            waypoints: vec![Point::new(50.0, 20.0), Point::new(50.0, 80.0)],
        };
        let jumps = find_wire_jumps(&[c1, c2]);
        assert_eq!(jumps.len(), 1);
        assert!((jumps[0].point.x - 50.0).abs() < 0.1);
        assert!((jumps[0].point.y - 50.0).abs() < 0.1);
    }

    #[test]
    fn test_vsdx_roundtrip_and_pure_xml_parsing() {
        let doc = template_floorplan_and_sfs_elec();
        let bytes = write_vsdx_bytes(&doc).expect("write_vsdx_bytes failed");
        assert!(bytes.starts_with(b"PK\x03\x04"));

        // 1. Roundtrip with embedded metadata
        let loaded = read_vsdx_bytes(&bytes).expect("read_vsdx_bytes failed");
        assert_eq!(loaded.pages[0].shapes.len(), doc.pages[0].shapes.len());
        assert_eq!(loaded.pages[0].scale, DrawingScale::Scale1To50);

        // 2. Strip photon_vision.json to test pure MS Visio XML parsing (`visio/pages/page1.xml`)
        let mut archive = ZipArchive::from_bytes(&bytes).unwrap();
        archive.files.remove("visio/photon_vision.json");
        let pure_vsdx_bytes = archive.to_bytes();
        let parsed_from_xml =
            read_vsdx_bytes(&pure_vsdx_bytes).expect("pure Visio XML parsing failed");
        assert_eq!(
            parsed_from_xml.pages[0].shapes.len(),
            doc.pages[0].shapes.len()
        );
        assert_eq!(
            parsed_from_xml.pages[0].connectors.len(),
            doc.pages[0].connectors.len()
        );
    }

    #[test]
    fn test_vssx_stencil_import() {
        let mut zip = ZipArchive::new();
        let master_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MasterContents>
  <Shapes>
    <Shape ID="1" NameU="CustomContactor">
      <Cell N="PinX" V="1.0"/>
      <Cell N="PinY" V="1.0"/>
      <Cell N="Width" V="0.8"/>
      <Cell N="Height" V="0.6"/>
      <Section N="Geometry" IX="0">
        <Row T="MoveTo" IX="1"><Cell N="X" V="0.0"/><Cell N="Y" V="0.0"/></Row>
        <Row T="LineTo" IX="2"><Cell N="X" V="0.8"/><Cell N="Y" V="0.6"/></Row>
      </Section>
      <Text>K99</Text>
    </Shape>
  </Shapes>
</MasterContents>"#;
        zip.insert("visio/masters/master1.xml", master_xml.as_bytes().to_vec());
        let bytes = zip.to_bytes();
        let imported = import_vssx_stencils(&bytes, "test_lib").expect("import_vssx_stencils failed");
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0].name, "CustomContactor");
        assert!(imported[0].category == StencilCategory::ImportedVisio("test_lib".to_string()));

        // Also test with a real downloaded VSSX file if present on disk
        let real_vssx = std::path::Path::new("/Users/nikopietinen/Downloads/visiostencilfactory-master/stencils/azure-docs.vssx");
        if real_vssx.exists() {
            let file_bytes = std::fs::read(real_vssx).unwrap();
            let real_imported = import_vssx_stencils(&file_bytes, "azure-docs").unwrap();
            assert!(real_imported.len() > 100);
            assert!(real_imported.iter().any(|s| s.name.contains("active directory")));
            assert!(real_imported.iter().all(|s| s.name != "Shape"));

            // Verify 'search' retains its vector ellipse and does NOT have an occluding dummy bounding box
            let search = real_imported.iter().find(|s| s.name == "search").expect("Missing 'search'");
            assert_eq!(search.default_label, "search");
            assert!(
                search.geometry.iter().any(|op| matches!(op, crate::model::PrimOp::Ellipse { .. })),
                "Search must contain an Ellipse vector primitive"
            );
            assert!(
                !search.geometry.iter().any(|op| {
                    if let crate::model::PrimOp::Path { points, fill_color, stroke_color, .. } = op {
                        points.len() == 5 && fill_color.is_none() && stroke_color.is_none()
                    } else {
                        false
                    }
                }),
                "Search must not have invisible dummy bounding box path"
            );

            // Verify 'notification hubs' uses clean monogram placeholder badge ("NH")
            let notif = real_imported.iter().find(|s| s.name == "notification hubs").expect("Missing 'notification hubs'");
            assert_eq!(notif.default_label, "notification hubs");
            assert!(
                notif.geometry.iter().any(|op| {
                    if let crate::model::PrimOp::Text { text, .. } = op {
                        text == "NH"
                    } else {
                        false
                    }
                }),
                "Notification hubs must have monogram 'NH'"
            );

            // Verify 'media services' uses clean monogram placeholder badge ("MS")
            let media = real_imported.iter().find(|s| s.name == "media services").expect("Missing 'media services'");
            assert_eq!(media.default_label, "media services");
            assert!(
                media.geometry.iter().any(|op| {
                    if let crate::model::PrimOp::Text { text, .. } = op {
                        text == "MS"
                    } else {
                        false
                    }
                }),
                "Media services must have monogram 'MS'"
            );
        }

        let logos_vssx = std::path::Path::new("/Users/nikopietinen/Downloads/visiostencilfactory-master/stencils/logos.vssx");
        if logos_vssx.exists() {
            let file_bytes = std::fs::read(logos_vssx).unwrap();
            let logos_imported = import_vssx_stencils(&file_bytes, "logos").unwrap();
            assert!(logos_imported.len() >= 100);
            assert!(logos_imported.iter().any(|s| s.name.contains("android")));
            assert!(logos_imported.iter().any(|s| s.name.contains("angular")));
            assert!(logos_imported.iter().all(|s| s.name != "Shape"));
            let android = logos_imported.iter().find(|s| s.name == "android-1").unwrap();
            assert!(!android.geometry.is_empty());
        }

        // Test general.vssx and specifically Free Services
        let gen_vssx = std::path::Path::new("/Users/nikopietinen/Downloads/visiostencilfactory-master/stencils/azure-icons/general.vssx");
        if gen_vssx.exists() {
            let file_bytes = std::fs::read(gen_vssx).unwrap();
            let gen_imported = import_vssx_stencils(&file_bytes, "general").unwrap();
            let free_svc = gen_imported.iter().find(|s| s.name.to_lowercase().contains("free services")).expect("Missing Free Services");
            
            // Verify all paths are well-formed and can be triangulated without miter explosion
            let mut path_count = 0;
            for op in &free_svc.geometry {
                if let crate::model::PrimOp::Path { points, closed, .. } = op {
                    path_count += 1;
                    if *closed && points.len() >= 3 {
                        let pos_pts: Vec<eframe::egui::Pos2> = points
                            .iter()
                            .map(|(x, y)| eframe::egui::Pos2::new(*x as f32, *y as f32))
                            .collect();
                        let tris = crate::triangulate::triangulate_polygon(&pos_pts);
                        assert!(!tris.is_empty(), "Failed to triangulate Free Services path");
                        for tri in tris {
                            for idx in tri {
                                assert!((idx as usize) < pos_pts.len());
                            }
                        }
                    }
                }
            }
            assert!(path_count > 0, "Free Services should have vector paths");
        }
    }

    #[test]
    fn test_vdx_and_vsd_readers() {
        let doc = template_electrical_schematic();
        let vdx_str = write_vdx_string(&doc);
        let parsed_vdx = read_vdx_str(&vdx_str).expect("read_vdx_str failed");
        assert_eq!(parsed_vdx.pages[0].shapes.len(), doc.pages[0].shapes.len());

        // Test legacy binary OLE2 .vsd header + UTF-16LE label extraction
        let mut fake_vsd = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
        fake_vsd.resize(512, 0);
        for ch in "R101 4.7kOhm".encode_utf16() {
            fake_vsd.extend_from_slice(&ch.to_le_bytes());
        }
        fake_vsd.extend_from_slice(&[0, 0, 0, 0]);
        let vsd_doc = read_vsd_bytes(&fake_vsd).expect("read_vsd_bytes failed");
        assert!(!vsd_doc.pages[0].shapes.is_empty());
        assert_eq!(vsd_doc.pages[0].shapes[0].label, "R101 4.7kOhm");
    }

    #[test]
    fn test_svg_dxf_and_csv_exports() {
        let doc = template_floorplan_and_sfs_elec();
        let svg = export_page_svg(doc.current_page(), true);
        assert!(svg.contains("<svg"));
        assert!(svg.contains("Olohuone"));

        let dxf = export_page_dxf(doc.current_page());
        assert!(dxf.contains("SECTION"));
        assert!(dxf.contains("ENTITIES"));
        assert!(dxf.contains("EOF"));

        let csv = export_bom_csv(&doc);
        assert!(csv.contains("Komponentti / Symboli"));
        assert!(csv.contains("Olohuone"));
        assert!(csv.contains("m²"));
    }

    #[test]
    fn test_photoncam_lbrn2_export_and_import() {
        use crate::formats::photoncam::{export_document_lbrn2, import_lbrn2_bytes};

        let doc = template_floorplan_and_sfs_elec();
        let lbrn2_xml = export_document_lbrn2(&doc);

        // Verify XML header and CutSettings
        assert!(lbrn2_xml.contains("<LightBurnProject"));
        assert!(lbrn2_xml.contains("C00 (Cut / Leikkaus)"));
        assert!(lbrn2_xml.contains("C01 (Scan / Kaiverrus)"));
        assert!(lbrn2_xml.contains("C02 (Mark / Merkintä &amp; Johdot)"));

        // Verify Shapes
        assert!(lbrn2_xml.contains("<Shape Type=\"Rect\""));
        assert!(lbrn2_xml.contains("<Shape Type=\"Text\""));

        // Verify Round-trip Import
        let imported = import_lbrn2_bytes(lbrn2_xml.as_bytes()).expect("import_lbrn2_bytes failed");
        assert_eq!(imported.pages.len(), 1);
        assert!(!imported.pages[0].shapes.is_empty(), "Imported page should have shapes");
        assert!(imported.pages[0].width_mm >= 297.0);
        assert!(imported.pages[0].height_mm >= 210.0);
    }

    #[test]
    fn test_flowchart_inside_text_and_stencil_labels() {
        use crate::formats::export::shape_display_caption;
        use crate::model::{DiagramPage, TextPosition};
        use crate::stencils::{instantiate_shape, StencilCategory, StencilItem};

        let page = DiagramPage::default();

        // 1. Process block text is centered inside
        let mut proc = instantiate_shape("flow_process", 1, 100.0, 50.0, 1).expect("flow_process");
        assert_eq!(proc.effective_text_position(), TextPosition::Center);
        assert_eq!(proc.label_position(), Point::new(100.0, 50.0));
        assert_eq!(shape_display_caption(&page, &proc), "Prosessi");

        // 2. Multiline text inside process block
        proc.label = "Vaihe 1:\nSyötä data".to_string();
        assert_eq!(shape_display_caption(&page, &proc), "Vaihe 1:\nSyötä data");
        assert_eq!(proc.label_position(), Point::new(100.0, 50.0));

        // 3. Decision block default label is clean "Ehto" (no unwanted counter numbers like "Ehto6")
        let dec = instantiate_shape("flow_decision", 2, 100.0, 90.0, 6).expect("flow_decision");
        assert_eq!(dec.label, "Ehto");
        assert_eq!(dec.effective_text_position(), TextPosition::Center);
        assert_eq!(dec.label_position(), Point::new(100.0, 90.0));

        // 4. Other flowchart stencils
        assert!(instantiate_shape("flow_start_end", 3, 10.0, 10.0, 1).is_some());
        assert!(instantiate_shape("flow_io", 4, 10.0, 10.0, 1).is_some());
        assert!(instantiate_shape("flow_document", 5, 10.0, 10.0, 1).is_some());
        assert!(instantiate_shape("flow_database", 6, 10.0, 10.0, 1).is_some());
        assert!(instantiate_shape("flow_circle", 7, 10.0, 10.0, 1).is_some());

        // 5. Imported stencil without custom text has NO floating label
        let imported_item = StencilItem {
            id: "vssx_icons_book".into(),
            name: "Book".into(),
            category: StencilCategory::ImportedVisio("icons".into()),
            default_label: "Book".into(),
            default_value: "".into(),
            default_width_mm: 20.0,
            default_height_mm: 20.0,
            default_layer: "Oletus".into(),
            stroke_color: [30, 41, 59, 255],
            fill_color: [241, 245, 249, 255],
            stroke_width: 1.5,
            ports: Vec::new(),
            geometry: Vec::new(),
            is_dimension: false,
            is_room_area: false,
        };
        let mut icon_shape = imported_item.instantiate(8, 50.0, 50.0, 1);
        assert_eq!(icon_shape.label, "");
        assert_eq!(shape_display_caption(&page, &icon_shape), "");

        // 6. User can add text and position it or hide it
        icon_shape.label = "Käsikirja".into();
        assert_eq!(shape_display_caption(&page, &icon_shape), "Käsikirja");
        assert_eq!(icon_shape.effective_text_position(), TextPosition::Bottom);
        assert!(icon_shape.label_position().y > 50.0);

        icon_shape.text_position = TextPosition::Hidden;
        assert_eq!(shape_display_caption(&page, &icon_shape), "");

        icon_shape.text_position = TextPosition::Center;
        assert_eq!(icon_shape.label_position(), Point::new(50.0, 50.0));
    }

    #[test]
    fn test_textbox_and_callout_components() {
        use crate::model::{TextAlign, TextPosition};
        use crate::stencils::instantiate_shape;

        // 1. Textbox stencil instantiated via "text" or "note_textbox"
        let tb = instantiate_shape("text", 101, 120.0, 80.0, 1).expect("Failed to create textbox");
        assert_eq!(tb.stencil_id, "note_textbox");
        assert_eq!(tb.effective_text_position(), TextPosition::Center);
        assert_eq!(tb.effective_text_align(), TextAlign::Left);
        assert_eq!(tb.font_size, 0.0);
        assert_eq!(tb.width, 60.0);
        assert_eq!(tb.height, 25.0);
        assert_eq!(tb.layer, "Mitat & Tekstit");

        // 2. Callout stencil instantiated via "callout" or "note_callout"
        let co = instantiate_shape("callout", 102, 140.0, 90.0, 1).expect("Failed to create callout");
        assert_eq!(co.stencil_id, "note_callout");
        assert_eq!(co.effective_text_position(), TextPosition::Center);
        assert_eq!(co.layer, "Mitat & Tekstit");

        // 3. Custom text alignment and font size
        let mut custom = tb;
        custom.text_align = TextAlign::Right;
        assert_eq!(custom.effective_text_align(), TextAlign::Right);
        custom.font_size = 14.5;
        assert_eq!(custom.font_size, 14.5);
    }
}
