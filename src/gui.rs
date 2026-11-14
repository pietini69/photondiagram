use crate::formats::export::shape_display_caption;
use crate::formats::{load_document_from_path, load_stencil_library, save_document_to_path};
use crate::model::{
    ArrowType, ConnectorStyle, DiagramConnector, DiagramDocument, DiagramPage, DiagramShape,
    DrawingScale, Point, PrimOp,
};
use crate::routing::{connector_hit_test, find_wire_jumps, update_all_connectors};
use crate::stencils::{built_in_stencils, instantiate_shape, StencilCategory, StencilItem};
use crate::templates::{template_electrical_schematic, template_floorplan_and_sfs_elec};
use crate::triangulate::draw_robust_path;
use eframe::egui::{
    self, Color32, CornerRadius, FontId, Key, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2,
};
use std::f64::consts::PI;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveTool {
    Select,
    Wire,
    DrawWall,
    DrawRoom,
    DrawDimension,
    Pan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResizeHandle {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Left,
    Right,
    Top,
    Bottom,
}

pub struct PhotonVisionApp {
    pub doc: DiagramDocument,
    pub stencils: Vec<StencilItem>,
    pub stencil_search: String,
    pub active_tool: ActiveTool,
    pub pending_stencil_id: Option<String>,
    pub selected_shape_id: Option<u64>,
    pub selected_connector_id: Option<u64>,
    pub current_file: Option<PathBuf>,
    pub status_msg: String,

    // Canvas viewport state (zoom in pixels per paper mm, pan offset in screen pixels)
    pub zoom_px_per_mm: f32,
    pub pan_offset: Vec2,

    // Interactive drag states
    dragging_shape_id: Option<u64>,
    drag_start_mouse_mm: Point,
    drag_start_shape_pos: Point,
    drag_start_shape_size: (f64, f64),
    active_resize_handle: Option<ResizeHandle>,

    // Interactive wire creation state: (from_shape_id, from_port_idx, start_pos_mm)
    wiring_from: Option<(Option<u64>, usize, Point)>,

    // Interactive creation drag (Wall / Room / Dimension)
    creation_drag_start_mm: Option<Point>,

    // Smart alignment guide lines in page mm: (vertical_x_mm, horizontal_y_mm)
    smart_guides: (Option<f64>, Option<f64>),

    // Undo / Redo history stacks
    undo_stack: Vec<DiagramDocument>,
    redo_stack: Vec<DiagramDocument>,
}

impl Default for PhotonVisionApp {
    fn default() -> Self {
        let mut doc = template_electrical_schematic();
        // Add the floor plan as Page 2 right out of the box so the user can explore both immediately!
        let floor_doc = template_floorplan_and_sfs_elec();
        if let Some(mut fp_page) = floor_doc.pages.into_iter().next() {
            // Re-index IDs on page 2 so they don't collide with page 1
            let offset = 1000;
            for s in &mut fp_page.shapes {
                s.id += offset;
            }
            for c in &mut fp_page.connectors {
                c.id += offset;
                if let Some(ref mut fs) = c.from_shape {
                    *fs += offset;
                }
                if let Some(ref mut ts) = c.to_shape {
                    *ts += offset;
                }
            }
            doc.next_id = 2500;
            doc.pages.push(fp_page);
        }

        Self {
            doc,
            stencils: built_in_stencils(),
            stencil_search: String::new(),
            active_tool: ActiveTool::Select,
            pending_stencil_id: None,
            selected_shape_id: None,
            selected_connector_id: None,
            current_file: None,
            status_msg: "Valmis — Valitse vasemmalta symboli tai vedä vihreästä liitospisteestä johdin. Sivu 1: Sähkökaavio, Sivu 2: Pohjapiirustus 1:50.".to_string(),
            zoom_px_per_mm: 3.4,
            pan_offset: Vec2::new(42.0, 36.0),
            dragging_shape_id: None,
            drag_start_mouse_mm: Point::ZERO,
            drag_start_shape_pos: Point::ZERO,
            drag_start_shape_size: (0.0, 0.0),
            active_resize_handle: None,
            wiring_from: None,
            creation_drag_start_mm: None,
            smart_guides: (None, None),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }
}

impl PhotonVisionApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self::default()
    }

    fn push_undo(&mut self) {
        if self.undo_stack.len() >= 40 {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(self.doc.clone());
        self.redo_stack.clear();
    }

    fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.redo_stack.push(self.doc.clone());
            self.doc = prev;
            self.status_msg = "Kumottu edellinen toiminto.".to_string();
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack.push(self.doc.clone());
            self.doc = next;
            self.status_msg = "Tehty uudelleen.".to_string();
        }
    }

    fn snap_val(&self, v: f64) -> f64 {
        if self.doc.snap_to_grid {
            let step = (self.doc.current_page().grid_spacing_mm * 0.5).max(1.0);
            (v / step).round() * step
        } else {
            v
        }
    }

    fn snap_point(&self, p: Point) -> Point {
        Point::new(self.snap_val(p.x), self.snap_val(p.y))
    }

    fn page_to_screen(&self, canvas_origin: Pos2, p: Point) -> Pos2 {
        Pos2::new(
            canvas_origin.x + self.pan_offset.x + (p.x as f32) * self.zoom_px_per_mm,
            canvas_origin.y + self.pan_offset.y + (p.y as f32) * self.zoom_px_per_mm,
        )
    }

    fn screen_to_page(&self, canvas_origin: Pos2, pos: Pos2) -> Point {
        let x = (pos.x - canvas_origin.x - self.pan_offset.x) / self.zoom_px_per_mm;
        let y = (pos.y - canvas_origin.y - self.pan_offset.y) / self.zoom_px_per_mm;
        Point::new(x as f64, y as f64)
    }

    fn rotate_selected(&mut self, delta_deg: f64) {
        if let Some(sid) = self.selected_shape_id {
            self.push_undo();
            let page = self.doc.current_page_mut();
            if let Some(s) = page.shapes.iter_mut().find(|s| s.id == sid) {
                s.rotation_deg = (s.rotation_deg + delta_deg).rem_euclid(360.0);
            }
            update_all_connectors(&mut page.connectors, &page.shapes);
        }
    }

    fn flip_selected(&mut self, horizontal: bool) {
        if let Some(sid) = self.selected_shape_id {
            self.push_undo();
            let page = self.doc.current_page_mut();
            if let Some(s) = page.shapes.iter_mut().find(|s| s.id == sid) {
                if horizontal {
                    s.flip_x = !s.flip_x;
                } else {
                    s.flip_y = !s.flip_y;
                }
            }
            update_all_connectors(&mut page.connectors, &page.shapes);
        }
    }

    fn delete_selected(&mut self) {
        if let Some(sid) = self.selected_shape_id.take() {
            self.push_undo();
            let page = self.doc.current_page_mut();
            page.shapes.retain(|s| s.id != sid);
            page.connectors
                .retain(|c| c.from_shape != Some(sid) && c.to_shape != Some(sid));
            self.status_msg = "Komponentti poistettu.".to_string();
        } else if let Some(cid) = self.selected_connector_id.take() {
            self.push_undo();
            let page = self.doc.current_page_mut();
            page.connectors.retain(|c| c.id != cid);
            self.status_msg = "Johdin poistettu.".to_string();
        }
    }

    fn duplicate_selected(&mut self) {
        if let Some(sid) = self.selected_shape_id {
            let cloned = self
                .doc
                .current_page()
                .shapes
                .iter()
                .find(|s| s.id == sid)
                .cloned();
            if let Some(mut s) = cloned {
                self.push_undo();
                let new_id = self.doc.alloc_id();
                s.id = new_id;
                s.x += 10.0;
                s.y += 10.0;
                self.doc.current_page_mut().shapes.push(s);
                self.selected_shape_id = Some(new_id);
                self.status_msg = "Komponentti monistettu.".to_string();
            }
        }
    }

    fn open_file_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter(
                "Tuetut kaaviot & projektit (.vsdx, .vdx, .vsd, .lbrn2, .pvis)",
                &["vsdx", "vdx", "vsd", "lbrn2", "lbrn", "pvis", "json"],
            )
            .add_filter("PhotonCAM / LightBurn (.lbrn2)", &["lbrn2", "lbrn"])
            .add_filter("Microsoft Visio (.vsdx, .vdx, .vsd)", &["vsdx", "vdx", "vsd"])
            .add_filter("PhotonVision (.pvis)", &["pvis"])
            .pick_file()
        {
            match load_document_from_path(&path) {
                Ok(doc) => {
                    self.push_undo();
                    self.doc = doc;
                    self.current_file = Some(path.clone());
                    self.selected_shape_id = None;
                    self.selected_connector_id = None;
                    self.status_msg = format!("Avattu tiedosto: {}", path.display());
                }
                Err(e) => {
                    self.status_msg = format!("Virhe avattaessa: {}", e);
                }
            }
        }
    }

    fn import_vssx_dialog(&mut self) {
        if let Some(paths) = rfd::FileDialog::new()
            .add_filter("Visio Stencil Library (.vssx)", &["vssx", "vsdx"])
            .pick_files()
        {
            let mut total = 0;
            let mut names = Vec::new();
            for path in paths {
                match load_stencil_library(&path) {
                    Ok(items) => {
                        let count = items.len();
                        total += count;
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            names.push(stem.to_string());
                        }
                        self.stencils.extend(items);
                    }
                    Err(e) => {
                        self.status_msg = format!("VSSX-tuontivirhe ({}): {}", path.display(), e);
                    }
                }
            }
            if total > 0 {
                self.status_msg = format!(
                    "Tuotu yhteensä {} Visio-symbolia kirjastoista: {}!",
                    total,
                    names.join(", ")
                );
            }
        }
    }

    fn save_file_as_dialog(&mut self, default_ext: &str, filter_label: &str) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter(filter_label, &[default_ext])
            .set_file_name(format!("kaavio.{}", default_ext))
            .save_file()
        {
            match save_document_to_path(&self.doc, &path) {
                Ok(()) => {
                    if default_ext == "pvis" || default_ext == "vsdx" {
                        self.current_file = Some(path.clone());
                    }
                    self.status_msg = format!("Tallennettu: {}", path.display());
                }
                Err(e) => {
                    self.status_msg = format!("Tallennusvirhe: {}", e);
                }
            }
        }
    }
}

impl eframe::App for PhotonVisionApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Keyboard shortcuts
        if !ctx.wants_keyboard_input() {
            ctx.input(|i| {
                if i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace) {
                    self.delete_selected();
                }
                if i.key_pressed(Key::R) {
                    self.rotate_selected(90.0);
                }
                if i.key_pressed(Key::H) {
                    self.flip_selected(true);
                }
                if i.key_pressed(Key::V) {
                    self.flip_selected(false);
                }
                if i.key_pressed(Key::Escape) {
                    self.pending_stencil_id = None;
                    self.wiring_from = None;
                    self.creation_drag_start_mm = None;
                    self.active_tool = ActiveTool::Select;
                }
                if i.modifiers.command && i.key_pressed(Key::Z) {
                    if i.modifiers.shift {
                        self.redo();
                    } else {
                        self.undo();
                    }
                }
                if i.modifiers.command && i.key_pressed(Key::D) {
                    self.duplicate_selected();
                }
            });
        }

        self.render_top_bar(ctx);
        self.render_bottom_bar(ctx);
        self.render_left_stencil_panel(ctx);
        self.render_right_inspector_panel(ctx);
        self.render_center_canvas(ctx);
    }
}

impl PhotonVisionApp {
    fn render_top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // 1. Menu bar
            egui::menu::bar(ui, |ui| {
                ui.menu_button("📁 Tiedosto", |ui| {
                    if ui.button("📄 Uusi tyhjä kaavio").clicked() {
                        self.push_undo();
                        self.doc = DiagramDocument::default();
                        self.selected_shape_id = None;
                        self.selected_connector_id = None;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui
                        .button("📂 Avaa (.vsdx, .vdx, .vsd, .lbrn2, .pvis)...")
                        .clicked()
                    {
                        ui.close_menu();
                        self.open_file_dialog();
                    }
                    if ui
                        .button("📦 Tuo Visio-symbolikirjasto (.vssx)...")
                        .clicked()
                    {
                        ui.close_menu();
                        self.import_vssx_dialog();
                    }
                    ui.separator();
                    if ui.button("💾 Tallenna PhotonVision (.pvis)...").clicked() {
                        ui.close_menu();
                        self.save_file_as_dialog("pvis", "PhotonVision (.pvis)");
                    }
                    if ui
                        .button("📘 Tallenna Microsoft Visio (.vsdx)...")
                        .clicked()
                    {
                        ui.close_menu();
                        self.save_file_as_dialog("vsdx", "Microsoft Visio 2013+ (.vsdx)");
                    }
                    if ui.button("📗 Tallenna Visio XML (.vdx)...").clicked() {
                        ui.close_menu();
                        self.save_file_as_dialog("vdx", "Visio XML Drawing (.vdx)");
                    }
                    ui.separator();
                    if ui.button("🔥 Vie PhotonCAM / LightBurn (.lbrn2)...").clicked() {
                        ui.close_menu();
                        self.save_file_as_dialog("lbrn2", "PhotonCAM / LightBurn Laser Project (.lbrn2)");
                    }
                    if ui.button("🖼️ Vie SVG-vektorikuva (.svg)...").clicked() {
                        ui.close_menu();
                        self.save_file_as_dialog("svg", "SVG Vector Graphics (.svg)");
                    }
                    if ui.button("📐 Vie AutoCAD DXF (.dxf)...").clicked() {
                        ui.close_menu();
                        self.save_file_as_dialog("dxf", "AutoCAD DXF (.dxf)");
                    }
                    if ui
                        .button("📋 Vie Osaluettelo / Huoneet (.csv)...")
                        .clicked()
                    {
                        ui.close_menu();
                        self.save_file_as_dialog("csv", "Osaluettelo BOM (.csv)");
                    }
                });

                ui.menu_button("📐 Valmiit mallipohjat", |ui| {
                    if ui
                        .button("⚡ Sähkö- & Ohjauskytkentäkaavio (IEC / PLC)")
                        .clicked()
                    {
                        self.push_undo();
                        self.doc = template_electrical_schematic();
                        self.selected_shape_id = None;
                        self.selected_connector_id = None;
                        self.status_msg =
                            "Ladattu mallipohja: Sähkö- ja ohjauskytkentäkaavio (IEC).".to_string();
                        ui.close_menu();
                    }
                    if ui
                        .button("🏠 Omakotitalon pohjapiirustus + SFS-sähkösuunnitelma (1:50)")
                        .clicked()
                    {
                        self.push_undo();
                        self.doc = template_floorplan_and_sfs_elec();
                        self.selected_shape_id = None;
                        self.selected_connector_id = None;
                        self.status_msg =
                            "Ladattu mallipohja: Pohjapiirustus + SFS-sähkösuunnitelma (1:50)."
                                .to_string();
                        ui.close_menu();
                    }
                });

                ui.menu_button("✏️ Muokkaa", |ui| {
                    if ui.button("↩ Kumoa (Cmd+Z)").clicked() {
                        self.undo();
                        ui.close_menu();
                    }
                    if ui.button("↪ Tee uudelleen (Cmd+Shift+Z)").clicked() {
                        self.redo();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("📋 Monista valittu (Cmd+D)").clicked() {
                        self.duplicate_selected();
                        ui.close_menu();
                    }
                    if ui.button("🔄 Kierrä 90° (R)").clicked() {
                        self.rotate_selected(90.0);
                        ui.close_menu();
                    }
                    if ui.button("↔ Peilaa vaakasuunnassa (H)").clicked() {
                        self.flip_selected(true);
                        ui.close_menu();
                    }
                    if ui.button("↕ Peilaa pystysuunnassa (V)").clicked() {
                        self.flip_selected(false);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("🗑 Poista valittu (Backspace)").clicked() {
                        self.delete_selected();
                        ui.close_menu();
                    }
                });
            });

            ui.separator();

            // 2. Primary Toolbar
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("Työkalu:").strong());
                for (tool, label) in [
                    (ActiveTool::Select, "↖ Valitse"),
                    (ActiveTool::Wire, "🔌 Johdin"),
                    (ActiveTool::DrawWall, "🧱 Seinä"),
                    (ActiveTool::DrawRoom, "🏠 Huone (m²)"),
                    (ActiveTool::DrawDimension, "📏 Mittaviiva"),
                    (ActiveTool::Pan, "✋ Vieritä"),
                ] {
                    if ui
                        .selectable_label(
                            self.active_tool == tool && self.pending_stencil_id.is_none(),
                            label,
                        )
                        .clicked()
                    {
                        self.active_tool = tool;
                        self.pending_stencil_id = None;
                        self.wiring_from = None;
                    }
                }

                ui.separator();

                // Quick actions on selection
                if ui.button("🔄 90°").on_hover_text("Kierrä 90° (R)").clicked() {
                    self.rotate_selected(90.0);
                }
                if ui.button("↔ Peilaa").on_hover_text("Peilaa (H)").clicked() {
                    self.flip_selected(true);
                }
                if ui
                    .button("📋 Monista")
                    .on_hover_text("Monista valittu (Cmd+D)")
                    .clicked()
                {
                    self.duplicate_selected();
                }
                if ui
                    .button("🗑 Poista")
                    .on_hover_text("Poista valittu (Delete)")
                    .clicked()
                {
                    self.delete_selected();
                }

                ui.separator();

                // Scale selector for current page
                ui.label("Mittakaava:");
                let current_scale = self.doc.current_page().scale;
                egui::ComboBox::from_id_salt("scale_combo")
                    .selected_text(current_scale.label())
                    .show_ui(ui, |ui| {
                        for sc in [
                            DrawingScale::Scale1To1,
                            DrawingScale::Scale1To10,
                            DrawingScale::Scale1To20,
                            DrawingScale::Scale1To50,
                            DrawingScale::Scale1To100,
                        ] {
                            if ui
                                .selectable_value(
                                    &mut self.doc.current_page_mut().scale,
                                    sc,
                                    sc.label(),
                                )
                                .clicked()
                            {
                                self.status_msg =
                                    format!("Sivun mittakaavaksi asetettu {}", sc.label());
                            }
                        }
                    });

                ui.separator();
                ui.checkbox(&mut self.doc.snap_to_grid, "Tartu ruudukkoon");
                ui.checkbox(&mut self.doc.show_line_jumps, "Johdinhypyt ⌒");
            });
        });
    }

    fn render_bottom_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Page tabs
                ui.label(egui::RichText::new("Sivut:").strong());
                let mut switch_to = None;
                for (idx, p) in self.doc.pages.iter().enumerate() {
                    if ui
                        .selectable_label(self.doc.active_page == idx, &p.name)
                        .clicked()
                    {
                        switch_to = Some(idx);
                    }
                }
                if let Some(idx) = switch_to {
                    self.doc.active_page = idx;
                    self.selected_shape_id = None;
                    self.selected_connector_id = None;
                }

                if ui.small_button("➕ Uusi sivu").clicked() {
                    let n = self.doc.pages.len() + 1;
                    let mut p = DiagramPage::default();
                    p.name = format!("Sivu {}", n);
                    self.doc.pages.push(p);
                    self.doc.active_page = self.doc.pages.len() - 1;
                }

                ui.separator();
                ui.label("Zoom:");
                if ui.small_button("➖").clicked() {
                    self.zoom_px_per_mm = (self.zoom_px_per_mm * 0.85).clamp(0.8, 15.0);
                }
                ui.label(format!("{:.0}%", (self.zoom_px_per_mm / 3.4) * 100.0));
                if ui.small_button("➕").clicked() {
                    self.zoom_px_per_mm = (self.zoom_px_per_mm * 1.18).clamp(0.8, 15.0);
                }
                if ui.small_button("Sopiva").clicked() {
                    self.zoom_px_per_mm = 3.4;
                    self.pan_offset = Vec2::new(42.0, 36.0);
                }

                ui.separator();
                ui.label(&self.status_msg);
            });
        });
    }

    fn render_left_stencil_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("stencil_panel")
            .default_width(250.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("📚 Symbolikirjasto");
                ui.horizontal(|ui| {
                    ui.label("🔍");
                    ui.text_edit_singleline(&mut self.stencil_search)
                        .on_hover_text("Hae esim. vastus, rele, pistorasia, ovi, kiuas...");
                    if !self.stencil_search.is_empty() && ui.small_button("✖").clicked() {
                        self.stencil_search.clear();
                    }
                });
                ui.separator();

                let query = self.stencil_search.trim().to_lowercase();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut categories = Vec::new();
                    for b in StencilCategory::built_in_categories() {
                        categories.push(b);
                    }
                    for s in &self.stencils {
                        if !categories.contains(&s.category) {
                            categories.push(s.category.clone());
                        }
                    }

                    for cat in categories {
                        let cat_items: Vec<StencilItem> = self
                            .stencils
                            .iter()
                            .filter(|s| {
                                s.category == cat
                                    && (query.is_empty()
                                        || s.name.to_lowercase().contains(&query)
                                        || s.id.to_lowercase().contains(&query))
                            })
                            .cloned()
                            .collect();

                        if cat_items.is_empty() {
                            continue;
                        }

                        let _is_imported = cat.is_imported();
                        egui::CollapsingHeader::new(
                            egui::RichText::new(format!("{} ({})", cat.title(), cat_items.len()))
                                .strong(),
                        )
                        .default_open(true)
                        .show(ui, |ui| {
                            for item in cat_items {
                                let is_armed =
                                    self.pending_stencil_id.as_deref() == Some(item.id.as_str());
                                ui.horizontal(|ui| {
                                    // Draw live mini vector preview of stencil
                                    let (rect, _) = ui.allocate_exact_size(
                                        Vec2::new(28.0, 22.0),
                                        Sense::hover(),
                                    );
                                    draw_stencil_mini_preview(ui.painter(), rect, &item);

                                    if ui
                                        .selectable_label(is_armed, &item.name)
                                        .on_hover_text(
                                            "Klikkaa lisätäksesi kaavioon tai valitse sijoitettavaksi",
                                        )
                                        .clicked()
                                    {
                                        self.pending_stencil_id = Some(item.id.clone());
                                        self.status_msg = format!(
                                            "Valittu symboli '{}' — klikkaa kaavioaluetta sijoittaaksesi.",
                                            item.name
                                        );
                                    }
                                });
                            }
                        });
                    }
                });
            });
    }

    fn render_right_inspector_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("inspector_panel")
            .default_width(275.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("⚙️ Ominaisuudet");
                ui.separator();

                let scale = self.doc.current_page().scale;
                let layer_names: Vec<String> = self
                    .doc
                    .current_page()
                    .layers
                    .iter()
                    .map(|l| l.name.clone())
                    .collect();

                if let Some(sid) = self.selected_shape_id {
                    let page = self.doc.current_page_mut();
                    if let Some(shape) = page.shapes.iter_mut().find(|s| s.id == sid) {
                        ui.label(
                            egui::RichText::new(format!("Komponentti: {}", shape.name)).strong(),
                        );
                        ui.add_space(4.0);

                        egui::Grid::new("shape_props_grid")
                            .num_columns(2)
                            .spacing([8.0, 6.0])
                            .show(ui, |ui| {
                                ui.label("Tunnus (Label):");
                                ui.text_edit_singleline(&mut shape.label);
                                ui.end_row();

                                ui.label("Arvo / Tieto:");
                                ui.text_edit_singleline(&mut shape.value);
                                ui.end_row();

                                ui.label("Sijainti X (mm):");
                                ui.add(egui::DragValue::new(&mut shape.x).speed(0.5));
                                ui.end_row();

                                ui.label("Sijainti Y (mm):");
                                ui.add(egui::DragValue::new(&mut shape.y).speed(0.5));
                                ui.end_row();

                                ui.label("Leveys (mm):");
                                ui.add(
                                    egui::DragValue::new(&mut shape.width)
                                        .speed(0.5)
                                        .range(2.0..=2000.0),
                                );
                                ui.end_row();

                                ui.label("Korkeus (mm):");
                                ui.add(
                                    egui::DragValue::new(&mut shape.height)
                                        .speed(0.5)
                                        .range(2.0..=2000.0),
                                );
                                ui.end_row();

                                if scale != DrawingScale::Scale1To1 || shape.is_room_area {
                                    ui.label("Todellinen koko:");
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{} × {}",
                                            scale.format_length(shape.width),
                                            scale.format_length(shape.height)
                                        ))
                                        .strong(),
                                    );
                                    ui.end_row();
                                }

                                if shape.is_room_area {
                                    ui.label("Huoneen pinta-ala:");
                                    ui.label(
                                        egui::RichText::new(
                                            scale.format_area(shape.width, shape.height),
                                        )
                                        .color(Color32::from_rgb(2, 132, 199))
                                        .strong(),
                                    );
                                    ui.end_row();
                                }

                                ui.label("Kierto (°):");
                                ui.add(
                                    egui::DragValue::new(&mut shape.rotation_deg)
                                        .speed(5.0)
                                        .range(0.0..=360.0),
                                );
                                ui.end_row();

                                ui.label("Viivan paksuus:");
                                ui.add(
                                    egui::DragValue::new(&mut shape.stroke_width)
                                        .speed(0.1)
                                        .range(0.5..=8.0),
                                );
                                ui.end_row();

                                ui.label("Viivaväri:");
                                let mut sc = Color32::from_rgba_unmultiplied(
                                    shape.stroke_color[0],
                                    shape.stroke_color[1],
                                    shape.stroke_color[2],
                                    shape.stroke_color[3],
                                );
                                if ui.color_edit_button_srgba(&mut sc).changed() {
                                    shape.stroke_color = sc.to_srgba_unmultiplied();
                                }
                                ui.end_row();

                                ui.label("Täyttöväri:");
                                let mut fc = Color32::from_rgba_unmultiplied(
                                    shape.fill_color[0],
                                    shape.fill_color[1],
                                    shape.fill_color[2],
                                    shape.fill_color[3],
                                );
                                if ui.color_edit_button_srgba(&mut fc).changed() {
                                    shape.fill_color = fc.to_srgba_unmultiplied();
                                }
                                ui.end_row();

                                ui.label("Taso (Layer):");
                                egui::ComboBox::from_id_salt("shape_layer_combo")
                                    .selected_text(&shape.layer)
                                    .show_ui(ui, |ui| {
                                        for ln in &layer_names {
                                            ui.selectable_value(&mut shape.layer, ln.clone(), ln);
                                        }
                                    });
                                ui.end_row();
                            });

                        update_all_connectors(&mut page.connectors, &page.shapes);
                    }
                } else if let Some(cid) = self.selected_connector_id {
                    let page = self.doc.current_page_mut();
                    if let Some(conn) = page.connectors.iter_mut().find(|c| c.id == cid) {
                        ui.label(egui::RichText::new("🔌 Johdin / Yhdysviiva").strong());
                        ui.add_space(4.0);

                        egui::Grid::new("conn_props_grid")
                            .num_columns(2)
                            .spacing([8.0, 6.0])
                            .show(ui, |ui| {
                                ui.label("Johdinmerkintä:");
                                ui.text_edit_singleline(&mut conn.label);
                                ui.end_row();

                                ui.label("Reititystyyli:");
                                egui::ComboBox::from_id_salt("conn_style_combo")
                                    .selected_text(match conn.style {
                                        ConnectorStyle::Orthogonal => "Suorakulmainen 90°",
                                        ConnectorStyle::Straight => "Suora viiva",
                                        ConnectorStyle::Curved => "Kaareva (SFS)",
                                    })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut conn.style,
                                            ConnectorStyle::Orthogonal,
                                            "Suorakulmainen 90°",
                                        );
                                        ui.selectable_value(
                                            &mut conn.style,
                                            ConnectorStyle::Straight,
                                            "Suora viiva",
                                        );
                                        ui.selectable_value(
                                            &mut conn.style,
                                            ConnectorStyle::Curved,
                                            "Kaareva (SFS)",
                                        );
                                    });
                                ui.end_row();

                                ui.label("Loppupää:");
                                egui::ComboBox::from_id_salt("conn_end_arrow")
                                    .selected_text(match conn.end_arrow {
                                        ArrowType::None => "Ei merkkiä",
                                        ArrowType::JunctionDot => "Liitospiste (●)",
                                        ArrowType::Arrow => "Nuoli (▶)",
                                        ArrowType::OpenArrow => "Avoin nuoli",
                                        ArrowType::DimensionTick => "Mittaviiva",
                                    })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut conn.end_arrow,
                                            ArrowType::None,
                                            "Ei merkkiä",
                                        );
                                        ui.selectable_value(
                                            &mut conn.end_arrow,
                                            ArrowType::JunctionDot,
                                            "Liitospiste (●)",
                                        );
                                        ui.selectable_value(
                                            &mut conn.end_arrow,
                                            ArrowType::Arrow,
                                            "Nuoli (▶)",
                                        );
                                    });
                                ui.end_row();

                                ui.label("Katkoviiva:");
                                ui.checkbox(&mut conn.dashed, "Katkoviiva (esim. SFS/ohjaus)");
                                ui.end_row();

                                ui.label("Johtimen väri:");
                                let mut cc = Color32::from_rgba_unmultiplied(
                                    conn.stroke_color[0],
                                    conn.stroke_color[1],
                                    conn.stroke_color[2],
                                    conn.stroke_color[3],
                                );
                                if ui.color_edit_button_srgba(&mut cc).changed() {
                                    conn.stroke_color = cc.to_srgba_unmultiplied();
                                }
                                ui.end_row();
                            });

                        update_all_connectors(&mut page.connectors, &page.shapes);
                    }
                } else {
                    ui.label("Ei valittua kohdetta.");
                    ui.add_space(6.0);
                    let page = self.doc.current_page_mut();
                    ui.label(egui::RichText::new("📄 Sivun asetukset").strong());
                    egui::Grid::new("page_props_grid")
                        .num_columns(2)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("Sivun nimi:");
                            ui.text_edit_singleline(&mut page.name);
                            ui.end_row();

                            ui.label("Leveys (mm):");
                            ui.add(
                                egui::DragValue::new(&mut page.width_mm)
                                    .speed(1.0)
                                    .range(100.0..=2000.0),
                            );
                            ui.end_row();

                            ui.label("Korkeus (mm):");
                            ui.add(
                                egui::DragValue::new(&mut page.height_mm)
                                    .speed(1.0)
                                    .range(100.0..=2000.0),
                            );
                            ui.end_row();

                            ui.label("Ruudukko (mm):");
                            ui.add(
                                egui::DragValue::new(&mut page.grid_spacing_mm)
                                    .speed(0.5)
                                    .range(1.0..=50.0),
                            );
                            ui.end_row();
                        });
                }

                ui.add_space(12.0);
                ui.separator();
                ui.heading("🗂 Tasot (Layers)");
                for layer in &mut self.doc.current_page_mut().layers {
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut layer.visible, "");
                        let c = Color32::from_rgba_unmultiplied(
                            layer.color[0],
                            layer.color[1],
                            layer.color[2],
                            255,
                        );
                        ui.colored_label(c, "●");
                        ui.label(&layer.name);
                    });
                }
            });
    }

    fn render_center_canvas(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::from_rgb(203, 213, 225)))
            .show(ctx, |ui| {
                let (rect, response) =
                    ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
                let painter = ui.painter_at(rect);
                let origin = rect.min;

                // Handle scroll-wheel zoom & middle-drag / two-finger pan
                if response.hovered() {
                    ctx.input(|i| {
                        let zoom_delta = i.zoom_delta();
                        if (zoom_delta - 1.0).abs() > 0.001 {
                            self.zoom_px_per_mm =
                                (self.zoom_px_per_mm * zoom_delta).clamp(0.8, 15.0);
                        }
                        if i.raw_scroll_delta != Vec2::ZERO && !i.modifiers.command {
                            self.pan_offset += i.raw_scroll_delta;
                        }
                    });
                }

                // Draw Paper sheet shadow and white background
                let page_w = self.doc.current_page().width_mm;
                let page_h = self.doc.current_page().height_mm;
                let paper_tl = self.page_to_screen(origin, Point::ZERO);
                let paper_br = self.page_to_screen(origin, Point::new(page_w, page_h));
                let paper_rect = Rect::from_min_max(paper_tl, paper_br);

                painter.rect_filled(
                    paper_rect.translate(Vec2::new(4.0, 5.0)),
                    CornerRadius::ZERO,
                    Color32::from_black_alpha(45),
                );
                painter.rect_filled(paper_rect, CornerRadius::ZERO, Color32::WHITE);

                // Draw Grid
                if self.doc.show_grid {
                    let step = self.doc.current_page().grid_spacing_mm.max(2.0);
                    let mut gx = 0.0;
                    let mut idx = 0;
                    while gx <= page_w {
                        let sx = self.page_to_screen(origin, Point::new(gx, 0.0)).x;
                        let col = if idx % 5 == 0 {
                            Color32::from_rgb(209, 218, 230)
                        } else {
                            Color32::from_rgb(236, 240, 245)
                        };
                        painter.line_segment(
                            [Pos2::new(sx, paper_tl.y), Pos2::new(sx, paper_br.y)],
                            Stroke::new(1.0_f32, col),
                        );
                        gx += step;
                        idx += 1;
                    }
                    let mut gy = 0.0;
                    idx = 0;
                    while gy <= page_h {
                        let sy = self.page_to_screen(origin, Point::new(0.0, gy)).y;
                        let col = if idx % 5 == 0 {
                            Color32::from_rgb(209, 218, 230)
                        } else {
                            Color32::from_rgb(236, 240, 245)
                        };
                        painter.line_segment(
                            [Pos2::new(paper_tl.x, sy), Pos2::new(paper_br.x, sy)],
                            Stroke::new(1.0_f32, col),
                        );
                        gy += step;
                        idx += 1;
                    }
                }

                // Draw Rulers (Top and Left)
                draw_rulers(&painter, rect, origin, self.doc.current_page(), self.zoom_px_per_mm);

                // Render shapes (rooms first so they stay in background, then other shapes)
                let page_snapshot = self.doc.current_page().clone();
                let mut hovered_port: Option<(u64, usize, Point)> = None;
                let mouse_pos = response.hover_pos().or_else(|| ctx.pointer_interact_pos());
                let mouse_mm = mouse_pos.map(|mp| self.screen_to_page(origin, mp));

                for pass in 0..2 {
                    for shape in &page_snapshot.shapes {
                        if (pass == 0) != shape.is_room_area {
                            continue;
                        }
                        if !page_snapshot
                            .layers
                            .iter()
                            .find(|l| l.name == shape.layer)
                            .map(|l| l.visible)
                            .unwrap_or(true)
                        {
                            continue;
                        }

                        let is_selected = self.selected_shape_id == Some(shape.id);
                        self.draw_shape_on_canvas(&painter, origin, &page_snapshot, shape, is_selected);

                        // Draw connection ports dynamically:
                        // Show ports only when the shape is selected, hovered, or when drawing wires nearby.
                        let is_hovered = mouse_mm
                            .map(|m| shape.contains_point(m, 1.5))
                            .unwrap_or(false);
                        let is_near_shape = mouse_mm
                            .map(|m| shape.contains_point(m, 12.0))
                            .unwrap_or(false);

                        let show_ports = is_selected
                            || is_hovered
                            || (self.active_tool == ActiveTool::Wire && is_near_shape)
                            || (self.wiring_from.is_some() && is_near_shape);

                        if show_ports {
                            for (p_idx, _port) in shape.ports.iter().enumerate() {
                                let (p_world, _) = shape.port_world_pos_and_dir(p_idx);
                                let sp = self.page_to_screen(origin, p_world);
                                let is_near = mouse_mm
                                    .map(|m| m.distance_to(p_world) <= 3.5)
                                    .unwrap_or(false);
                                if is_near {
                                    hovered_port = Some((shape.id, p_idx, p_world));
                                    painter.circle_filled(sp, 5.5, Color32::from_rgb(34, 197, 94));
                                    painter.circle_stroke(
                                        sp,
                                        6.5,
                                        Stroke::new(1.5_f32, Color32::WHITE),
                                    );
                                } else {
                                    painter.circle_filled(
                                        sp,
                                        2.8,
                                        Color32::from_rgba_unmultiplied(16, 185, 129, 190),
                                    );
                                }
                            }
                        }
                    }
                }

                // Render connectors & wire jumps
                let jumps = if self.doc.show_line_jumps {
                    find_wire_jumps(&page_snapshot.connectors)
                } else {
                    Vec::new()
                };

                for conn in &page_snapshot.connectors {
                    if !page_snapshot
                        .layers
                        .iter()
                        .find(|l| l.name == conn.layer)
                        .map(|l| l.visible)
                        .unwrap_or(true)
                    {
                        continue;
                    }
                    let is_sel = self.selected_connector_id == Some(conn.id);
                    self.draw_connector_on_canvas(&painter, origin, conn, &jumps, is_sel);
                }

                // Draw active wire preview while dragging a new connection
                if let (Some((_, _, start_mm)), Some(cur_mm)) = (self.wiring_from, mouse_mm) {
                    let target_mm = hovered_port.map(|(_, _, p)| p).unwrap_or(cur_mm);
                    let s_pos = self.page_to_screen(origin, start_mm);
                    let mid_pos = self.page_to_screen(origin, Point::new(target_mm.x, start_mm.y));
                    let e_pos = self.page_to_screen(origin, target_mm);
                    let preview_stroke = Stroke::new(2.2_f32, Color32::from_rgb(34, 197, 94));
                    painter.line_segment([s_pos, mid_pos], preview_stroke);
                    painter.line_segment([mid_pos, e_pos], preview_stroke);
                }

                // Draw active creation box preview (Wall / Room / Dimension)
                if let (Some(start_mm), Some(cur_mm)) = (self.creation_drag_start_mm, mouse_mm) {
                    let p0 = self.page_to_screen(origin, start_mm);
                    let p1 = self.page_to_screen(origin, cur_mm);
                    let r = Rect::from_two_pos(p0, p1);
                    painter.rect_stroke(
                        r,
                        CornerRadius::ZERO,
                        Stroke::new(2.0_f32, Color32::from_rgb(37, 99, 235)),
                        StrokeKind::Middle,
                    );
                }

                // Draw Smart Alignment Guides when moving a shape
                if let Some(gx) = self.smart_guides.0 {
                    let sx = self.page_to_screen(origin, Point::new(gx, 0.0)).x;
                    painter.line_segment(
                        [Pos2::new(sx, paper_tl.y), Pos2::new(sx, paper_br.y)],
                        Stroke::new(1.2_f32, Color32::from_rgb(6, 182, 212)),
                    );
                }
                if let Some(gy) = self.smart_guides.1 {
                    let sy = self.page_to_screen(origin, Point::new(0.0, gy)).y;
                    painter.line_segment(
                        [Pos2::new(paper_tl.x, sy), Pos2::new(paper_br.x, sy)],
                        Stroke::new(1.2_f32, Color32::from_rgb(6, 182, 212)),
                    );
                }

                // Handle Canvas Mouse Interactions
                self.handle_canvas_interactions(&response, origin, hovered_port, mouse_mm);
            });
    }

    fn handle_canvas_interactions(
        &mut self,
        response: &egui::Response,
        origin: Pos2,
        hovered_port: Option<(u64, usize, Point)>,
        mouse_mm: Option<Point>,
    ) {
        let Some(cur_mm) = mouse_mm else {
            return;
        };

        // 1. Place armed stencil on click
        if response.clicked() {
            if let Some(st_id) = self.pending_stencil_id.clone() {
                self.push_undo();
                let new_id = self.doc.alloc_id();
                let count = self.doc.current_page().shapes.len() + 1;
                let snapped = self.snap_point(cur_mm);
                if let Some(st) = self.stencils.iter().find(|s| s.id == st_id) {
                    let shape = st.instantiate(new_id, snapped.x, snapped.y, count);
                    self.doc.current_page_mut().shapes.push(shape);
                    self.selected_shape_id = Some(new_id);
                    self.selected_connector_id = None;
                } else if let Some(shape) =
                    instantiate_shape(&st_id, new_id, snapped.x, snapped.y, count)
                {
                    self.doc.current_page_mut().shapes.push(shape);
                    self.selected_shape_id = Some(new_id);
                    self.selected_connector_id = None;
                }
                // Keep stencil armed only if Shift is held; otherwise return to Select tool
                if !response.ctx.input(|i| i.modifiers.shift) {
                    self.pending_stencil_id = None;
                }
                return;
            }
        }

        // 2. Pan tool
        if self.active_tool == ActiveTool::Pan && response.dragged() {
            self.pan_offset += response.drag_delta();
            return;
        }

        // 3. Start of drag
        if response.drag_started() {
            // Check if dragging from a connection port -> immediately start wiring!
            if let Some((sid, pidx, p_pos)) = hovered_port {
                self.wiring_from = Some((Some(sid), pidx, p_pos));
                return;
            }
            if self.active_tool == ActiveTool::Wire {
                self.wiring_from = Some((None, 0, self.snap_point(cur_mm)));
                return;
            }
            if matches!(
                self.active_tool,
                ActiveTool::DrawWall | ActiveTool::DrawRoom | ActiveTool::DrawDimension
            ) {
                self.creation_drag_start_mm = Some(self.snap_point(cur_mm));
                return;
            }

            // Check if dragging a resize handle of the currently selected shape
            if let Some(sel_id) = self.selected_shape_id {
                if let Some(sel_shape) = self
                    .doc
                    .current_page()
                    .shapes
                    .iter()
                    .find(|s| s.id == sel_id)
                    .cloned()
                {
                    if let Some(handle) = hit_test_resize_handle(&sel_shape, cur_mm, 3.0) {
                        self.push_undo();
                        self.active_resize_handle = Some(handle);
                        self.dragging_shape_id = Some(sel_id);
                        self.drag_start_mouse_mm = cur_mm;
                        self.drag_start_shape_pos = Point::new(sel_shape.x, sel_shape.y);
                        self.drag_start_shape_size = (sel_shape.width, sel_shape.height);
                        return;
                    }
                }
            }

            // Hit-test non-room shapes first, then room shapes
            let hit_shape = self
                .doc
                .current_page()
                .shapes
                .iter()
                .rev()
                .find(|s| !s.is_room_area && s.contains_point(cur_mm, 1.5))
                .or_else(|| {
                    self.doc
                        .current_page()
                        .shapes
                        .iter()
                        .rev()
                        .find(|s| s.is_room_area && s.contains_point(cur_mm, 1.5))
                })
                .cloned();

            if let Some(sh) = hit_shape {
                self.push_undo();
                self.selected_shape_id = Some(sh.id);
                self.selected_connector_id = None;
                self.dragging_shape_id = Some(sh.id);
                self.active_resize_handle = None;
                self.drag_start_mouse_mm = cur_mm;
                self.drag_start_shape_pos = Point::new(sh.x, sh.y);
                self.drag_start_shape_size = (sh.width, sh.height);
            }
        }

        // 4. Ongoing drag (moving or resizing shape)
        if response.dragged() {
            if let Some(sid) = self.dragging_shape_id {
                let dx = cur_mm.x - self.drag_start_mouse_mm.x;
                let dy = cur_mm.y - self.drag_start_mouse_mm.y;

                if let Some(handle) = self.active_resize_handle {
                    let (mut nw, mut nh) = self.drag_start_shape_size;
                    match handle {
                        ResizeHandle::Right | ResizeHandle::BottomRight | ResizeHandle::TopRight => {
                            nw = self.snap_val((self.drag_start_shape_size.0 + dx * 2.0).max(4.0));
                        }
                        ResizeHandle::Left | ResizeHandle::BottomLeft | ResizeHandle::TopLeft => {
                            nw = self.snap_val((self.drag_start_shape_size.0 - dx * 2.0).max(4.0));
                        }
                        _ => {}
                    }
                    match handle {
                        ResizeHandle::Bottom
                        | ResizeHandle::BottomRight
                        | ResizeHandle::BottomLeft => {
                            nh = self.snap_val((self.drag_start_shape_size.1 + dy * 2.0).max(3.0));
                        }
                        ResizeHandle::Top | ResizeHandle::TopRight | ResizeHandle::TopLeft => {
                            nh = self.snap_val((self.drag_start_shape_size.1 - dy * 2.0).max(3.0));
                        }
                        _ => {}
                    }
                    let page = self.doc.current_page_mut();
                    if let Some(s) = page.shapes.iter_mut().find(|s| s.id == sid) {
                        s.width = nw;
                        s.height = nh;
                    }
                    update_all_connectors(&mut page.connectors, &page.shapes);
                } else {
                    let mut target = self.snap_point(Point::new(
                        self.drag_start_shape_pos.x + dx,
                        self.drag_start_shape_pos.y + dy,
                    ));

                    // Smart Alignment Guides against other shapes
                    let mut guide_x = None;
                    let mut guide_y = None;
                    if self.doc.snap_to_shapes {
                        for other in &self.doc.current_page().shapes {
                            if other.id == sid {
                                continue;
                            }
                            if (other.x - target.x).abs() < 2.2 {
                                target.x = other.x;
                                guide_x = Some(other.x);
                            }
                            if (other.y - target.y).abs() < 2.2 {
                                target.y = other.y;
                                guide_y = Some(other.y);
                            }
                        }
                    }
                    self.smart_guides = (guide_x, guide_y);

                    let page = self.doc.current_page_mut();
                    if let Some(s) = page.shapes.iter_mut().find(|s| s.id == sid) {
                        s.x = target.x;
                        s.y = target.y;
                    }
                    update_all_connectors(&mut page.connectors, &page.shapes);
                }
            }
        }

        // 5. End of drag
        if response.drag_stopped() {
            self.dragging_shape_id = None;
            self.active_resize_handle = None;
            self.smart_guides = (None, None);

            // Finish interactive wiring
            if let Some((from_s, from_p, start_pos)) = self.wiring_from.take() {
                let (to_s, to_p, end_pos) = if let Some((ts, tp, tpos)) = hovered_port {
                    (Some(ts), tp, tpos)
                } else {
                    (None, 0, self.snap_point(cur_mm))
                };

                if start_pos.distance_to(end_pos) > 2.0 {
                    self.push_undo();
                    let cid = self.doc.alloc_id();
                    let page = self.doc.current_page_mut();
                    page.connectors.push(DiagramConnector {
                        id: cid,
                        from_shape: from_s,
                        from_port: from_p,
                        from_pos: start_pos,
                        to_shape: to_s,
                        to_port: to_p,
                        to_pos: end_pos,
                        style: ConnectorStyle::Orthogonal,
                        start_arrow: ArrowType::None,
                        end_arrow: ArrowType::None,
                        label: String::new(),
                        stroke_color: [220, 38, 38, 255],
                        stroke_width: 1.8,
                        dashed: false,
                        layer: "Sähkö & Kytkennät".to_string(),
                        waypoints: Vec::new(),
                    });
                    update_all_connectors(&mut page.connectors, &page.shapes);
                    self.selected_connector_id = Some(cid);
                    self.selected_shape_id = None;
                    self.status_msg = "Johdin kytketty.".to_string();
                }
            }

            // Finish Wall / Room / Dimension creation drag
            if let Some(start_mm) = self.creation_drag_start_mm.take() {
                let end_mm = self.snap_point(cur_mm);
                let cx = (start_mm.x + end_mm.x) * 0.5;
                let cy = (start_mm.y + end_mm.y) * 0.5;
                let w = (end_mm.x - start_mm.x).abs().max(6.0);
                let h = (end_mm.y - start_mm.y).abs().max(4.0);

                let st_id = match self.active_tool {
                    ActiveTool::DrawWall => Some("fp_wall_ext"),
                    ActiveTool::DrawRoom => Some("fp_room"),
                    ActiveTool::DrawDimension => Some("fp_dimension"),
                    _ => None,
                };
                if let Some(stid) = st_id {
                    self.push_undo();
                    let id = self.doc.alloc_id();
                    let count = self.doc.current_page().shapes.len() + 1;
                    if let Some(mut sh) = instantiate_shape(stid, id, cx, cy, count) {
                        if self.active_tool == ActiveTool::DrawWall {
                            if w >= h {
                                sh.width = w;
                                sh.height = 4.0;
                            } else {
                                sh.width = w.max(6.0);
                                sh.height = 4.0;
                                sh.rotation_deg = 90.0;
                            }
                        } else if self.active_tool == ActiveTool::DrawDimension {
                            sh.width = w.max(h);
                            sh.height = 8.0;
                            if h > w {
                                sh.rotation_deg = 90.0;
                            }
                        } else {
                            sh.width = w;
                            sh.height = h;
                        }
                        self.doc.current_page_mut().shapes.push(sh);
                        self.selected_shape_id = Some(id);
                        self.active_tool = ActiveTool::Select;
                    }
                }
            }
        }

        // 6. Simple click selection
        if response.clicked() && self.pending_stencil_id.is_none() {
            let page = self.doc.current_page();
            // Hit-test connectors first
            if let Some(conn) = page
                .connectors
                .iter()
                .find(|c| connector_hit_test(c, cur_mm, 2.5))
            {
                self.selected_connector_id = Some(conn.id);
                self.selected_shape_id = None;
                return;
            }

            // Hit-test foreground shapes then rooms
            let hit = page
                .shapes
                .iter()
                .rev()
                .find(|s| !s.is_room_area && s.contains_point(cur_mm, 1.5))
                .or_else(|| {
                    page.shapes
                        .iter()
                        .rev()
                        .find(|s| s.is_room_area && s.contains_point(cur_mm, 1.5))
                });

            if let Some(s) = hit {
                self.selected_shape_id = Some(s.id);
                self.selected_connector_id = None;
            } else {
                self.selected_shape_id = None;
                self.selected_connector_id = None;
            }
        }

        let _ = origin;
    }

    fn draw_shape_on_canvas(
        &self,
        painter: &egui::Painter,
        origin: Pos2,
        page: &DiagramPage,
        shape: &DiagramShape,
        is_selected: bool,
    ) {
        let stroke_col = Color32::from_rgba_unmultiplied(
            shape.stroke_color[0],
            shape.stroke_color[1],
            shape.stroke_color[2],
            shape.stroke_color[3],
        );
        let fill_col = Color32::from_rgba_unmultiplied(
            shape.fill_color[0],
            shape.fill_color[1],
            shape.fill_color[2],
            shape.fill_color[3],
        );
        let stroke = Stroke::new(shape.stroke_width, stroke_col);

        for op in &shape.geometry {
            match op {
                PrimOp::Line { x1, y1, x2, y2 } => {
                    let p1 = self.page_to_screen(origin, shape.local_to_page(*x1, *y1));
                    let p2 = self.page_to_screen(origin, shape.local_to_page(*x2, *y2));
                    painter.line_segment([p1, p2], stroke);
                }
                PrimOp::Rect {
                    x,
                    y,
                    w,
                    h,
                    filled,
                } => {
                    let pts = vec![
                        self.page_to_screen(origin, shape.local_to_page(*x, *y)),
                        self.page_to_screen(origin, shape.local_to_page(*x + *w, *y)),
                        self.page_to_screen(origin, shape.local_to_page(*x + *w, *y + *h)),
                        self.page_to_screen(origin, shape.local_to_page(*x, *y + *h)),
                    ];
                    let fill = if *filled {
                        fill_col
                    } else {
                        Color32::TRANSPARENT
                    };
                    painter.add(Shape::convex_polygon(pts, fill, stroke));
                }
                PrimOp::Circle {
                    cx,
                    cy,
                    r,
                    filled,
                } => {
                    let center = self.page_to_screen(origin, shape.local_to_page(*cx, *cy));
                    let rad_px =
                        (*r as f32) * ((shape.width + shape.height) as f32 * 0.5) * self.zoom_px_per_mm;
                    if *filled && fill_col.a() > 0 {
                        painter.circle_filled(center, rad_px, fill_col);
                    }
                    painter.circle_stroke(center, rad_px, stroke);
                }
                PrimOp::Arc {
                    cx,
                    cy,
                    rx,
                    ry,
                    start_deg,
                    sweep_deg,
                } => {
                    let steps = 16;
                    let mut pts = Vec::with_capacity(steps + 1);
                    for i in 0..=steps {
                        let t = i as f64 / steps as f64;
                        let ang = (start_deg + sweep_deg * t) * PI / 180.0;
                        let lx = cx + rx * ang.cos();
                        let ly = cy + ry * ang.sin();
                        pts.push(self.page_to_screen(origin, shape.local_to_page(lx, ly)));
                    }
                    painter.add(Shape::line(pts, stroke));
                }
                PrimOp::Polyline {
                    points,
                    closed,
                    filled,
                } => {
                    let pts: Vec<Pos2> = points
                        .iter()
                        .map(|(lx, ly)| self.page_to_screen(origin, shape.local_to_page(*lx, *ly)))
                        .collect();
                    let fill = if *filled {
                        fill_col
                    } else {
                        Color32::TRANSPARENT
                    };
                    draw_robust_path(painter, &pts, *closed, fill, stroke);
                }
                PrimOp::Path {
                    points,
                    closed,
                    filled,
                    fill_color,
                    stroke_color,
                    stroke_width: sw_opt,
                } => {
                    let pts: Vec<Pos2> = points
                        .iter()
                        .map(|(lx, ly)| self.page_to_screen(origin, shape.local_to_page(*lx, *ly)))
                        .collect();
                    let f_col = fill_color
                        .map(|c| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
                        .unwrap_or(fill_col);
                    let s_col = stroke_color
                        .map(|c| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
                        .unwrap_or(stroke_col);
                    let cur_sw = sw_opt.unwrap_or(shape.stroke_width);
                    let st = if stroke_color.is_some() || !*filled {
                        Stroke::new(cur_sw, s_col)
                    } else {
                        Stroke::NONE
                    };
                    let f = if *filled && f_col.a() > 0 {
                        f_col
                    } else {
                        Color32::TRANSPARENT
                    };
                    draw_robust_path(painter, &pts, *closed, f, st);
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
                    let center = self.page_to_screen(origin, shape.local_to_page(*cx, *cy));
                    let erx = (*rx as f32) * (shape.width as f32) * self.zoom_px_per_mm;
                    let ery = (*ry as f32) * (shape.height as f32) * self.zoom_px_per_mm;
                    let f_col = fill_color
                        .map(|c| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
                        .unwrap_or(fill_col);
                    let s_col = stroke_color
                        .map(|c| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
                        .unwrap_or(stroke_col);
                    let st = if stroke_color.is_some() || !*filled {
                        Stroke::new(shape.stroke_width, s_col)
                    } else {
                        Stroke::NONE
                    };
                    let steps = 24;
                    let pts: Vec<Pos2> = (0..steps)
                        .map(|i| {
                            let t = (i as f32 / steps as f32) * std::f32::consts::TAU;
                            Pos2::new(center.x + erx * t.cos(), center.y + ery * t.sin())
                        })
                        .collect();
                    let f = if *filled && f_col.a() > 0 {
                        f_col
                    } else {
                        Color32::TRANSPARENT
                    };
                    draw_robust_path(painter, &pts, true, f, st);
                }
                PrimOp::Text {
                    x,
                    y,
                    text,
                    size_rel,
                } => {
                    let pos = self.page_to_screen(origin, shape.local_to_page(*x, *y));
                    let px =
                        ((*size_rel as f32) * (shape.height as f32) * self.zoom_px_per_mm).clamp(8.0, 22.0);
                    painter.text(
                        pos,
                        egui::Align2::CENTER_CENTER,
                        text,
                        FontId::proportional(px),
                        stroke_col,
                    );
                }
            }
        }

        // Draw component label / value or live architectural room area / dimension
        let caption = shape_display_caption(page, shape);
        if !caption.is_empty() {
            let label_pt = if shape.is_room_area || shape.is_dimension {
                Point::new(shape.x, shape.y)
            } else {
                let bb = shape.bounding_box();
                Point::new(shape.x, bb.min_y - 2.6)
            };
            let sp = self.page_to_screen(origin, label_pt);
            painter.text(
                sp,
                egui::Align2::CENTER_CENTER,
                &caption,
                FontId::proportional((3.3 * self.zoom_px_per_mm).clamp(9.5, 16.0)),
                stroke_col,
            );
        }

        // Draw Selection Box & 8 Resize Handles
        if is_selected {
            let bb = shape.bounding_box();
            let tl = self.page_to_screen(origin, Point::new(bb.min_x, bb.min_y));
            let br = self.page_to_screen(origin, Point::new(bb.max_x, bb.max_y));
            let sel_rect = Rect::from_min_max(tl, br).expand(2.0);
            let sel_col = Color32::from_rgb(37, 99, 235);
            painter.rect_stroke(
                sel_rect,
                CornerRadius::ZERO,
                Stroke::new(1.5_f32, sel_col),
                StrokeKind::Middle,
            );

            for (_, hpt) in resize_handle_points(shape) {
                let hp = self.page_to_screen(origin, hpt);
                let hrect = Rect::from_center_size(hp, Vec2::splat(6.5));
                painter.rect_filled(hrect, CornerRadius::ZERO, Color32::WHITE);
                painter.rect_stroke(
                    hrect,
                    CornerRadius::ZERO,
                    Stroke::new(1.5_f32, sel_col),
                    StrokeKind::Middle,
                );
            }
        }
    }

    fn draw_connector_on_canvas(
        &self,
        painter: &egui::Painter,
        origin: Pos2,
        conn: &DiagramConnector,
        jumps: &[crate::routing::WireJump],
        is_selected: bool,
    ) {
        if conn.waypoints.len() < 2 {
            return;
        }
        let col = if is_selected {
            Color32::from_rgb(37, 99, 235)
        } else {
            Color32::from_rgba_unmultiplied(
                conn.stroke_color[0],
                conn.stroke_color[1],
                conn.stroke_color[2],
                conn.stroke_color[3],
            )
        };
        let sw = if is_selected {
            conn.stroke_width + 1.0
        } else {
            conn.stroke_width
        };
        let stroke = Stroke::new(sw, col);

        let screen_pts: Vec<Pos2> = conn
            .waypoints
            .iter()
            .map(|p| self.page_to_screen(origin, *p))
            .collect();

        if conn.dashed {
            for seg in screen_pts.windows(2) {
                draw_dashed_segment(painter, seg[0], seg[1], stroke, 7.0, 4.5);
            }
        } else {
            painter.add(Shape::line(screen_pts.clone(), stroke));
        }

        // Draw bridge jump arcs (`WireJump`) where this horizontal wire crosses a vertical wire
        for j in jumps.iter().filter(|j| j.connector_id == conn.id) {
            let center = self.page_to_screen(origin, j.point);
            let r_px = (1.6 * self.zoom_px_per_mm).max(4.0);
            // White mask over the underlying segment gap
            painter.line_segment(
                [
                    Pos2::new(center.x - r_px, center.y),
                    Pos2::new(center.x + r_px, center.y),
                ],
                Stroke::new(sw + 2.5, Color32::WHITE),
            );
            // Draw top semicircle bridge
            let mut arc_pts = Vec::with_capacity(11);
            for k in 0..=10 {
                let ang = PI + (k as f64 / 10.0) * PI;
                arc_pts.push(Pos2::new(
                    center.x + r_px * (ang.cos() as f32),
                    center.y + r_px * (ang.sin() as f32),
                ));
            }
            painter.add(Shape::line(arc_pts, stroke));
        }

        // End markers (JunctionDot / Arrow)
        if conn.start_arrow == ArrowType::JunctionDot {
            painter.circle_filled(screen_pts[0], 3.8, col);
        }
        if conn.end_arrow == ArrowType::JunctionDot {
            if let Some(&last) = screen_pts.last() {
                painter.circle_filled(last, 3.8, col);
            }
        } else if conn.end_arrow == ArrowType::Arrow && screen_pts.len() >= 2 {
            let p_prev = screen_pts[screen_pts.len() - 2];
            let p_end = screen_pts[screen_pts.len() - 1];
            let dir = (p_end - p_prev).normalized();
            let n = Vec2::new(-dir.y, dir.x);
            let tip1 = p_end - dir * 8.5 + n * 4.0;
            let tip2 = p_end - dir * 8.5 - n * 4.0;
            painter.add(Shape::convex_polygon(
                vec![p_end, tip1, tip2],
                col,
                Stroke::NONE,
            ));
        }

        if !conn.label.is_empty() && screen_pts.len() >= 2 {
            let mid_idx = screen_pts.len() / 2;
            let mid = Pos2::new(
                (screen_pts[mid_idx - 1].x + screen_pts[mid_idx].x) * 0.5,
                (screen_pts[mid_idx - 1].y + screen_pts[mid_idx].y) * 0.5 - 8.0,
            );
            painter.text(
                mid,
                egui::Align2::CENTER_CENTER,
                &conn.label,
                FontId::proportional(10.5),
                col,
            );
        }
    }
}

fn resize_handle_points(shape: &DiagramShape) -> [(ResizeHandle, Point); 8] {
    let bb = shape.bounding_box();
    let mx = (bb.min_x + bb.max_x) * 0.5;
    let my = (bb.min_y + bb.max_y) * 0.5;
    [
        (ResizeHandle::TopLeft, Point::new(bb.min_x, bb.min_y)),
        (ResizeHandle::TopRight, Point::new(bb.max_x, bb.min_y)),
        (ResizeHandle::BottomLeft, Point::new(bb.min_x, bb.max_y)),
        (ResizeHandle::BottomRight, Point::new(bb.max_x, bb.max_y)),
        (ResizeHandle::Top, Point::new(mx, bb.min_y)),
        (ResizeHandle::Bottom, Point::new(mx, bb.max_y)),
        (ResizeHandle::Left, Point::new(bb.min_x, my)),
        (ResizeHandle::Right, Point::new(bb.max_x, my)),
    ]
}

fn hit_test_resize_handle(
    shape: &DiagramShape,
    p: Point,
    tol_mm: f64,
) -> Option<ResizeHandle> {
    for (h, pt) in resize_handle_points(shape) {
        if p.distance_to(pt) <= tol_mm {
            return Some(h);
        }
    }
    None
}

fn draw_dashed_segment(
    painter: &egui::Painter,
    a: Pos2,
    b: Pos2,
    stroke: Stroke,
    dash_px: f32,
    gap_px: f32,
) {
    let delta = b - a;
    let len = delta.length();
    if len < 0.5 {
        return;
    }
    let dir = delta / len;
    let mut t = 0.0;
    while t < len {
        let t_end = (t + dash_px).min(len);
        painter.line_segment([a + dir * t, a + dir * t_end], stroke);
        t += dash_px + gap_px;
    }
}

fn draw_rulers(
    painter: &egui::Painter,
    canvas_rect: Rect,
    origin: Pos2,
    page: &DiagramPage,
    zoom_px_per_mm: f32,
) {
    let ruler_thick = 20.0;
    let top_rect = Rect::from_min_max(
        canvas_rect.min,
        Pos2::new(canvas_rect.max.x, canvas_rect.min.y + ruler_thick),
    );
    let left_rect = Rect::from_min_max(
        canvas_rect.min,
        Pos2::new(canvas_rect.min.x + ruler_thick, canvas_rect.max.y),
    );
    let bg = Color32::from_rgb(241, 245, 249);
    let fg = Color32::from_rgb(71, 85, 105);
    painter.rect_filled(top_rect, CornerRadius::ZERO, bg);
    painter.rect_filled(left_rect, CornerRadius::ZERO, bg);

    // Top horizontal ticks every 10 mm
    let mut x_mm = 0.0;
    while x_mm <= page.width_mm {
        let sx = origin.x + 42.0 + (x_mm as f32) * zoom_px_per_mm;
        if sx >= canvas_rect.min.x + ruler_thick && sx <= canvas_rect.max.x {
            painter.line_segment(
                [
                    Pos2::new(sx, canvas_rect.min.y + 13.0),
                    Pos2::new(sx, canvas_rect.min.y + ruler_thick),
                ],
                Stroke::new(1.0_f32, fg),
            );
            if (x_mm as i32) % 20 == 0 {
                painter.text(
                    Pos2::new(sx + 2.0, canvas_rect.min.y + 2.0),
                    egui::Align2::LEFT_TOP,
                    format!("{:.0}", x_mm),
                    FontId::proportional(9.0),
                    fg,
                );
            }
        }
        x_mm += 10.0;
    }
}

fn draw_stencil_mini_preview(painter: &egui::Painter, rect: Rect, item: &StencilItem) {
    let inner = rect.shrink(2.0);
    let stroke = Stroke::new(
        1.2_f32,
        Color32::from_rgba_unmultiplied(
            item.stroke_color[0],
            item.stroke_color[1],
            item.stroke_color[2],
            255,
        ),
    );
    let map = |rx: f64, ry: f64| -> Pos2 {
        Pos2::new(
            inner.min.x + (rx as f32) * inner.width(),
            inner.min.y + (ry as f32) * inner.height(),
        )
    };
    for op in &item.geometry {
        match op {
            PrimOp::Line { x1, y1, x2, y2 } => {
                painter.line_segment([map(*x1, *y1), map(*x2, *y2)], stroke);
            }
            PrimOp::Rect { x, y, w, h, filled } => {
                let r = Rect::from_min_max(map(*x, *y), map(*x + *w, *y + *h));
                let f = if *filled {
                    Color32::from_rgba_unmultiplied(
                        item.fill_color[0],
                        item.fill_color[1],
                        item.fill_color[2],
                        item.fill_color[3],
                    )
                } else {
                    Color32::TRANSPARENT
                };
                painter.rect_filled(r, CornerRadius::ZERO, f);
                painter.rect_stroke(r, CornerRadius::ZERO, stroke, StrokeKind::Middle);
            }
            PrimOp::Circle { cx, cy, r, filled } => {
                let rad = (*r as f32) * inner.height();
                let f = if *filled {
                    Color32::from_rgba_unmultiplied(
                        item.fill_color[0],
                        item.fill_color[1],
                        item.fill_color[2],
                        item.fill_color[3],
                    )
                } else {
                    Color32::TRANSPARENT
                };
                painter.circle_filled(map(*cx, *cy), rad, f);
                painter.circle_stroke(map(*cx, *cy), rad, stroke);
            }
            PrimOp::Polyline { points, closed, filled } => {
                let pts: Vec<Pos2> = points.iter().map(|(x, y)| map(*x, *y)).collect();
                let f = if *filled {
                    Color32::from_rgba_unmultiplied(
                        item.fill_color[0],
                        item.fill_color[1],
                        item.fill_color[2],
                        item.fill_color[3],
                    )
                } else {
                    Color32::TRANSPARENT
                };
                draw_robust_path(painter, &pts, *closed, f, stroke);
            }
            PrimOp::Path {
                points,
                closed,
                filled,
                fill_color,
                stroke_color,
                ..
            } => {
                let pts: Vec<Pos2> = points.iter().map(|(x, y)| map(*x, *y)).collect();
                let f_col = fill_color
                    .map(|c| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
                    .unwrap_or_else(|| {
                        Color32::from_rgba_unmultiplied(
                            item.fill_color[0],
                            item.fill_color[1],
                            item.fill_color[2],
                            item.fill_color[3],
                        )
                    });
                let s_col = stroke_color
                    .map(|c| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
                    .unwrap_or(stroke.color);
                let st = if stroke_color.is_some() || !*filled {
                    Stroke::new(1.0_f32, s_col)
                } else {
                    Stroke::NONE
                };
                let f = if *filled && f_col.a() > 0 {
                    f_col
                } else {
                    Color32::TRANSPARENT
                };
                draw_robust_path(painter, &pts, *closed, f, st);
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
                let center = map(*cx, *cy);
                let erx = (*rx as f32) * inner.width();
                let ery = (*ry as f32) * inner.height();
                let f_col = fill_color
                    .map(|c| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
                    .unwrap_or_else(|| {
                        Color32::from_rgba_unmultiplied(
                            item.fill_color[0],
                            item.fill_color[1],
                            item.fill_color[2],
                            item.fill_color[3],
                        )
                    });
                let s_col = stroke_color
                    .map(|c| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
                    .unwrap_or(stroke.color);
                let st = if stroke_color.is_some() || !*filled {
                    Stroke::new(1.0_f32, s_col)
                } else {
                    Stroke::NONE
                };
                let steps = 16;
                let pts: Vec<Pos2> = (0..steps)
                    .map(|i| {
                        let t = (i as f32 / steps as f32) * std::f32::consts::TAU;
                        Pos2::new(center.x + erx * t.cos(), center.y + ery * t.sin())
                    })
                    .collect();
                let f = if *filled && f_col.a() > 0 {
                    f_col
                } else {
                    Color32::TRANSPARENT
                };
                draw_robust_path(painter, &pts, true, f, st);
            }
            PrimOp::Text {
                x,
                y,
                text,
                size_rel,
            } => {
                let pos = map(*x, *y);
                let px = ((*size_rel as f32) * inner.height()).clamp(7.5, 12.0);
                painter.text(
                    pos,
                    egui::Align2::CENTER_CENTER,
                    text,
                    FontId::proportional(px),
                    stroke.color,
                );
            }
            _ => {}
        }
    }
}
