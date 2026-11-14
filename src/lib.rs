pub mod formats;
pub mod gui;
pub mod model;
pub mod routing;
pub mod stencils;
pub mod templates;

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
}
