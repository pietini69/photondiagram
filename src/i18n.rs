use crate::model::{ArrowType, ConnectorStyle, DiagramDocument, DrawingScale, TextAlign, TextPosition};
use crate::stencils::StencilCategory;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum UiLang {
    #[default]
    Fi,
    En,
}

impl UiLang {
    /// Detects default language from environment or defaults to English for international users
    /// if system locale is not Finnish, or Finnish if `LANG`/`LC_ALL` starts with `fi`.
    pub fn detect_system() -> Self {
        for var in ["PHOTONDIAGRAM_LANG", "LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(val) = std::env::var(var) {
                let lower = val.to_lowercase();
                if lower.starts_with("en") {
                    return Self::En;
                }
                if lower.starts_with("fi") {
                    return Self::Fi;
                }
            }
        }
        Self::Fi
    }

    pub fn tr<'a>(&self, fi: &'a str, en: &'a str) -> &'a str {
        match self {
            Self::Fi => fi,
            Self::En => en,
        }
    }

    pub fn window_title(&self) -> &'static str {
        match self {
            Self::Fi => "PhotonDiagram — Sähkökaaviot, Pohjapiirustukset & Kaavio-CAD (.vsdx/.vdx/.vsd)",
            Self::En => "PhotonDiagram — Electrical Schematics, Floor Plans & Technical CAD (.vsdx/.vdx/.vsd)",
        }
    }
}

pub fn scale_label(scale: DrawingScale, lang: UiLang) -> &'static str {
    match (scale, lang) {
        (DrawingScale::Scale1To1, UiLang::Fi) => "1:1 (Kaavio / Todellinen)",
        (DrawingScale::Scale1To1, UiLang::En) => "1:1 (Schematic / Actual)",
        (DrawingScale::Scale1To10, UiLang::Fi) => "1:10 (Detalji)",
        (DrawingScale::Scale1To10, UiLang::En) => "1:10 (Detail)",
        (DrawingScale::Scale1To20, UiLang::Fi) => "1:20 (Tilasuunnitelma)",
        (DrawingScale::Scale1To20, UiLang::En) => "1:20 (Room Layout)",
        (DrawingScale::Scale1To50, UiLang::Fi) => "1:50 (Pohjapiirustus)",
        (DrawingScale::Scale1To50, UiLang::En) => "1:50 (Floor Plan)",
        (DrawingScale::Scale1To100, UiLang::Fi) => "1:100 (Pääpiirustus)",
        (DrawingScale::Scale1To100, UiLang::En) => "1:100 (Master Plan)",
    }
}

pub fn category_title(cat: &StencilCategory, lang: UiLang) -> String {
    match (cat, lang) {
        (StencilCategory::Electrical, UiLang::Fi) => "Sähkö & Elektroniikka (IEC)".to_string(),
        (StencilCategory::Electrical, UiLang::En) => "Electrical & Electronics (IEC)".to_string(),
        (StencilCategory::BuildingElec, UiLang::Fi) => "Rakennussähkö (SFS)".to_string(),
        (StencilCategory::BuildingElec, UiLang::En) => "Building Electrical (SFS/IEC)".to_string(),
        (StencilCategory::FloorStructure, UiLang::Fi) => "Pohjapiirustus: Rakenteet".to_string(),
        (StencilCategory::FloorStructure, UiLang::En) => "Floor Plan: Structures".to_string(),
        (StencilCategory::FloorFurniture, UiLang::Fi) => "Pohjapiirustus: LVI & Kalusteet".to_string(),
        (StencilCategory::FloorFurniture, UiLang::En) => "Floor Plan: HVAC & Furniture".to_string(),
        (StencilCategory::FlowAndNet, UiLang::Fi) => "Vuokaaviot & Automaatio".to_string(),
        (StencilCategory::FlowAndNet, UiLang::En) => "Flowcharts & Automation".to_string(),
        (StencilCategory::ImportedVisio(name), UiLang::Fi) => {
            if name.is_empty() || name == "default" {
                "Tuodut Visio-symbolit (.vssx)".to_string()
            } else {
                format!("Visio: {}", name)
            }
        }
        (StencilCategory::ImportedVisio(name), UiLang::En) => {
            if name.is_empty() || name == "default" {
                "Imported Visio Stencils (.vssx)".to_string()
            } else {
                format!("Visio: {}", name)
            }
        }
    }
}

pub fn stencil_name_en(id: &str) -> Option<&'static str> {
    Some(match id {
        "elec_resistor" => "Resistor (IEC)",
        "elec_resistor_zigzag" => "Resistor (IEEE Zigzag)",
        "elec_potentiometer" => "Potentiometer / Trimmer",
        "elec_capacitor" => "Capacitor",
        "elec_inductor" => "Inductor / Coil",
        "elec_transformer" => "Transformer",
        "elec_diode" => "Diode",
        "elec_led" => "LED Diode",
        "elec_npn" => "NPN Transistor",
        "elec_opamp" => "Operational Amplifier (Op-Amp)",
        "elec_switch_spst" => "Switch (SPST)",
        "elec_relay" => "Relay (Coil + SPDT Contact)",
        "elec_fuse" => "Fuse (IEC)",
        "elec_breaker" => "Circuit Breaker (MCB)",
        "elec_dc_source" => "DC Voltage Source / Battery",
        "elec_ac_source" => "AC Voltage Source",
        "elec_motor" => "Electric Motor (M)",
        "elec_lamp" => "Indicator Lamp / Light (X)",
        "elec_gnd" => "Ground (GND)",
        "elec_pe" => "Protective Earth (PE)",
        "inst_socket_2" => "Dual Socket Outlet (Grounded)",
        "inst_socket_3phase" => "3-Phase Power Outlet (400V)",
        "inst_switch_1" => "Light Switch 1-Pole",
        "inst_switch_stair" => "2-Way / Staircase Switch",
        "inst_ceiling_light" => "Ceiling Light Point",
        "inst_panel" => "Distribution Board / Panel (DB)",
        "inst_smoke_detector" => "Smoke Detector",
        "floor_room" => "Room / Space (Auto m²)",
        "floor_wall_ext" => "Exterior Wall (Load-Bearing)",
        "floor_wall_int" => "Interior Partition Wall",
        "floor_door_right" => "Door (90° Swing Arc)",
        "floor_door_double" => "Double Door",
        "floor_window" => "Window",
        "floor_stairs" => "Stairs (With Up Arrow)",
        "floor_dimension" => "Dimension Line (Auto Measure)",
        "furn_stove" => "Stove & Oven",
        "furn_sink_kitchen" => "Kitchen Sink",
        "furn_wc" => "WC / Toilet",
        "furn_sauna_stove" => "Sauna Heater / Stove",
        "furn_sofa" => "Sofa (3-Seater)",
        "furn_bed_double" => "Double Bed",
        "furn_table_dining" => "Dining Table (6-Person)",
        "flow_start_end" => "Start / End (Terminator)",
        "flow_process" => "Process / Rectangle",
        "flow_decision" => "Decision / Condition (Diamond)",
        "flow_io" => "Input / Output (Data)",
        "flow_document" => "Document / Report",
        "flow_database" => "Database / Storage",
        "flow_circle" => "Connector / State (Circle)",
        "net_plc" => "PLC Controller / MCU (8-Pin)",
        "note_textbox" => "Text Box (Free Text)",
        "note_callout" => "Callout / Annotation Bubble",
        _ => return None,
    })
}

pub fn stencil_display_name<'a>(stencil_id: &str, fallback_name: &'a str, lang: UiLang) -> &'a str {
    match lang {
        UiLang::Fi => fallback_name,
        UiLang::En => stencil_name_en(stencil_id).unwrap_or(fallback_name),
    }
}

pub fn layer_display_name(layer_name: &str, lang: UiLang) -> &str {
    match lang {
        UiLang::Fi => layer_name,
        UiLang::En => match layer_name {
            "Mitat & Tekstit" => "Dimensions & Text",
            "Sähkö & Kytkennät" => "Electrical & Wiring",
            "Kalusteet & LVI" => "Furniture & HVAC",
            "Rakenteet & Seinät" => "Structures & Walls",
            "Oletus" => "Default",
            other => other,
        },
    }
}

pub fn text_pos_label(pos: TextPosition, lang: UiLang) -> &'static str {
    match (pos, lang) {
        (TextPosition::Auto, UiLang::Fi) => "Automaattinen",
        (TextPosition::Auto, UiLang::En) => "Automatic",
        (TextPosition::Center, UiLang::Fi) => "Keskellä sisällä",
        (TextPosition::Center, UiLang::En) => "Center Inside",
        (TextPosition::Top, UiLang::Fi) => "Yläpuolella",
        (TextPosition::Top, UiLang::En) => "Above (Top)",
        (TextPosition::Bottom, UiLang::Fi) => "Alapuolella",
        (TextPosition::Bottom, UiLang::En) => "Below (Bottom)",
        (TextPosition::Hidden, UiLang::Fi) => "Piilotettu",
        (TextPosition::Hidden, UiLang::En) => "Hidden",
    }
}

pub fn text_align_label(align: TextAlign, lang: UiLang) -> &'static str {
    match (align, lang) {
        (TextAlign::Auto, UiLang::Fi) => "Automaattinen",
        (TextAlign::Auto, UiLang::En) => "Automatic",
        (TextAlign::Left, UiLang::Fi) => "Vasen",
        (TextAlign::Left, UiLang::En) => "Left",
        (TextAlign::Center, UiLang::Fi) => "Keskitetty",
        (TextAlign::Center, UiLang::En) => "Center",
        (TextAlign::Right, UiLang::Fi) => "Oikea",
        (TextAlign::Right, UiLang::En) => "Right",
    }
}

pub fn conn_style_label(style: ConnectorStyle, lang: UiLang) -> &'static str {
    match (style, lang) {
        (ConnectorStyle::Orthogonal, UiLang::Fi) => "Suorakulmainen 90°",
        (ConnectorStyle::Orthogonal, UiLang::En) => "Orthogonal 90°",
        (ConnectorStyle::Straight, UiLang::Fi) => "Suora viiva",
        (ConnectorStyle::Straight, UiLang::En) => "Straight Line",
        (ConnectorStyle::Curved, UiLang::Fi) => "Kaareva (SFS)",
        (ConnectorStyle::Curved, UiLang::En) => "Curved (Bezier)",
    }
}

pub fn arrow_type_label(arrow: ArrowType, lang: UiLang) -> &'static str {
    match (arrow, lang) {
        (ArrowType::None, UiLang::Fi) => "Ei merkkiä",
        (ArrowType::None, UiLang::En) => "None",
        (ArrowType::JunctionDot, UiLang::Fi) => "Liitospiste (●)",
        (ArrowType::JunctionDot, UiLang::En) => "Junction Dot (●)",
        (ArrowType::Arrow, UiLang::Fi) => "Nuoli (▶)",
        (ArrowType::Arrow, UiLang::En) => "Arrow (▶)",
        (ArrowType::OpenArrow, UiLang::Fi) => "Avoin nuoli",
        (ArrowType::OpenArrow, UiLang::En) => "Open Arrow",
        (ArrowType::DimensionTick, UiLang::Fi) => "Mittaviiva",
        (ArrowType::DimensionTick, UiLang::En) => "Dimension Tick",
    }
}

/// Translates built-in template page names and labels in-place when toggling between FI and EN.
pub fn translate_document_template_strings(doc: &mut DiagramDocument, lang: UiLang) {
    let title_pairs = [
        (
            "Moottorin & Releen Ohjauskytkentä (IEC)",
            "Motor & Relay Control Schematic (IEC)",
        ),
        (
            "Pohjapiirustus & SFS-Sähkösuunnitelma (1:50)",
            "Floor Plan & Electrical Layout (1:50)",
        ),
        ("Uusi kaavio", "New Diagram"),
    ];
    for (fi, en) in title_pairs {
        if lang == UiLang::En && doc.title == fi {
            doc.title = en.to_string();
        } else if lang == UiLang::Fi && doc.title == en {
            doc.title = fi.to_string();
        }
    }

    let page_pairs = [
        ("Kytkentäkaavio (IEC)", "Schematic (IEC)"),
        ("Pohjapiirustus 1:50", "Floor Plan 1:50"),
        ("Sivu 1", "Page 1"),
        ("Sivu 2", "Page 2"),
        ("Sivu 3", "Page 3"),
        ("Sivu 4", "Page 4"),
    ];

    let label_pairs = [
        ("S1 (Käynnistys)", "S1 (Start)"),
        ("PLC1 (Ohjauslogiikka)", "PLC1 (Control Logic)"),
        ("H1 (Tila-LED)", "H1 (Status LED)"),
        ("Olohuone & Keittiö", "Living Room & Kitchen"),
        ("Olohuone", "Living Room"),
        ("Makuuhuone", "Bedroom"),
        ("Kylpyhuone / KHH", "Bathroom / Utility"),
        ("Kokonaisleveys", "Total Width"),
        ("OH+KT", "Living+Kitchen"),
        ("Portaat", "Stairs"),
        ("Liesi", "Stove"),
        ("Allas", "Sink"),
        ("Kiuas", "Sauna Heater"),
        ("Sohva", "Sofa"),
        ("Parisänky", "Double Bed"),
        ("Ruokapöytä", "Dining Table"),
        ("Aloitus", "Start"),
        ("Prosessi", "Process"),
        ("Ehto", "Condition"),
        ("Raportti", "Report"),
        ("Kanta", "Database"),
        ("Kirjoita teksti...", "Enter text..."),
        ("Huomautus!", "Note!"),
        ("Huomautus", "Note"),
    ];

    let value_pairs = [
        ("Vihreä", "Green"),
        ("Punainen", "Red"),
        ("Keltainen", "Yellow"),
    ];

    let wire_pairs = [
        ("MMJ 5x2.5S (Kiuas)", "5G2.5mm² (Sauna Heater)"),
    ];

    for page in &mut doc.pages {
        for (fi, en) in page_pairs {
            if lang == UiLang::En && page.name == fi {
                page.name = en.to_string();
            } else if lang == UiLang::Fi && page.name == en {
                page.name = fi.to_string();
            }
        }
        for shape in &mut page.shapes {
            if let Some(en_name) = stencil_name_en(&shape.stencil_id) {
                if lang == UiLang::En {
                    shape.name = en_name.to_string();
                } else if let Some(st) = crate::stencils::built_in_stencils()
                    .into_iter()
                    .find(|s| s.id == shape.stencil_id)
                {
                    shape.name = st.name;
                }
            }
            for (fi, en) in label_pairs {
                if lang == UiLang::En && shape.label == fi {
                    shape.label = en.to_string();
                } else if lang == UiLang::Fi && shape.label == en {
                    shape.label = fi.to_string();
                }
            }
            for (fi, en) in value_pairs {
                if lang == UiLang::En && shape.value == fi {
                    shape.value = en.to_string();
                } else if lang == UiLang::Fi && shape.value == en {
                    shape.value = fi.to_string();
                }
            }
        }
        for conn in &mut page.connectors {
            for (fi, en) in wire_pairs {
                if lang == UiLang::En && conn.label == fi {
                    conn.label = en.to_string();
                } else if lang == UiLang::Fi && conn.label == en {
                    conn.label = fi.to_string();
                }
            }
        }
    }
}
