use crate::model::{
    ArrowType, ConnectorStyle, DiagramConnector, DiagramDocument, DiagramPage, DrawingScale, Point,
};
use crate::routing::update_all_connectors;
use crate::stencils::instantiate_shape;

/// Creates a complete IEC electrical & PLC control schematic template.
pub fn template_electrical_schematic() -> DiagramDocument {
    let mut doc = DiagramDocument {
        title: "Moottorin & Releen Ohjauskytkentä (IEC)".to_string(),
        ..Default::default()
    };

    let mut page = DiagramPage {
        name: "Kytkentäkaavio (IEC)".to_string(),
        width_mm: 297.0,
        height_mm: 210.0,
        grid_spacing_mm: 5.0,
        scale: DrawingScale::Scale1To1,
        ..Default::default()
    };

    // Instantiate IEC components
    let mut g1 = instantiate_shape("iec_dc_source", doc.alloc_id(), 35.0, 95.0, 1).unwrap();
    g1.label = "G1".to_string();
    g1.value = "24V DC".to_string();

    let mut f1 = instantiate_shape("iec_breaker", doc.alloc_id(), 70.0, 45.0, 1).unwrap();
    f1.label = "F1".to_string();
    f1.value = "10A C".to_string();

    let mut s1 = instantiate_shape("iec_switch_spst", doc.alloc_id(), 115.0, 45.0, 1).unwrap();
    s1.label = "S1 (Käynnistys)".to_string();
    s1.value = "NO".to_string();

    let mut plc = instantiate_shape("plc_block", doc.alloc_id(), 165.0, 85.0, 1).unwrap();
    plc.label = "PLC1 (Ohjauslogiikka)".to_string();
    plc.value = "24VDC I/O".to_string();

    let mut k1 = instantiate_shape("iec_relay", doc.alloc_id(), 220.0, 72.0, 1).unwrap();
    k1.label = "K1".to_string();
    k1.value = "24V / 16A".to_string();

    let mut r1 = instantiate_shape("iec_resistor", doc.alloc_id(), 215.0, 130.0, 1).unwrap();
    r1.label = "R1".to_string();
    r1.value = "1.2 kΩ".to_string();

    let mut led1 = instantiate_shape("iec_led", doc.alloc_id(), 250.0, 130.0, 1).unwrap();
    led1.label = "H1 (Tila-LED)".to_string();
    led1.value = "Vihreä".to_string();

    let mut m1 = instantiate_shape("iec_motor", doc.alloc_id(), 262.0, 65.0, 1).unwrap();
    m1.label = "M1".to_string();
    m1.value = "1.5 kW".to_string();

    let gnd1 = instantiate_shape("iec_gnd", doc.alloc_id(), 35.0, 160.0, 1).unwrap();
    let gnd2 = instantiate_shape("iec_gnd", doc.alloc_id(), 262.0, 168.0, 2).unwrap();
    let pe1 = instantiate_shape("iec_pe", doc.alloc_id(), 284.0, 98.0, 1).unwrap();

    let g1_id = g1.id;
    let f1_id = f1.id;
    let s1_id = s1.id;
    let plc_id = plc.id;
    let k1_id = k1.id;
    let r1_id = r1.id;
    let led1_id = led1.id;
    let m1_id = m1.id;
    let gnd1_id = gnd1.id;
    let gnd2_id = gnd2.id;
    let pe1_id = pe1.id;

    page.shapes.extend([
        g1, f1, s1, plc, k1, r1, led1, m1, gnd1, gnd2, pe1,
    ]);

    let mut add_wire =
        |from_s: u64, from_p: usize, to_s: u64, to_p: usize, color: [u8; 4], label: &str, dot_end: bool| {
            let cid = doc.alloc_id();
            page.connectors.push(DiagramConnector {
                id: cid,
                from_shape: Some(from_s),
                from_port: from_p,
                from_pos: Point::ZERO,
                to_shape: Some(to_s),
                to_port: to_p,
                to_pos: Point::ZERO,
                style: ConnectorStyle::Orthogonal,
                start_arrow: ArrowType::None,
                end_arrow: if dot_end {
                    ArrowType::JunctionDot
                } else {
                    ArrowType::None
                },
                label: label.to_string(),
                stroke_color: color,
                stroke_width: 1.8,
                dashed: false,
                layer: "Sähkö & Kytkennät".to_string(),
                waypoints: Vec::new(),
            });
        };

    // +24V feed (Red)
    add_wire(g1_id, 0, f1_id, 0, [220, 38, 38, 255], "+24V", false);
    add_wire(f1_id, 1, s1_id, 0, [220, 38, 38, 255], "", true);
    add_wire(s1_id, 1, plc_id, 0, [220, 38, 38, 255], "", false);
    // Branch from F1 output to K1 switch common (Port 2 = COM)
    add_wire(f1_id, 1, k1_id, 2, [220, 38, 38, 255], "", false);

    // PLC outputs (Blue control wires)
    add_wire(plc_id, 4, k1_id, 0, [37, 99, 235, 255], "", false);
    add_wire(plc_id, 5, r1_id, 0, [37, 99, 235, 255], "", false);
    add_wire(r1_id, 1, led1_id, 0, [37, 99, 235, 255], "", false);

    // Relay NO contact (Port 3) to Motor M1 (Port 0)
    add_wire(k1_id, 3, m1_id, 0, [220, 38, 38, 255], "", false);

    // Return / 0V GND wires (Dark slate)
    add_wire(g1_id, 1, gnd1_id, 0, [30, 41, 59, 255], "", false);
    add_wire(k1_id, 1, gnd2_id, 0, [30, 41, 59, 255], "", true);
    add_wire(led1_id, 1, gnd2_id, 0, [30, 41, 59, 255], "", true);
    add_wire(m1_id, 3, gnd2_id, 0, [30, 41, 59, 255], "", true);
    add_wire(m1_id, 1, pe1_id, 0, [22, 163, 74, 255], "", false);

    update_all_connectors(&mut page.connectors, &page.shapes);
    doc.pages = vec![page];
    doc
}

/// Creates an architectural floor plan (1:50) with rooms (`m²`), doors, windows, furniture,
/// and Finnish SFS electrical installation symbols + wiring.
pub fn template_floorplan_and_sfs_elec() -> DiagramDocument {
    let mut doc = DiagramDocument {
        title: "Pohjapiirustus & SFS-Sähkösuunnitelma (1:50)".to_string(),
        ..Default::default()
    };

    let mut page = DiagramPage {
        name: "Pohjapiirustus 1:50".to_string(),
        width_mm: 297.0,
        height_mm: 210.0,
        grid_spacing_mm: 5.0,
        scale: DrawingScale::Scale1To50,
        ..Default::default()
    };

    // 1. Rooms (background area calculation shapes)
    // At 1:50 scale: 100 mm on paper = 5.0 m in real life!
    let mut room_living = instantiate_shape("fp_room", doc.alloc_id(), 95.0, 95.0, 1).unwrap();
    room_living.label = "Olohuone & Keittiö".to_string();
    room_living.width = 130.0; // 6.5 m
    room_living.height = 120.0; // 6.0 m

    let mut room_bed = instantiate_shape("fp_room", doc.alloc_id(), 210.0, 65.0, 2).unwrap();
    room_bed.label = "Makuuhuone".to_string();
    room_bed.width = 100.0; // 5.0 m
    room_bed.height = 60.0; // 3.0 m

    let mut room_bath = instantiate_shape("fp_room", doc.alloc_id(), 185.0, 125.0, 3).unwrap();
    room_bath.label = "Kylpyhuone / KHH".to_string();
    room_bath.width = 50.0; // 2.5 m
    room_bath.height = 60.0; // 3.0 m

    let mut room_sauna = instantiate_shape("fp_room", doc.alloc_id(), 235.0, 125.0, 4).unwrap();
    room_sauna.label = "Sauna".to_string();
    room_sauna.width = 50.0; // 2.5 m
    room_sauna.height = 60.0; // 3.0 m

    page.shapes
        .extend([room_living, room_bed, room_bath, room_sauna]);

    // 2. Doors & Windows
    let mut main_door = instantiate_shape("fp_door_single", doc.alloc_id(), 30.0, 135.0, 1).unwrap();
    main_door.rotation_deg = 90.0;
    main_door.value = "9x21".to_string();

    let mut bed_door = instantiate_shape("fp_door_single", doc.alloc_id(), 160.0, 75.0, 2).unwrap();
    bed_door.rotation_deg = 270.0;

    let mut bath_door = instantiate_shape("fp_door_single", doc.alloc_id(), 160.0, 125.0, 3).unwrap();
    bath_door.rotation_deg = 270.0;

    let mut sauna_door = instantiate_shape("fp_door_single", doc.alloc_id(), 210.0, 125.0, 4).unwrap();
    sauna_door.rotation_deg = 270.0;

    let win1 = instantiate_shape("fp_window", doc.alloc_id(), 85.0, 35.0, 1).unwrap();
    let win2 = instantiate_shape("fp_window", doc.alloc_id(), 210.0, 35.0, 2).unwrap();
    let win3 = instantiate_shape("fp_window", doc.alloc_id(), 95.0, 155.0, 3).unwrap();

    page.shapes
        .extend([main_door, bed_door, bath_door, sauna_door, win1, win2, win3]);

    // 3. Furniture, Kitchen, Bathroom & Sauna
    let stove = instantiate_shape("fp_stove", doc.alloc_id(), 48.0, 50.0, 1).unwrap();
    let sink = instantiate_shape("fp_sink", doc.alloc_id(), 72.0, 48.0, 1).unwrap();
    let table = instantiate_shape("fp_table_chairs", doc.alloc_id(), 115.0, 68.0, 1).unwrap();
    let sofa = instantiate_shape("fp_sofa", doc.alloc_id(), 95.0, 128.0, 1).unwrap();
    let bed = instantiate_shape("fp_bed_double", doc.alloc_id(), 232.0, 62.0, 1).unwrap();
    let wc = instantiate_shape("fp_wc", doc.alloc_id(), 180.0, 146.0, 1).unwrap();
    let kiuas = instantiate_shape("fp_sauna_stove", doc.alloc_id(), 248.0, 108.0, 1).unwrap();

    page.shapes
        .extend([stove, sink, table, sofa, bed, wc, kiuas]);

    // 4. Finnish SFS Building Electrical Symbols (Rakennussähkö)
    let panel = instantiate_shape("sfs_panel", doc.alloc_id(), 40.0, 118.0, 1).unwrap();
    let sw_living = instantiate_shape("sfs_switch_1", doc.alloc_id(), 42.0, 142.0, 1).unwrap();
    let light_living =
        instantiate_shape("sfs_ceiling_light", doc.alloc_id(), 100.0, 100.0, 1).unwrap();
    let sw_bed = instantiate_shape("sfs_switch_1", doc.alloc_id(), 168.0, 84.0, 2).unwrap();
    let light_bed = instantiate_shape("sfs_ceiling_light", doc.alloc_id(), 205.0, 65.0, 2).unwrap();
    let sock1 = instantiate_shape("sfs_socket_2", doc.alloc_id(), 36.0, 75.0, 1).unwrap();
    let sock2 = instantiate_shape("sfs_socket_2", doc.alloc_id(), 130.0, 148.0, 2).unwrap();
    let sock_3p = instantiate_shape("sfs_socket_3phase", doc.alloc_id(), 252.0, 124.0, 1).unwrap();
    let smoke = instantiate_shape("sfs_smoke_detector", doc.alloc_id(), 135.0, 95.0, 1).unwrap();

    let panel_id = panel.id;
    let sw_living_id = sw_living.id;
    let light_living_id = light_living.id;
    let sw_bed_id = sw_bed.id;
    let light_bed_id = light_bed.id;
    let sock_3p_id = sock_3p.id;

    page.shapes.extend([
        panel,
        sw_living,
        light_living,
        sw_bed,
        light_bed,
        sock1,
        sock2,
        sock_3p,
        smoke,
    ]);

    // Dashed SFS electrical installation lines (switch -> ceiling lamp, panel -> 3-phase sauna feed)
    for (from_s, to_s, lbl) in [
        (sw_living_id, light_living_id, "MMJ 3x1.5S"),
        (sw_bed_id, light_bed_id, "MMJ 3x1.5S"),
        (panel_id, sock_3p_id, "MMJ 5x2.5S (Kiuas)"),
    ] {
        let cid = doc.alloc_id();
        page.connectors.push(DiagramConnector {
            id: cid,
            from_shape: Some(from_s),
            from_port: 0,
            from_pos: Point::ZERO,
            to_shape: Some(to_s),
            to_port: 0,
            to_pos: Point::ZERO,
            style: ConnectorStyle::Curved,
            start_arrow: ArrowType::None,
            end_arrow: ArrowType::None,
            label: lbl.to_string(),
            stroke_color: [220, 38, 38, 220],
            stroke_width: 1.4,
            dashed: true,
            layer: "Sähkö & Kytkennät".to_string(),
            waypoints: Vec::new(),
        });
    }

    // 5. Architectural Dimension Lines (Mitat)
    let mut dim_total_w = instantiate_shape("fp_dimension", doc.alloc_id(), 145.0, 20.0, 1).unwrap();
    dim_total_w.width = 230.0; // 11.50 m
    dim_total_w.label = "Kokonaisleveys".to_string();

    let mut dim_living_w = instantiate_shape("fp_dimension", doc.alloc_id(), 95.0, 172.0, 2).unwrap();
    dim_living_w.width = 130.0; // 6.50 m
    dim_living_w.label = "OH+KT".to_string();

    page.shapes.extend([dim_total_w, dim_living_w]);

    update_all_connectors(&mut page.connectors, &page.shapes);
    doc.pages = vec![page];
    doc
}
