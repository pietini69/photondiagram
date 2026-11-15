use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn distance_to(&self, other: Self) -> f64 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }

    pub fn midpoint(&self, other: Self) -> Self {
        Self {
            x: (self.x + other.x) * 0.5,
            y: (self.y + other.y) * 0.5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoundingBox {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl BoundingBox {
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self {
            min_x: min_x.min(max_x),
            min_y: min_y.min(max_y),
            max_x: min_x.max(max_x),
            max_y: min_y.max(max_y),
        }
    }

    pub fn width(&self) -> f64 {
        self.max_x - self.min_x
    }

    pub fn height(&self) -> f64 {
        self.max_y - self.min_y
    }

    pub fn center(&self) -> Point {
        Point::new((self.min_x + self.max_x) * 0.5, (self.min_y + self.max_y) * 0.5)
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.min_x && p.x <= self.max_x && p.y >= self.min_y && p.y <= self.max_y
    }

    pub fn intersects(&self, other: &Self) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
    }

    pub fn expand(&self, margin: f64) -> Self {
        Self {
            min_x: self.min_x - margin,
            min_y: self.min_y - margin,
            max_x: self.max_x + margin,
            max_y: self.max_y + margin,
        }
    }
}

/// Architectural / technical drawing scale
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DrawingScale {
    Scale1To1,   // 1:1 (Sähkökaaviot, vuokaaviot, kytkennät)
    Scale1To10,  // 1:10 (Detaljipiirustus)
    Scale1To20,  // 1:20 (Huone-/kalustekaavio)
    Scale1To50,  // 1:50 (Rakennuksen pohjapiirustus, standardi)
    Scale1To100, // 1:100 (Koko talon / tontin pohjapiirustus)
}

impl DrawingScale {
    pub fn factor(&self) -> f64 {
        match self {
            Self::Scale1To1 => 1.0,
            Self::Scale1To10 => 10.0,
            Self::Scale1To20 => 20.0,
            Self::Scale1To50 => 50.0,
            Self::Scale1To100 => 100.0,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Scale1To1 => "1:1 (Kaavio / Todellinen)",
            Self::Scale1To10 => "1:10 (Detalji)",
            Self::Scale1To20 => "1:20 (Tilasuunnitelma)",
            Self::Scale1To50 => "1:50 (Pohjapiirustus)",
            Self::Scale1To100 => "1:100 (Pääpiirustus)",
        }
    }

    /// Converts paper millimeters into real-world millimeters
    pub fn paper_mm_to_real_mm(&self, paper_mm: f64) -> f64 {
        paper_mm * self.factor()
    }

    /// Formats a paper length (mm) into a human-readable measurement string
    pub fn format_length(&self, paper_mm: f64) -> String {
        let real_mm = self.paper_mm_to_real_mm(paper_mm);
        if *self == Self::Scale1To1 {
            format!("{:.1} mm", real_mm)
        } else if real_mm >= 1000.0 {
            format!("{:.2} m ({:.0} mm)", real_mm / 1000.0, real_mm)
        } else {
            format!("{:.0} mm", real_mm)
        }
    }

    /// Formats a paper area (mm²) into square meters (m²)
    pub fn format_area(&self, paper_w_mm: f64, paper_h_mm: f64) -> String {
        let real_w_m = self.paper_mm_to_real_mm(paper_w_mm) / 1000.0;
        let real_h_m = self.paper_mm_to_real_mm(paper_h_mm) / 1000.0;
        let area_m2 = real_w_m * real_h_m;
        format!("{:.1} m²", area_m2)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PortDir {
    Left,
    Right,
    Up,
    Down,
    Any,
}

impl PortDir {
    pub fn unit_vector(&self) -> (f64, f64) {
        match self {
            Self::Left => (-1.0, 0.0),
            Self::Right => (1.0, 0.0),
            Self::Up => (0.0, -1.0),
            Self::Down => (0.0, 1.0),
            Self::Any => (0.0, 0.0),
        }
    }
}

/// A glue/connection point on a shape (normalized 0.0..=1.0 within shape bounds)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectionPort {
    pub name: String,
    pub rel_x: f64,
    pub rel_y: f64,
    pub dir: PortDir,
}

impl ConnectionPort {
    pub fn new(name: &str, rel_x: f64, rel_y: f64, dir: PortDir) -> Self {
        Self {
            name: name.to_string(),
            rel_x,
            rel_y,
            dir,
        }
    }
}

/// Normalized vector primitive (coordinates in 0.0..=1.0 relative to shape top-left..bottom-right)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PrimOp {
    Line {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
    },
    Rect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        filled: bool,
    },
    Circle {
        cx: f64,
        cy: f64,
        r: f64,
        filled: bool,
    },
    Arc {
        cx: f64,
        cy: f64,
        rx: f64,
        ry: f64,
        start_deg: f64,
        sweep_deg: f64,
    },
    Polyline {
        points: Vec<(f64, f64)>,
        closed: bool,
        filled: bool,
    },
    Text {
        x: f64,
        y: f64,
        text: String,
        size_rel: f64,
    },
    Path {
        points: Vec<(f64, f64)>,
        closed: bool,
        filled: bool,
        fill_color: Option<[u8; 4]>,
        stroke_color: Option<[u8; 4]>,
        stroke_width: Option<f32>,
    },
    Ellipse {
        cx: f64,
        cy: f64,
        rx: f64,
        ry: f64,
        filled: bool,
        fill_color: Option<[u8; 4]>,
        stroke_color: Option<[u8; 4]>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TextPosition {
    #[default]
    Auto,   // Automaattinen: vuokaavio/tilat -> Keskellä; sähkömerkit -> Yläpuolella; ikonit -> Alapuolella
    Center, // Keskellä sisällä (Center)
    Top,    // Yläpuolella (Top)
    Bottom, // Alapuolella (Bottom)
    Hidden, // Piilotettu (Hidden)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TextAlign {
    #[default]
    Auto,   // Automaattinen (Tekstilaatikot: Vasen; Vuokaaviot: Keskitetty)
    Left,   // Vasen
    Center, // Keskitetty
    Right,  // Oikea
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagramShape {
    pub id: u64,
    pub stencil_id: String,
    pub name: String,
    pub label: String,
    pub value: String,
    pub x: f64,          // Center X in page mm (Visio PinX)
    pub y: f64,          // Center Y in page mm (Visio PinY)
    pub width: f64,      // Width in mm
    pub height: f64,     // Height in mm
    pub rotation_deg: f64,
    pub flip_x: bool,
    pub flip_y: bool,
    pub stroke_color: [u8; 4],
    pub fill_color: [u8; 4],
    pub stroke_width: f32,
    pub dashed: bool,
    pub layer: String,
    pub ports: Vec<ConnectionPort>,
    pub geometry: Vec<PrimOp>,
    pub properties: Vec<(String, String)>,
    pub is_dimension: bool,
    pub is_room_area: bool,
    pub locked: bool,
    #[serde(default)]
    pub text_position: TextPosition,
    #[serde(default)]
    pub font_size: f32, // Font size in pt (0.0 = Auto)
    #[serde(default)]
    pub text_align: TextAlign,
}

impl DiagramShape {
    /// Transforms a normalized (0.0..=1.0) local point into page coordinates (mm),
    /// applying flip, scale, and rotation around the shape center `(self.x, self.y)`.
    pub fn local_to_page(&self, mut rx: f64, mut ry: f64) -> Point {
        if self.flip_x {
            rx = 1.0 - rx;
        }
        if self.flip_y {
            ry = 1.0 - ry;
        }
        let lx = (rx - 0.5) * self.width;
        let ly = (ry - 0.5) * self.height;
        let rad = self.rotation_deg * PI / 180.0;
        let (sin_a, cos_a) = rad.sin_cos();
        let wx = self.x + lx * cos_a - ly * sin_a;
        let wy = self.y + lx * sin_a + ly * cos_a;
        Point::new(wx, wy)
    }

    /// Returns the world position and outward direction of a connection port
    pub fn port_world_pos_and_dir(&self, port_idx: usize) -> (Point, (f64, f64)) {
        if let Some(port) = self.ports.get(port_idx) {
            let pos = self.local_to_page(port.rel_x, port.rel_y);
            let (mut dx, mut dy) = port.dir.unit_vector();
            if self.flip_x {
                dx = -dx;
            }
            if self.flip_y {
                dy = -dy;
            }
            let rad = self.rotation_deg * PI / 180.0;
            let (sin_a, cos_a) = rad.sin_cos();
            let rdx = dx * cos_a - dy * sin_a;
            let rdy = dx * sin_a + dy * cos_a;
            (pos, (rdx, rdy))
        } else {
            (Point::new(self.x, self.y), (0.0, 0.0))
        }
    }

    /// Computes axis-aligned bounding box in page mm
    pub fn bounding_box(&self) -> BoundingBox {
        let corners = [
            self.local_to_page(0.0, 0.0),
            self.local_to_page(1.0, 0.0),
            self.local_to_page(1.0, 1.0),
            self.local_to_page(0.0, 1.0),
        ];
        let min_x = corners.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
        let max_x = corners.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
        let min_y = corners.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
        let max_y = corners.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max);
        BoundingBox::new(min_x, min_y, max_x, max_y)
    }

    pub fn contains_point(&self, p: Point, tolerance_mm: f64) -> bool {
        self.bounding_box().expand(tolerance_mm).contains(p)
    }

    /// Determines the resolved text position (Center, Top, Bottom, or Hidden).
    pub fn effective_text_position(&self) -> TextPosition {
        match self.text_position {
            TextPosition::Auto => {
                if self.is_room_area || self.is_dimension {
                    TextPosition::Center
                } else if self.stencil_id.starts_with("flow_")
                    || self.stencil_id.starts_with("note_")
                    || self.stencil_id == "flow_process"
                    || self.stencil_id == "flow_decision"
                    || self.stencil_id == "note_textbox"
                    || self.stencil_id == "note_callout"
                    || self.name.to_lowercase().contains("prosessi")
                    || self.name.to_lowercase().contains("pääto")
                    || self.name.to_lowercase().contains("ehto")
                    || self.name.to_lowercase().contains("terminaattori")
                    || self.name.to_lowercase().contains("flowchart")
                    || self.name.to_lowercase().contains("process")
                    || self.name.to_lowercase().contains("decision")
                    || self.name.to_lowercase().contains("suorakaide")
                    || self.name.to_lowercase().contains("salmiakki")
                    || self.name.to_lowercase().contains("teksti")
                    || self.name.to_lowercase().contains("huomautus")
                    || self.name.to_lowercase().contains("callout")
                    || self.name.to_lowercase().contains("note")
                {
                    TextPosition::Center
                } else if self.stencil_id == "elec_gnd"
                    || self.stencil_id == "iec_gnd"
                    || self.stencil_id == "elec_pe"
                    || self.stencil_id == "iec_pe"
                {
                    TextPosition::Bottom
                } else if self.stencil_id.starts_with("elec_")
                    || self.stencil_id.starts_with("inst_")
                    || self.stencil_id.starts_with("iec_")
                    || self.stencil_id.starts_with("sfs_")
                {
                    TextPosition::Top
                } else {
                    TextPosition::Bottom
                }
            }
            pos => pos,
        }
    }

    /// Determines the resolved text alignment (Left, Center, or Right).
    pub fn effective_text_align(&self) -> TextAlign {
        match self.text_align {
            TextAlign::Auto => {
                if self.stencil_id.starts_with("note_")
                    || self.stencil_id == "note_textbox"
                    || self.name.to_lowercase().contains("tekstilaatikko")
                {
                    TextAlign::Left
                } else {
                    TextAlign::Center
                }
            }
            align => align,
        }
    }

    /// Calculates world coordinates for displaying the shape label / text.
    pub fn label_position(&self) -> Point {
        let bb = self.bounding_box();
        match self.effective_text_position() {
            TextPosition::Center => Point::new(self.x, self.y),
            TextPosition::Top => Point::new(self.x, bb.min_y - 2.6),
            TextPosition::Bottom => Point::new(self.x, bb.max_y + 2.6),
            TextPosition::Hidden => Point::new(self.x, self.y),
            TextPosition::Auto => Point::new(self.x, self.y),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectorStyle {
    Orthogonal, // 90° suorakulmainen reititys (Sähkökaaviot & Vuokaaviot)
    Straight,   // Suora viiva
    Curved,     // Kaareva Bezier-yhdysviiva
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArrowType {
    None,          // Pelkkä johdin
    Arrow,         // Täytetty nuoli
    OpenArrow,     // Avoin nuoli
    JunctionDot,   // Sähkökaavion liitospiste (pallo)
    DimensionTick, // Pohjapiirustuksen mittaviivan vinoviiva
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagramConnector {
    pub id: u64,
    pub from_shape: Option<u64>,
    pub from_port: usize,
    pub from_pos: Point,
    pub to_shape: Option<u64>,
    pub to_port: usize,
    pub to_pos: Point,
    pub style: ConnectorStyle,
    pub start_arrow: ArrowType,
    pub end_arrow: ArrowType,
    pub label: String,
    pub stroke_color: [u8; 4],
    pub stroke_width: f32,
    pub dashed: bool,
    pub layer: String,
    pub waypoints: Vec<Point>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerInfo {
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub color: [u8; 4],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagramPage {
    pub name: String,
    pub width_mm: f64,
    pub height_mm: f64,
    pub grid_spacing_mm: f64,
    pub scale: DrawingScale,
    pub shapes: Vec<DiagramShape>,
    pub connectors: Vec<DiagramConnector>,
    pub layers: Vec<LayerInfo>,
}

impl Default for DiagramPage {
    fn default() -> Self {
        Self {
            name: "Sivu 1".to_string(),
            width_mm: 297.0, // A4 Landscape
            height_mm: 210.0,
            grid_spacing_mm: 5.0,
            scale: DrawingScale::Scale1To1,
            shapes: Vec::new(),
            connectors: Vec::new(),
            layers: vec![
                LayerInfo {
                    name: "Mitat & Tekstit".to_string(),
                    visible: true,
                    locked: false,
                    color: [22, 163, 74, 255],
                },
                LayerInfo {
                    name: "Sähkö & Kytkennät".to_string(),
                    visible: true,
                    locked: false,
                    color: [220, 38, 38, 255],
                },
                LayerInfo {
                    name: "Kalusteet & LVI".to_string(),
                    visible: true,
                    locked: false,
                    color: [2, 132, 199, 255],
                },
                LayerInfo {
                    name: "Rakenteet & Seinät".to_string(),
                    visible: true,
                    locked: false,
                    color: [51, 65, 85, 255],
                },
                LayerInfo {
                    name: "Oletus".to_string(),
                    visible: true,
                    locked: false,
                    color: [30, 41, 59, 255],
                },
            ],
        }
    }
}

impl DiagramPage {
    /// Moves the specified shape to the very top (rendered last, in front of all shapes on its layer)
    pub fn bring_to_front(&mut self, shape_id: u64) {
        if let Some(idx) = self.shapes.iter().position(|s| s.id == shape_id) {
            let shape = self.shapes.remove(idx);
            self.shapes.push(shape);
        }
    }

    /// Moves the specified shape to the very bottom (rendered first, in the background)
    pub fn send_to_back(&mut self, shape_id: u64) {
        if let Some(idx) = self.shapes.iter().position(|s| s.id == shape_id) {
            let shape = self.shapes.remove(idx);
            self.shapes.insert(0, shape);
        }
    }

    /// Moves the specified shape one step forward in z-order
    pub fn bring_forward(&mut self, shape_id: u64) {
        if let Some(idx) = self.shapes.iter().position(|s| s.id == shape_id) {
            if idx + 1 < self.shapes.len() {
                self.shapes.swap(idx, idx + 1);
            }
        }
    }

    /// Moves the specified shape one step backward in z-order
    pub fn send_backward(&mut self, shape_id: u64) {
        if let Some(idx) = self.shapes.iter().position(|s| s.id == shape_id) {
            if idx > 0 {
                self.shapes.swap(idx, idx - 1);
            }
        }
    }

    /// Moves a layer up in the stack (closer to foreground / index 0)
    pub fn move_layer_up(&mut self, idx: usize) {
        if idx > 0 && idx < self.layers.len() {
            self.layers.swap(idx, idx - 1);
        }
    }

    /// Moves a layer down in the stack (closer to background / bottom)
    pub fn move_layer_down(&mut self, idx: usize) {
        if idx + 1 < self.layers.len() {
            self.layers.swap(idx, idx + 1);
        }
    }

    /// Brings the named layer to the very top (index 0 / foremost layer)
    pub fn bring_layer_to_top(&mut self, name: &str) {
        if let Some(idx) = self.layers.iter().position(|l| l.name == name) {
            if idx > 0 {
                let layer = self.layers.remove(idx);
                self.layers.insert(0, layer);
            }
        }
    }

    /// Finds the topmost shape at the given point, respecting layer visibility, locks, and layer z-order.
    pub fn find_shape_at(&self, p: Point, tol_mm: f64) -> Option<&DiagramShape> {
        // 1. Check layers from topmost (index 0) down to bottommost (last index)
        for layer in &self.layers {
            if !layer.visible || layer.locked {
                continue;
            }
            // Check non-room shapes on this layer in reverse creation order (newest first)
            if let Some(s) = self
                .shapes
                .iter()
                .rev()
                .find(|s| !s.is_room_area && s.layer == layer.name && s.contains_point(p, tol_mm))
            {
                return Some(s);
            }
        }

        // 2. Check room areas on visible layers
        for layer in &self.layers {
            if !layer.visible || layer.locked {
                continue;
            }
            if let Some(s) = self
                .shapes
                .iter()
                .rev()
                .find(|s| s.is_room_area && s.layer == layer.name && s.contains_point(p, tol_mm))
            {
                return Some(s);
            }
        }

        // 3. Fallback: check any shapes whose layer is unlisted
        let known_layers: Vec<&str> = self.layers.iter().map(|l| l.name.as_str()).collect();
        self.shapes
            .iter()
            .rev()
            .find(|s| !known_layers.contains(&s.layer.as_str()) && s.contains_point(p, tol_mm))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagramDocument {
    pub title: String,
    pub author: String,
    pub pages: Vec<DiagramPage>,
    pub active_page: usize,
    pub next_id: u64,
    pub show_line_jumps: bool,
    pub show_connection_points: bool,
    pub show_grid: bool,
    pub snap_to_grid: bool,
    pub snap_to_shapes: bool,
}

impl Default for DiagramDocument {
    fn default() -> Self {
        Self {
            title: "Uusi kaavio".to_string(),
            author: "Niko Pietinen".to_string(),
            pages: vec![DiagramPage::default()],
            active_page: 0,
            next_id: 1,
            show_line_jumps: true,
            show_connection_points: true,
            show_grid: true,
            snap_to_grid: true,
            snap_to_shapes: true,
        }
    }
}

impl DiagramDocument {
    pub fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn current_page(&self) -> &DiagramPage {
        let idx = self.active_page.min(self.pages.len().saturating_sub(1));
        &self.pages[idx]
    }

    pub fn current_page_mut(&mut self) -> &mut DiagramPage {
        let idx = self.active_page.min(self.pages.len().saturating_sub(1));
        &mut self.pages[idx]
    }
}
