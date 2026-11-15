use crate::formats::export::shape_display_caption;
use crate::formats::{load_document_from_path, load_stencil_library, save_document_to_path};
use crate::model::{
    ArrowType, ConnectorStyle, DiagramConnector, DiagramDocument, DiagramPage, DiagramShape,
    DrawingScale, Point, PrimOp, TextAlign, TextPosition,
};
use crate::routing::{
    compute_connector_path, connector_hit_test, connector_label_position_smart, find_wire_jumps,
    resolve_clean_label_position, update_all_connectors,
};
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
    DrawText,
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

pub enum FileAction {
    OpenDocument(PathBuf),
    SaveDocument(PathBuf),
    ImportVssx(Vec<PathBuf>),
    SetStatus(String),
}

pub struct PhotonDiagramApp {
    pub doc: DiagramDocument,
    pub stencils: Vec<StencilItem>,
    pub stencil_search: String,
    pub active_tool: ActiveTool,
    pub pending_stencil_id: Option<String>,
    pub selected_shape_id: Option<u64>,
    pub selected_shape_ids: Vec<u64>,
    pub selected_connector_id: Option<u64>,
    pub editing_shape_id: Option<u64>,
    pub focus_text_edit: bool,
    pub canvas_origin: Pos2,
    pub current_file: Option<PathBuf>,
    pub status_msg: String,

    // Thread-safe asynchronous file dialog channel
    file_action_tx: std::sync::mpsc::Sender<FileAction>,
    file_action_rx: std::sync::mpsc::Receiver<FileAction>,

    // Canvas viewport state (zoom in pixels per paper mm, pan offset in screen pixels)
    pub zoom_px_per_mm: f32,
    pub pan_offset: Vec2,

    // Interactive drag states
    dragging_shape_id: Option<u64>,
    drag_start_mouse_mm: Point,
    drag_start_shape_pos: Point,
    drag_start_shape_size: (f64, f64),
    drag_start_group_positions: Vec<(u64, Point)>,
    marquee_drag_start_mm: Option<Point>,
    marquee_current_mm: Option<Point>,
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

impl Default for PhotonDiagramApp {
    fn default() -> Self {
        let (file_action_tx, file_action_rx) = std::sync::mpsc::channel();
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
            selected_shape_ids: Vec::new(),
            selected_connector_id: None,
            editing_shape_id: None,
            focus_text_edit: false,
            canvas_origin: Pos2::ZERO,
            current_file: None,
            status_msg: "Valmis — Valitse vasemmalta symboli tai vedä vihreästä liitospisteestä johdin. Sivu 1: Sähkökaavio, Sivu 2: Pohjapiirustus 1:50.".to_string(),
            file_action_tx,
            file_action_rx,
            zoom_px_per_mm: 3.4,
            pan_offset: Vec2::new(42.0, 36.0),
            dragging_shape_id: None,
            drag_start_mouse_mm: Point::ZERO,
            drag_start_shape_pos: Point::ZERO,
            drag_start_shape_size: (0.0, 0.0),
            drag_start_group_positions: Vec::new(),
            marquee_drag_start_mm: None,
            marquee_current_mm: None,
            active_resize_handle: None,
            wiring_from: None,
            creation_drag_start_mm: None,
            smart_guides: (None, None),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }
}

impl PhotonDiagramApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut fonts = egui::FontDefinitions::default();
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .push("Hack".to_owned());
        cc.egui_ctx.set_fonts(fonts);

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

    pub fn is_shape_selected(&self, id: u64) -> bool {
        self.selected_shape_id == Some(id) || self.selected_shape_ids.contains(&id)
    }

    pub fn selected_shape_ids(&self) -> Vec<u64> {
        let mut ids = self.selected_shape_ids.clone();
        if let Some(id) = self.selected_shape_id {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        ids
    }

    pub fn select_single_shape(&mut self, id: u64) {
        self.selected_shape_id = Some(id);
        self.selected_shape_ids = vec![id];
        self.selected_connector_id = None;
    }

    pub fn select_shapes(&mut self, ids: Vec<u64>) {
        self.selected_shape_id = ids.last().copied();
        self.selected_shape_ids = ids;
        self.selected_connector_id = None;
    }

    pub fn clear_selection(&mut self) {
        self.selected_shape_id = None;
        self.selected_shape_ids.clear();
        self.selected_connector_id = None;
    }

    pub fn toggle_shape_selection(&mut self, id: u64) {
        if self.is_shape_selected(id) {
            self.selected_shape_ids.retain(|&x| x != id);
            if self.selected_shape_id == Some(id) {
                self.selected_shape_id = self.selected_shape_ids.last().copied();
            }
        } else {
            self.selected_shape_ids.push(id);
            self.selected_shape_id = Some(id);
            self.selected_connector_id = None;
        }
    }

    fn rotate_selected(&mut self, delta_deg: f64) {
        let sids = self.selected_shape_ids();
        if !sids.is_empty() {
            self.push_undo();
            let page = self.doc.current_page_mut();
            for s in &mut page.shapes {
                if sids.contains(&s.id) {
                    s.rotation_deg = (s.rotation_deg + delta_deg).rem_euclid(360.0);
                }
            }
            update_all_connectors(&mut page.connectors, &page.shapes);
        }
    }

    fn flip_selected(&mut self, horizontal: bool) {
        let sids = self.selected_shape_ids();
        if !sids.is_empty() {
            self.push_undo();
            let page = self.doc.current_page_mut();
            for s in &mut page.shapes {
                if sids.contains(&s.id) {
                    if horizontal {
                        s.flip_x = !s.flip_x;
                    } else {
                        s.flip_y = !s.flip_y;
                    }
                }
            }
            update_all_connectors(&mut page.connectors, &page.shapes);
        }
    }

    fn delete_selected(&mut self) {
        let sids = self.selected_shape_ids();
        if !sids.is_empty() {
            self.push_undo();
            let page = self.doc.current_page_mut();
            page.shapes.retain(|s| !sids.contains(&s.id));
            page.connectors
                .retain(|c| !c.from_shape.map_or(false, |id| sids.contains(&id))
                         && !c.to_shape.map_or(false, |id| sids.contains(&id)));
            let count = sids.len();
            self.clear_selection();
            self.status_msg = if count == 1 {
                "Komponentti poistettu.".to_string()
            } else {
                format!("{} komponenttia poistettu.", count)
            };
        } else if let Some(cid) = self.selected_connector_id.take() {
            self.push_undo();
            let page = self.doc.current_page_mut();
            page.connectors.retain(|c| c.id != cid);
            self.status_msg = "Johdin poistettu.".to_string();
        }
    }

    fn duplicate_selected(&mut self) {
        let sids = self.selected_shape_ids();
        if !sids.is_empty() {
            self.push_undo();
            let mut new_ids = Vec::new();
            let shapes_to_clone: Vec<DiagramShape> = self
                .doc
                .current_page()
                .shapes
                .iter()
                .filter(|s| sids.contains(&s.id))
                .cloned()
                .collect();

            for mut s in shapes_to_clone {
                let new_id = self.doc.alloc_id();
                s.id = new_id;
                s.x += 10.0;
                s.y += 10.0;
                new_ids.push(new_id);
                self.doc.current_page_mut().shapes.push(s);
            }
            let count = new_ids.len();
            self.select_shapes(new_ids);
            self.status_msg = if count == 1 {
                "Komponentti monistettu.".to_string()
            } else {
                format!("{} komponenttia monistettu.", count)
            };
        }
    }

    fn open_file_dialog(&self, ctx: &egui::Context) {
        let tx = self.file_action_tx.clone();
        let ctx = ctx.clone();

        std::thread::spawn(move || {
            pollster::block_on(async move {
                if let Some(handle) = rfd::AsyncFileDialog::new()
                    .add_filter(
                        "Tuetut kaaviot & projektit (.vsdx, .vdx, .vsd, .lbrn2, .pvis)",
                        &["vsdx", "vdx", "vsd", "lbrn2", "lbrn", "pvis", "json"],
                    )
                    .add_filter("PhotonCAM / LightBurn (.lbrn2)", &["lbrn2", "lbrn"])
                    .add_filter("Microsoft Visio (.vsdx, .vdx, .vsd)", &["vsdx", "vdx", "vsd"])
                    .add_filter("PhotonDiagram (.pdiag, .pvis)", &["pdiag", "pvis"])
                    .pick_file()
                    .await
                {
                    let path = handle.path().to_path_buf();
                    let _ = tx.send(FileAction::OpenDocument(path));
                    ctx.request_repaint();
                }
            });
        });
    }

    fn import_vssx_dialog(&self, ctx: &egui::Context) {
        let tx = self.file_action_tx.clone();
        let ctx = ctx.clone();

        std::thread::spawn(move || {
            pollster::block_on(async move {
                if let Some(handles) = rfd::AsyncFileDialog::new()
                    .add_filter("Visio Stencil Library (.vssx, .vsdx)", &["vssx", "vsdx"])
                    .pick_files()
                    .await
                {
                    let paths: Vec<PathBuf> =
                        handles.into_iter().map(|h| h.path().to_path_buf()).collect();
                    let _ = tx.send(FileAction::ImportVssx(paths));
                    ctx.request_repaint();
                }
            });
        });
    }

    fn save_file_as_dialog(&self, default_ext: &str, filter_label: &str, ctx: &egui::Context) {
        let tx = self.file_action_tx.clone();
        let ctx = ctx.clone();
        let ext = default_ext.to_string();
        let label = filter_label.to_string();
        let default_name = if let Some(ref cf) = self.current_file {
            let stem = cf.file_stem().and_then(|s| s.to_str()).unwrap_or("kaavio");
            format!("{}.{}", stem, ext)
        } else {
            format!("kaavio.{}", ext)
        };

        std::thread::spawn(move || {
            pollster::block_on(async move {
                let ext_lower = ext.to_lowercase();
                let ext_upper = ext.to_uppercase();
                let extensions = [ext_lower.as_str(), ext_upper.as_str()];
                if let Some(handle) = rfd::AsyncFileDialog::new()
                    .add_filter(&label, &extensions)
                    .set_file_name(&default_name)
                    .save_file()
                    .await
                {
                    let path = handle.path().to_path_buf();
                    let _ = tx.send(FileAction::SaveDocument(path));
                    ctx.request_repaint();
                }
            });
        });
    }
}

impl eframe::App for PhotonDiagramApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process asynchronous file dialog actions
        while let Ok(action) = self.file_action_rx.try_recv() {
            match action {
                FileAction::OpenDocument(path) => match load_document_from_path(&path) {
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
                },
                FileAction::SaveDocument(path) => match save_document_to_path(&self.doc, &path) {
                    Ok(()) => {
                        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                        if ext == "pvis" || ext == "vsdx" {
                            self.current_file = Some(path.clone());
                        }
                        self.status_msg = format!("Tallennettu: {}", path.display());
                    }
                    Err(e) => {
                        self.status_msg = format!("Tallennusvirhe: {}", e);
                    }
                },
                FileAction::ImportVssx(paths) => {
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
                                self.status_msg =
                                    format!("VSSX-tuontivirhe ({}): {}", path.display(), e);
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
                FileAction::SetStatus(msg) => {
                    self.status_msg = msg;
                }
            }
        }
        // Keyboard shortcuts
        if !ctx.wants_keyboard_input() && self.editing_shape_id.is_none() {
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
                    if self.pending_stencil_id.is_some() || self.wiring_from.is_some() || self.creation_drag_start_mm.is_some() || self.active_tool != ActiveTool::Select {
                        self.pending_stencil_id = None;
                        self.wiring_from = None;
                        self.creation_drag_start_mm = None;
                        self.active_tool = ActiveTool::Select;
                    } else {
                        self.clear_selection();
                    }
                }
                if i.modifiers.command && i.key_pressed(Key::A) {
                    let all_ids: Vec<u64> = self.doc.current_page().shapes.iter().map(|s| s.id).collect();
                    let count = all_ids.len();
                    self.select_shapes(all_ids);
                    self.status_msg = format!("Valittu kaikki {} komponenttia.", count);
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
                if i.modifiers.command && i.key_pressed(Key::CloseBracket) {
                    let sids = self.selected_shape_ids();
                    if !sids.is_empty() {
                        self.push_undo();
                        for sid in sids {
                            if i.modifiers.shift {
                                self.doc.current_page_mut().bring_to_front(sid);
                            } else {
                                self.doc.current_page_mut().bring_forward(sid);
                            }
                        }
                    }
                }
                if i.modifiers.command && i.key_pressed(Key::OpenBracket) {
                    let sids = self.selected_shape_ids();
                    if !sids.is_empty() {
                        self.push_undo();
                        for sid in sids {
                            if i.modifiers.shift {
                                self.doc.current_page_mut().send_to_back(sid);
                            } else {
                                self.doc.current_page_mut().send_backward(sid);
                            }
                        }
                    }
                }
            });
        }

        self.render_top_bar(ctx);
        self.render_bottom_bar(ctx);
        self.render_left_stencil_panel(ctx);
        self.render_right_inspector_panel(ctx);
        self.render_center_canvas(ctx);
        self.render_text_edit_dialog(ctx);
    }
}

impl PhotonDiagramApp {
    fn render_top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // 1. Menu bar
            egui::menu::bar(ui, |ui| {
                ui.menu_button("Tiedosto", |ui| {
                    if ui.button("Uusi tyhjä kaavio").clicked() {
                        self.push_undo();
                        self.doc = DiagramDocument::default();
                        self.selected_shape_id = None;
                        self.selected_connector_id = None;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui
                        .button("Avaa (.vsdx, .vdx, .vsd, .lbrn2, .pvis)...")
                        .clicked()
                    {
                        ui.close_menu();
                        self.open_file_dialog(ui.ctx());
                    }
                    if ui
                        .button("Tuo Visio-symbolikirjasto (.vssx)...")
                        .clicked()
                    {
                        ui.close_menu();
                        self.import_vssx_dialog(ui.ctx());
                    }
                    ui.separator();
                    if ui.button("Tallenna PhotonDiagram (.pdiag)...").clicked() {
                        ui.close_menu();
                        self.save_file_as_dialog("pdiag", "PhotonDiagram (.pdiag)", ui.ctx());
                    }
                    if ui
                        .button("Tallenna Microsoft Visio (.vsdx)...")
                        .clicked()
                    {
                        ui.close_menu();
                        self.save_file_as_dialog("vsdx", "Microsoft Visio 2013+ (.vsdx)", ui.ctx());
                    }
                    if ui.button("Tallenna Visio XML (.vdx)...").clicked() {
                        ui.close_menu();
                        self.save_file_as_dialog("vdx", "Visio XML Drawing (.vdx)", ui.ctx());
                    }
                    ui.separator();
                    if ui.button("Vie PhotonCAM / LightBurn (.lbrn2)...").clicked() {
                        ui.close_menu();
                        self.save_file_as_dialog("lbrn2", "PhotonCAM / LightBurn Laser Project (.lbrn2)", ui.ctx());
                    }
                    if ui.button("Vie SVG-vektorikuva (.svg)...").clicked() {
                        ui.close_menu();
                        self.save_file_as_dialog("svg", "SVG Vector Graphics (.svg)", ui.ctx());
                    }
                    if ui.button("Vie AutoCAD DXF (.dxf)...").clicked() {
                        ui.close_menu();
                        self.save_file_as_dialog("dxf", "AutoCAD DXF (.dxf)", ui.ctx());
                    }
                    if ui
                        .button("Vie Osaluettelo / Huoneet (.csv)...")
                        .clicked()
                    {
                        ui.close_menu();
                        self.save_file_as_dialog("csv", "Osaluettelo BOM (.csv)", ui.ctx());
                    }
                });

                ui.menu_button("Valmiit mallipohjat", |ui| {
                    if ui
                        .button("Sähkö- & Ohjauskytkentäkaavio (IEC / PLC)")
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
                        .button("Omakotitalon pohjapiirustus + SFS-sähkösuunnitelma (1:50)")
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

                ui.menu_button("Muokkaa", |ui| {
                    if ui.button("↩ Kumoa (Cmd+Z)").clicked() {
                        self.undo();
                        ui.close_menu();
                    }
                    if ui.button("↪ Tee uudelleen (Cmd+Shift+Z)").clicked() {
                        self.redo();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Monista valittu (Cmd+D)").clicked() {
                        self.duplicate_selected();
                        ui.close_menu();
                    }
                    if ui.button("Kierrä 90° (R)").clicked() {
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
                    if ui.button("Poista valittu (Backspace)").clicked() {
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
                    (ActiveTool::Wire, "Johdin"),
                    (ActiveTool::DrawText, "Teksti"),
                    (ActiveTool::DrawWall, "Seinä"),
                    (ActiveTool::DrawRoom, "Huone (m²)"),
                    (ActiveTool::DrawDimension, "Mittaviiva"),
                    (ActiveTool::Pan, "Vieritä"),
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
                if ui.button("90°").on_hover_text("Kierrä 90° (R)").clicked() {
                    self.rotate_selected(90.0);
                }
                if ui.button("↔ Peilaa").on_hover_text("Peilaa (H)").clicked() {
                    self.flip_selected(true);
                }
                if ui
                    .button("Monista")
                    .on_hover_text("Monista valittu (Cmd+D)")
                    .clicked()
                {
                    self.duplicate_selected();
                }
                if ui
                    .button("Poista")
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
                ui.checkbox(&mut self.doc.show_line_jumps, "Johdinhypyt (sillat)");
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

                if ui.small_button("+ Uusi sivu").clicked() {
                    let n = self.doc.pages.len() + 1;
                    let mut p = DiagramPage::default();
                    p.name = format!("Sivu {}", n);
                    self.doc.pages.push(p);
                    self.doc.active_page = self.doc.pages.len() - 1;
                }

                ui.separator();
                ui.label("Zoom:");
                if ui.small_button("-").clicked() {
                    self.zoom_px_per_mm = (self.zoom_px_per_mm * 0.85).clamp(0.8, 15.0);
                }
                ui.label(format!("{:.0}%", (self.zoom_px_per_mm / 3.4) * 100.0));
                if ui.small_button("+").clicked() {
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
                ui.heading("Symbolikirjasto");
                ui.horizontal(|ui| {
                    ui.label("Etsi:");
                    ui.text_edit_singleline(&mut self.stencil_search)
                        .on_hover_text("Hae esim. vastus, rele, pistorasia, ovi, kiuas...");
                    if !self.stencil_search.is_empty() && ui.small_button("×").clicked() {
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
                ui.heading("Ominaisuudet");
                ui.separator();

                let scale = self.doc.current_page().scale;
                let layer_names: Vec<String> = self
                    .doc
                    .current_page()
                    .layers
                    .iter()
                    .map(|l| l.name.clone())
                    .collect();

                let sel_ids = self.selected_shape_ids();
                if sel_ids.len() > 1 {
                    ui.label(
                        egui::RichText::new(format!("Ryhmävalinta ({} kpl)", sel_ids.len()))
                            .strong()
                            .color(Color32::from_rgb(37, 99, 235)),
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("Monista").on_hover_text("Monista valitut (Cmd+D)").clicked() {
                            self.duplicate_selected();
                        }
                        if ui.button("Poista").on_hover_text("Poista valitut (Delete)").clicked() {
                            self.delete_selected();
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button("90°").on_hover_text("Kierrä 90° (R)").clicked() {
                            self.rotate_selected(90.0);
                        }
                        if ui.button("↔ Peilaa").on_hover_text("Peilaa vaakasuunnassa (H)").clicked() {
                            self.flip_selected(true);
                        }
                    });
                    ui.add_space(8.0);
                    ui.label("Aseta taso kaikille:");
                    egui::ComboBox::from_id_salt("multi_shape_layer_combo")
                        .selected_text("Valitse taso...")
                        .show_ui(ui, |ui| {
                            for ln in &layer_names {
                                if ui.button(ln).clicked() {
                                    self.push_undo();
                                    let page = self.doc.current_page_mut();
                                    for s in &mut page.shapes {
                                        if sel_ids.contains(&s.id) {
                                            s.layer = ln.clone();
                                        }
                                    }
                                }
                            }
                        });
                    ui.add_space(8.0);
                    ui.label("Järjestys (Z-Order):");
                    ui.horizontal(|ui| {
                        if ui.small_button("▲▲ Ylimmäksi").clicked() {
                            self.push_undo();
                            for sid in &sel_ids {
                                self.doc.current_page_mut().bring_to_front(*sid);
                            }
                        }
                        if ui.small_button("▲ Eteen").clicked() {
                            self.push_undo();
                            for sid in &sel_ids {
                                self.doc.current_page_mut().bring_forward(*sid);
                            }
                        }
                        if ui.small_button("▼ Taakse").clicked() {
                            self.push_undo();
                            for sid in &sel_ids {
                                self.doc.current_page_mut().send_backward(*sid);
                            }
                        }
                        if ui.small_button("▼▼ Alimmaksi").clicked() {
                            self.push_undo();
                            for sid in &sel_ids {
                                self.doc.current_page_mut().send_to_back(*sid);
                            }
                        }
                    });
                } else if let Some(sid) = self.selected_shape_id {
                    let mut z_action: Option<&'static str> = None;
                    {
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
                                ui.label("Teksti / Tunnus:");
                                let te = egui::TextEdit::multiline(&mut shape.label)
                                    .id_salt("inspector_shape_label_field")
                                    .desired_rows(2)
                                    .desired_width(f32::INFINITY)
                                    .hint_text("Kirjoita teksti...");
                                ui.add(te);
                                ui.end_row();

                                ui.label("Arvo / Tieto:");
                                ui.text_edit_singleline(&mut shape.value);
                                ui.end_row();

                                ui.label("Tekstin sijainti:");
                                egui::ComboBox::from_id_salt("shape_text_pos_combo")
                                    .selected_text(match shape.text_position {
                                        TextPosition::Auto => "Automaattinen",
                                        TextPosition::Center => "Keskellä sisällä",
                                        TextPosition::Top => "Yläpuolella",
                                        TextPosition::Bottom => "Alapuolella",
                                        TextPosition::Hidden => "Piilotettu",
                                    })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut shape.text_position, TextPosition::Auto, "Automaattinen");
                                        ui.selectable_value(&mut shape.text_position, TextPosition::Center, "Keskellä sisällä");
                                        ui.selectable_value(&mut shape.text_position, TextPosition::Top, "Yläpuolella");
                                        ui.selectable_value(&mut shape.text_position, TextPosition::Bottom, "Alapuolella");
                                        ui.selectable_value(&mut shape.text_position, TextPosition::Hidden, "Piilotettu");
                                    });
                                ui.end_row();

                                ui.label("Tekstin tasaus:");
                                egui::ComboBox::from_id_salt("shape_text_align_combo")
                                    .selected_text(match shape.text_align {
                                        TextAlign::Auto => "Automaattinen",
                                        TextAlign::Left => "Vasen",
                                        TextAlign::Center => "Keskitetty",
                                        TextAlign::Right => "Oikea",
                                    })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut shape.text_align, TextAlign::Auto, "Automaattinen");
                                        ui.selectable_value(&mut shape.text_align, TextAlign::Left, "Vasen");
                                        ui.selectable_value(&mut shape.text_align, TextAlign::Center, "Keskitetty");
                                        ui.selectable_value(&mut shape.text_align, TextAlign::Right, "Oikea");
                                    });
                                ui.end_row();

                                ui.label("Fonttikoko:");
                                ui.horizontal(|ui| {
                                    if shape.font_size <= 0.0 {
                                        if ui.button("Auto (Oletus)").clicked() {
                                            shape.font_size = 11.0;
                                        }
                                        ui.label(egui::RichText::new("Auto").weak());
                                    } else {
                                        ui.add(
                                            egui::DragValue::new(&mut shape.font_size)
                                                .speed(0.5)
                                                .range(6.0..=72.0)
                                                .suffix(" pt"),
                                        );
                                        if ui.small_button("Auto").clicked() {
                                            shape.font_size = 0.0;
                                        }
                                    }
                                });
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
                                ui.horizontal(|ui| {
                                    let mut sc = Color32::from_rgba_unmultiplied(
                                        shape.stroke_color[0],
                                        shape.stroke_color[1],
                                        shape.stroke_color[2],
                                        shape.stroke_color[3],
                                    );
                                    if ui.color_edit_button_srgba(&mut sc).changed() {
                                        shape.stroke_color = sc.to_srgba_unmultiplied();
                                    }
                                    if shape.stroke_color[3] == 0 {
                                        if ui.small_button("Näytä viiva").clicked() {
                                            shape.stroke_color = [148, 163, 184, 180];
                                            shape.stroke_width = 1.2;
                                        }
                                    } else if ui.small_button("Ei viivaa").clicked() {
                                        shape.stroke_color[3] = 0;
                                    }
                                });
                                ui.end_row();

                                ui.label("Täyttöväri:");
                                ui.horizontal(|ui| {
                                    let mut fc = Color32::from_rgba_unmultiplied(
                                        shape.fill_color[0],
                                        shape.fill_color[1],
                                        shape.fill_color[2],
                                        shape.fill_color[3],
                                    );
                                    if ui.color_edit_button_srgba(&mut fc).changed() {
                                        shape.fill_color = fc.to_srgba_unmultiplied();
                                    }
                                    if shape.fill_color[3] == 0 {
                                        if ui.small_button("Valkoinen tausta").clicked() {
                                            shape.fill_color = [255, 255, 255, 240];
                                        }
                                    } else if ui.small_button("Läpinäkyvä").clicked() {
                                        shape.fill_color = [0, 0, 0, 0];
                                    }
                                });
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

                                ui.label("Järjestys (Z-Order):");
                                ui.horizontal(|ui| {
                                    if ui
                                        .small_button("▲▲ Ylimmäksi")
                                        .on_hover_text("Tuo etualalle (Bring to Front) • Cmd+Shift+]")
                                        .clicked()
                                    {
                                        z_action = Some("front");
                                    }
                                    if ui
                                        .small_button("▲ Eteen")
                                        .on_hover_text("Siirrä askel eteenpäin (Bring Forward) • Cmd+]")
                                        .clicked()
                                    {
                                        z_action = Some("forward");
                                    }
                                    if ui
                                        .small_button("▼ Taakse")
                                        .on_hover_text("Siirrä askel taaksepäin (Send Backward) • Cmd+[")
                                        .clicked()
                                    {
                                        z_action = Some("backward");
                                    }
                                    if ui
                                        .small_button("▼▼ Alimmaksi")
                                        .on_hover_text("Vie taustalle (Send to Back) • Cmd+Shift+[")
                                        .clicked()
                                    {
                                        z_action = Some("back");
                                    }
                                });
                                ui.end_row();
                            });

                        update_all_connectors(&mut page.connectors, &page.shapes);
                    }
                }

                if let Some(act) = z_action {
                    self.push_undo();
                    match act {
                        "front" => self.doc.current_page_mut().bring_to_front(sid),
                        "forward" => self.doc.current_page_mut().bring_forward(sid),
                        "backward" => self.doc.current_page_mut().send_backward(sid),
                        "back" => self.doc.current_page_mut().send_to_back(sid),
                        _ => {}
                    }
                }
            } else if let Some(cid) = self.selected_connector_id {
                    let page = self.doc.current_page_mut();
                    if let Some(conn) = page.connectors.iter_mut().find(|c| c.id == cid) {
                        ui.label(egui::RichText::new("Johdin / Yhdysviiva").strong());
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
                    ui.label(egui::RichText::new("Sivun asetukset").strong());
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

                ui.add_space(10.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.heading("Tasot (Layers)");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button("⬆ Tekstit päällimmäiseksi")
                            .on_hover_text("Tuo 'Mitat & Tekstit' -taso heti ylimmäksi")
                            .clicked()
                        {
                            self.push_undo();
                            self.doc.current_page_mut().bring_layer_to_top("Mitat & Tekstit");
                        }
                    });
                });
                ui.label(
                    egui::RichText::new("Ylin taso piirretään päällimmäiseksi. Käytä ▲/▼ -painikkeita järjestyksen vaihtoon.")
                        .small()
                        .weak(),
                );
                ui.add_space(4.0);

                let mut move_up_idx: Option<usize> = None;
                let mut move_down_idx: Option<usize> = None;

                {
                    let page = self.doc.current_page_mut();
                    let num_layers = page.layers.len();

                    for (idx, layer) in page.layers.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut layer.visible, "").on_hover_text("Näytä / piilota taso");
                            let mut c = Color32::from_rgba_unmultiplied(
                                layer.color[0],
                                layer.color[1],
                                layer.color[2],
                                255,
                            );
                            if ui.color_edit_button_srgba(&mut c).changed() {
                                layer.color = c.to_srgba_unmultiplied();
                            }
                            ui.label(egui::RichText::new(&layer.name).strong());

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if idx + 1 < num_layers {
                                    if ui
                                        .small_button("▼")
                                        .on_hover_text("Siirrä taso alemmas (taustalle)")
                                        .clicked()
                                    {
                                        move_down_idx = Some(idx);
                                    }
                                }
                                if idx > 0 {
                                    if ui
                                        .small_button("▲")
                                        .on_hover_text("Siirrä taso ylemmäs (etualalle)")
                                        .clicked()
                                    {
                                        move_up_idx = Some(idx);
                                    }
                                }
                            });
                        });
                    }
                }

                if let Some(idx) = move_up_idx {
                    self.push_undo();
                    self.doc.current_page_mut().move_layer_up(idx);
                } else if let Some(idx) = move_down_idx {
                    self.push_undo();
                    self.doc.current_page_mut().move_layer_down(idx);
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
                self.canvas_origin = origin;

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

                // Render shapes & connectors strictly by layer z-order (from bottommost layer up to topmost)
                let page_snapshot = self.doc.current_page().clone();
                let mut hovered_port: Option<(u64, usize, Point)> = None;
                let mouse_pos = response.hover_pos().or_else(|| ctx.pointer_interact_pos());
                let mouse_mm = mouse_pos.map(|mp| self.screen_to_page(origin, mp));

                let jumps = if self.doc.show_line_jumps {
                    find_wire_jumps(&page_snapshot.connectors)
                } else {
                    Vec::new()
                };

                let known_layers: Vec<&str> =
                    page_snapshot.layers.iter().map(|l| l.name.as_str()).collect();

                // Helper to render a shape and track port hovers
                let mut render_shape = |shape: &DiagramShape| {
                    let is_selected = self.is_shape_selected(shape.id);
                    self.draw_shape_on_canvas(&painter, origin, &page_snapshot, shape, is_selected);

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
                };

                // 1. Draw any orphan shapes / connectors whose layer is not in page.layers
                for shape in &page_snapshot.shapes {
                    if shape.is_room_area && !known_layers.contains(&shape.layer.as_str()) {
                        render_shape(shape);
                    }
                }
                for shape in &page_snapshot.shapes {
                    if !shape.is_room_area && !known_layers.contains(&shape.layer.as_str()) {
                        render_shape(shape);
                    }
                }
                for conn in &page_snapshot.connectors {
                    if !known_layers.contains(&conn.layer.as_str()) {
                        let is_sel = self.selected_connector_id == Some(conn.id);
                        self.draw_connector_on_canvas(&painter, origin, conn, &jumps, is_sel);
                    }
                }

                // 2. Draw layers from bottom (last index) to top (index 0)
                for layer in page_snapshot.layers.iter().rev() {
                    if !layer.visible {
                        continue;
                    }
                    // Pass 0: Room areas on this layer
                    for shape in &page_snapshot.shapes {
                        if shape.is_room_area && shape.layer == layer.name {
                            render_shape(shape);
                        }
                    }
                    // Pass 1: Regular shapes on this layer (in shapes vector order)
                    for shape in &page_snapshot.shapes {
                        if !shape.is_room_area && shape.layer == layer.name {
                            render_shape(shape);
                        }
                    }
                    // Pass 2: Connectors on this layer
                    for conn in &page_snapshot.connectors {
                        if conn.layer == layer.name {
                            let is_sel = self.selected_connector_id == Some(conn.id);
                            self.draw_connector_on_canvas(&painter, origin, conn, &jumps, is_sel);
                        }
                    }
                }

                // 3. Foreground Text Pass: Draw all connector labels and shape captions ON TOP of wires
                // so no wire ever strikes through a label!
                for conn in &page_snapshot.connectors {
                    let is_vis = page_snapshot
                        .layers
                        .iter()
                        .find(|l| l.name == conn.layer)
                        .map(|l| l.visible)
                        .unwrap_or(true);
                    if is_vis {
                        let is_sel = self.selected_connector_id == Some(conn.id);
                        self.draw_connector_label_on_canvas(
                            &painter,
                            origin,
                            &page_snapshot,
                            conn,
                            is_sel,
                        );
                    }
                }
                for shape in &page_snapshot.shapes {
                    let is_vis = page_snapshot
                        .layers
                        .iter()
                        .find(|l| l.name == shape.layer)
                        .map(|l| l.visible)
                        .unwrap_or(true);
                    if is_vis {
                        let is_selected = self.is_shape_selected(shape.id);
                        self.draw_shape_caption_on_canvas(
                            &painter,
                            origin,
                            &page_snapshot,
                            shape,
                            is_selected,
                        );
                    }
                }

                // Draw active wire preview while dragging a new connection (with full obstacle avoidance)
                if let (Some((from_s, from_p, start_mm)), Some(cur_mm)) = (self.wiring_from, mouse_mm) {
                    let (to_s, to_p, target_mm) = hovered_port
                        .map(|(s, p, pt)| (Some(s), p, pt))
                        .unwrap_or((None, 0, cur_mm));
                    let mut start_dir = (0.0, 0.0);
                    if let Some(fid) = from_s {
                        if let Some(sh) = page_snapshot.shapes.iter().find(|s| s.id == fid) {
                            let (_, d) = sh.port_world_pos_and_dir(from_p);
                            start_dir = d;
                        }
                    }
                    let mut end_dir = (0.0, 0.0);
                    if let Some(tid) = to_s {
                        if let Some(sh) = page_snapshot.shapes.iter().find(|s| s.id == tid) {
                            let (_, d) = sh.port_world_pos_and_dir(to_p);
                            end_dir = d;
                        }
                    }
                    let temp_conn = DiagramConnector {
                        id: 0,
                        from_shape: from_s,
                        from_port: from_p,
                        from_pos: start_mm,
                        to_shape: to_s,
                        to_port: to_p,
                        to_pos: target_mm,
                        style: ConnectorStyle::Orthogonal,
                        start_arrow: ArrowType::None,
                        end_arrow: ArrowType::None,
                        label: String::new(),
                        stroke_color: [34, 197, 94, 255],
                        stroke_width: 2.2,
                        dashed: false,
                        layer: String::new(),
                        waypoints: Vec::new(),
                    };
                    let preview_pts = compute_connector_path(
                        &temp_conn,
                        start_dir,
                        end_dir,
                        &page_snapshot.shapes,
                    );
                    let screen_pts: Vec<Pos2> = preview_pts
                        .into_iter()
                        .map(|p| self.page_to_screen(origin, p))
                        .collect();
                    let preview_stroke = Stroke::new(2.2_f32, Color32::from_rgb(34, 197, 94));
                    painter.add(Shape::line(screen_pts, preview_stroke));
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

                // Draw group bounding box around all selected shapes if multiple shapes are selected
                let sel_ids = self.selected_shape_ids();
                if sel_ids.len() > 1 {
                    let mut min_x = f64::INFINITY;
                    let mut max_x = f64::NEG_INFINITY;
                    let mut min_y = f64::INFINITY;
                    let mut max_y = f64::NEG_INFINITY;
                    for s in &page_snapshot.shapes {
                        if sel_ids.contains(&s.id) {
                            let bb = s.bounding_box();
                            min_x = min_x.min(bb.min_x);
                            max_x = max_x.max(bb.max_x);
                            min_y = min_y.min(bb.min_y);
                            max_y = max_y.max(bb.max_y);
                        }
                    }
                    if min_x.is_finite() && max_x > min_x {
                        let p_min = self.page_to_screen(origin, Point::new(min_x, min_y));
                        let p_max = self.page_to_screen(origin, Point::new(max_x, max_y));
                        let group_rect = Rect::from_two_pos(p_min, p_max).expand(4.0);
                        painter.rect_stroke(
                            group_rect,
                            CornerRadius::ZERO,
                            Stroke::new(1.2_f32, Color32::from_rgb(59, 130, 246)),
                            StrokeKind::Middle,
                        );
                    }
                }

                // Draw active rubber-band / marquee box selection
                if let (Some(m_start), Some(m_cur)) = (self.marquee_drag_start_mm, self.marquee_current_mm) {
                    let p1 = self.page_to_screen(origin, m_start);
                    let p2 = self.page_to_screen(origin, m_cur);
                    let m_rect = Rect::from_two_pos(p1, p2);
                    painter.rect_filled(
                        m_rect,
                        CornerRadius::ZERO,
                        Color32::from_rgba_unmultiplied(59, 130, 246, 35),
                    );
                    painter.rect_stroke(
                        m_rect,
                        CornerRadius::ZERO,
                        Stroke::new(1.2_f32, Color32::from_rgb(59, 130, 246)),
                        StrokeKind::Middle,
                    );
                }

                // Handle Canvas Mouse Interactions
                self.handle_canvas_interactions(&response, origin, hovered_port, mouse_mm);

                // Right-click Context Menu for element & canvas actions
                response.context_menu(|ui| {
                    if let Some(sid) = self.selected_shape_id {
                        ui.label(egui::RichText::new("Elementin toiminnot").strong());
                        ui.separator();
                        if ui.button("Muokkaa tekstiä...").clicked() {
                            self.editing_shape_id = Some(sid);
                            self.focus_text_edit = true;
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui
                            .button("▲▲ Tuo etualalle (Bring to Front)      Cmd+Shift+]")
                            .clicked()
                        {
                            self.push_undo();
                            self.doc.current_page_mut().bring_to_front(sid);
                            ui.close_menu();
                        }
                        if ui
                            .button("▲ Siirrä eteenpäin (Bring Forward)  Cmd+]")
                            .clicked()
                        {
                            self.push_undo();
                            self.doc.current_page_mut().bring_forward(sid);
                            ui.close_menu();
                        }
                        if ui
                            .button("▼ Siirrä taaksepäin (Send Backward) Cmd+[")
                            .clicked()
                        {
                            self.push_undo();
                            self.doc.current_page_mut().send_backward(sid);
                            ui.close_menu();
                        }
                        if ui
                            .button("▼▼ Vie taustalle (Send to Back)        Cmd+Shift+[")
                            .clicked()
                        {
                            self.push_undo();
                            self.doc.current_page_mut().send_to_back(sid);
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Kahdenna (Duplicate)                 Cmd+D").clicked() {
                            self.duplicate_selected();
                            ui.close_menu();
                        }
                        if ui.button("Kierrä 90°                           R").clicked() {
                            self.rotate_selected(90.0);
                            ui.close_menu();
                        }
                        if ui.button("Poista                              Del").clicked() {
                            self.delete_selected();
                            ui.close_menu();
                        }
                    } else {
                        ui.label(egui::RichText::new("Kaavion toiminnot").strong());
                        ui.separator();
                        if ui.button("Lisää tekstilaatikko tähän").clicked() {
                            if let Some(mp) = mouse_mm {
                                self.push_undo();
                                let id = self.doc.alloc_id();
                                let count = self.doc.current_page().shapes.len() + 1;
                                if let Some(mut sh) = instantiate_shape("note_textbox", id, mp.x, mp.y, count) {
                                    sh.label.clear();
                                    self.doc.current_page_mut().shapes.push(sh);
                                    self.selected_shape_id = Some(id);
                                    self.editing_shape_id = Some(id);
                                    self.focus_text_edit = true;
                                }
                            }
                            ui.close_menu();
                        }
                        if ui.button("Lisää huomautuskupla (Callout)").clicked() {
                            if let Some(mp) = mouse_mm {
                                self.push_undo();
                                let id = self.doc.alloc_id();
                                let count = self.doc.current_page().shapes.len() + 1;
                                if let Some(sh) = instantiate_shape("note_callout", id, mp.x, mp.y, count) {
                                    self.doc.current_page_mut().shapes.push(sh);
                                    self.selected_shape_id = Some(id);
                                    self.editing_shape_id = Some(id);
                                    self.focus_text_edit = true;
                                }
                            }
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("⬆ Tuo tekstit päällimmäiseksi").clicked() {
                            self.push_undo();
                            self.doc.current_page_mut().bring_layer_to_top("Mitat & Tekstit");
                            ui.close_menu();
                        }
                        if ui.button("Nollaa zoomi (100%)").clicked() {
                            self.zoom_px_per_mm = 3.78;
                            self.pan_offset = Vec2::ZERO;
                            ui.close_menu();
                        }
                    }
                });
            });
    }

    fn render_text_edit_dialog(&mut self, ctx: &egui::Context) {
        if let Some(edit_id) = self.editing_shape_id {
            let shape_info = self
                .doc
                .current_page()
                .shapes
                .iter()
                .find(|s| s.id == edit_id)
                .map(|s| (s.name.clone(), Point::new(s.x, s.y)));

            let mut close_editor = false;
            if let Some((name, center_pt)) = shape_info {
                let center_sp = self.page_to_screen(self.canvas_origin, center_pt);
                let title = format!("Muokkaa tekstiä: {}", name);
                let mut is_open = true;

                let page = self.doc.current_page_mut();
                if let Some(shape) = page.shapes.iter_mut().find(|s| s.id == edit_id) {
                    let is_note = shape.stencil_id.starts_with("note_")
                        || shape.stencil_id == "note_textbox"
                        || shape.stencil_id == "note_callout"
                        || shape.name.to_lowercase().contains("teksti");
                    let (dlg_w, rows) = if is_note { (380.0, 6) } else { (300.0, 3) };

                    egui::Window::new(title)
                        .open(&mut is_open)
                        .id(egui::Id::new("shape_text_edit_modal_dialog"))
                        .default_pos(center_sp + Vec2::new(15.0, -25.0))
                        .collapsible(false)
                        .resizable(true)
                        .default_width(dlg_w)
                        .order(egui::Order::Foreground)
                        .show(ctx, |ui| {
                            ui.add_space(2.0);
                            ui.label(
                                egui::RichText::new("Kirjoita laatikon tai muodon teksti:")
                                    .small()
                                    .color(Color32::from_rgb(100, 116, 139)),
                            );

                            let te = egui::TextEdit::multiline(&mut shape.label)
                                .id_salt("dialog_shape_label_field")
                                .desired_width(f32::INFINITY)
                                .desired_rows(rows)
                                .cursor_at_end(true)
                                .hint_text("Kirjoita teksti...");
                            let te_res = ui.add(te);

                            if self.focus_text_edit {
                                te_res.request_focus();
                                self.focus_text_edit = false;
                            }

                            // Shortcuts:
                            // Escape = Valmis / Sulje
                            // For notes: Cmd/Ctrl+Enter = Valmis; Enter = Uusi rivi
                            // For other shapes: Enter = Valmis (unless Shift is held)
                            if ui.input(|i| i.key_pressed(Key::Escape)) {
                                close_editor = true;
                            } else if is_note {
                                if ui.input(|i| i.key_pressed(Key::Enter) && (i.modifiers.command || i.modifiers.ctrl)) {
                                    shape.label = shape.label.trim_end_matches('\n').to_string();
                                    close_editor = true;
                                }
                            } else if ui.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.shift) {
                                shape.label = shape.label.trim_end_matches('\n').to_string();
                                close_editor = true;
                            }

                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                if ui
                                    .button(egui::RichText::new(" Valmis (OK) ").strong())
                                    .clicked()
                                {
                                    close_editor = true;
                                }
                                if ui.button("Tyhjennä").clicked() {
                                    shape.label.clear();
                                }
                                let hint = if is_note {
                                    "Cmd+Enter = Valmis • Enter = Rivi"
                                } else {
                                    "Enter = Valmis • Shift+Enter = Rivi"
                                };
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(
                                        egui::RichText::new(hint)
                                            .weak()
                                            .small(),
                                    );
                                });
                            });
                        });
                }

                if !is_open {
                    close_editor = true;
                }
            } else {
                close_editor = true;
            }

            if close_editor {
                self.editing_shape_id = None;
                self.focus_text_edit = false;
            }
        }
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

        // Right-click: select shape at pointer before context menu opens
        if response.secondary_clicked() {
            if let Some(sh) = self.doc.current_page().find_shape_at(cur_mm, 1.5) {
                if !self.is_shape_selected(sh.id) {
                    self.select_single_shape(sh.id);
                }
            } else {
                self.clear_selection();
            }
        }

        // 1. Place armed stencil on click or place text box
        if response.clicked() {
            if self.active_tool == ActiveTool::DrawText {
                self.push_undo();
                let new_id = self.doc.alloc_id();
                let count = self.doc.current_page().shapes.len() + 1;
                let snapped = self.snap_point(cur_mm);
                if let Some(mut shape) = instantiate_shape("note_textbox", new_id, snapped.x, snapped.y, count) {
                    shape.width = 60.0;
                    shape.height = 25.0;
                    shape.label.clear();
                    self.doc.current_page_mut().shapes.push(shape);
                    self.select_single_shape(new_id);
                    self.editing_shape_id = Some(new_id);
                    self.focus_text_edit = true;
                    self.active_tool = ActiveTool::Select;
                }
                return;
            }

            if let Some(st_id) = self.pending_stencil_id.clone() {
                self.push_undo();
                let new_id = self.doc.alloc_id();
                let count = self.doc.current_page().shapes.len() + 1;
                let snapped = self.snap_point(cur_mm);
                if let Some(st) = self.stencils.iter().find(|s| s.id == st_id) {
                    let shape = st.instantiate(new_id, snapped.x, snapped.y, count);
                    self.doc.current_page_mut().shapes.push(shape);
                    self.select_single_shape(new_id);
                } else if let Some(shape) =
                    instantiate_shape(&st_id, new_id, snapped.x, snapped.y, count)
                {
                    self.doc.current_page_mut().shapes.push(shape);
                    self.select_single_shape(new_id);
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
                ActiveTool::DrawWall
                    | ActiveTool::DrawRoom
                    | ActiveTool::DrawDimension
                    | ActiveTool::DrawText
            ) {
                self.creation_drag_start_mm = Some(self.snap_point(cur_mm));
                return;
            }

            // Check if dragging a resize handle of the currently selected shape (only when 1 shape selected)
            if self.selected_shape_ids.len() <= 1 {
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
            }

            // Hit-test shapes respecting layer z-order
            let hit_shape = self.doc.current_page().find_shape_at(cur_mm, 1.5).cloned();

            if let Some(sh) = hit_shape {
                self.push_undo();
                let is_modifier = response.ctx.input(|i| i.modifiers.shift || i.modifiers.command);
                if self.is_shape_selected(sh.id) {
                    // Already in selection -> keep group selection intact for dragging as a group!
                } else if is_modifier {
                    self.toggle_shape_selection(sh.id);
                } else {
                    self.select_single_shape(sh.id);
                }

                self.selected_connector_id = None;
                self.dragging_shape_id = Some(sh.id);
                self.active_resize_handle = None;
                self.drag_start_mouse_mm = cur_mm;
                self.drag_start_shape_pos = Point::new(sh.x, sh.y);
                self.drag_start_shape_size = (sh.width, sh.height);

                let sids = self.selected_shape_ids();
                self.drag_start_group_positions = self.doc.current_page().shapes
                    .iter()
                    .filter(|s| sids.contains(&s.id))
                    .map(|s| (s.id, Point::new(s.x, s.y)))
                    .collect();
            } else if self.active_tool == ActiveTool::Select {
                // Dragging on empty canvas with Select tool starts rubber-band marquee selection!
                let is_modifier = response.ctx.input(|i| i.modifiers.shift || i.modifiers.command);
                if !is_modifier {
                    self.clear_selection();
                }
                self.marquee_drag_start_mm = Some(cur_mm);
                self.marquee_current_mm = Some(cur_mm);
            }
        }

        // 4. Ongoing drag (moving shapes as group, resizing shape, or rubber-band selection)
        if response.dragged() {
            if self.marquee_drag_start_mm.is_some() {
                self.marquee_current_mm = Some(cur_mm);
            } else if let Some(sid) = self.dragging_shape_id {
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
                    // Move the entire selected group together!
                    let snapped_dx = if self.doc.snap_to_grid {
                        let step = (self.doc.current_page().grid_spacing_mm * 0.5).max(1.0);
                        (dx / step).round() * step
                    } else {
                        dx
                    };
                    let snapped_dy = if self.doc.snap_to_grid {
                        let step = (self.doc.current_page().grid_spacing_mm * 0.5).max(1.0);
                        (dy / step).round() * step
                    } else {
                        dy
                    };

                    let page = self.doc.current_page_mut();
                    for (gsid, orig_pos) in &self.drag_start_group_positions {
                        if let Some(s) = page.shapes.iter_mut().find(|s| s.id == *gsid) {
                            s.x = orig_pos.x + snapped_dx;
                            s.y = orig_pos.y + snapped_dy;
                        }
                    }
                    update_all_connectors(&mut page.connectors, &page.shapes);
                }
            }
        }

        // 5. End of drag
        if response.drag_stopped() {
            // Finish rubber-band / marquee box selection
            if let Some(start_mm) = self.marquee_drag_start_mm.take() {
                self.marquee_current_mm = None;
                let end_mm = cur_mm;
                let min_x = start_mm.x.min(end_mm.x);
                let max_x = start_mm.x.max(end_mm.x);
                let min_y = start_mm.y.min(end_mm.y);
                let max_y = start_mm.y.max(end_mm.y);

                if (max_x - min_x) > 1.2 || (max_y - min_y) > 1.2 {
                    let page = self.doc.current_page();
                    let mut hit_ids = Vec::new();
                    for shape in &page.shapes {
                        let bb = shape.bounding_box();
                        if bb.max_x >= min_x && bb.min_x <= max_x && bb.max_y >= min_y && bb.min_y <= max_y {
                            hit_ids.push(shape.id);
                        }
                    }
                    let is_modifier = response.ctx.input(|i| i.modifiers.shift || i.modifiers.command);
                    if is_modifier {
                        for id in hit_ids {
                            if !self.selected_shape_ids.contains(&id) {
                                self.selected_shape_ids.push(id);
                            }
                        }
                        self.selected_shape_id = self.selected_shape_ids.last().copied();
                    } else {
                        self.select_shapes(hit_ids);
                    }
                    if !self.selected_shape_ids.is_empty() {
                        self.status_msg = format!("Valittu {} komponenttia.", self.selected_shape_ids.len());
                    }
                }
            }

            self.dragging_shape_id = None;
            self.drag_start_group_positions.clear();
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

                    // Choose smart IEC wire color based on connected port / component type
                    let mut wire_color = [220, 38, 38, 255]; // Default +24V / Phase Red
                    let mut port_hints = String::new();
                    for (sid_opt, pidx) in [(from_s, from_p), (to_s, to_p)] {
                        if let Some(sid) = sid_opt {
                            if let Some(sh) = page.shapes.iter().find(|s| s.id == sid) {
                                port_hints.push_str(&sh.stencil_id.to_lowercase());
                                port_hints.push(' ');
                                port_hints.push_str(&sh.label.to_lowercase());
                                port_hints.push(' ');
                                if let Some(port) = sh.ports.get(pidx) {
                                    port_hints.push_str(&port.name.to_lowercase());
                                    port_hints.push(' ');
                                }
                            }
                        }
                    }
                    if port_hints.contains("elec_pe")
                        || port_hints.contains("iec_pe")
                        || port_hints.contains(" pe")
                    {
                        wire_color = [22, 163, 74, 255]; // Protective Earth Green
                    } else if port_hints.contains("gnd")
                        || port_hints.contains("0v")
                        || port_hints.contains("miinus")
                        || port_hints.contains("a2")
                    {
                        wire_color = [30, 41, 59, 255]; // 0V / GND Dark Slate
                    } else if port_hints.contains("q1")
                        || port_hints.contains("q2")
                        || port_hints.contains("q3")
                        || port_hints.contains("do1")
                        || port_hints.contains("do2")
                    {
                        wire_color = [37, 99, 235, 255]; // PLC Control Output Blue
                    }

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
                        stroke_color: wire_color,
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

            // Finish Wall / Room / Dimension / Text creation drag
            if let Some(start_mm) = self.creation_drag_start_mm.take() {
                let end_mm = self.snap_point(cur_mm);
                let cx = (start_mm.x + end_mm.x) * 0.5;
                let cy = (start_mm.y + end_mm.y) * 0.5;
                let drag_dist = start_mm.distance_to(end_mm);
                let w = (end_mm.x - start_mm.x).abs();
                let h = (end_mm.y - start_mm.y).abs();

                let st_id = match self.active_tool {
                    ActiveTool::DrawWall => Some("fp_wall_ext"),
                    ActiveTool::DrawRoom => Some("fp_room"),
                    ActiveTool::DrawDimension => Some("fp_dimension"),
                    ActiveTool::DrawText => Some("note_textbox"),
                    _ => None,
                };
                if let Some(stid) = st_id {
                    self.push_undo();
                    let id = self.doc.alloc_id();
                    let count = self.doc.current_page().shapes.len() + 1;
                    if let Some(mut sh) = instantiate_shape(stid, id, cx, cy, count) {
                        if self.active_tool == ActiveTool::DrawWall {
                            if w >= h {
                                sh.width = w.max(6.0);
                                sh.height = 4.0;
                            } else {
                                sh.width = w.max(6.0);
                                sh.height = 4.0;
                                sh.rotation_deg = 90.0;
                            }
                        } else if self.active_tool == ActiveTool::DrawDimension {
                            sh.width = w.max(h).max(6.0);
                            sh.height = 8.0;
                            if h > w {
                                sh.rotation_deg = 90.0;
                            }
                        } else if self.active_tool == ActiveTool::DrawText {
                            if drag_dist >= 4.0 {
                                sh.width = w.max(20.0);
                                sh.height = h.max(12.0);
                            } else {
                                sh.width = 60.0;
                                sh.height = 25.0;
                                sh.x = start_mm.x;
                                sh.y = start_mm.y;
                            }
                            sh.label.clear();
                            self.editing_shape_id = Some(id);
                            self.focus_text_edit = true;
                        } else {
                            sh.width = w.max(6.0);
                            sh.height = h.max(4.0);
                        }
                        self.doc.current_page_mut().shapes.push(sh);
                        self.select_single_shape(id);
                        self.active_tool = ActiveTool::Select;
                    }
                }
            }
        }

        // Double-click to directly edit text inside a shape
        if response.double_clicked() && self.pending_stencil_id.is_none() {
            let hit_id = self.doc.current_page().find_shape_at(cur_mm, 1.5).map(|s| s.id);
            if let Some(id) = hit_id {
                self.select_single_shape(id);
                self.editing_shape_id = Some(id);
                self.focus_text_edit = true;
                return;
            }
        }

        // 6. Simple click selection
        if response.clicked() && self.pending_stencil_id.is_none() {
            if self.editing_shape_id.is_some() {
                self.editing_shape_id = None;
                self.focus_text_edit = false;
            }
            let is_modifier = response.ctx.input(|i| i.modifiers.shift || i.modifiers.command);

            // Hit-test shapes respecting layer z-order first (so text boxes / shapes above wires get clicked)
            let hit_shape_id = self.doc.current_page().find_shape_at(cur_mm, 1.5).map(|s| s.id);
            if let Some(id) = hit_shape_id {
                if is_modifier {
                    self.toggle_shape_selection(id);
                } else {
                    self.select_single_shape(id);
                }
                return;
            }

            // Hit-test connectors if no shape was clicked
            let hit_conn_id = self.doc.current_page()
                .connectors
                .iter()
                .find(|c| connector_hit_test(c, cur_mm, 2.5))
                .map(|c| c.id);

            if let Some(cid) = hit_conn_id {
                self.clear_selection();
                self.selected_connector_id = Some(cid);
                return;
            }

            if !is_modifier {
                self.clear_selection();
            }
        }

        let _ = origin;
    }

    fn draw_shape_on_canvas(
        &self,
        painter: &egui::Painter,
        origin: Pos2,
        _page: &DiagramPage,
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
                    let steps = 20;
                    let mut prev: Option<Pos2> = None;
                    for i in 0..=steps {
                        let t = i as f64 / steps as f64;
                        let ang = (start_deg + sweep_deg * t) * PI / 180.0;
                        let lx = cx + rx * ang.cos();
                        let ly = cy + ry * ang.sin();
                        let cur = self.page_to_screen(origin, shape.local_to_page(lx, ly));
                        if let Some(p0) = prev {
                            painter.line_segment([p0, cur], stroke);
                        }
                        prev = Some(cur);
                    }
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

            // Only draw individual resize handles when exactly one shape is selected
            if self.selected_shape_ids.len() <= 1 {
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
    }

    fn draw_shape_caption_on_canvas(
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

        // Draw component label / value or live architectural room area / dimension
        let caption = shape_display_caption(page, shape);
        if !caption.is_empty() {
            let eff_pos = shape.effective_text_position();
            let label_pt =
                resolve_clean_label_position(shape, &page.connectors, caption.chars().count());
            let sp = self.page_to_screen(origin, label_pt);

            let (text_col, font_px) = if eff_pos == TextPosition::Center {
                let fill_lum = (shape.fill_color[0] as f32 * 0.299
                    + shape.fill_color[1] as f32 * 0.587
                    + shape.fill_color[2] as f32 * 0.114)
                    / 255.0;
                let has_fill = shape.fill_color[3] > 40;
                let col = if has_fill && fill_lum < 0.45 {
                    Color32::WHITE
                } else if has_fill && fill_lum > 0.82 {
                    // Dark navy on light pastel fills
                    Color32::from_rgb(15, 23, 42)
                } else if shape.fill_color[3] == 0 && shape.stroke_color[3] == 0 {
                    Color32::from_rgb(15, 23, 42)
                } else {
                    stroke_col
                };

                let font_size = if shape.font_size > 0.0 {
                    (shape.font_size * 0.3528 * self.zoom_px_per_mm).clamp(6.0, 150.0)
                } else if shape.stencil_id.starts_with("note_")
                    || shape.stencil_id == "note_textbox"
                    || shape.name.to_lowercase().contains("teksti")
                {
                    (11.0 * 0.3528 * self.zoom_px_per_mm).clamp(9.0, 24.0)
                } else {
                    let line_count = caption.lines().count().max(1) as f32;
                    let h_px = (shape.height as f32) * self.zoom_px_per_mm;
                    (h_px / (line_count + 0.9)).clamp(9.0, 24.0)
                };
                (col, font_size)
            } else {
                let font_size = if shape.font_size > 0.0 {
                    (shape.font_size * 0.3528 * self.zoom_px_per_mm).clamp(6.0, 150.0)
                } else {
                    (3.3 * self.zoom_px_per_mm).clamp(9.5, 16.0)
                };
                (stroke_col, font_size)
            };

            let w_px = (shape.width as f32) * self.zoom_px_per_mm;
            let wrap_width = if eff_pos == TextPosition::Center {
                (w_px - 8.0).max(12.0)
            } else {
                f32::INFINITY
            };

            let eff_align = shape.effective_text_align();
            let halign = match eff_align {
                TextAlign::Left => egui::Align::Min,
                TextAlign::Center => egui::Align::Center,
                TextAlign::Right => egui::Align::Max,
                TextAlign::Auto => egui::Align::Center,
            };

            let mut job = egui::text::LayoutJob::simple(
                caption,
                FontId::proportional(font_px),
                text_col,
                wrap_width,
            );
            job.halign = halign;
            let galley = painter.layout_job(job);
            let g_size = galley.size();

            let galley_pos = match eff_pos {
                TextPosition::Center => {
                    let y = sp.y - g_size.y * 0.5;
                    let x = match eff_align {
                        TextAlign::Left => sp.x - w_px * 0.5 + 4.0 - galley.rect.min.x,
                        TextAlign::Center | TextAlign::Auto => sp.x - galley.rect.center().x,
                        TextAlign::Right => sp.x + w_px * 0.5 - 4.0 - galley.rect.max.x,
                    };
                    Pos2::new(x, y)
                }
                TextPosition::Top => {
                    if (label_pt.y - shape.y).abs() < 1.0 {
                        // Label was nudged horizontally to the side of the component
                        Pos2::new(sp.x - galley.rect.center().x, sp.y - g_size.y * 0.5)
                    } else if label_pt.y > shape.y {
                        // Label was flipped below the component
                        Pos2::new(sp.x - galley.rect.center().x, sp.y)
                    } else {
                        Pos2::new(sp.x - galley.rect.center().x, sp.y - g_size.y)
                    }
                }
                TextPosition::Bottom => {
                    if (label_pt.y - shape.y).abs() < 1.0 {
                        Pos2::new(sp.x - galley.rect.center().x, sp.y - g_size.y * 0.5)
                    } else if label_pt.y < shape.y {
                        Pos2::new(sp.x - galley.rect.center().x, sp.y - g_size.y)
                    } else {
                        Pos2::new(sp.x - galley.rect.center().x, sp.y)
                    }
                }
                TextPosition::Hidden | TextPosition::Auto => {
                    Pos2::new(sp.x - galley.rect.center().x, sp.y - g_size.y * 0.5)
                }
            };

            // Draw clean paper halo pill behind external component labels so wires never bleed through text
            if eff_pos != TextPosition::Center || shape.is_dimension {
                let bg_rect = Rect::from_min_size(
                    Pos2::new(galley_pos.x + galley.rect.min.x, galley_pos.y + galley.rect.min.y),
                    g_size,
                )
                .expand2(Vec2::new(3.0, 1.5));
                painter.rect_filled(
                    bg_rect,
                    CornerRadius::same(3),
                    Color32::from_rgba_unmultiplied(255, 255, 255, 236),
                );
            }

            painter.galley(galley_pos, galley, text_col);
        } else if shape.stencil_id == "note_textbox"
            && self.editing_shape_id != Some(shape.id)
            && is_selected
        {
            // Subtle placeholder for empty selected textbox
            let sp = self.page_to_screen(origin, Point::new(shape.x, shape.y));
            painter.text(
                sp,
                egui::Align2::CENTER_CENTER,
                "Kirjoita tekstiä...",
                FontId::proportional((10.0 * 0.3528 * self.zoom_px_per_mm).clamp(9.0, 14.0)),
                Color32::from_rgba_unmultiplied(148, 163, 184, 180),
            );
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
    }

    fn draw_connector_label_on_canvas(
        &self,
        painter: &egui::Painter,
        origin: Pos2,
        page: &DiagramPage,
        conn: &DiagramConnector,
        is_selected: bool,
    ) {
        if conn.label.is_empty() || conn.waypoints.len() < 2 {
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
        let font_px = (3.1 * self.zoom_px_per_mm).clamp(9.5, 14.5);
        let galley = painter.layout_no_wrap(
            conn.label.clone(),
            FontId::proportional(font_px),
            col,
        );
        let g_size = galley.size();
        let half_w_mm = ((g_size.x * 0.5) / self.zoom_px_per_mm.max(1.0)) as f64 + 1.0;

        let lpt = connector_label_position_smart(
            &conn.waypoints,
            &page.connectors,
            &page.shapes,
            half_w_mm,
        );
        let sp = self.page_to_screen(origin, lpt);
        let pos = Pos2::new(sp.x - g_size.x * 0.5, sp.y - g_size.y * 0.5);
        let bg_rect = Rect::from_min_size(pos, g_size).expand2(Vec2::new(3.0, 1.5));
        painter.rect_filled(
            bg_rect,
            CornerRadius::same(3),
            Color32::from_rgba_unmultiplied(255, 255, 255, 236),
        );
        painter.galley(pos, galley, col);
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
            PrimOp::Arc {
                cx,
                cy,
                rx,
                ry,
                start_deg,
                sweep_deg,
            } => {
                let steps = 16;
                let mut prev: Option<Pos2> = None;
                for i in 0..=steps {
                    let t = i as f64 / steps as f64;
                    let ang = (start_deg + sweep_deg * t) * PI / 180.0;
                    let lx = cx + rx * ang.cos();
                    let ly = cy + ry * ang.sin();
                    let cur = map(lx, ly);
                    if let Some(p0) = prev {
                        painter.line_segment([p0, cur], stroke);
                    }
                    prev = Some(cur);
                }
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
