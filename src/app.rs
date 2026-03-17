use crate::syntax::SyntaxHighlighter;
use crate::theme::GtkTheme;
use egui_phosphor::regular as icons;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, PartialEq, Eq)]
enum FocusPane {
    Files,
    Viewer,
}

#[derive(Clone)]
enum DirtyNavTarget {
    FocusFiles,
    OpenFile(PathBuf),
    EnterDirectory(PathBuf),
}

#[derive(Clone, Debug)]
enum FileRowKind {
    Parent,
    Directory,
    File,
}

#[derive(Clone, Debug)]
struct FileRow {
    path: PathBuf,
    name: String,
    kind: FileRowKind,
}

impl FileRow {
    fn is_file(&self) -> bool {
        matches!(self.kind, FileRowKind::File)
    }

    fn is_directory_like(&self) -> bool {
        matches!(self.kind, FileRowKind::Parent | FileRowKind::Directory)
    }

    fn display_label(&self) -> String {
        match self.kind {
            FileRowKind::Parent => "..".to_string(),
            FileRowKind::Directory => format!("{} {}", icons::FOLDER, self.name),
            FileRowKind::File => format!("{} {}", icons::FILE_TEXT, self.name),
        }
    }
}

pub struct FerritorApp {
    pub theme: GtkTheme,
    current_dir: PathBuf,
    selected_file: Option<PathBuf>,
    loaded_content: Option<String>,
    load_error: Option<String>,
    filter: String,
    last_message: Option<String>,
    nav_rows: Vec<FileRow>,
    nav_cursor: usize,
    focus_pane: FocusPane,
    show_help: bool,
    nav_moved: bool,
    editing: bool,
    edit_buffer: String,
    original_content: Option<String>,
    dirty_nav_target: Option<DirtyNavTarget>,
    viewer_scroll_delta: f32,
    editor_wants_focus: bool,
    show_open_dir_dialog: bool,
    open_dir_input: String,
    open_dir_input_wants_focus: bool,
}

impl FerritorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let theme = GtkTheme::load();

        let mut visuals = egui::Visuals::light();
        visuals.override_text_color = Some(theme.view_fg());
        visuals.panel_fill = theme.view_bg();
        visuals.window_fill = theme.card_bg();
        visuals.extreme_bg_color = theme.view_bg();
        visuals.faint_bg_color = theme.headerbar_bg();
        visuals.selection.bg_fill = theme.accent();
        visuals.selection.stroke = egui::Stroke::new(1.0, theme.accent_fg());
        visuals.widgets.noninteractive.bg_fill = theme.card_bg();
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, theme.card_fg());
        visuals.widgets.hovered.bg_fill = theme.shade();
        visuals.widgets.active.bg_fill = theme.accent();

        cc.egui_ctx.set_visuals(visuals);
        load_fonts(&cc.egui_ctx);

        let current_dir = std::env::current_dir()
            .ok()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."));

        let mut app = Self {
            theme,
            current_dir: current_dir.clone(),
            selected_file: None,
            loaded_content: None,
            load_error: None,
            filter: String::new(),
            last_message: None,
            nav_rows: Vec::new(),
            nav_cursor: 0,
            focus_pane: FocusPane::Files,
            show_help: false,
            nav_moved: false,
            editing: false,
            edit_buffer: String::new(),
            original_content: None,
            dirty_nav_target: None,
            viewer_scroll_delta: 0.0,
            editor_wants_focus: false,
            show_open_dir_dialog: false,
            open_dir_input: current_dir.display().to_string(),
            open_dir_input_wants_focus: false,
        };
        app.refresh_nav_rows();
        app
    }

    fn visible_rows_for_dir(dir: &Path, filter: &str) -> Vec<FileRow> {
        let mut rows = Vec::new();

        if let Some(parent) = dir.parent() {
            rows.push(FileRow {
                path: parent.to_path_buf(),
                name: "..".to_string(),
                kind: FileRowKind::Parent,
            });
        }

        let mut dirs = Vec::new();
        let mut files = Vec::new();
        let filter_lc = filter.trim().to_lowercase();

        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let file_name = entry.file_name();
                let name = file_name.to_string_lossy().to_string();

                if !filter_lc.is_empty() && !name.to_lowercase().contains(&filter_lc) {
                    continue;
                }

                match entry.file_type() {
                    Ok(ft) if ft.is_dir() => dirs.push(FileRow {
                        path,
                        name,
                        kind: FileRowKind::Directory,
                    }),
                    Ok(ft) if ft.is_file() => files.push(FileRow {
                        path,
                        name,
                        kind: FileRowKind::File,
                    }),
                    _ => {}
                }
            }
        }

        dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        rows.extend(dirs);
        rows.extend(files);
        rows
    }

    fn refresh_nav_rows(&mut self) {
        self.nav_rows = Self::visible_rows_for_dir(&self.current_dir, &self.filter);
        if self.nav_rows.is_empty() {
            self.nav_cursor = 0;
            return;
        }

        if self.nav_cursor >= self.nav_rows.len() {
            self.nav_cursor = self.nav_rows.len() - 1;
        }
    }

    fn current_row(&self) -> Option<&FileRow> {
        self.nav_rows.get(self.nav_cursor)
    }

    fn is_dirty(&self) -> bool {
        self.editing && self.original_content.as_deref() != Some(&self.edit_buffer)
    }

    fn apply_dirty_nav(&mut self, target: &DirtyNavTarget) {
        match target {
            DirtyNavTarget::FocusFiles => {
                self.focus_pane = FocusPane::Files;
            }
            DirtyNavTarget::OpenFile(path) => {
                self.open_file(path.clone());
            }
            DirtyNavTarget::EnterDirectory(path) => {
                self.open_directory(path.clone());
            }
        }
    }

    fn exit_edit_mode(&mut self, save: bool) {
        if save && self.is_dirty() {
            if let Some(path) = self.selected_file.clone() {
                match fs::write(&path, &self.edit_buffer) {
                    Ok(()) => {
                        self.loaded_content = Some(self.edit_buffer.clone());
                        self.original_content = Some(self.edit_buffer.clone());
                        let file_name = path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("file");
                        self.last_message = Some(format!("Saved '{}'", file_name));
                    }
                    Err(err) => {
                        self.last_message = Some(format!("Save error: {}", err));
                    }
                }
            }
        } else if !save {
            self.loaded_content = self.original_content.clone();
        }

        self.editing = false;
        self.edit_buffer.clear();
    }

    fn open_directory(&mut self, path: PathBuf) {
        self.current_dir = path;
        self.selected_file = None;
        self.loaded_content = None;
        self.load_error = None;
        self.editing = false;
        self.edit_buffer.clear();
        self.original_content = None;
        self.open_dir_input = self.current_dir.display().to_string();
        self.refresh_nav_rows();
        self.last_message = Some(format!("Opened {}", self.current_dir.display()));
    }

    fn open_parent_directory(&mut self) {
        if let Some(parent) = self.current_dir.parent() {
            self.open_directory(parent.to_path_buf());
        }
    }

    fn open_file(&mut self, path: PathBuf) {
        if let Some(idx) = self.nav_rows.iter().position(|row| row.path == path) {
            self.nav_cursor = idx;
        }
        self.selected_file = Some(path.clone());
        self.load_error = None;
        self.editing = false;
        self.edit_buffer.clear();

        match fs::read_to_string(&path) {
            Ok(content) => {
                self.loaded_content = Some(content.clone());
                self.original_content = Some(content);
                self.focus_pane = FocusPane::Viewer;
            }
            Err(err) => {
                self.loaded_content = None;
                self.original_content = None;
                self.load_error = Some(format!("Cannot open as UTF-8 text: {}", err));
                self.focus_pane = FocusPane::Viewer;
            }
        }

        self.refresh_nav_rows();
    }

    fn enter_edit_mode(&mut self) {
        if let Some(content) = &self.loaded_content {
            self.edit_buffer = content.clone();
            self.original_content = Some(content.clone());
            self.editing = true;
            self.editor_wants_focus = true;
        }
    }

    fn try_open_row(&mut self, row: &FileRow) {
        if self.is_dirty() {
            self.dirty_nav_target = Some(match row.kind {
                FileRowKind::File => DirtyNavTarget::OpenFile(row.path.clone()),
                FileRowKind::Parent | FileRowKind::Directory => {
                    DirtyNavTarget::EnterDirectory(row.path.clone())
                }
            });
            return;
        }

        match row.kind {
            FileRowKind::File => self.open_file(row.path.clone()),
            FileRowKind::Parent | FileRowKind::Directory => self.open_directory(row.path.clone()),
        }
    }

    fn render_open_dir_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_open_dir_dialog {
            return;
        }

        let mut open = true;
        egui::Window::new("Open Directory")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("Enter a directory path");
                ui.add_space(8.0);
                let input_id = egui::Id::new("open_dir_input");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.open_dir_input)
                        .id(input_id)
                        .desired_width(480.0),
                );
                if self.open_dir_input_wants_focus {
                    response.request_focus();
                    self.open_dir_input_wants_focus = false;
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button(format!("{} Open", icons::FOLDER_OPEN)).clicked() {
                        let path = PathBuf::from(self.open_dir_input.trim());
                        if path.is_dir() {
                            self.open_directory(path);
                            self.show_open_dir_dialog = false;
                        } else {
                            self.last_message = Some("That path is not a directory".to_string());
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        self.show_open_dir_dialog = false;
                    }
                });
            });

        if !open {
            self.show_open_dir_dialog = false;
        }
    }

    fn open_terminal_here(&mut self) {
        match Command::new("kitty")
            .arg("--directory")
            .arg(&self.current_dir)
            .spawn()
        {
            Ok(_) => {
                self.last_message = Some(format!(
                    "Opened kitty in {}",
                    self.current_dir.display()
                ));
            }
            Err(err) => {
                self.last_message = Some(format!("Failed to open kitty: {}", err));
            }
        }
    }
}

impl eframe::App for FerritorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let title = match self.selected_file.as_ref().and_then(|path| path.file_name()) {
            Some(file_name) => format!("{} — Ferritor", file_name.to_string_lossy()),
            None => format!("Ferritor — {}", self.current_dir.display()),
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        self.refresh_nav_rows();

        let key_up = ctx.input(|i| i.key_pressed(egui::Key::ArrowUp));
        let key_down = ctx.input(|i| i.key_pressed(egui::Key::ArrowDown));
        let key_left = ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft));
        let key_right = ctx.input(|i| i.key_pressed(egui::Key::ArrowRight));
        let ctrl_left = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::ArrowLeft));
        let ctrl_right = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::ArrowRight));
        let ctrl_up = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::ArrowUp));
        let ctrl_t = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::T));
        let ctrl_shift_o =
            ctx.input(|i| i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::O));
        let key_esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let ctrl_h = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::H));
        let ctrl_s = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::S));
        let ctrl_f = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::F));
        let key_e = ctx.input(|i| i.key_pressed(egui::Key::E));

        let filter_id = egui::Id::new("files_filter");
        let filter_focused = ctx.memory(|m| m.focused().is_some_and(|id| id == filter_id));
        let editor_focused = self.editing && self.focus_pane == FocusPane::Viewer;
        let kb_active = !filter_focused && !editor_focused;

        if (ctrl_f || ctrl_s) && self.focus_pane == FocusPane::Files && !filter_focused {
            ctx.memory_mut(|m| m.request_focus(filter_id));
        }
        if filter_focused && key_esc {
            self.filter.clear();
            self.refresh_nav_rows();
            ctx.memory_mut(|m| m.surrender_focus(filter_id));
        }

        if ctrl_shift_o {
            self.open_dir_input = self.current_dir.display().to_string();
            self.show_open_dir_dialog = true;
            self.open_dir_input_wants_focus = true;
        }
        if ctrl_t && kb_active {
            self.open_terminal_here();
        }
        if ctrl_up && kb_active && self.focus_pane == FocusPane::Files {
            if self.is_dirty() {
                self.dirty_nav_target = Some(DirtyNavTarget::EnterDirectory(
                    self.current_dir
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| self.current_dir.clone()),
                ));
            } else {
                self.open_parent_directory();
            }
        }

        if kb_active && self.focus_pane == FocusPane::Viewer && !self.editing && key_e {
            self.enter_edit_mode();
        }
        if ctrl_s && self.focus_pane == FocusPane::Viewer && self.editing {
            self.exit_edit_mode(true);
        }
        if key_esc && self.editing && self.focus_pane == FocusPane::Viewer {
            if self.is_dirty() {
                self.dirty_nav_target = Some(DirtyNavTarget::FocusFiles);
            } else {
                self.exit_edit_mode(false);
            }
        }

        self.viewer_scroll_delta = 0.0;
        if kb_active && self.focus_pane == FocusPane::Viewer && !self.editing {
            let scroll_step = 40.0;
            if key_up {
                self.viewer_scroll_delta = -scroll_step;
            }
            if key_down {
                self.viewer_scroll_delta = scroll_step;
            }
        }

        if let Some(target) = self.dirty_nav_target.clone() {
            let mut open = true;
            egui::Window::new("Unsaved Changes")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("You have unsaved changes.");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("(S)ave").clicked() || ctrl_s {
                            self.exit_edit_mode(true);
                            self.apply_dirty_nav(&target);
                            self.dirty_nav_target = None;
                        }
                        if ui.button("(D)iscard").clicked()
                            || ctx.input(|i| i.key_pressed(egui::Key::D))
                        {
                            self.exit_edit_mode(false);
                            self.apply_dirty_nav(&target);
                            self.dirty_nav_target = None;
                        }
                        if ui.button("Cancel").clicked() || key_esc {
                            self.dirty_nav_target = None;
                        }
                    });
                });
            if !open {
                self.dirty_nav_target = None;
            }
            return;
        }

        if kb_active && ctrl_h {
            self.show_help = !self.show_help;
        }
        if self.show_help {
            if key_esc {
                self.show_help = false;
            }
            egui::Window::new("Keybindings")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    egui::Grid::new("help_grid")
                        .num_columns(2)
                        .spacing([24.0, 4.0])
                        .show(ui, |ui| {
                            let row = |ui: &mut egui::Ui, key: &str, desc: &str| {
                                ui.label(egui::RichText::new(key).strong().monospace());
                                ui.label(desc);
                                ui.end_row();
                            };
                            row(ui, "Ctrl+Shift+O", "Open a directory");
                            row(ui, "Ctrl+T", "Open kitty in the current directory");
                            row(ui, "Ctrl+Up", "Move to the parent directory");
                            row(ui, "Up / Down", "Move cursor in the file list");
                            row(ui, "Left", "Move up one directory");
                            row(ui, "Right", "Open folder or focus viewer");
                            row(ui, "Ctrl+Left / Ctrl+Right", "Switch panes");
                            row(ui, "Ctrl+F", "Focus file filter");
                            row(ui, "E", "Edit selected file");
                            row(ui, "Ctrl+S", "Save changes");
                            row(ui, "Ctrl+H", "Toggle this help");
                        });
                });
            return;
        }

        if kb_active && ctrl_left {
            if self.is_dirty() {
                self.dirty_nav_target = Some(DirtyNavTarget::FocusFiles);
            } else {
                if self.editing {
                    self.exit_edit_mode(false);
                }
                self.focus_pane = FocusPane::Files;
            }
        }
        if kb_active && ctrl_right {
            self.focus_pane = FocusPane::Viewer;
        }

        self.nav_moved = false;
        if kb_active && self.focus_pane == FocusPane::Files && !self.nav_rows.is_empty() {
            let ctrl = ctx.input(|i| i.modifiers.ctrl);
            if key_down && !ctrl && self.nav_cursor + 1 < self.nav_rows.len() {
                self.nav_cursor += 1;
                self.nav_moved = true;
            }
            if key_up && !ctrl && self.nav_cursor > 0 {
                self.nav_cursor -= 1;
                self.nav_moved = true;
            }
            if key_left && !ctrl {
                if self.is_dirty() {
                    self.dirty_nav_target = Some(DirtyNavTarget::EnterDirectory(
                        self.current_dir
                            .parent()
                            .map(Path::to_path_buf)
                            .unwrap_or_else(|| self.current_dir.clone()),
                    ));
                } else {
                    self.open_parent_directory();
                }
            }
            if key_right && !ctrl {
                if let Some(row) = self.current_row().cloned() {
                    if row.is_directory_like() {
                        self.try_open_row(&row);
                    } else if row.is_file() {
                        self.focus_pane = FocusPane::Viewer;
                        if self.selected_file.as_ref() != Some(&row.path) {
                            self.try_open_row(&row);
                        }
                    }
                }
            }
        }

        let header_file_label = self
            .selected_file
            .as_ref()
            .and_then(|path| path.file_name().map(|name| name.to_string_lossy().to_string()));
        let header_dirty = self.is_dirty();

        egui::TopBottomPanel::top("headerbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Ferritor");
                ui.separator();
                if let Some(file_label) = header_file_label {
                    let color = if header_dirty {
                        self.theme.warning()
                    } else {
                        self.theme.view_fg()
                    };
                    ui.label(egui::RichText::new(file_label).color(color));
                } else {
                    ui.label("Markdown and basic file editor");
                }
            });
        });

        egui::TopBottomPanel::bottom("statusbar")
            .min_height(24.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Files").small().weak());
                    ui.separator();
                    let mode = if self.editing { "Editing" } else { "Viewing" };
                    let mode_color = if self.is_dirty() {
                        self.theme.warning()
                    } else {
                        self.theme.accent()
                    };
                    ui.label(egui::RichText::new(mode).color(mode_color).strong());
                    ui.separator();
                    ui.label(
                        egui::RichText::new(self.current_dir.display().to_string())
                            .small()
                            .weak(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(msg) = &self.last_message {
                            ui.label(egui::RichText::new(msg).small().weak());
                        }
                    });
                });
            });

        let focus_stroke = egui::Stroke::new(2.0, self.theme.accent());
        let no_stroke = egui::Stroke::NONE;
        let sidebar_frame = egui::Frame::side_top_panel(ctx.style().as_ref()).stroke(
            if self.focus_pane == FocusPane::Files {
                focus_stroke
            } else {
                no_stroke
            },
        );
        let viewer_frame = egui::Frame::central_panel(ctx.style().as_ref()).stroke(
            if self.focus_pane == FocusPane::Viewer {
                focus_stroke
            } else {
                no_stroke
            },
        );

        egui::SidePanel::left("files")
            .default_width(320.0)
            .frame(sidebar_frame)
            .show(ctx, |ui| {
                if ui.rect_contains_pointer(ui.max_rect()) && ctx.input(|i| i.pointer.any_click()) {
                    self.focus_pane = FocusPane::Files;
                }

                ui.horizontal(|ui| {
                    let button_clicked = ui
                        .button(format!("{} Open", icons::FOLDER_OPEN))
                        .on_hover_text("Ctrl+Shift+O")
                        .clicked();
                    if button_clicked {
                        self.open_dir_input = self.current_dir.display().to_string();
                        self.show_open_dir_dialog = true;
                        self.open_dir_input_wants_focus = true;
                    }

                    let terminal_clicked = ui
                        .button(format!("{}", icons::TERMINAL_WINDOW))
                        .on_hover_text("Open kitty here (Ctrl+T)")
                        .clicked();
                    if terminal_clicked {
                        self.open_terminal_here();
                    }

                    let filter_resp = ui.add(
                        egui::TextEdit::singleline(&mut self.filter)
                            .id(filter_id)
                            .desired_width(ui.available_width()),
                    );
                    if filter_resp.changed() {
                        self.refresh_nav_rows();
                    }
                    if !filter_resp.has_focus() && self.filter.is_empty() {
                        let rect = filter_resp.rect;
                        ui.painter().text(
                            rect.left_center() + egui::vec2(4.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            "Filter files",
                            egui::FontId::proportional(12.0),
                            self.theme.shade(),
                        );
                    }
                });

                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(self.current_dir.display().to_string())
                        .small()
                        .weak(),
                );
                ui.add_space(8.0);

                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut clicked_row: Option<FileRow> = None;

                    for (idx, row) in self.nav_rows.iter().enumerate() {
                        let is_selected = row
                            .is_file()
                            && self.selected_file.as_ref() == Some(&row.path);
                        let is_cursor = idx == self.nav_cursor;
                        let text = row.display_label();
                        let text = if row.is_directory_like() {
                            egui::RichText::new(text).color(self.theme.accent())
                        } else {
                            egui::RichText::new(text)
                        };

                        let response = ui.add(
                            egui::Button::new(text)
                                .selected(is_selected || is_cursor)
                                .fill(if is_selected {
                                    self.theme.headerbar_bg()
                                } else {
                                    egui::Color32::TRANSPARENT
                                }),
                        );

                        if is_cursor {
                            paint_cursor_bar(ui, &response, self.theme.accent());
                            if self.nav_moved {
                                response.scroll_to_me(Some(egui::Align::Center));
                            }
                        }

                        if response.clicked() {
                            self.nav_cursor = idx;
                            clicked_row = Some(row.clone());
                        }
                    }

                    if let Some(row) = clicked_row {
                        self.try_open_row(&row);
                    }
                });
            });

        egui::CentralPanel::default()
            .frame(viewer_frame)
            .show(ctx, |ui| {
                if ui.rect_contains_pointer(ui.max_rect()) && ctx.input(|i| i.pointer.any_click()) {
                    self.focus_pane = FocusPane::Viewer;
                }

                if let Some(path) = self.selected_file.clone() {
                    let file_name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.display().to_string());

                    ui.horizontal(|ui| {
                        ui.heading(&file_name);
                        if self.is_dirty() {
                            ui.label(
                                egui::RichText::new("~")
                                    .strong()
                                    .color(self.theme.warning()),
                            );
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add_enabled(
                                    !self.editing,
                                    egui::Button::new(format!(
                                        "{} (E)dit",
                                        icons::PENCIL_SIMPLE
                                    )),
                                )
                                .clicked()
                            {
                                self.enter_edit_mode();
                            }

                            if ui
                                .add_enabled(
                                    self.editing && self.is_dirty(),
                                    egui::Button::new(format!("{} Save", icons::FLOPPY_DISK)),
                                )
                                .clicked()
                            {
                                self.exit_edit_mode(true);
                            }
                        });
                    });

                    ui.label(
                        egui::RichText::new(path.display().to_string())
                            .small()
                            .weak(),
                    );
                    ui.separator();

                    if let Some(err) = &self.load_error {
                        ui.colored_label(self.theme.warning(), err);
                        return;
                    }

                    let scroll_delta = self.viewer_scroll_delta;
                    let scroll_out = egui::ScrollArea::vertical().show(ui, |ui| {
                        if self.editing {
                            let editor_id = egui::Id::new("viewer_editor");
                            let highlighter = SyntaxHighlighter::get();
                            let path_for_layout = path.clone();
                            let mut layouter = move |ui: &egui::Ui,
                                                     text: &dyn egui::TextBuffer,
                                                     wrap_width: f32| {
                                let mut job =
                                    highlighter.highlight(text.as_str(), &path_for_layout);
                                job.wrap.max_width = wrap_width;
                                ui.fonts_mut(|fonts| fonts.layout_job(job))
                            };
                            let te_resp = ui.add(
                                egui::TextEdit::multiline(&mut self.edit_buffer)
                                    .id(editor_id)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(f32::INFINITY)
                                    .code_editor()
                                    .layouter(&mut layouter),
                            );
                            if self.editor_wants_focus {
                                te_resp.request_focus();
                                self.editor_wants_focus = false;
                            }
                        } else if let Some(content) = &self.loaded_content {
                            let highlighter = SyntaxHighlighter::get();
                            let job = highlighter.highlight(content, &path);
                            ui.add(egui::Label::new(job).selectable(true));
                        }
                    });

                    if scroll_delta != 0.0 {
                        let new_offset = (scroll_out.state.offset.y + scroll_delta).max(0.0);
                        let mut state = scroll_out.state;
                        state.offset.y = new_offset;
                        state.store(ui.ctx(), scroll_out.id);
                    }
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.vertical(|ui| {
                            ui.heading("Ferritor");
                            ui.label("Open a folder from the left pane.");
                            ui.label("Select a file to view or edit it here.");
                        });
                    });
                }
            });

        self.render_open_dir_dialog(ctx);
    }
}

fn paint_cursor_bar(ui: &mut egui::Ui, resp: &egui::Response, color: egui::Color32) {
    let rect = resp.rect;
    let bar = egui::Rect::from_min_max(
        egui::pos2(rect.left() - 4.0, rect.top()),
        egui::pos2(rect.left() - 1.0, rect.bottom()),
    );
    ui.painter()
        .rect_filled(bar, egui::CornerRadius::ZERO, color);
}

fn load_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    let mono_path = "fonts/spline-sans-mono-latin-400-normal.ttf";
    let sans_path = "fonts/spline-sans-latin-400-normal.ttf";
    let mono_bold_path = "fonts/spline-sans-mono-latin-700-normal.ttf";

    if let Ok(mono_data) = std::fs::read(mono_path) {
        fonts.font_data.insert(
            "SplineSansMono".to_owned(),
            egui::FontData::from_owned(mono_data).into(),
        );
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .insert(0, "SplineSansMono".to_owned());
    }

    if let Ok(mono_bold_data) = std::fs::read(mono_bold_path) {
        fonts.font_data.insert(
            "SplineSansMono-Bold".to_owned(),
            egui::FontData::from_owned(mono_bold_data).into(),
        );
        fonts
            .families
            .entry(egui::FontFamily::Name("MonoBold".into()))
            .or_default()
            .push("SplineSansMono-Bold".to_owned());
    }

    if let Ok(sans_data) = std::fs::read(sans_path) {
        fonts.font_data.insert(
            "SplineSans".to_owned(),
            egui::FontData::from_owned(sans_data).into(),
        );
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "SplineSans".to_owned());
    }

    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);

    use egui::{FontFamily, FontId, TextStyle};
    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(20.0, FontFamily::Name("MonoBold".into())),
    );
    style
        .text_styles
        .insert(TextStyle::Body, FontId::new(15.0, FontFamily::Proportional));
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(15.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(13.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(14.0, FontFamily::Monospace),
    );
    ctx.set_style(style);
}
