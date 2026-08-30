use crate::fuzzy::FuzzySearch;
use crate::syntax::SyntaxHighlighter;
use crate::theme::GtkTheme;
use egui_commonmark::CommonMarkCache;
use egui_file_dialog::{DialogState as FileDialogState, FileDialog};
use egui_phosphor::regular as icons;
use egui_shadcn::{
    card, table, table_body, table_cell, table_head, table_header, table_row, CardProps,
    CardVariant, ColorPalette, TableCellProps, TableProps, TableRowProps, TableVariant, Theme,
};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

// --- Undo/Redo ---

const UNDO_GROUP_SIZE: usize = 20;
const UNDO_MAX_DEPTH: usize = 5;
const RECENTS_CAP: usize = 5;
const THEME_POLL_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileFingerprint {
    canonical_path: PathBuf,
    modified: Option<SystemTime>,
    len: u64,
}

impl FileFingerprint {
    fn capture(path: Option<PathBuf>) -> Option<Self> {
        let path = path?;
        let metadata = fs::metadata(&path).ok()?;
        Some(Self {
            canonical_path: fs::canonicalize(&path).unwrap_or(path),
            modified: metadata.modified().ok(),
            len: metadata.len(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ThemeFingerprint {
    gtk: Option<FileFingerprint>,
    syntect: Option<FileFingerprint>,
}

impl ThemeFingerprint {
    fn capture() -> Self {
        Self {
            gtk: FileFingerprint::capture(Some(GtkTheme::source_path())),
            syntect: FileFingerprint::capture(SyntaxHighlighter::theme_path()),
        }
    }
}

struct UndoHistory {
    undo_stack: Vec<String>,
    redo_stack: Vec<String>,
    last_snapshot: String,
    last_checked: String,
    chars_since_snapshot: usize,
}

impl UndoHistory {
    fn new() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_snapshot: String::new(),
            last_checked: String::new(),
            chars_since_snapshot: 0,
        }
    }

    fn begin(&mut self, content: &str) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.last_snapshot = content.to_string();
        self.last_checked = content.to_string();
        self.chars_since_snapshot = 0;
    }

    fn push_snapshot(&mut self, content: &str) {
        if content != self.last_snapshot {
            self.undo_stack.push(self.last_snapshot.clone());
            if self.undo_stack.len() > UNDO_MAX_DEPTH {
                self.undo_stack.remove(0);
            }
            self.last_snapshot = content.to_string();
            self.redo_stack.clear();
        }
        self.chars_since_snapshot = 0;
        self.last_checked = content.to_string();
    }

    fn track_changes(&mut self, current: &str) {
        if current == self.last_checked {
            return;
        }
        let frame_diff = (current.len() as isize - self.last_checked.len() as isize).unsigned_abs();
        self.chars_since_snapshot += frame_diff.max(1);
        self.last_checked = current.to_string();

        if self.chars_since_snapshot >= UNDO_GROUP_SIZE {
            self.push_snapshot(current);
        }
    }

    fn undo(&mut self, buffer: &mut String) -> bool {
        // Snapshot current unsaved edits before undoing
        if buffer.as_str() != self.last_snapshot {
            self.undo_stack.push(self.last_snapshot.clone());
            if self.undo_stack.len() > UNDO_MAX_DEPTH {
                self.undo_stack.remove(0);
            }
            self.last_snapshot = buffer.clone();
        }
        if let Some(prev) = self.undo_stack.pop() {
            self.redo_stack.push(self.last_snapshot.clone());
            if self.redo_stack.len() > UNDO_MAX_DEPTH {
                self.redo_stack.remove(0);
            }
            self.last_snapshot = prev.clone();
            self.last_checked = prev.clone();
            *buffer = prev;
            self.chars_since_snapshot = 0;
            true
        } else {
            false
        }
    }

    fn redo(&mut self, buffer: &mut String) -> bool {
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack.push(self.last_snapshot.clone());
            if self.undo_stack.len() > UNDO_MAX_DEPTH {
                self.undo_stack.remove(0);
            }
            self.last_snapshot = next.clone();
            self.last_checked = next.clone();
            *buffer = next;
            self.chars_since_snapshot = 0;
            true
        } else {
            false
        }
    }
}

// --- Core types ---

#[derive(Clone, Copy, PartialEq, Eq)]
enum FocusPane {
    Files,
    Viewer,
    Dialog,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum WorkMode {
    Viewing,
    Editing,
    Preview,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum OverlayState {
    None,
    OpenDir,
    Fuzzy,
    Recents,
    Rename,
    Create,
    Delete,
    CutConfirm,
    FileAction,
    DirtyNav,
    Help,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum OpenDirBackend {
    Zenity,
    EguiFileDialog,
    PathEntry,
}

impl OpenDirBackend {
    fn label(self) -> &'static str {
        match self {
            OpenDirBackend::Zenity => "native (zenity)",
            OpenDirBackend::EguiFileDialog => "egui-file-dialog",
            OpenDirBackend::PathEntry => "path entry",
        }
    }
}

enum AppAction {
    FilterFocusToggle {
        filter_id: egui::Id,
        currently_focused: bool,
    },
    FilterClearAndUnfocus {
        filter_id: egui::Id,
    },
    ShowOpenDirDialog,
    ShowFuzzyDialog,
    ShowRecentsDialog,
    OpenTerminalHere,
    TogglePreview,
    ToggleSidebar,
    SetDirtyNav(DirtyNavTarget),
    CloseFile,
    QuitApp,
    OpenParentDirectory,
    OpenRenameDialogForPath(PathBuf),
    CloseRenameDialog,
    OpenCreateDialog {
        is_dir: bool,
    },
    OpenDeleteDialog {
        targets: Vec<PathBuf>,
    },
    OpenCutDialog {
        sources: Vec<PathBuf>,
    },
    SetFileClipboard {
        sources: Vec<PathBuf>,
        is_cut: bool,
    },
    PasteFileClipboard,
    EnterEditMode,
    SaveAndExitEditMode,
    ExitEditModeDiscard,
    SetFocusPane(FocusPane),
    DirtyDialogSaveAndApply {
        target: DirtyNavTarget,
    },
    DirtyDialogDiscardAndApply {
        target: DirtyNavTarget,
    },
    DirtyDialogCancelToEditor,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RecentsMode {
    Files,
    Directories,
}

impl RecentsMode {
    fn label(self) -> &'static str {
        match self {
            RecentsMode::Files => "Files",
            RecentsMode::Directories => "Directories",
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, Default)]
struct RecentsData {
    files: Vec<PathBuf>,
    dirs: Vec<PathBuf>,
}

struct CollectedIntents {
    key_up: bool,
    key_down: bool,
    key_left: bool,
    key_right: bool,
    key_enter: bool,
    key_space: bool,
    key_delete: bool,
    ctrl_left: bool,
    ctrl_right: bool,
    ctrl_up: bool,
    ctrl_t: bool,
    ctrl_o: bool,
    key_z: bool,
    ctrl_l: bool,
    key_esc: bool,
    ctrl_h: bool,
    ctrl_s: bool,
    ctrl_f: bool,
    ctrl_p: bool,
    ctrl_e: bool,
    ctrl_w: bool,
    ctrl_q: bool,
    ctrl_r: bool,
    ctrl_n: bool,
    ctrl_shift_n: bool,
    ctrl_c: bool,
    ctrl_x: bool,
    ctrl_v: bool,
    key_e: bool,
    alt_r: bool,
    alt_c: bool,
    page_up: bool,
    page_down: bool,
    ctrl_z: bool,
    ctrl_y: bool,
    editor_para_up: bool,
    editor_para_down: bool,
    editor_word_left: bool,
    editor_word_right: bool,
    editor_select_para_up: bool,
    editor_select_para_down: bool,
    editor_select_word_left: bool,
    editor_select_word_right: bool,
    doc_find_prev: bool,
    doc_find_next: bool,
    dialog_tab: bool,
    filter_focused: bool,
    editor_focused: bool,
    doc_find_focused: bool,
}

#[derive(Clone)]
enum DirtyNavTarget {
    FocusFiles,
    OpenFile(PathBuf),
    OpenSearchResult(PathBuf),
    EnterDirectory(PathBuf),
    CloseFile,
    QuitApp,
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
    depth: usize,
}

impl FileRow {
    fn is_file(&self) -> bool {
        matches!(self.kind, FileRowKind::File)
    }

    fn is_directory(&self) -> bool {
        matches!(self.kind, FileRowKind::Directory)
    }

    fn is_directory_like(&self) -> bool {
        matches!(self.kind, FileRowKind::Parent | FileRowKind::Directory)
    }
}

// --- App ---

pub struct FerritorApp {
    pub theme: GtkTheme,
    heading_color: egui::Color32,
    syntax_highlighter: SyntaxHighlighter,
    theme_fingerprint: ThemeFingerprint,
    pending_theme_fingerprint: Option<ThemeFingerprint>,
    last_theme_check: Instant,
    current_dir: PathBuf,
    selected_file: Option<PathBuf>,
    loaded_content: Option<String>,
    load_error: Option<String>,
    filter: String,
    last_message: Option<String>,
    nav_rows: Vec<FileRow>,
    nav_cursor: usize,
    focus_pane: FocusPane,
    shadcn_theme: Theme,
    show_help: bool,
    show_fuzzy_dialog: bool,
    fuzzy_input_wants_focus: bool,
    fuzzy_search: FuzzySearch,
    show_recents_dialog: bool,
    recents_mode: RecentsMode,
    recents_cursor: usize,
    recent_files: Vec<PathBuf>,
    recent_dirs: Vec<PathBuf>,
    nav_moved: bool,
    editing: bool,
    edit_buffer: String,
    original_content: Option<String>,
    dirty_nav_target: Option<DirtyNavTarget>,
    viewer_scroll_delta: f32,
    viewer_scroll_y: f32,
    viewer_line_height_px: f32,
    viewer_top_char: usize,
    pending_viewer_scroll_char: Option<usize>,
    editor_wants_focus: bool,
    pending_editor_cursor_char: Option<usize>,
    show_doc_find: bool,
    doc_find_query: String,
    doc_find_wants_focus: bool,
    doc_find_cursor_char: Option<usize>,
    show_sidebar: bool,
    open_dir_backend: OpenDirBackend,
    open_dir_fallback_status: Option<String>,
    open_dir_file_dialog: FileDialog,
    show_open_dir_dialog: bool,
    open_dir_input: String,
    open_dir_input_wants_focus: bool,
    open_dir_cursor_to_end: bool,
    open_dir_autocomplete_cursor: usize,
    open_dir_show_suggestions: bool,
    show_preview: bool,
    commonmark_cache: CommonMarkCache,
    expanded_dirs: HashSet<PathBuf>,
    // Rename
    show_rename_dialog: bool,
    rename_input: String,
    rename_input_wants_focus: bool,
    rename_target: Option<PathBuf>,
    // Undo/Redo
    undo_history: UndoHistory,
    // Create file/dir
    show_create_dialog: bool,
    create_input: String,
    create_input_wants_focus: bool,
    create_is_dir: bool,
    create_target_dir: PathBuf,
    // Multi-selection
    selected_paths: HashSet<PathBuf>,
    // File clipboard
    clipboard_paths: Vec<PathBuf>,
    clipboard_is_cut: bool,
    show_cut_dialog: bool,
    pending_cut_paths: Vec<PathBuf>,
    file_action_confirmation: Option<String>,
    // Delete confirmation
    show_delete_dialog: bool,
    delete_targets: Vec<PathBuf>,
    delete_focus: u8, // 0 = delete, 1 = cancel
    // Dirty nav dialog
    dirty_focus: u8, // 0 = save, 1 = discard, 2 = cancel
    // Dialog focus state
    prev_focus_pane: FocusPane,
    dialog_tab: bool,
}

impl FerritorApp {
    pub fn new(cc: &eframe::CreationContext<'_>, file_arg: Option<PathBuf>) -> Self {
        let theme = GtkTheme::load();
        let heading_color = theme.heading_fg();
        let syntax_highlighter = SyntaxHighlighter::new(heading_color);
        let theme_fingerprint = ThemeFingerprint::capture();
        let shadcn_palette = shadcn_palette_from_gtk(&theme);

        apply_egui_visuals(&cc.egui_ctx, &theme);
        load_fonts(&cc.egui_ctx);

        // Resolve starting directory from file arg or cwd
        let (start_dir, open_file) = if let Some(ref path) = file_arg {
            let abs = if path.is_absolute() {
                path.clone()
            } else {
                std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join(path)
            };
            if abs.is_file() {
                let dir = abs
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| PathBuf::from("."));
                (dir, Some(abs))
            } else if abs.is_dir() {
                (abs, None)
            } else {
                // Path doesn't exist yet — treat parent as dir
                let dir = abs
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| PathBuf::from("."));
                (dir, None)
            }
        } else {
            let dir = std::env::current_dir()
                .ok()
                .or_else(dirs::home_dir)
                .unwrap_or_else(|| PathBuf::from("."));
            (dir, None)
        };

        let current_dir = start_dir;

        let mut app = Self {
            theme,
            heading_color,
            syntax_highlighter,
            theme_fingerprint,
            pending_theme_fingerprint: None,
            last_theme_check: Instant::now(),
            current_dir: current_dir.clone(),
            selected_file: None,
            loaded_content: None,
            load_error: None,
            filter: String::new(),
            last_message: None,
            nav_rows: Vec::new(),
            nav_cursor: 0,
            focus_pane: FocusPane::Files,
            shadcn_theme: Theme::new(shadcn_palette),
            show_help: false,
            show_fuzzy_dialog: false,
            fuzzy_input_wants_focus: false,
            fuzzy_search: FuzzySearch::default(),
            show_recents_dialog: false,
            recents_mode: RecentsMode::Files,
            recents_cursor: 0,
            recent_files: Vec::new(),
            recent_dirs: Vec::new(),
            nav_moved: false,
            editing: false,
            edit_buffer: String::new(),
            original_content: None,
            dirty_nav_target: None,
            viewer_scroll_delta: 0.0,
            viewer_scroll_y: 0.0,
            viewer_line_height_px: 18.0,
            viewer_top_char: 0,
            pending_viewer_scroll_char: None,
            editor_wants_focus: false,
            pending_editor_cursor_char: None,
            show_doc_find: false,
            doc_find_query: String::new(),
            doc_find_wants_focus: false,
            doc_find_cursor_char: None,
            show_sidebar: true,
            open_dir_backend: OpenDirBackend::PathEntry,
            open_dir_fallback_status: None,
            open_dir_file_dialog: Self::build_open_dir_file_dialog(current_dir.clone()),
            show_open_dir_dialog: false,
            open_dir_input: current_dir.display().to_string(),
            open_dir_input_wants_focus: false,
            open_dir_cursor_to_end: false,
            open_dir_autocomplete_cursor: 0,
            open_dir_show_suggestions: false,
            show_preview: false,
            commonmark_cache: CommonMarkCache::default(),
            expanded_dirs: HashSet::new(),
            show_rename_dialog: false,
            rename_input: String::new(),
            rename_input_wants_focus: false,
            rename_target: None,
            undo_history: UndoHistory::new(),
            show_create_dialog: false,
            create_input: String::new(),
            create_input_wants_focus: false,
            create_is_dir: false,
            create_target_dir: current_dir,
            selected_paths: HashSet::new(),
            clipboard_paths: Vec::new(),
            clipboard_is_cut: false,
            show_cut_dialog: false,
            pending_cut_paths: Vec::new(),
            file_action_confirmation: None,
            show_delete_dialog: false,
            delete_targets: Vec::new(),
            delete_focus: 0,
            dirty_focus: 0,
            prev_focus_pane: FocusPane::Files,
            dialog_tab: false,
        };
        app.refresh_nav_rows();
        app.load_recents_from_disk();

        // Open file from CLI argument
        if let Some(path) = open_file {
            debug_notify("Startup", &format!("Opening file: {}", path.display()));
            app.open_file(path);
        }

        app
    }

    fn refresh_theme_if_needed(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(THEME_POLL_INTERVAL);
        if self.last_theme_check.elapsed() < THEME_POLL_INTERVAL {
            return;
        }
        self.last_theme_check = Instant::now();

        let fingerprint = ThemeFingerprint::capture();
        if fingerprint == self.theme_fingerprint {
            self.pending_theme_fingerprint = None;
            return;
        }

        // Theme switchers commonly replace several files in quick succession.
        // Require one stable poll before adopting the new set.
        if self.pending_theme_fingerprint.as_ref() != Some(&fingerprint) {
            self.pending_theme_fingerprint = Some(fingerprint);
            return;
        }

        // Keep the last valid state if a replacement file is temporarily
        // incomplete or otherwise cannot be parsed.
        let Some(theme) = GtkTheme::try_load() else {
            return;
        };
        let heading_color = theme.heading_fg();
        let Some(syntax_highlighter) = SyntaxHighlighter::try_new(heading_color) else {
            return;
        };

        apply_egui_visuals(ctx, &theme);
        self.shadcn_theme = Theme::new(shadcn_palette_from_gtk(&theme));
        self.theme = theme;
        self.heading_color = heading_color;
        self.syntax_highlighter = syntax_highlighter;
        self.theme_fingerprint = fingerprint;
        self.pending_theme_fingerprint = None;
        ctx.request_repaint();
    }

    // --- Tree building ---

    fn build_tree_rows(
        dir: &Path,
        filter: &str,
        expanded: &HashSet<PathBuf>,
        depth: usize,
    ) -> Vec<FileRow> {
        let mut rows = Vec::new();

        if depth == 0 {
            if let Some(parent) = dir.parent() {
                rows.push(FileRow {
                    path: parent.to_path_buf(),
                    name: "..".to_string(),
                    kind: FileRowKind::Parent,
                    depth: 0,
                });
            }
        }

        let mut dirs = Vec::new();
        let mut files = Vec::new();
        let filter_lc = filter.trim().to_lowercase();

        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let file_name = entry.file_name();
                let name = file_name.to_string_lossy().to_string();

                if depth == 0 && !filter_lc.is_empty() && !name.to_lowercase().contains(&filter_lc)
                {
                    continue;
                }

                match entry.file_type() {
                    Ok(ft) if ft.is_dir() => dirs.push(FileRow {
                        path,
                        name,
                        kind: FileRowKind::Directory,
                        depth,
                    }),
                    Ok(ft) if ft.is_file() => files.push(FileRow {
                        path,
                        name,
                        kind: FileRowKind::File,
                        depth,
                    }),
                    _ => {}
                }
            }
        }

        dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        for dir_row in dirs {
            let is_expanded = expanded.contains(&dir_row.path);
            rows.push(dir_row.clone());
            if is_expanded {
                let children = Self::build_tree_rows(&dir_row.path, filter, expanded, depth + 1);
                rows.extend(children);
            }
        }

        rows.extend(files);
        rows
    }

    fn refresh_nav_rows(&mut self) {
        self.nav_rows =
            Self::build_tree_rows(&self.current_dir, &self.filter, &self.expanded_dirs, 0);
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

    fn selected_file_is_markdown(&self) -> bool {
        self.selected_file
            .as_ref()
            .and_then(|p| p.extension())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
    }

    fn is_dirty(&self) -> bool {
        self.editing && self.original_content.as_deref() != Some(&self.edit_buffer)
    }

    /// Directory that "contains" the cursor item — used for create/paste targets.
    fn cursor_target_dir(&self) -> PathBuf {
        match self.current_row() {
            Some(row) => match row.kind {
                FileRowKind::Parent => self.current_dir.clone(),
                FileRowKind::Directory => row.path.clone(),
                FileRowKind::File => row
                    .path
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| self.current_dir.clone()),
            },
            None => self.current_dir.clone(),
        }
    }

    fn recents_file_path() -> Option<PathBuf> {
        dirs::home_dir().map(|home| home.join(".ferritor").join("recents.json"))
    }

    fn load_recents_from_disk(&mut self) {
        let Some(path) = Self::recents_file_path() else {
            return;
        };
        let Ok(raw) = fs::read_to_string(path) else {
            return;
        };
        let Ok(mut data) = serde_json::from_str::<RecentsData>(&raw) else {
            return;
        };
        data.files.retain(|p| p.is_file());
        data.dirs.retain(|p| p.is_dir());
        data.files.truncate(RECENTS_CAP);
        data.dirs.truncate(RECENTS_CAP);
        self.recent_files = data.files;
        self.recent_dirs = data.dirs;
    }

    fn save_recents_to_disk(&self) {
        let Some(path) = Self::recents_file_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let data = RecentsData {
            files: self.recent_files.clone(),
            dirs: self.recent_dirs.clone(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&data) {
            let _ = fs::write(path, json);
        }
    }

    fn push_recent_file(&mut self, path: PathBuf) {
        if !path.is_file() {
            return;
        }
        self.recent_files.retain(|p| p != &path && p.is_file());
        self.recent_files.insert(0, path);
        self.recent_files.truncate(RECENTS_CAP);
    }

    fn push_recent_dir(&mut self, path: PathBuf) {
        if !path.is_dir() {
            return;
        }
        self.recent_dirs.retain(|p| p != &path && p.is_dir());
        self.recent_dirs.insert(0, path);
        self.recent_dirs.truncate(RECENTS_CAP);
    }

    fn recents_len(&self) -> usize {
        match self.recents_mode {
            RecentsMode::Files => self.recent_files.len(),
            RecentsMode::Directories => self.recent_dirs.len(),
        }
    }

    fn open_selected_recent(&mut self) {
        match self.recents_mode {
            RecentsMode::Files => {
                let Some(path) = self.recent_files.get(self.recents_cursor).cloned() else {
                    return;
                };
                if path.is_file() {
                    self.close_recents_dialog();
                    if let Some(parent) = path.parent() {
                        self.open_directory(parent.to_path_buf());
                    }
                    self.open_file(path);
                } else {
                    self.recent_files.retain(|p| p.is_file());
                    self.recents_cursor = 0;
                    self.save_recents_to_disk();
                }
            }
            RecentsMode::Directories => {
                let Some(path) = self.recent_dirs.get(self.recents_cursor).cloned() else {
                    return;
                };
                if path.is_dir() {
                    self.close_recents_dialog();
                    self.open_directory(path);
                } else {
                    self.recent_dirs.retain(|p| p.is_dir());
                    self.recents_cursor = 0;
                    self.save_recents_to_disk();
                }
            }
        }
    }

    fn open_selected_fuzzy(&mut self) {
        let Some(result) = self.fuzzy_search.selected().cloned() else {
            return;
        };
        let path = result.path;
        self.close_fuzzy_dialog();

        if self.is_dirty() {
            self.set_dirty_nav(DirtyNavTarget::OpenSearchResult(path));
            return;
        }

        if let Some(parent) = path.parent() {
            self.open_directory(parent.to_path_buf());
        }
        self.open_file(path);
    }

    // --- Navigation ---

    fn apply_dirty_nav(&mut self, ctx: &egui::Context, target: &DirtyNavTarget) {
        match target {
            DirtyNavTarget::FocusFiles => {
                self.focus_pane = FocusPane::Files;
            }
            DirtyNavTarget::OpenFile(path) => {
                self.open_file(path.clone());
            }
            DirtyNavTarget::OpenSearchResult(path) => {
                if let Some(parent) = path.parent() {
                    self.open_directory(parent.to_path_buf());
                }
                self.open_file(path.clone());
            }
            DirtyNavTarget::EnterDirectory(path) => {
                self.open_directory(path.clone());
            }
            DirtyNavTarget::CloseFile => {
                self.close_file();
            }
            DirtyNavTarget::QuitApp => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn exit_edit_mode(&mut self, save: bool) {
        if save && self.is_dirty() {
            // Push final snapshot before saving
            self.undo_history.push_snapshot(&self.edit_buffer);
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
        self.show_doc_find = false;
        self.doc_find_cursor_char = None;
        self.pending_viewer_scroll_char = None;
        self.open_dir_input = self.current_dir.display().to_string();
        self.expanded_dirs.clear();
        self.selected_paths.clear();
        self.refresh_nav_rows();
        self.push_recent_dir(self.current_dir.clone());
        self.save_recents_to_disk();
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
        self.show_preview = false;
        self.edit_buffer.clear();
        self.doc_find_cursor_char = None;
        self.pending_viewer_scroll_char = None;
        self.push_recent_file(path.clone());
        self.save_recents_to_disk();

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

    fn close_file(&mut self) {
        self.selected_file = None;
        self.loaded_content = None;
        self.load_error = None;
        self.editing = false;
        self.edit_buffer.clear();
        self.original_content = None;
        self.show_preview = false;
        self.show_doc_find = false;
        self.doc_find_cursor_char = None;
        self.pending_viewer_scroll_char = None;
        self.focus_pane = FocusPane::Files;
        self.show_sidebar = true;
    }

    fn enter_edit_mode(&mut self) {
        if let Some(content) = &self.loaded_content {
            let content = content.clone();
            self.edit_buffer = content.clone();
            self.original_content = Some(content.clone());
            self.editing = true;
            self.editor_wants_focus = true;
            // Preserve reading context on edit entry when viewing raw text.
            let anchor_char = if self.show_preview {
                0
            } else {
                self.viewer_top_char.min(content.chars().count())
            };
            self.pending_editor_cursor_char = Some(anchor_char);
            self.undo_history.begin(&content);
        }
    }

    fn try_open_row(&mut self, row: &FileRow) {
        if self.is_dirty() {
            self.set_dirty_nav(match row.kind {
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

    fn collapse_directory(&mut self, path: &Path) {
        self.expanded_dirs.retain(|p| !p.starts_with(path));
    }

    fn find_parent_row_index(&self, idx: usize) -> Option<usize> {
        let row = self.nav_rows.get(idx)?;
        if row.depth == 0 {
            return None;
        }
        let target_depth = row.depth - 1;
        for i in (0..idx).rev() {
            if self.nav_rows[i].depth == target_depth && self.nav_rows[i].is_directory_like() {
                return Some(i);
            }
        }
        None
    }

    // --- File operations ---

    fn rename_path(&mut self, new_name: &str) {
        let new_name = new_name.trim();
        if new_name.is_empty() {
            self.last_message = Some("Rename cancelled: empty name".to_string());
            return;
        }

        if let Some(old_path) = self.rename_target.clone() {
            let new_path = old_path.with_file_name(new_name);
            if new_path == old_path {
                return;
            }
            if new_path.exists() {
                self.last_message = Some(format!("Rename failed: '{}' already exists", new_name));
                self.close_rename_dialog();
                return;
            }
            match fs::rename(&old_path, &new_path) {
                Ok(()) => {
                    // Update selected_file if it was the renamed item
                    if self.selected_file.as_ref() == Some(&old_path) {
                        self.selected_file = Some(new_path.clone());
                    }
                    // Update expanded_dirs
                    if self.expanded_dirs.remove(&old_path) {
                        self.expanded_dirs.insert(new_path.clone());
                    }
                    // Update multi-selection
                    if self.selected_paths.remove(&old_path) {
                        self.selected_paths.insert(new_path.clone());
                    }
                    self.last_message = Some(format!("Renamed to '{}'", new_name));
                    self.refresh_nav_rows();
                }
                Err(err) => {
                    self.last_message = Some(format!("Rename error: {}", err));
                }
            }
        }
    }

    fn execute_create(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            self.last_message = Some("Create cancelled: empty name".to_string());
            return;
        }
        let target = self.create_target_dir.join(name);
        if target.exists() {
            self.last_message = Some(format!("'{}' already exists", name));
            return;
        }
        let result = if self.create_is_dir {
            fs::create_dir(&target)
        } else {
            fs::write(&target, "")
        };
        match result {
            Ok(()) => {
                let kind = if self.create_is_dir {
                    "directory"
                } else {
                    "file"
                };
                self.last_message = Some(format!("Created {} '{}'", kind, name));
                // Ensure the parent dir is expanded so the new item is visible
                if self.create_target_dir != self.current_dir {
                    self.expanded_dirs.insert(self.create_target_dir.clone());
                }
                self.refresh_nav_rows();
                // Move cursor to the new item
                if let Some(idx) = self.nav_rows.iter().position(|r| r.path == target) {
                    self.nav_cursor = idx;
                    self.nav_moved = true;
                }
            }
            Err(err) => {
                self.last_message = Some(format!("Create error: {}", err));
            }
        }
    }

    fn execute_delete(&mut self) {
        let targets: Vec<PathBuf> = self.delete_targets.drain(..).collect();
        let mut deleted = 0;
        let mut should_close_file = false;
        for target in &targets {
            let result = if target.is_dir() {
                fs::remove_dir_all(target)
            } else {
                fs::remove_file(target)
            };
            if result.is_ok() {
                deleted += 1;
                if self.selected_file.as_ref() == Some(target) {
                    should_close_file = true;
                }
                self.expanded_dirs.retain(|p| !p.starts_with(target));
                self.selected_paths.remove(target);
            }
        }
        if should_close_file {
            self.close_file();
        }
        if deleted > 0 {
            let label = if deleted == 1 {
                "Deleted 1 item".to_string()
            } else {
                format!("Deleted {} items", deleted)
            };
            self.last_message = Some(label);
        }
        self.refresh_nav_rows();
    }

    fn execute_paste(&mut self) {
        if self.clipboard_paths.is_empty() {
            return;
        }
        let target_dir = self.cursor_target_dir();
        let is_cut = self.clipboard_is_cut;
        let mut completed = Vec::new();
        let mut failed = Vec::new();

        for source in self.clipboard_paths.clone() {
            let file_name = match source.file_name() {
                Some(n) => n.to_os_string(),
                None => {
                    failed.push(source);
                    continue;
                }
            };
            let direct_dest = target_dir.join(&file_name);
            if is_cut && direct_dest == source {
                // Pasting a cut item back into its existing parent is a no-op.
                completed.push((source.clone(), source));
                continue;
            }
            if source.is_dir() && target_dir.starts_with(&source) {
                failed.push(source);
                continue;
            }

            let dest = unique_path(direct_dest);
            let result = if is_cut {
                move_path(&source, &dest)
            } else {
                copy_path(&source, &dest)
            };
            match result {
                Ok(()) => completed.push((source, dest)),
                Err(_) => failed.push(source),
            }
        }

        let failed_count = failed.len();
        if is_cut {
            for (source, dest) in &completed {
                if source == dest {
                    continue;
                }
                if let Some(open_file) = self.selected_file.clone() {
                    if let Ok(relative) = open_file.strip_prefix(source) {
                        self.selected_file = Some(dest.join(relative));
                    }
                }
                self.expanded_dirs = self
                    .expanded_dirs
                    .iter()
                    .map(|path| {
                        path.strip_prefix(source)
                            .map(|relative| dest.join(relative))
                            .unwrap_or_else(|_| path.clone())
                    })
                    .collect();
                self.selected_paths.remove(source);
            }
            self.clipboard_paths = failed;
            self.clipboard_is_cut = !self.clipboard_paths.is_empty();
        }

        if !completed.is_empty() {
            if target_dir != self.current_dir {
                self.expanded_dirs.insert(target_dir);
            }
        }
        let verb = if is_cut { "Moved" } else { "Copied" };
        let message = if failed_count == 0 {
            format!("{} {} item(s)", verb, completed.len())
        } else {
            format!(
                "{} {} item(s); {} failed",
                verb,
                completed.len(),
                failed_count
            )
        };
        self.last_message = Some(message.clone());
        self.file_action_confirmation = Some(message);
        self.enter_dialog_mode();
        self.refresh_nav_rows();
    }

    fn build_open_dir_file_dialog(initial_dir: PathBuf) -> FileDialog {
        FileDialog::new()
            .initial_directory(initial_dir)
            .as_modal(true)
    }

    fn open_dir_file_dialog_active(&self) -> bool {
        !matches!(self.open_dir_file_dialog.state(), FileDialogState::Closed)
    }

    fn launch_open_dir_flow(&mut self) {
        // Default to path-entry: most ergonomic — type a path, press Enter.
        // "Browse..." button inside the dialog escalates to native zenity then
        // egui-file-dialog when the user wants a GUI picker.
        self.open_dir_input = self.current_dir.display().to_string();
        self.open_dir_fallback_status = None;
        self.launch_open_dir_path_dialog();
    }

    fn launch_open_dir_browse_chain(&mut self) {
        // Used by the "Browse..." button inside the path-entry dialog.
        self.open_dir_backend = OpenDirBackend::Zenity;
        match self.try_pick_directory_with_zenity() {
            Ok(Some(path)) => {
                self.close_open_dir_dialog();
                self.open_directory(path);
                self.last_message = Some("Opened directory via native picker".to_string());
            }
            Ok(None) => {
                self.last_message = Some("Open directory canceled".to_string());
            }
            Err(err) => {
                self.open_dir_fallback_status =
                    Some(format!("Native picker unavailable ({err}); using GUI picker."));
                self.close_open_dir_dialog();
                self.launch_open_dir_file_dialog();
            }
        }
    }

    fn expand_user_path(input: &str) -> PathBuf {
        let trimmed = input.trim();
        if let Some(stripped) = trimmed.strip_prefix("~/") {
            if let Some(home) = dirs::home_dir() {
                return home.join(stripped);
            }
        } else if trimmed == "~" {
            if let Some(home) = dirs::home_dir() {
                return home;
            }
        }
        PathBuf::from(trimmed)
    }

    fn launch_open_dir_file_dialog(&mut self) {
        self.open_dir_backend = OpenDirBackend::EguiFileDialog;
        self.open_dir_file_dialog.config_mut().initial_directory = self.current_dir.clone();
        self.open_dir_file_dialog.pick_directory();
        if matches!(self.open_dir_file_dialog.state(), FileDialogState::Open) {
            self.enter_dialog_mode();
            if let Some(status) = &self.open_dir_fallback_status {
                self.last_message = Some(format!(
                    "{status} Using {} (Enter navigates, Ctrl+Enter opens highlighted directory).",
                    self.open_dir_backend.label()
                ));
            }
        } else {
            self.open_dir_fallback_status = Some(format!(
                "Could not open {} backend; using {}.",
                OpenDirBackend::EguiFileDialog.label(),
                OpenDirBackend::PathEntry.label()
            ));
            self.launch_open_dir_path_dialog();
        }
    }

    fn launch_open_dir_path_dialog(&mut self) {
        self.open_dir_backend = OpenDirBackend::PathEntry;
        self.show_open_dir_dialog = true;
        self.open_dir_input_wants_focus = true;
        self.open_dir_show_suggestions = false;
        self.open_dir_autocomplete_cursor = 0;
        self.enter_dialog_mode();
        if let Some(status) = &self.open_dir_fallback_status {
            self.last_message = Some(format!("{status}"));
        }
    }

    fn try_pick_directory_with_zenity(&self) -> Result<Option<PathBuf>, String> {
        let mut args = vec!["--file-selection", "--directory"];
        let mut start = self.current_dir.display().to_string();
        if !start.ends_with('/') {
            start.push('/');
        }
        args.push("--filename");
        args.push(&start);

        let output = Command::new("zenity")
            .args(args)
            .output()
            .map_err(|err| err.to_string())?;

        if output.status.success() {
            let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if selected.is_empty() {
                return Err("empty path from zenity".to_string());
            }
            let path = PathBuf::from(selected);
            if path.is_dir() {
                return Ok(Some(path));
            }
            return Err("selected path is not a directory".to_string());
        }

        if output.status.code() == Some(1) {
            return Ok(None);
        }

        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            Err(format!(
                "zenity exited with code {:?}",
                output.status.code()
            ))
        } else {
            Err(stderr)
        }
    }

    // --- Dialogs ---

    fn render_open_dir_file_dialog(&mut self, ctx: &egui::Context) {
        if !self.open_dir_file_dialog_active() {
            return;
        }

        let ctrl_enter = ctx
            .input(|i| i.modifiers.ctrl && !i.modifiers.shift && i.key_pressed(egui::Key::Enter));
        if ctrl_enter {
            if let Some(path) = self
                .open_dir_file_dialog
                .selected_entry()
                .filter(|entry| entry.is_dir())
                .map(|entry| entry.to_path_buf())
            {
                self.open_directory(path);
                self.last_message =
                    Some("Opened directory from picker selection (Ctrl+Enter)".to_string());
                self.open_dir_file_dialog =
                    Self::build_open_dir_file_dialog(self.current_dir.clone());
                self.exit_dialog_mode();
                return;
            }
            self.last_message = Some("Ctrl+Enter: highlight a directory first".to_string());
        }

        if matches!(self.open_dir_file_dialog.state(), FileDialogState::Open) {
            self.open_dir_file_dialog.update(ctx);
        }
        if let Some(path) = self.open_dir_file_dialog.take_picked() {
            if path.is_dir() {
                self.open_directory(path);
            } else {
                self.last_message = Some("Selected path is not a directory".to_string());
            }
            self.exit_dialog_mode();
            return;
        }

        if matches!(
            self.open_dir_file_dialog.state(),
            FileDialogState::Cancelled
        ) {
            self.last_message = Some("Open directory canceled".to_string());
            self.open_dir_file_dialog = Self::build_open_dir_file_dialog(self.current_dir.clone());
            self.exit_dialog_mode();
        }
    }

    fn render_open_dir_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_open_dir_dialog {
            return;
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            debug_notify("Dialog", "Open Dir: Esc → close");
            self.close_open_dir_dialog();
            return;
        }

        let mut open = true;
        let submit_key = ctx.input(|i| i.key_pressed(egui::Key::Enter));
        let right_arrow = ctx.input(|i| i.key_pressed(egui::Key::ArrowRight));
        let up_key = ctx.input(|i| i.key_pressed(egui::Key::ArrowUp));
        let down_key = ctx.input(|i| i.key_pressed(egui::Key::ArrowDown));
        let mut try_open = false;
        let mut launch_browse = false;
        let mut close_now = false;
        let mut accept_suggestion = false;

        // Get suggestions for current input
        let suggestions = self.get_open_dir_suggestions();
        let has_suggestions = !suggestions.is_empty();

        // Handle navigation keys
        if up_key && self.open_dir_show_suggestions {
            if self.open_dir_autocomplete_cursor > 0 {
                self.open_dir_autocomplete_cursor -= 1;
            }
        }
        if down_key && self.open_dir_show_suggestions {
            if self.open_dir_autocomplete_cursor < suggestions.len().saturating_sub(1) {
                self.open_dir_autocomplete_cursor += 1;
            }
        }
        // Right arrow accepts current suggestion
        if right_arrow && self.open_dir_show_suggestions && has_suggestions {
            accept_suggestion = true;
        }

        egui::Window::new("Open Directory")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("Directory path  (~ expands to home; Tab cycles fields; ↑↓ to navigate, → to accept)");
                if let Some(status) = &self.open_dir_fallback_status {
                    ui.add_space(2.0);
                    ui.label(egui::RichText::new(status).small().weak());
                }
                ui.add_space(8.0);

                let input_id = egui::Id::new("open_dir_input");
                
                // Track if input changed
                let prev_input = self.open_dir_input.clone();
                
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.open_dir_input)
                        .id(input_id)
                        .desired_width(480.0)
                        .hint_text("~/path/to/directory"),
                );
                
                // Check if input changed (typing occurred)
                if self.open_dir_input != prev_input {
                    self.open_dir_show_suggestions = true;
                    self.open_dir_autocomplete_cursor = 0;
                }
                
                if self.open_dir_input_wants_focus {
                    response.request_focus();
                    self.open_dir_input_wants_focus = false;
                }
                
                // Set cursor to end after accepting suggestion
                if self.open_dir_cursor_to_end {
                    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), input_id) {
                        use egui::text::{CCursor, CCursorRange};
                        let end_pos = self.open_dir_input.chars().count();
                        state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(end_pos))));
                        state.store(ui.ctx(), input_id);
                    }
                    response.request_focus();
                    self.open_dir_cursor_to_end = false;
                }
                
                // Handle acceptance from Right arrow key
                if accept_suggestion && has_suggestions {
                    if let Some(suggestion) = suggestions.get(self.open_dir_autocomplete_cursor) {
                        self.open_dir_input = suggestion.clone();
                        self.open_dir_autocomplete_cursor = 0;
                        self.open_dir_cursor_to_end = true;
                    }
                }
                
                if response.lost_focus() && submit_key {
                    try_open = true;
                }

                // Render autocomplete dropdown below input
                if self.open_dir_show_suggestions && has_suggestions && !self.open_dir_input.is_empty() {
                    ui.add_space(4.0);
                    
                    egui::Frame::new()
                        .fill(ui.visuals().widgets.inactive.bg_fill)
                        .corner_radius(4.0)
                        .show(ui, |ui| {
                            ui.set_width(480.0);
                            ui.set_max_height(200.0);
                            
                            egui::ScrollArea::vertical()
                                .max_height(200.0)
                                .show(ui, |ui| {
                                    for (idx, suggestion) in suggestions.iter().enumerate() {
                                        let is_selected = idx == self.open_dir_autocomplete_cursor;
                                        let text = egui::RichText::new(suggestion)
                                            .monospace()
                                            .color(if is_selected {
                                                ui.visuals().selection.bg_fill
                                            } else {
                                                ui.visuals().text_color()
                                            });
                                        
                                        let label_response = ui.selectable_label(is_selected, text);
                                        
                                        if label_response.clicked() {
                                            self.open_dir_input = suggestion.clone();
                                            self.open_dir_autocomplete_cursor = 0;
                                            self.open_dir_cursor_to_end = true;
                                        }
                                    }
                                });
                        });
                }

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui
                        .button(format!("{} Open (Enter)", icons::FOLDER_OPEN))
                        .clicked()
                    {
                        try_open = true;
                    }
                    if ui.button("Browse…").clicked() {
                        launch_browse = true;
                    }
                    if ui.button("Cancel (Esc)").clicked() {
                        close_now = true;
                    }
                });
            });

        if try_open {
            let path = Self::expand_user_path(&self.open_dir_input);
            if path.is_dir() {
                debug_notify("Dialog", &format!("Open Dir: opening {}", path.display()));
                self.open_directory(path);
                self.close_open_dir_dialog();
                return;
            }
            self.last_message = Some(format!(
                "Not a directory: {}",
                self.open_dir_input.trim()
            ));
        }

        if launch_browse {
            self.launch_open_dir_browse_chain();
            return;
        }

        if close_now || !open {
            self.close_open_dir_dialog();
        }
    }

    /// Complete the final component of the typed path from its parent directory.
    /// This deliberately does not use the fuzzy file index: that index contains
    /// files, applies ignore rules, and is capped, none of which should affect
    /// direct directory-path completion.
    fn get_open_dir_suggestions(&self) -> Vec<String> {
        let input = self.open_dir_input.trim();
        if input.is_empty() {
            return Vec::new();
        }

        let expanded = Self::expand_user_path(input);
        let ends_with_separator = input.ends_with(std::path::MAIN_SEPARATOR);
        let (parent, prefix) = if ends_with_separator {
            (expanded.as_path(), "")
        } else {
            let Some(parent) = expanded.parent() else {
                return Vec::new();
            };
            let prefix = expanded
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            (parent, prefix)
        };
        let prefix_lower = prefix.to_lowercase();

        let Ok(entries) = fs::read_dir(parent) else {
            return Vec::new();
        };
        let home = dirs::home_dir();
        let preserve_tilde = input == "~" || input.starts_with("~/");
        let mut results = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter_map(|entry| {
                let name = entry.file_name();
                let name = name.to_str()?;
                if !name.to_lowercase().starts_with(&prefix_lower) {
                    return None;
                }
                let path = entry.path();
                let mut display = if preserve_tilde {
                    home.as_ref()
                        .and_then(|home| path.strip_prefix(home).ok())
                        .map(|relative| format!("~/{}", relative.display()))
                        .unwrap_or_else(|| path.display().to_string())
                } else {
                    path.display().to_string()
                };
                display.push(std::path::MAIN_SEPARATOR);
                Some(display)
            })
            .collect::<Vec<_>>();
        results.sort_by_key(|value| value.to_lowercase());
        results.truncate(20);
        results
    }

    fn render_rename_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_rename_dialog {
            return;
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.close_rename_dialog();
            return;
        }

        let mut open = true;
        egui::Window::new("Rename")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 48.0])
            .show(ctx, |ui| {
                ui.label("New name:");
                ui.add_space(4.0);
                let input_id = egui::Id::new("rename_input");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.rename_input)
                        .id(input_id)
                        .desired_width(360.0),
                );
                if self.rename_input_wants_focus {
                    response.request_focus();
                    self.rename_input_wants_focus = false;
                }
                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let new_name = self.rename_input.clone();
                    self.rename_path(&new_name);
                    self.close_rename_dialog();
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Rename (Enter)").clicked() {
                        debug_notify("Dialog", "Rename: button click → confirm");
                        let new_name = self.rename_input.clone();
                        self.rename_path(&new_name);
                        self.close_rename_dialog();
                    }
                    if ui.button("Cancel (Esc)").clicked() {
                        self.close_rename_dialog();
                    }
                });
            });

        if !open {
            self.close_rename_dialog();
        }
    }

    fn render_create_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_create_dialog {
            return;
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.close_create_dialog();
            return;
        }

        let title = if self.create_is_dir {
            "New Directory"
        } else {
            "New File"
        };

        let mut open = true;
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 48.0])
            .show(ctx, |ui| {
                ui.label(format!("Create in: {}", self.create_target_dir.display()));
                ui.add_space(4.0);
                let input_id = egui::Id::new("create_input");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.create_input)
                        .id(input_id)
                        .desired_width(360.0),
                );
                if self.create_input_wants_focus {
                    response.request_focus();
                    self.create_input_wants_focus = false;
                }
                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let name = self.create_input.clone();
                    self.execute_create(&name);
                    self.close_create_dialog();
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Create (Enter)").clicked() {
                        debug_notify("Dialog", "Create: button click → confirm");
                        let name = self.create_input.clone();
                        self.execute_create(&name);
                        self.close_create_dialog();
                    }
                    if ui.button("Cancel (Esc)").clicked() {
                        self.close_create_dialog();
                    }
                });
            });

        if !open {
            self.close_create_dialog();
        }
    }

    fn render_delete_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_delete_dialog {
            return;
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.close_delete_dialog();
            return;
        }

        // Tab cycles focus, D/C move focus to their button
        if self.dialog_tab {
            self.delete_focus = (self.delete_focus + 1) % 2;
        }
        if ctx.input(|i| !i.modifiers.ctrl && i.key_pressed(egui::Key::D)) {
            self.delete_focus = 0;
        }
        if ctx.input(|i| !i.modifiers.ctrl && i.key_pressed(egui::Key::C)) {
            self.delete_focus = 1;
        }

        // Enter confirms the focused button
        let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));

        let accent = self.theme.accent();
        let focus_stroke = egui::Stroke::new(2.0, accent);

        egui::Window::new("Delete")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                let count = self.delete_targets.len();
                if count == 1 {
                    let name = self.delete_targets[0]
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("item");
                    ui.label(format!("Delete '{}'?", name));
                } else {
                    ui.label(format!("Delete {} items?", count));
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Button::new("(D)elete")
                            .sense(egui::Sense::hover())
                            .stroke(if self.delete_focus == 0 {
                                focus_stroke
                            } else {
                                egui::Stroke::NONE
                            }),
                    );
                    ui.add(
                        egui::Button::new("(C)ancel")
                            .sense(egui::Sense::hover())
                            .stroke(if self.delete_focus == 1 {
                                focus_stroke
                            } else {
                                egui::Stroke::NONE
                            }),
                    );
                    if enter && self.delete_focus == 0 {
                        debug_notify("Dialog", "Delete: Enter on Delete");
                        self.execute_delete();
                        self.close_delete_dialog();
                    }
                    if enter && self.delete_focus == 1 {
                        debug_notify("Dialog", "Delete: Enter on Cancel");
                        self.close_delete_dialog();
                    }
                });
            });
    }

    fn render_cut_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_cut_dialog {
            return;
        }

        let yes = ctx.input(|i| !i.modifiers.ctrl && i.key_pressed(egui::Key::Y));
        let no = ctx.input(|i| {
            i.key_pressed(egui::Key::Escape)
                || i.key_pressed(egui::Key::Enter)
                || (!i.modifiers.ctrl && i.key_pressed(egui::Key::N))
        });
        let mut confirm = false;
        let mut cancel = no;

        egui::Window::new("Cut files")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                let count = self.pending_cut_paths.len();
                ui.label(if count == 1 {
                    "Cut this item? It will be moved when you paste it.".to_string()
                } else {
                    format!("Cut these {count} items? They will be moved when you paste them.")
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("(Y)es").clicked() {
                        confirm = true;
                    }
                    if ui.button("(N)o").clicked() {
                        cancel = true;
                    }
                });
            });

        if yes || confirm {
            let sources = std::mem::take(&mut self.pending_cut_paths);
            self.close_cut_dialog();
            self.apply_action(
                ctx,
                AppAction::SetFileClipboard {
                    sources,
                    is_cut: true,
                },
            );
        } else if cancel {
            self.close_cut_dialog();
        }
    }

    fn render_file_action_confirmation(&mut self, ctx: &egui::Context) {
        let Some(message) = self.file_action_confirmation.clone() else {
            return;
        };
        let dismiss = ctx.input(|i| {
            i.key_pressed(egui::Key::Escape)
                || i.key_pressed(egui::Key::Enter)
                || i.key_pressed(egui::Key::Space)
        });
        let mut clicked = false;
        egui::Window::new("File action complete")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(message);
                ui.add_space(8.0);
                clicked = ui.button("OK (Enter)").clicked();
            });
        if dismiss || clicked {
            self.close_file_action_confirmation();
        }
    }

    fn open_terminal_here(&mut self) {
        match Command::new("kitty")
            .arg("--directory")
            .arg(&self.current_dir)
            .spawn()
        {
            Ok(_) => {
                self.last_message = Some(format!("Opened kitty in {}", self.current_dir.display()));
            }
            Err(err) => {
                self.last_message = Some(format!("Failed to open kitty: {}", err));
            }
        }
    }

    fn set_dirty_nav(&mut self, target: DirtyNavTarget) {
        self.dirty_nav_target = Some(target);
        self.dirty_focus = 0;
        self.enter_dialog_mode();
    }

    fn enter_dialog_mode(&mut self) {
        if self.focus_pane != FocusPane::Dialog {
            self.prev_focus_pane = self.focus_pane;
        }
        self.focus_pane = FocusPane::Dialog;
    }

    fn exit_dialog_mode(&mut self) {
        self.focus_pane = self.prev_focus_pane;
    }

    fn in_dialog(&self) -> bool {
        self.focus_pane == FocusPane::Dialog
    }

    fn overlay_state(&self) -> OverlayState {
        if self.dirty_nav_target.is_some() {
            OverlayState::DirtyNav
        } else if self.show_fuzzy_dialog {
            OverlayState::Fuzzy
        } else if self.show_recents_dialog {
            OverlayState::Recents
        } else if self.show_open_dir_dialog || self.open_dir_file_dialog_active() {
            OverlayState::OpenDir
        } else if self.show_rename_dialog {
            OverlayState::Rename
        } else if self.show_create_dialog {
            OverlayState::Create
        } else if self.show_delete_dialog {
            OverlayState::Delete
        } else if self.show_cut_dialog {
            OverlayState::CutConfirm
        } else if self.file_action_confirmation.is_some() {
            OverlayState::FileAction
        } else if self.show_help {
            OverlayState::Help
        } else {
            OverlayState::None
        }
    }

    fn work_mode(&self) -> WorkMode {
        if self.editing {
            WorkMode::Editing
        } else if self.show_preview && self.selected_file_is_markdown() {
            WorkMode::Preview
        } else {
            WorkMode::Viewing
        }
    }

    fn overlay_active(&self) -> bool {
        self.overlay_state() != OverlayState::None
    }

    fn close_open_dir_dialog(&mut self) {
        self.show_open_dir_dialog = false;
        self.open_dir_fallback_status = None;
        self.open_dir_autocomplete_cursor = 0;
        self.open_dir_show_suggestions = false;
        self.open_dir_cursor_to_end = false;
        self.exit_dialog_mode();
    }

    fn close_recents_dialog(&mut self) {
        self.show_recents_dialog = false;
        self.exit_dialog_mode();
    }

    fn close_fuzzy_dialog(&mut self) {
        self.show_fuzzy_dialog = false;
        self.exit_dialog_mode();
    }

    fn close_rename_dialog(&mut self) {
        self.show_rename_dialog = false;
        self.exit_dialog_mode();
    }

    fn close_create_dialog(&mut self) {
        self.show_create_dialog = false;
        self.exit_dialog_mode();
    }

    fn close_delete_dialog(&mut self) {
        self.show_delete_dialog = false;
        self.exit_dialog_mode();
    }

    fn close_cut_dialog(&mut self) {
        self.show_cut_dialog = false;
        self.pending_cut_paths.clear();
        self.exit_dialog_mode();
    }

    fn close_file_action_confirmation(&mut self) {
        self.file_action_confirmation = None;
        self.exit_dialog_mode();
    }

    fn close_dirty_nav(&mut self) {
        self.dirty_nav_target = None;
        self.exit_dialog_mode();
    }

    fn collect_intents(
        &mut self,
        ctx: &egui::Context,
        filter_id: egui::Id,
        doc_find_id: egui::Id,
    ) -> CollectedIntents {
        let key_up = ctx.input(|i| i.key_pressed(egui::Key::ArrowUp));
        let key_down = ctx.input(|i| i.key_pressed(egui::Key::ArrowDown));
        let key_left = ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft));
        let key_right = ctx.input(|i| i.key_pressed(egui::Key::ArrowRight));
        let key_enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
        let key_space = ctx.input(|i| i.key_pressed(egui::Key::Space));
        let key_delete = ctx.input(|i| i.key_pressed(egui::Key::Delete));
        let ctrl_left = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::ArrowLeft));
        let ctrl_right = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::ArrowRight));
        let ctrl_up = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::ArrowUp));
        let ctrl_t = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::T));
        let ctrl_o = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::O));
        let key_z = ctx.input(|i| !i.modifiers.ctrl && i.key_pressed(egui::Key::Z));
        let ctrl_l = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::L));
        let key_esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let ctrl_h = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::H));
        let ctrl_s = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::S));
        let ctrl_f = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::F));
        let ctrl_p = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::P));
        let ctrl_e = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::E));
        let ctrl_w = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::W));
        let ctrl_q = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Q));
        let ctrl_r = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::R));
        let ctrl_n =
            ctx.input(|i| i.modifiers.ctrl && !i.modifiers.shift && i.key_pressed(egui::Key::N));
        let ctrl_shift_n =
            ctx.input(|i| i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::N));
        let ctrl_c = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::C));
        let ctrl_x = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::X));
        let ctrl_v = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::V));
        let key_e = ctx.input(|i| !i.modifiers.ctrl && i.key_pressed(egui::Key::E));

        // Table editing keybinds (Alt+R = add row, Alt+C = add column)
        // Must consume from events list directly — consume_key only eats the Key
        // event, but Alt+letter also generates a Text("r"/"c") event that TextEdit
        // would insert as a character.
        let (alt_r, alt_c) = if self.editing && self.focus_pane == FocusPane::Viewer {
            let mut got_r = false;
            let mut got_c = false;
            ctx.input_mut(|input| {
                // First pass: detect Alt+R / Alt+C key events
                for event in input.events.iter() {
                    if let egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } = event
                    {
                        if modifiers.alt && !modifiers.ctrl && !modifiers.shift {
                            match key {
                                egui::Key::R => got_r = true,
                                egui::Key::C => got_c = true,
                                _ => {}
                            }
                        }
                    }
                }
                // Second pass: remove both Key and Text events so TextEdit
                // doesn't also type the letter
                if got_r || got_c {
                    input.events.retain(|event| match event {
                        egui::Event::Key {
                            key,
                            pressed: true,
                            modifiers,
                            ..
                        } if modifiers.alt
                            && !modifiers.ctrl
                            && matches!(key, egui::Key::R | egui::Key::C) =>
                        {
                            false
                        }
                        egui::Event::Text(t) if (got_r && t == "r") || (got_c && t == "c") => false,
                        _ => true,
                    });
                }
            });
            (got_r, got_c)
        } else {
            (false, false)
        };

        let _ctrl_down = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::ArrowDown));
        let page_up = ctx.input(|i| i.key_pressed(egui::Key::PageUp));
        let page_down = ctx.input(|i| i.key_pressed(egui::Key::PageDown));
        let ctrl_shift_mods = egui::Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        };

        // Consume Ctrl+Z/Y when editing so egui's TextEdit doesn't also process them
        let ctrl_z = if self.editing {
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Z))
        } else {
            false
        };
        let ctrl_y = if self.editing {
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Y))
        } else {
            false
        };

        let editor_select_para_up = if self.editing && self.focus_pane == FocusPane::Viewer {
            ctx.input_mut(|i| i.consume_key(ctrl_shift_mods, egui::Key::ArrowUp))
        } else {
            false
        };
        let editor_select_para_down = if self.editing && self.focus_pane == FocusPane::Viewer {
            ctx.input_mut(|i| i.consume_key(ctrl_shift_mods, egui::Key::ArrowDown))
        } else {
            false
        };
        let editor_select_word_left = if self.editing && self.focus_pane == FocusPane::Viewer {
            ctx.input_mut(|i| i.consume_key(ctrl_shift_mods, egui::Key::ArrowLeft))
        } else {
            false
        };
        let editor_select_word_right = if self.editing && self.focus_pane == FocusPane::Viewer {
            ctx.input_mut(|i| i.consume_key(ctrl_shift_mods, egui::Key::ArrowRight))
        } else {
            false
        };

        // Consume Ctrl+Up/Down when editing so egui doesn't jump to document start/end
        let editor_para_up = if self.editing && self.focus_pane == FocusPane::Viewer {
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::ArrowUp))
        } else {
            false
        };
        let editor_para_down = if self.editing && self.focus_pane == FocusPane::Viewer {
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::ArrowDown))
        } else {
            false
        };
        let editor_word_left = if self.editing && self.focus_pane == FocusPane::Viewer {
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::ArrowLeft))
        } else {
            false
        };
        let editor_word_right = if self.editing && self.focus_pane == FocusPane::Viewer {
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::ArrowRight))
        } else {
            false
        };

        // In Dialog mode: consume Tab only for dialogs with no focusable
        // chain (delete confirm, dirty-nav). Dialogs with text inputs and
        // multiple buttons (open-dir, rename, create) let Tab cycle fields.
        let dialog_tab = if self.in_dialog() {
            let no_input_dialog = self.show_delete_dialog
                || self.show_cut_dialog
                || self.file_action_confirmation.is_some()
                || self.dirty_nav_target.is_some();
            let tab = if no_input_dialog {
                ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab))
            } else {
                false
            };
            if no_input_dialog {
                ctx.input_mut(|i| {
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp);
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown);
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft);
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight);
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Space);
                });
            }
            tab
        } else {
            false
        };

        let filter_focused = ctx.memory(|m| m.focused().is_some_and(|id| id == filter_id));
        let doc_find_focused = ctx.memory(|m| m.focused().is_some_and(|id| id == doc_find_id));
        let editor_focused = self.editing && self.focus_pane == FocusPane::Viewer;
        let doc_find_prev = if doc_find_focused {
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::Enter))
        } else {
            false
        };
        let doc_find_next = if doc_find_focused {
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
        } else {
            false
        };

        CollectedIntents {
            key_up,
            key_down,
            key_left,
            key_right,
            key_enter,
            key_space,
            key_delete,
            ctrl_left,
            ctrl_right,
            ctrl_up,
            ctrl_t,
            ctrl_o,
            key_z,
            ctrl_l,
            key_esc,
            ctrl_h,
            ctrl_s,
            ctrl_f,
            ctrl_p,
            ctrl_e,
            ctrl_w,
            ctrl_q,
            ctrl_r,
            ctrl_n,
            ctrl_shift_n,
            ctrl_c,
            ctrl_x,
            ctrl_v,
            key_e,
            alt_r,
            alt_c,
            page_up,
            page_down,
            ctrl_z,
            ctrl_y,
            editor_para_up,
            editor_para_down,
            editor_word_left,
            editor_word_right,
            editor_select_para_up,
            editor_select_para_down,
            editor_select_word_left,
            editor_select_word_right,
            doc_find_prev,
            doc_find_next,
            dialog_tab,
            filter_focused,
            editor_focused,
            doc_find_focused,
        }
    }

    fn apply_action(&mut self, ctx: &egui::Context, action: AppAction) {
        match action {
            AppAction::FilterFocusToggle {
                filter_id,
                currently_focused,
            } => {
                if currently_focused {
                    debug_notify("Keybind", "Ctrl+S/F: filter unfocus (toggle off)");
                    ctx.memory_mut(|m| m.surrender_focus(filter_id));
                } else {
                    debug_notify("Keybind", "Ctrl+S/F: filter focus (toggle on)");
                    ctx.memory_mut(|m| m.request_focus(filter_id));
                }
            }
            AppAction::FilterClearAndUnfocus { filter_id } => {
                self.filter.clear();
                self.refresh_nav_rows();
                ctx.memory_mut(|m| m.surrender_focus(filter_id));
            }
            AppAction::ShowOpenDirDialog => {
                self.launch_open_dir_flow();
            }
            AppAction::ShowFuzzyDialog => {
                self.show_fuzzy_dialog = true;
                self.fuzzy_input_wants_focus = true;
                self.fuzzy_search.open_for_root(self.current_dir.clone());
                self.enter_dialog_mode();
            }
            AppAction::ShowRecentsDialog => {
                self.recent_files.retain(|p| p.is_file());
                self.recent_dirs.retain(|p| p.is_dir());
                self.recent_files.truncate(RECENTS_CAP);
                self.recent_dirs.truncate(RECENTS_CAP);
                self.save_recents_to_disk();
                self.show_recents_dialog = true;
                self.recents_cursor = 0;
                self.enter_dialog_mode();
            }
            AppAction::OpenTerminalHere => self.open_terminal_here(),
            AppAction::TogglePreview => {
                self.show_preview = !self.show_preview;
            }
            AppAction::ToggleSidebar => {
                self.show_sidebar = !self.show_sidebar;
                if !self.show_sidebar {
                    self.focus_pane = FocusPane::Viewer;
                }
            }
            AppAction::SetDirtyNav(target) => self.set_dirty_nav(target),
            AppAction::CloseFile => self.close_file(),
            AppAction::QuitApp => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            AppAction::OpenParentDirectory => self.open_parent_directory(),
            AppAction::OpenRenameDialogForPath(path) => {
                self.rename_target = Some(path.clone());
                self.rename_input = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                self.show_rename_dialog = true;
                self.rename_input_wants_focus = true;
                self.enter_dialog_mode();
            }
            AppAction::CloseRenameDialog => self.close_rename_dialog(),
            AppAction::OpenCreateDialog { is_dir } => {
                self.create_target_dir = self.cursor_target_dir();
                self.create_is_dir = is_dir;
                self.create_input.clear();
                self.show_create_dialog = true;
                self.create_input_wants_focus = true;
                self.enter_dialog_mode();
            }
            AppAction::OpenDeleteDialog { targets } => {
                if !targets.is_empty() {
                    self.delete_targets = targets;
                    self.delete_focus = 1; // default to Cancel
                    self.show_delete_dialog = true;
                    self.enter_dialog_mode();
                }
            }
            AppAction::OpenCutDialog { sources } => {
                if !sources.is_empty() {
                    self.pending_cut_paths = sources;
                    self.show_cut_dialog = true;
                    self.enter_dialog_mode();
                }
            }
            AppAction::SetFileClipboard { sources, is_cut } => {
                if !sources.is_empty() {
                    let verb = if is_cut { "Cut" } else { "Copied" };
                    self.clipboard_paths = sources;
                    self.clipboard_is_cut = is_cut;
                    self.last_message =
                        Some(format!("{} {} item(s)", verb, self.clipboard_paths.len()));
                    // Seed system clipboard so Ctrl+V generates Event::Paste
                    let paths_text: String = self
                        .clipboard_paths
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join("\n");
                    ctx.copy_text(paths_text);
                    if !is_cut {
                        let message = self.last_message.clone().unwrap_or_default();
                        self.file_action_confirmation = Some(message);
                        self.enter_dialog_mode();
                    }
                }
            }
            AppAction::PasteFileClipboard => {
                if !self.clipboard_paths.is_empty() {
                    self.execute_paste();
                }
            }
            AppAction::EnterEditMode => self.enter_edit_mode(),
            AppAction::SaveAndExitEditMode => {
                debug_notify("Keybind", "Ctrl+S: editor save");
                self.undo_history.push_snapshot(&self.edit_buffer);
                self.exit_edit_mode(true);
            }
            AppAction::ExitEditModeDiscard => self.exit_edit_mode(false),
            AppAction::SetFocusPane(pane) => self.focus_pane = pane,
            AppAction::DirtyDialogSaveAndApply { target } => {
                debug_notify("Dialog", "Dirty: Save");
                self.exit_edit_mode(true);
                self.apply_dirty_nav(ctx, &target);
                self.close_dirty_nav();
            }
            AppAction::DirtyDialogDiscardAndApply { target } => {
                debug_notify("Dialog", "Dirty: Discard");
                self.exit_edit_mode(false);
                self.apply_dirty_nav(ctx, &target);
                self.close_dirty_nav();
            }
            AppAction::DirtyDialogCancelToEditor => {
                debug_notify("Dialog", "Dirty: Cancel → back to editing");
                self.close_dirty_nav();
                self.focus_pane = FocusPane::Viewer;
                self.editor_wants_focus = true;
            }
        }
    }

    fn route_undo_layer(&mut self, intents: &CollectedIntents) {
        if self.editing {
            self.undo_history.track_changes(&self.edit_buffer);

            if intents.ctrl_z {
                if self.undo_history.undo(&mut self.edit_buffer) {
                    self.last_message = Some("Undo".to_string());
                }
            }
            if intents.ctrl_y {
                if self.undo_history.redo(&mut self.edit_buffer) {
                    self.last_message = Some("Redo".to_string());
                }
            }
        }
    }

    fn route_global_layer(
        &mut self,
        ctx: &egui::Context,
        intents: &CollectedIntents,
        filter_id: egui::Id,
        doc_find_id: egui::Id,
        filter_focused: bool,
        kb_active: bool,
        overlay_active: bool,
    ) {
        if intents.ctrl_f && self.focus_pane == FocusPane::Files {
            self.apply_action(
                ctx,
                AppAction::FilterFocusToggle {
                    filter_id,
                    currently_focused: filter_focused,
                },
            );
        }
        if intents.ctrl_s && self.focus_pane == FocusPane::Files {
            self.apply_action(
                ctx,
                AppAction::FilterFocusToggle {
                    filter_id,
                    currently_focused: filter_focused,
                },
            );
        }
        if intents.ctrl_f
            && self.focus_pane == FocusPane::Viewer
            && self.selected_file.is_some()
            && !overlay_active
        {
            self.toggle_document_find(ctx, doc_find_id);
        }
        if filter_focused && intents.key_esc {
            self.apply_action(ctx, AppAction::FilterClearAndUnfocus { filter_id });
        }
        if intents.doc_find_focused && intents.key_esc {
            self.close_document_find(ctx, doc_find_id);
        }

        if intents.ctrl_o && !overlay_active {
            self.apply_action(ctx, AppAction::ShowOpenDirDialog);
        }
        if intents.key_z && kb_active && self.focus_pane == FocusPane::Files && !overlay_active {
            self.apply_action(ctx, AppAction::ShowFuzzyDialog);
        }
        if intents.ctrl_l && !overlay_active {
            self.apply_action(ctx, AppAction::ShowRecentsDialog);
        }
        if intents.ctrl_t && kb_active {
            self.apply_action(ctx, AppAction::OpenTerminalHere);
        }
        if intents.ctrl_p && kb_active && !self.editing && self.selected_file_is_markdown() {
            self.apply_action(ctx, AppAction::TogglePreview);
        }
        if intents.ctrl_e && !overlay_active {
            self.apply_action(ctx, AppAction::ToggleSidebar);
        }
        if intents.ctrl_w && self.selected_file.is_some() && !overlay_active {
            if self.is_dirty() {
                self.apply_action(ctx, AppAction::SetDirtyNav(DirtyNavTarget::CloseFile));
            } else {
                self.apply_action(ctx, AppAction::CloseFile);
            }
        }
        if intents.ctrl_q && !overlay_active {
            if self.is_dirty() {
                self.apply_action(ctx, AppAction::SetDirtyNav(DirtyNavTarget::QuitApp));
            } else {
                self.apply_action(ctx, AppAction::QuitApp);
            }
        }
        if intents.ctrl_up && kb_active && self.focus_pane == FocusPane::Files {
            if self.is_dirty() {
                self.apply_action(
                    ctx,
                    AppAction::SetDirtyNav(DirtyNavTarget::EnterDirectory(
                        self.current_dir
                            .parent()
                            .map(Path::to_path_buf)
                            .unwrap_or_else(|| self.current_dir.clone()),
                    )),
                );
            } else {
                self.apply_action(ctx, AppAction::OpenParentDirectory);
            }
        }

        // Ctrl+R — rename (toggle, works from both panes)
        if intents.ctrl_r && !overlay_active {
            let target = if self.focus_pane == FocusPane::Files {
                self.current_row()
                    .filter(|r| !matches!(r.kind, FileRowKind::Parent))
                    .map(|r| r.path.clone())
            } else {
                self.selected_file.clone()
            };
            if let Some(path) = target {
                self.apply_action(ctx, AppAction::OpenRenameDialogForPath(path));
            }
        } else if intents.ctrl_r && self.show_rename_dialog {
            self.apply_action(ctx, AppAction::CloseRenameDialog);
        }

        // Ctrl+N / Ctrl+Shift+N — create file/directory
        if (intents.ctrl_n || intents.ctrl_shift_n)
            && kb_active
            && self.focus_pane == FocusPane::Files
        {
            self.apply_action(
                ctx,
                AppAction::OpenCreateDialog {
                    is_dir: intents.ctrl_shift_n,
                },
            );
        }

        // File clipboard (Ctrl+C/X/V) — filetree only
        if kb_active && self.focus_pane == FocusPane::Files {
            // winit translates Ctrl+C/X/V into Event::Copy/Cut/Paste
            // rather than Event::Key, so we intercept those directly.
            let mut file_copy = intents.ctrl_c;
            let mut file_cut = intents.ctrl_x;
            let mut file_paste = intents.ctrl_v;
            ctx.input_mut(|input| {
                input.events.retain(|event| match event {
                    egui::Event::Copy => {
                        file_copy = true;
                        false
                    }
                    egui::Event::Cut => {
                        file_cut = true;
                        false
                    }
                    egui::Event::Paste(_) => {
                        file_paste = true;
                        false
                    }
                    _ => true,
                });
            });

            if file_copy || file_cut {
                let sources: Vec<PathBuf> = if !self.selected_paths.is_empty() {
                    self.selected_paths.iter().cloned().collect()
                } else {
                    self.current_row()
                        .filter(|r| !matches!(r.kind, FileRowKind::Parent))
                        .map(|r| vec![r.path.clone()])
                        .unwrap_or_default()
                };
                if file_cut {
                    self.apply_action(ctx, AppAction::OpenCutDialog { sources });
                } else {
                    self.apply_action(
                        ctx,
                        AppAction::SetFileClipboard {
                            sources,
                            is_cut: false,
                        },
                    );
                }
            }
            if file_paste {
                self.apply_action(ctx, AppAction::PasteFileClipboard);
            }
        }

        // Delete
        if intents.key_delete && kb_active && self.focus_pane == FocusPane::Files {
            let targets: Vec<PathBuf> = if !self.selected_paths.is_empty() {
                self.selected_paths.iter().cloned().collect()
            } else {
                self.current_row()
                    .filter(|r| !matches!(r.kind, FileRowKind::Parent))
                    .map(|r| vec![r.path.clone()])
                    .unwrap_or_default()
            };
            self.apply_action(ctx, AppAction::OpenDeleteDialog { targets });
        }
    }

    fn route_mode_layer(
        &mut self,
        ctx: &egui::Context,
        intents: &CollectedIntents,
        kb_active: bool,
        overlay_active: bool,
    ) {
        // Editor keybinds
        if kb_active && self.focus_pane == FocusPane::Viewer && !self.editing && intents.key_e {
            self.apply_action(ctx, AppAction::EnterEditMode);
        }
        if intents.ctrl_s && self.focus_pane == FocusPane::Viewer && self.editing {
            self.apply_action(ctx, AppAction::SaveAndExitEditMode);
        }
        if intents.key_esc
            && !intents.doc_find_focused
            && self.editing
            && self.focus_pane == FocusPane::Viewer
            && !overlay_active
        {
            if self.is_dirty() {
                self.apply_action(ctx, AppAction::SetDirtyNav(DirtyNavTarget::FocusFiles));
            } else {
                self.apply_action(ctx, AppAction::ExitEditModeDiscard);
            }
        }

        // Editor paragraph navigation (Ctrl+Up/Down, Shift+Ctrl+Up/Down)
        if intents.editor_para_up
            || intents.editor_para_down
            || intents.editor_select_para_up
            || intents.editor_select_para_down
        {
            let moving_up = intents.editor_para_up || intents.editor_select_para_up;
            let extend_selection = intents.editor_select_para_up || intents.editor_select_para_down;
            self.move_editor_cursor_custom(ctx, extend_selection, |text, pos| {
                if moving_up {
                    find_prev_paragraph(text, pos)
                } else {
                    find_next_paragraph(text, pos)
                }
            });
        }

        // Editor word navigation (Ctrl+Left/Right, Shift+Ctrl+Left/Right)
        if intents.editor_word_left
            || intents.editor_word_right
            || intents.editor_select_word_left
            || intents.editor_select_word_right
        {
            let moving_left = intents.editor_word_left || intents.editor_select_word_left;
            let extend_selection =
                intents.editor_select_word_left || intents.editor_select_word_right;
            self.move_editor_cursor_custom(ctx, extend_selection, |text, pos| {
                if moving_left {
                    find_prev_word(text, pos)
                } else {
                    find_next_word(text, pos)
                }
            });
        }

        if intents.doc_find_prev {
            self.find_in_document(ctx, false);
        }
        if intents.doc_find_next {
            self.find_in_document(ctx, true);
        }

        // Table editing (Alt+R = add row, Alt+C = add column)
        if intents.alt_r || intents.alt_c {
            let editor_id = egui::Id::new("viewer_editor");
            if let Some(mut state) = egui::TextEdit::load_state(ctx, editor_id) {
                if let Some(ccursor_range) = state.cursor.char_range() {
                    let pos = ccursor_range.primary.index;
                    if let Some(result) = if intents.alt_r {
                        table_add_row(&self.edit_buffer, pos)
                    } else {
                        table_add_column(&self.edit_buffer, pos)
                    } {
                        self.edit_buffer = result.new_text;
                        let new_ccursor = egui::text::CCursor::new(result.new_cursor);
                        state
                            .cursor
                            .set_char_range(Some(egui::text::CCursorRange::one(new_ccursor)));
                        state.store(ctx, editor_id);
                        self.last_message = Some(if intents.alt_r {
                            "Added table row".to_string()
                        } else {
                            "Added table column".to_string()
                        });
                    } else {
                        self.last_message = Some("Cursor is not inside a table".to_string());
                    }
                }
            }
        }

        // Viewer scroll
        self.viewer_scroll_delta = 0.0;
        let page_scroll_step = 400.0;
        if self.focus_pane == FocusPane::Viewer {
            if intents.page_up {
                self.viewer_scroll_delta = -page_scroll_step;
            }
            if intents.page_down {
                self.viewer_scroll_delta = page_scroll_step;
            }
        }
        if kb_active && self.focus_pane == FocusPane::Viewer && !self.editing {
            let scroll_step = 40.0;
            if intents.key_up {
                self.viewer_scroll_delta += -scroll_step;
            }
            if intents.key_down {
                self.viewer_scroll_delta += scroll_step;
            }
        }

        if self.editing && (intents.page_up || intents.page_down) {
            let line_h = self.viewer_line_height_px.max(1.0);
            let page_lines = ((page_scroll_step / line_h).round() as isize).max(1);
            let delta_lines = if intents.page_up {
                -page_lines
            } else {
                page_lines
            };
            self.move_editor_cursor_by_line_delta(ctx, delta_lines);
        }
    }

    fn route_overlay_layer(
        &mut self,
        ctx: &egui::Context,
        intents: &CollectedIntents,
        kb_active: bool,
    ) -> bool {
        // Dirty nav dialog
        if let Some(target) = self.dirty_nav_target.clone() {
            // Tab cycles focus, S/D/C move focus to their button
            if self.dialog_tab {
                self.dirty_focus = (self.dirty_focus + 1) % 3;
            }
            if intents.ctrl_s || ctx.input(|i| !i.modifiers.ctrl && i.key_pressed(egui::Key::S)) {
                self.dirty_focus = 0;
            }
            if ctx.input(|i| !i.modifiers.ctrl && i.key_pressed(egui::Key::D)) {
                self.dirty_focus = 1;
            }
            if ctx.input(|i| !i.modifiers.ctrl && i.key_pressed(egui::Key::C)) {
                self.dirty_focus = 2;
            }
            if intents.key_esc {
                self.close_dirty_nav();
                // Esc = Cancel: restore editor focus
                self.focus_pane = FocusPane::Viewer;
                self.editor_wants_focus = true;
                return true;
            }

            let enter = intents.key_enter;
            let accent = self.theme.accent();
            let focus_stroke = egui::Stroke::new(2.0, accent);

            egui::Window::new("Unsaved Changes")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("You have unsaved changes.");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Button::new("(S)ave")
                                .sense(egui::Sense::hover())
                                .stroke(if self.dirty_focus == 0 {
                                    focus_stroke
                                } else {
                                    egui::Stroke::NONE
                                }),
                        );
                        ui.add(
                            egui::Button::new("(D)iscard")
                                .sense(egui::Sense::hover())
                                .stroke(if self.dirty_focus == 1 {
                                    focus_stroke
                                } else {
                                    egui::Stroke::NONE
                                }),
                        );
                        ui.add(
                            egui::Button::new("(C)ancel")
                                .sense(egui::Sense::hover())
                                .stroke(if self.dirty_focus == 2 {
                                    focus_stroke
                                } else {
                                    egui::Stroke::NONE
                                }),
                        );
                    });
                });

            // Handle actions after render (outside closure)
            if enter && self.dirty_focus == 0 {
                self.apply_action(
                    ctx,
                    AppAction::DirtyDialogSaveAndApply {
                        target: target.clone(),
                    },
                );
            } else if enter && self.dirty_focus == 1 {
                self.apply_action(
                    ctx,
                    AppAction::DirtyDialogDiscardAndApply {
                        target: target.clone(),
                    },
                );
            } else if enter && self.dirty_focus == 2 {
                self.apply_action(ctx, AppAction::DirtyDialogCancelToEditor);
            }
            return true;
        }

        // Fuzzy search dialog
        if self.show_fuzzy_dialog {
            if intents.key_esc {
                self.close_fuzzy_dialog();
                return true;
            }
            if intents.key_up {
                self.fuzzy_search.move_cursor_up();
            }
            if intents.key_down {
                self.fuzzy_search.move_cursor_down();
            }
            if intents.key_enter {
                self.open_selected_fuzzy();
                return true;
            }

            let mut open = true;
            let shadcn_theme = self.shadcn_theme.clone();
            let viewport_size = ctx.input(|i| i.content_rect().size());
            let max_w = (viewport_size.x - 24.0).max(260.0);
            let max_h = (viewport_size.y - 24.0).max(240.0);
            let rows = self.fuzzy_search.results().to_vec();
            let sample_count = rows.len().min(24);
            let mut file_chars = "File".chars().count();
            let mut path_chars = "Path".chars().count();
            for row in rows.iter().take(sample_count) {
                file_chars = file_chars.max(row.file_name.chars().count());
                path_chars = path_chars.max(row.relative.chars().count());
            }
            file_chars = file_chars.clamp(20, 42);
            path_chars = path_chars.clamp(32, 84);
            let score_chars = 7usize;
            let pad_chars = 14usize;
            let total_chars = file_chars + path_chars + score_chars + pad_chars;
            let char_w = ctx.fonts_mut(|fonts| {
                let galley = fonts.layout_no_wrap(
                    "M".to_string(),
                    egui::FontId::proportional(14.0),
                    self.theme.view_fg(),
                );
                galley.size().x.max(7.0)
            });
            let preferred_w = (total_chars as f32 * char_w).clamp(560.0, 980.0);
            let win_w = preferred_w.min(max_w);
            let visible_rows = rows.len().clamp(8, 16) as f32;
            let row_h = 24.0;
            let chrome_h = 150.0;
            let preferred_h = (chrome_h + visible_rows * row_h).clamp(340.0, 700.0);
            let win_h = preferred_h.min(max_h);
            egui::Window::new("Fuzzy Search")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .fixed_size([win_w, win_h])
                .show(ctx, |ui| {
                    egui::ScrollArea::both()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Jump To File").strong());
                                ui.separator();
                                ui.label(
                                    egui::RichText::new("Z open · Enter open · Esc close")
                                        .small()
                                        .weak(),
                                );
                            });
                            ui.add_space(8.0);

                            card(
                                ui,
                                &shadcn_theme,
                                CardProps::default().variant(CardVariant::Subtle),
                                |ui| {
                                    let mut query = self.fuzzy_search.query().to_string();
                                    let response = ui.add(
                                        egui::TextEdit::singleline(&mut query)
                                            .hint_text("Type file name or path...")
                                            .desired_width(ui.available_width()),
                                    );
                                    if self.fuzzy_input_wants_focus {
                                        response.request_focus();
                                        self.fuzzy_input_wants_focus = false;
                                    }
                                    if response.changed() {
                                        self.fuzzy_search.set_query(query);
                                    }
                                },
                            );
                            ui.add_space(10.0);

                            egui::ScrollArea::vertical()
                                .max_height(ui.available_height())
                                .show(ui, |ui| {
                                    table(
                                        ui,
                                        &shadcn_theme,
                                        TableProps::new().variant(TableVariant::Muted),
                                        |ui, table_ctx| {
                                            table_header(ui, table_ctx, |ui| {
                                                table_row(
                                                    ui,
                                                    table_ctx,
                                                    TableRowProps::new("fuzzy_head"),
                                                    |ui| {
                                                        table_head(
                                                            ui,
                                                            table_ctx,
                                                            TableCellProps::new(),
                                                            |ui| {
                                                                ui.label(
                                                                    egui::RichText::new("File")
                                                                        .strong(),
                                                                );
                                                            },
                                                        );
                                                        table_head(
                                                            ui,
                                                            table_ctx,
                                                            TableCellProps::new(),
                                                            |ui| {
                                                                ui.label(
                                                                    egui::RichText::new("Path")
                                                                        .strong(),
                                                                );
                                                            },
                                                        );
                                                        table_head(
                                                            ui,
                                                            table_ctx,
                                                            TableCellProps::new(),
                                                            |ui| {
                                                                ui.label(
                                                                    egui::RichText::new("Score")
                                                                        .strong(),
                                                                );
                                                            },
                                                        );
                                                    },
                                                );
                                            });
                                            table_body(ui, table_ctx, |ui| {
                                                let rows = self.fuzzy_search.results().to_vec();
                                                if rows.is_empty() {
                                                    table_row(
                                                        ui,
                                                        table_ctx,
                                                        TableRowProps::new("fuzzy_empty")
                                                            .hoverable(false),
                                                        |ui| {
                                                            table_cell(
                                                                ui,
                                                                table_ctx,
                                                                TableCellProps::new().fill(true),
                                                                |ui| {
                                                                    ui.label("No matches.");
                                                                },
                                                            );
                                                        },
                                                    );
                                                } else {
                                                    for (idx, row) in rows.iter().enumerate() {
                                                        let resp = table_row(
                                                            ui,
                                                            table_ctx,
                                                            TableRowProps::new(format!("fz_{idx}"))
                                                                .selected(
                                                                    idx == self.fuzzy_search.cursor(),
                                                                ),
                                                            |ui| {
                                                                table_cell(
                                                                    ui,
                                                                    table_ctx,
                                                                    TableCellProps::new(),
                                                                    |ui| {
                                                                        ui.label(&row.file_name);
                                                                    },
                                                                );
                                                                table_cell(
                                                                    ui,
                                                                    table_ctx,
                                                                    TableCellProps::new(),
                                                                    |ui| {
                                                                        ui.label(
                                                                            egui::RichText::new(
                                                                                &row.relative,
                                                                            )
                                                                            .small()
                                                                            .weak(),
                                                                        );
                                                                    },
                                                                );
                                                                table_cell(
                                                                    ui,
                                                                    table_ctx,
                                                                    TableCellProps::new(),
                                                                    |ui| {
                                                                        ui.label(format!(
                                                                            "{}",
                                                                            row.score
                                                                        ));
                                                                    },
                                                                );
                                                            },
                                                        );
                                                        if resp.response.clicked() {
                                                            self.fuzzy_search.set_cursor(idx);
                                                        }
                                                        if resp.response.double_clicked() {
                                                            self.fuzzy_search.set_cursor(idx);
                                                            self.open_selected_fuzzy();
                                                        }
                                                    }
                                                }
                                            });
                                        },
                                    );
                                });
                        });
                });

            if !open {
                self.close_fuzzy_dialog();
            }
            return true;
        }

        // Recents dialog
        if self.show_recents_dialog {
            if intents.key_esc {
                self.close_recents_dialog();
                return true;
            }

            let switch_files = ctx.input(|i| {
                !i.modifiers.ctrl
                    && !i.modifiers.alt
                    && !i.modifiers.shift
                    && i.key_pressed(egui::Key::F)
            });
            let switch_dirs = ctx.input(|i| {
                !i.modifiers.ctrl
                    && !i.modifiers.alt
                    && !i.modifiers.shift
                    && i.key_pressed(egui::Key::D)
            });
            if switch_files {
                self.recents_mode = RecentsMode::Files;
                self.recents_cursor = 0;
            }
            if switch_dirs {
                self.recents_mode = RecentsMode::Directories;
                self.recents_cursor = 0;
            }

            let len = self.recents_len();
            if intents.key_down && len > 0 {
                self.recents_cursor = (self.recents_cursor + 1).min(len - 1);
            }
            if intents.key_up && len > 0 {
                self.recents_cursor = self.recents_cursor.saturating_sub(1);
            }
            if intents.key_enter {
                self.open_selected_recent();
                return true;
            }

            let mut open = true;
            let viewport_size = ctx.input(|i| i.content_rect().size());
            let max_w = (viewport_size.x - 12.0).max(240.0);
            let max_h = (viewport_size.y - 12.0).max(220.0);
            let win_w = (viewport_size.x * 0.92).clamp(320.0, 960.0).min(max_w);
            let win_h = (viewport_size.y * 0.88).clamp(260.0, 760.0).min(max_h);
            egui::Window::new("Recents")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .fixed_size([win_w, win_h])
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Recent Locations").strong());
                        ui.separator();
                        ui.label(
                            egui::RichText::new("Ctrl+L open, Esc close, Enter open")
                                .small()
                                .weak(),
                        );
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let files_label = format!(
                            "(F) {} ({})",
                            RecentsMode::Files.label(),
                            self.recent_files.len()
                        );
                        let dirs_label = format!(
                            "(D) {} ({})",
                            RecentsMode::Directories.label(),
                            self.recent_dirs.len()
                        );
                        ui.label(if self.recents_mode == RecentsMode::Files {
                            egui::RichText::new(files_label)
                                .strong()
                                .color(self.theme.accent())
                        } else {
                            egui::RichText::new(files_label)
                        });
                        ui.separator();
                        ui.label(if self.recents_mode == RecentsMode::Directories {
                            egui::RichText::new(dirs_label)
                                .strong()
                                .color(self.theme.accent())
                        } else {
                            egui::RichText::new(dirs_label)
                        });
                    });
                    ui.add_space(8.0);

                    let items: Vec<PathBuf> = if self.recents_mode == RecentsMode::Files {
                        self.recent_files.clone()
                    } else {
                        self.recent_dirs.clone()
                    };
                    if items.is_empty() {
                        ui.label(egui::RichText::new("No recent entries yet.").weak());
                    } else {
                        egui::ScrollArea::vertical()
                            .max_height(ui.available_height())
                            .show(ui, |ui| {
                                for (idx, path) in items.iter().enumerate() {
                                    let selected = idx == self.recents_cursor;
                                    let icon = if self.recents_mode == RecentsMode::Files {
                                        icons::FILE_TEXT
                                    } else {
                                        icons::FOLDER
                                    };
                                    let label = format!("{}  {}", icon, path.display());
                                    let response = ui.selectable_label(selected, label);
                                    if response.clicked() {
                                        self.recents_cursor = idx;
                                    }
                                    if response.double_clicked() {
                                        self.open_selected_recent();
                                    }
                                }
                            });
                    }
                });
            if !open {
                self.close_recents_dialog();
            }
            return true;
        }

        // Help overlay
        if kb_active && intents.ctrl_h {
            self.show_help = !self.show_help;
        }
        if self.show_help {
            if intents.key_esc {
                self.show_help = false;
            }
            egui::Window::new("Keybindings")
                .collapsible(false)
                .resizable(true)
                .default_width(640.0)
                .max_width((ctx.content_rect().width() - 32.0).max(360.0))
                .max_height((ctx.content_rect().height() - 32.0).max(240.0))
                .vscroll(true)
                .hscroll(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .frame(
                    egui::Frame::new()
                        .fill(self.theme.popover_bg())
                        .stroke(egui::Stroke::new(1.0, self.theme.border())),
                )
                .show(ctx, |ui| {
                    let key_color = self.theme.popover_fg();
                    let desc_color = self.theme.popover_fg();
                    let section_color = self.theme.accent();
                    egui::Grid::new("help_grid")
                        .num_columns(2)
                        .spacing([24.0, 4.0])
                        .show(ui, |ui| {
                            let row = |ui: &mut egui::Ui, key: &str, desc: &str| {
                                ui.label(
                                    egui::RichText::new(key)
                                        .strong()
                                        .monospace()
                                        .color(key_color),
                                );
                                ui.label(egui::RichText::new(desc).color(desc_color));
                                ui.end_row();
                            };
                            ui.label(egui::RichText::new("Global").strong().color(section_color));
                            ui.end_row();
                            row(ui, "Ctrl+O", "Open a directory");
                            row(ui, "Z", "Open fuzzy file search");
                            row(ui, "Ctrl+L", "Open recent files/directories");
                            row(ui, "Ctrl+T", "Open kitty in current directory");
                            row(ui, "Ctrl+Up", "Move to parent directory");
                            row(ui, "Ctrl+Left / Right", "Switch panes");
                            row(ui, "Ctrl+E", "Toggle file tree sidebar");
                            row(ui, "Ctrl+R", "Rename file/directory");
                            row(ui, "Ctrl+W", "Close current file");
                            row(ui, "Ctrl+Q", "Quit Ferritor");
                            row(ui, "Ctrl+H", "Toggle this help");
                            ui.end_row();

                            ui.label(
                                egui::RichText::new("Filetree")
                                    .strong()
                                    .color(section_color),
                            );
                            ui.end_row();
                            row(ui, "Up / Down", "Move cursor");
                            row(ui, "Left", "Collapse dir / go to parent row");
                            row(ui, "Right", "Expand directory");
                            row(ui, "Enter", "Enter directory / open file");
                            row(ui, "Space", "Toggle select");
                            row(ui, "Ctrl+N", "New file");
                            row(ui, "Ctrl+Shift+N", "New directory");
                            row(ui, "Ctrl+C / X", "Copy / cut files");
                            row(ui, "Ctrl+V", "Paste files");
                            row(ui, "Delete", "Delete (with confirm)");
                            row(ui, "Ctrl+F / S", "Toggle file filter (files pane)");
                            ui.end_row();

                            ui.label(egui::RichText::new("Editor").strong().color(section_color));
                            ui.end_row();
                            row(ui, "E", "Enter edit mode");
                            row(ui, "Esc", "Exit edit mode (dirty prompt)");
                            row(ui, "Ctrl+S", "Save changes");
                            row(ui, "Ctrl+F", "Open in-document find");
                            row(ui, "Enter / Shift+Enter", "Find next / previous");
                            row(ui, "Ctrl+Z / Y", "Undo / redo");
                            row(ui, "Ctrl+Up / Down", "Jump paragraphs");
                            row(ui, "Shift+Ctrl+Up / Down", "Select by paragraph");
                            row(ui, "Shift+Ctrl+Left / Right", "Select by word");
                            row(ui, "PageUp / Down", "Scroll viewer");
                            row(ui, "Ctrl+P", "Toggle markdown preview (.md)");
                            row(ui, "Alt+R", "Add table row");
                            row(ui, "Alt+C", "Add table column");
                        });
                });
            return true;
        }

        false
    }

    fn route_pane_layer(
        &mut self,
        ctx: &egui::Context,
        intents: &CollectedIntents,
        kb_active: bool,
    ) {
        // Pane focus
        if kb_active && intents.ctrl_left {
            if self.is_dirty() {
                self.apply_action(ctx, AppAction::SetDirtyNav(DirtyNavTarget::FocusFiles));
            } else {
                if self.editing {
                    self.apply_action(ctx, AppAction::ExitEditModeDiscard);
                }
                self.apply_action(ctx, AppAction::SetFocusPane(FocusPane::Files));
            }
        }
        if kb_active && intents.ctrl_right {
            self.apply_action(ctx, AppAction::SetFocusPane(FocusPane::Viewer));
        }

        // Filetree navigation
        self.nav_moved = false;
        if kb_active && self.focus_pane == FocusPane::Files && !self.nav_rows.is_empty() {
            let ctrl = ctx.input(|i| i.modifiers.ctrl);

            if intents.key_down && !ctrl && self.nav_cursor + 1 < self.nav_rows.len() {
                self.move_nav_cursor_down();
            }
            if intents.key_up && !ctrl && self.nav_cursor > 0 {
                self.move_nav_cursor_up();
            }

            // Space: toggle select
            if intents.key_space && !ctrl {
                self.toggle_select_current_row();
            }

            // Left: collapse / parent row / parent dir
            if intents.key_left && !ctrl {
                self.handle_filetree_left(ctx);
            }

            // Right: expand directory / move into expanded dir (files: no-op, use Enter)
            if intents.key_right && !ctrl {
                self.handle_filetree_right();
            }

            // Enter: enter directory / open file
            if intents.key_enter && !ctrl {
                self.handle_filetree_enter();
            }
        }
    }

    fn move_nav_cursor_down(&mut self) {
        self.nav_cursor += 1;
        self.nav_moved = true;
    }

    fn move_nav_cursor_up(&mut self) {
        self.nav_cursor -= 1;
        self.nav_moved = true;
    }

    fn move_nav_cursor_to(&mut self, idx: usize) {
        self.nav_cursor = idx;
        self.nav_moved = true;
    }

    fn toggle_select_current_row(&mut self) {
        if let Some(row) = self.current_row().cloned() {
            if !matches!(row.kind, FileRowKind::Parent) {
                if !self.selected_paths.remove(&row.path) {
                    self.selected_paths.insert(row.path);
                }
            }
        }
    }

    fn handle_filetree_left(&mut self, ctx: &egui::Context) {
        if let Some(row) = self.current_row().cloned() {
            if row.is_directory() && self.expanded_dirs.contains(&row.path) {
                self.collapse_directory(&row.path);
                self.refresh_nav_rows();
            } else if row.depth > 0 {
                if let Some(parent_idx) = self.find_parent_row_index(self.nav_cursor) {
                    self.move_nav_cursor_to(parent_idx);
                }
            } else if self.is_dirty() {
                self.apply_action(
                    ctx,
                    AppAction::SetDirtyNav(DirtyNavTarget::EnterDirectory(
                        self.current_dir
                            .parent()
                            .map(Path::to_path_buf)
                            .unwrap_or_else(|| self.current_dir.clone()),
                    )),
                );
            } else {
                self.apply_action(ctx, AppAction::OpenParentDirectory);
            }
        }
    }

    fn handle_filetree_right(&mut self) {
        if let Some(row) = self.current_row().cloned() {
            if row.is_directory() {
                if self.expanded_dirs.contains(&row.path) {
                    if self.nav_cursor + 1 < self.nav_rows.len()
                        && self.nav_rows[self.nav_cursor + 1].depth > row.depth
                    {
                        self.move_nav_cursor_down();
                    }
                } else {
                    self.expanded_dirs.insert(row.path.clone());
                    self.refresh_nav_rows();
                }
            }
            // Files: no-op. Use Enter to open.
        }
    }

    fn handle_filetree_enter(&mut self) {
        if let Some(row) = self.current_row().cloned() {
            self.try_open_row(&row);
        }
    }

    fn toggle_document_find(&mut self, ctx: &egui::Context, doc_find_id: egui::Id) {
        if self.show_doc_find {
            self.close_document_find(ctx, doc_find_id);
            return;
        }
        self.show_doc_find = true;
        self.doc_find_wants_focus = true;
    }

    fn close_document_find(&mut self, ctx: &egui::Context, doc_find_id: egui::Id) {
        self.show_doc_find = false;
        self.doc_find_wants_focus = false;
        self.doc_find_cursor_char = None;
        ctx.memory_mut(|m| m.surrender_focus(doc_find_id));
    }

    fn move_editor_cursor_custom<F>(
        &mut self,
        ctx: &egui::Context,
        extend_selection: bool,
        move_fn: F,
    ) where
        F: FnOnce(&str, usize) -> usize,
    {
        let editor_id = egui::Id::new("viewer_editor");
        if let Some(mut state) = egui::TextEdit::load_state(ctx, editor_id) {
            if let Some(ccursor_range) = state.cursor.char_range() {
                let new_pos = move_fn(&self.edit_buffer, ccursor_range.primary.index);
                let new_range = if extend_selection {
                    egui::text::CCursorRange {
                        primary: egui::text::CCursor::new(new_pos),
                        secondary: ccursor_range.secondary,
                        h_pos: None,
                    }
                } else {
                    egui::text::CCursorRange::one(egui::text::CCursor::new(new_pos))
                };
                state.cursor.set_char_range(Some(new_range));
                state.store(ctx, editor_id);
            }
        }
    }

    fn set_editor_selection(&mut self, ctx: &egui::Context, start: usize, end: usize) {
        let editor_id = egui::Id::new("viewer_editor");
        if let Some(mut state) = egui::TextEdit::load_state(ctx, editor_id) {
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(start),
                    egui::text::CCursor::new(end),
                )));
            state.store(ctx, editor_id);
        }
    }

    fn active_document_text(&self) -> Option<&str> {
        if self.editing {
            Some(&self.edit_buffer)
        } else {
            self.loaded_content.as_deref()
        }
    }

    fn current_editor_cursor_char(&self, ctx: &egui::Context) -> Option<usize> {
        let editor_id = egui::Id::new("viewer_editor");
        egui::TextEdit::load_state(ctx, editor_id)
            .and_then(|state| state.cursor.char_range())
            .map(|range| range.primary.index)
    }

    fn document_find_status(&self) -> Option<(usize, usize)> {
        let text = self.active_document_text()?;
        if self.doc_find_query.is_empty() {
            return None;
        }
        let matches = find_match_positions(text, &self.doc_find_query);
        if matches.is_empty() {
            return Some((0, 0));
        }
        let current = self
            .doc_find_cursor_char
            .and_then(|cursor| matches.iter().position(|&pos| pos == cursor))
            .map(|idx| idx + 1)
            .unwrap_or(0);
        Some((current, matches.len()))
    }

    fn find_in_document(&mut self, ctx: &egui::Context, forward: bool) {
        if self.doc_find_query.is_empty() {
            self.last_message = Some("Find: enter a search string".to_string());
            return;
        }

        let Some(text) = self.active_document_text() else {
            self.last_message = Some("Find: no document loaded".to_string());
            return;
        };
        let matches = find_match_positions(text, &self.doc_find_query);
        if matches.is_empty() {
            self.last_message = Some("Find: no matches".to_string());
            return;
        }

        let origin = if let Some(current) = self.doc_find_cursor_char {
            current
        } else if self.editing {
            self.current_editor_cursor_char(ctx).unwrap_or(0)
        } else {
            self.viewer_top_char
        };
        let has_current = self.doc_find_cursor_char.is_some();

        let target = if forward {
            if has_current {
                find_next_match(&matches, origin)
            } else {
                find_next_match_inclusive(&matches, origin)
            }
        } else if has_current {
            find_prev_match(&matches, origin)
        } else {
            find_prev_match_inclusive(&matches, origin)
        }
        .unwrap_or(matches[0]);

        self.doc_find_cursor_char = Some(target);
        if self.editing {
            let query_chars = self.doc_find_query.chars().count();
            self.set_editor_selection(ctx, target, target + query_chars);
            self.editor_wants_focus = true;
        } else {
            if self.show_preview {
                self.show_preview = false;
            }
            self.pending_viewer_scroll_char = Some(target);
        }

        let current = matches
            .iter()
            .position(|&pos| pos == target)
            .map(|idx| idx + 1)
            .unwrap_or(1);
        self.last_message = Some(format!("Match {current}/{}", matches.len()));
    }

    fn move_editor_cursor_by_line_delta(&mut self, ctx: &egui::Context, delta_lines: isize) {
        let editor_id = egui::Id::new("viewer_editor");
        if let Some(mut state) = egui::TextEdit::load_state(ctx, editor_id) {
            if let Some(ccursor_range) = state.cursor.char_range() {
                let pos = ccursor_range.primary.index;
                let current_line = line_index_from_char_pos(&self.edit_buffer, pos) as isize;
                let max_line = self.edit_buffer.lines().count().saturating_sub(1) as isize;
                let target_line = (current_line + delta_lines).clamp(0, max_line.max(0)) as usize;
                let target_char = char_index_of_line_start(&self.edit_buffer, target_line);
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::one(
                        egui::text::CCursor::new(target_char),
                    )));
                state.store(ctx, editor_id);
            }
        }
    }
}

impl eframe::App for FerritorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.refresh_theme_if_needed(ctx);

        let title = match self
            .selected_file
            .as_ref()
            .and_then(|path| path.file_name())
        {
            Some(file_name) => format!("{} — Ferritor", file_name.to_string_lossy()),
            None => format!("Ferritor — {}", self.current_dir.display()),
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        self.refresh_nav_rows();

        // --- Key input collection ---
        let filter_id = egui::Id::new("files_filter");
        let doc_find_id = egui::Id::new("viewer_doc_find");
        let intents = self.collect_intents(ctx, filter_id, doc_find_id);
        self.dialog_tab = intents.dialog_tab;
        let filter_focused = intents.filter_focused;
        let editor_focused = intents.editor_focused;
        let doc_find_focused = intents.doc_find_focused;
        let overlay_active = self.overlay_active();
        let kb_active = !filter_focused && !editor_focused && !doc_find_focused && !overlay_active;

        // --- Router layers (behavior-preserving in phase 2B) ---
        self.route_undo_layer(&intents);
        self.route_global_layer(
            ctx,
            &intents,
            filter_id,
            doc_find_id,
            filter_focused,
            kb_active,
            overlay_active,
        );
        self.route_mode_layer(ctx, &intents, kb_active, overlay_active);
        if self.route_overlay_layer(ctx, &intents, kb_active) {
            return;
        }

        // --- Header ---

        let header_file_label = self.selected_file.as_ref().and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
        });
        let header_dirty = self.is_dirty();

        egui::TopBottomPanel::top("headerbar")
            .frame(
                egui::Frame::new()
                    .fill(self.theme.headerbar_bg())
                    .stroke(egui::Stroke::new(1.0, self.theme.headerbar_border())),
            )
            .show(ctx, |ui| {
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

        // --- Status bar ---

        egui::TopBottomPanel::bottom("statusbar")
            .min_height(24.0)
            .frame(
                egui::Frame::new()
                    .fill(self.theme.card_bg())
                    .stroke(egui::Stroke::new(1.0, self.theme.border())),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Files").small().weak());
                    ui.separator();
                    let mode = match self.overlay_state() {
                        OverlayState::None => match self.work_mode() {
                            WorkMode::Viewing => "Viewing",
                            WorkMode::Editing => "Editing",
                            WorkMode::Preview => "Preview",
                        },
                        _ => "Dialog",
                    };
                    let mode_color = if self.is_dirty() {
                        self.theme.warning()
                    } else if overlay_active {
                        self.theme.headerbar_fg()
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
                    if let Some(status) = &self.open_dir_fallback_status {
                        ui.separator();
                        ui.label(
                            egui::RichText::new(format!("Open fallback: {status}"))
                                .small()
                                .weak(),
                        );
                    }
                    if !self.selected_paths.is_empty() {
                        ui.separator();
                        ui.label(
                            egui::RichText::new(format!("{} selected", self.selected_paths.len()))
                                .small()
                                .color(self.theme.accent()),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(msg) = &self.last_message {
                            ui.label(egui::RichText::new(msg).small().weak());
                        }
                    });
                });
            });

        // --- Dialog early return ---
        // Render only the active dialog (no panels behind it).
        // This is why dirty-nav works: no background widgets = no Tab leaking.

        if self.in_dialog() {
            self.render_open_dir_file_dialog(ctx);
            self.render_open_dir_dialog(ctx);
            self.render_rename_dialog(ctx);
            self.render_create_dialog(ctx);
            self.render_delete_dialog(ctx);
            self.render_cut_dialog(ctx);
            self.render_file_action_confirmation(ctx);
            return;
        }

        self.route_pane_layer(ctx, &intents, kb_active);

        // --- Panels ---

        let focus_stroke = egui::Stroke::new(2.0, self.theme.accent());
        let no_stroke = egui::Stroke::NONE;
        let sidebar_frame = egui::Frame::side_top_panel(ctx.style().as_ref())
            .fill(self.theme.card_bg())
            .stroke(if self.focus_pane == FocusPane::Files {
                focus_stroke
            } else {
                no_stroke
            });
        let viewer_frame = egui::Frame::central_panel(ctx.style().as_ref())
            .fill(self.theme.view_bg())
            .stroke(if self.focus_pane == FocusPane::Viewer {
                focus_stroke
            } else {
                no_stroke
            });

        // Selection highlight color
        let select_fill = self.theme.selection_bg();

        if self.show_sidebar {
            egui::SidePanel::left("files")
                .default_width(320.0)
                .frame(sidebar_frame)
                .show(ctx, |ui| {
                    if !self.in_dialog()
                        && ui.rect_contains_pointer(ui.max_rect())
                        && ctx.input(|i| i.pointer.any_click())
                    {
                        self.focus_pane = FocusPane::Files;
                    }

                    ui.horizontal(|ui| {
                        let button_clicked = ui
                            .button(format!("{} Open", icons::FOLDER_OPEN))
                            .on_hover_text("Ctrl+O")
                            .clicked();
                        if button_clicked {
                            self.apply_action(ctx, AppAction::ShowOpenDirDialog);
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
                                self.theme.muted_fg(),
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
                            let is_open_in_editor =
                                row.is_file() && self.selected_file.as_ref() == Some(&row.path);
                            let is_multi_selected = self.selected_paths.contains(&row.path);
                            let is_cursor = idx == self.nav_cursor;

                            // Build display text
                            let indent = "\u{2003}".repeat(row.depth);
                            let text = match row.kind {
                                FileRowKind::Parent => "..".to_string(),
                                FileRowKind::Directory => {
                                    let arrow = if self.expanded_dirs.contains(&row.path) {
                                        icons::CARET_DOWN
                                    } else {
                                        icons::CARET_RIGHT
                                    };
                                    format!("{}{} {} {}", indent, arrow, icons::FOLDER, row.name)
                                }
                                FileRowKind::File => {
                                    format!("{}  {} {}", indent, icons::FILE_TEXT, row.name)
                                }
                            };

                            let text = if row.is_directory_like() {
                                egui::RichText::new(text).color(self.theme.accent())
                            } else {
                                egui::RichText::new(text)
                            };

                            let fill = if is_open_in_editor {
                                self.theme.headerbar_bg()
                            } else if is_multi_selected {
                                select_fill
                            } else {
                                egui::Color32::TRANSPARENT
                            };

                            let response = ui.add(
                                egui::Button::new(text)
                                    .selected(is_open_in_editor || is_cursor || is_multi_selected)
                                    .fill(fill),
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

                        // Click: toggle expand for dirs, open for files
                        if let Some(row) = clicked_row {
                            match row.kind {
                                FileRowKind::Directory => {
                                    if self.expanded_dirs.contains(&row.path) {
                                        self.collapse_directory(&row.path);
                                    } else {
                                        self.expanded_dirs.insert(row.path.clone());
                                    }
                                    self.refresh_nav_rows();
                                }
                                FileRowKind::File => {
                                    self.open_file(row.path.clone());
                                }
                                FileRowKind::Parent => {
                                    self.try_open_row(&row);
                                }
                            }
                        }
                    });
                });
        } // show_sidebar

        egui::CentralPanel::default()
            .frame(viewer_frame)
            .show(ctx, |ui| {
                if !self.in_dialog()
                    && ui.rect_contains_pointer(ui.max_rect())
                    && ctx.input(|i| i.pointer.any_click())
                {
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
                                    egui::Button::new(format!("{} (E)dit", icons::PENCIL_SIMPLE)),
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
                                self.undo_history.push_snapshot(&self.edit_buffer);
                                self.exit_edit_mode(true);
                            }

                            if self.selected_file_is_markdown() && !self.editing {
                                let preview_label = if self.show_preview {
                                    format!("{} Raw", icons::CODE)
                                } else {
                                    format!("{} Preview", icons::EYE)
                                };
                                if ui.button(preview_label).on_hover_text("Ctrl+P").clicked() {
                                    self.show_preview = !self.show_preview;
                                }
                            }
                        });
                    });

                    ui.label(
                        egui::RichText::new(path.display().to_string())
                            .small()
                            .weak(),
                    );
                    if self.show_doc_find && self.focus_pane == FocusPane::Viewer {
                        let mut run_prev = false;
                        let mut run_next = false;
                        let mut close_find = false;
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("Find:")
                                    .small()
                                    .strong()
                                    .color(self.theme.accent()),
                            );
                            let find_resp = ui.add(
                                egui::TextEdit::singleline(&mut self.doc_find_query)
                                    .id(doc_find_id)
                                    .desired_width(260.0)
                                    .hint_text("Search in document"),
                            );
                            if self.doc_find_wants_focus {
                                find_resp.request_focus();
                                self.doc_find_wants_focus = false;
                            }
                            if find_resp.changed() {
                                self.doc_find_cursor_char = None;
                            }

                            let status = self
                                .document_find_status()
                                .map(|(current, total)| format!("{current}/{total}"))
                                .unwrap_or_else(|| "0/0".to_string());
                            ui.label(
                                egui::RichText::new(status)
                                    .small()
                                    .color(self.theme.muted_fg()),
                            );
                            if ui.button("Prev").clicked() {
                                run_prev = true;
                            }
                            if ui.button("Next").clicked() {
                                run_next = true;
                            }
                            if ui.button("Close").clicked() {
                                close_find = true;
                            }
                        });

                        if run_prev {
                            self.find_in_document(ctx, false);
                        }
                        if run_next {
                            self.find_in_document(ctx, true);
                        }
                        if close_find {
                            self.close_document_find(ctx, doc_find_id);
                        }
                    }
                    ui.separator();

                    if let Some(err) = &self.load_error {
                        ui.colored_label(self.theme.warning(), err);
                        return;
                    }

                    let scroll_delta = self.viewer_scroll_delta;
                    let scroll_out = egui::ScrollArea::vertical().show(ui, |ui| {
                        self.viewer_line_height_px =
                            ui.text_style_height(&egui::TextStyle::Monospace);
                        if self.editing {
                            let editor_id = egui::Id::new("viewer_editor");
                            let highlighter = &self.syntax_highlighter;
                            let path_for_layout = path.clone();
                            let mut layouter =
                                move |ui: &egui::Ui,
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
                                // Place cursor at requested edit-entry anchor.
                                if let Some(cursor_char) = self.pending_editor_cursor_char.take() {
                                    if let Some(mut state) =
                                        egui::TextEdit::load_state(ui.ctx(), editor_id)
                                    {
                                        state.cursor.set_char_range(Some(
                                            egui::text::CCursorRange::one(
                                                egui::text::CCursor::new(cursor_char),
                                            ),
                                        ));
                                        state.store(ui.ctx(), editor_id);
                                    }
                                }
                                self.editor_wants_focus = false;
                            }
                        } else if self.show_preview && self.selected_file_is_markdown() {
                            if let Some(content) = &self.loaded_content {
                                // Use custom viewer with table support
                                crate::markdown_viewer::show_with_tables(
                                    ui,
                                    &mut self.commonmark_cache,
                                    content,
                                    self.heading_color,
                                );
                            }
                        } else if let Some(content) = &self.loaded_content {
                            let highlighter = &self.syntax_highlighter;
                            let mut job = highlighter.highlight(content, &path);
                            if self.show_doc_find {
                                if let Some(match_start) = self.doc_find_cursor_char {
                                    apply_layout_job_find_highlight(
                                        &mut job,
                                        content,
                                        match_start,
                                        self.doc_find_query.chars().count(),
                                        self.theme.selection_bg(),
                                    );
                                }
                            }
                            ui.add(egui::Label::new(job).selectable(true));
                        }
                    });

                    let mut final_offset = scroll_out.state.offset.y;
                    if scroll_delta != 0.0 {
                        let new_offset = (final_offset + scroll_delta).max(0.0);
                        let mut state = scroll_out.state;
                        state.offset.y = new_offset;
                        state.store(ui.ctx(), scroll_out.id);
                        final_offset = new_offset;
                    }
                    self.viewer_scroll_y = final_offset;

                    // Capture center-of-viewport char anchor from the same content/layout path
                    // used by the raw text viewer and the final applied scroll offset.
                    if !self.editing && !(self.show_preview && self.selected_file_is_markdown()) {
                        if let Some(content) = &self.loaded_content {
                            let highlighter = &self.syntax_highlighter;
                            let job = highlighter.highlight(content, &path);
                            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
                            if let Some(target_char) = self.pending_viewer_scroll_char.take() {
                                let char_count = content.chars().count();
                                let target_cursor =
                                    egui::text::CCursor::new(target_char.min(char_count));
                                let target_rect = galley.pos_from_cursor(target_cursor);
                                let target_offset = (target_rect.center().y
                                    - (scroll_out.inner_rect.height() * 0.5))
                                    .max(0.0);
                                let mut state = scroll_out.state;
                                state.offset.y = target_offset;
                                state.store(ui.ctx(), scroll_out.id);
                                final_offset = target_offset;
                                self.viewer_scroll_y = final_offset;
                            }
                            let anchor_y =
                                final_offset.max(0.0) + (scroll_out.inner_rect.height() * 0.5);
                            self.viewer_top_char =
                                galley.cursor_from_pos(egui::vec2(0.0, anchor_y)).index;
                        }
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
    }
}

// --- Debug ---

#[cfg(debug_assertions)]
fn debug_notify(summary: &str, body: &str) {
    let _ = Command::new("notify-send")
        .arg("--app-name=Ferritor")
        .arg("--expire-time=2000")
        .arg(summary)
        .arg(body)
        .spawn();
}

#[cfg(not(debug_assertions))]
fn debug_notify(_summary: &str, _body: &str) {}

// --- Paragraph navigation ---

/// Find the char index of the previous paragraph boundary (blank line) from char `pos`.
fn find_prev_paragraph(text: &str, pos: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    if pos == 0 {
        return 0;
    }
    let mut i = pos.min(chars.len()).saturating_sub(1);
    // Skip past current line's newline if we're sitting on one
    while i > 0 && chars[i] == '\n' {
        i -= 1;
    }
    // Find previous blank line (\n\n)
    while i > 0 {
        if chars[i] == '\n' && i > 0 && chars[i - 1] == '\n' {
            return i + 1;
        }
        i -= 1;
    }
    0
}

/// Find the char index of the next paragraph boundary (blank line) from char `pos`.
fn find_next_paragraph(text: &str, pos: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    if pos >= len {
        return len;
    }
    let mut i = pos;
    // Skip past current newlines if sitting on them
    while i < len && chars[i] == '\n' {
        i += 1;
    }
    // Find next blank line (\n\n)
    while i < len {
        if chars[i] == '\n' && i + 1 < len && chars[i + 1] == '\n' {
            return i + 2;
        }
        i += 1;
    }
    len
}

fn char_index_of_line_start(text: &str, target_line: usize) -> usize {
    if target_line == 0 {
        return 0;
    }
    let mut line = 0usize;
    let mut char_idx = 0usize;
    for ch in text.chars() {
        if line >= target_line {
            break;
        }
        char_idx += 1;
        if ch == '\n' {
            line += 1;
            if line == target_line {
                break;
            }
        }
    }
    char_idx
}

fn line_index_from_char_pos(text: &str, char_pos: usize) -> usize {
    let mut line = 0usize;
    let mut idx = 0usize;
    for ch in text.chars() {
        if idx >= char_pos {
            break;
        }
        if ch == '\n' {
            line += 1;
        }
        idx += 1;
    }
    line
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn find_prev_word(text: &str, pos: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() || pos == 0 {
        return 0;
    }
    let mut i = pos.min(chars.len()).saturating_sub(1);
    while i > 0 && chars[i].is_whitespace() {
        i -= 1;
    }
    while i > 0 && is_word_char(chars[i - 1]) {
        i -= 1;
    }
    i
}

fn find_next_word(text: &str, pos: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    if pos >= len {
        return len;
    }
    let mut i = pos;
    while i < len && !is_word_char(chars[i]) {
        i += 1;
    }
    while i < len && is_word_char(chars[i]) {
        i += 1;
    }
    i
}

fn char_to_byte_index(text: &str, char_index: usize) -> usize {
    if char_index == 0 {
        return 0;
    }
    text.char_indices()
        .nth(char_index)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len())
}

fn byte_to_char_index(text: &str, byte_index: usize) -> usize {
    text[..byte_index.min(text.len())].chars().count()
}

fn apply_layout_job_find_highlight(
    job: &mut egui::text::LayoutJob,
    text: &str,
    match_start_char: usize,
    match_len_chars: usize,
    highlight_bg: egui::Color32,
) {
    if match_len_chars == 0 {
        return;
    }

    let start_byte = char_to_byte_index(text, match_start_char);
    let end_byte = char_to_byte_index(text, match_start_char.saturating_add(match_len_chars));
    if start_byte >= end_byte {
        return;
    }

    let mut sections = Vec::with_capacity(job.sections.len() + 2);
    for section in &job.sections {
        let s = section.byte_range.start;
        let e = section.byte_range.end;
        if e <= start_byte || s >= end_byte {
            sections.push(section.clone());
            continue;
        }

        let overlap_start = s.max(start_byte);
        let overlap_end = e.min(end_byte);

        if s < overlap_start {
            let mut before = section.clone();
            before.byte_range = s..overlap_start;
            sections.push(before);
        }

        let mut middle = section.clone();
        middle.byte_range = overlap_start..overlap_end;
        middle.leading_space = if s < overlap_start {
            0.0
        } else {
            middle.leading_space
        };
        middle.format.background = highlight_bg;
        sections.push(middle);

        if overlap_end < e {
            let mut after = section.clone();
            after.byte_range = overlap_end..e;
            after.leading_space = 0.0;
            sections.push(after);
        }
    }

    job.sections = sections;
}

fn find_match_positions(text: &str, query: &str) -> Vec<usize> {
    if query.is_empty() {
        return Vec::new();
    }
    text.match_indices(query)
        .map(|(byte_idx, _)| byte_to_char_index(text, byte_idx))
        .collect()
}

fn find_next_match(matches: &[usize], pos: usize) -> Option<usize> {
    matches
        .iter()
        .copied()
        .find(|&match_pos| match_pos > pos)
        .or_else(|| matches.first().copied())
}

fn find_next_match_inclusive(matches: &[usize], pos: usize) -> Option<usize> {
    matches
        .iter()
        .copied()
        .find(|&match_pos| match_pos >= pos)
        .or_else(|| matches.first().copied())
}

fn find_prev_match(matches: &[usize], pos: usize) -> Option<usize> {
    matches
        .iter()
        .rev()
        .copied()
        .find(|&match_pos| match_pos < pos)
        .or_else(|| matches.last().copied())
}

fn find_prev_match_inclusive(matches: &[usize], pos: usize) -> Option<usize> {
    matches
        .iter()
        .rev()
        .copied()
        .find(|&match_pos| match_pos <= pos)
        .or_else(|| matches.last().copied())
}

// --- Table editing ---

struct TableEditResult {
    new_text: String,
    new_cursor: usize,
}

/// Find the line range (as byte offsets into `text`) of contiguous table lines
/// surrounding char position `char_pos`. A table line starts with `|` (optionally
/// preceded by whitespace). Returns (start_byte, end_byte, line the cursor is on
/// relative to table start).
fn find_table_bounds(text: &str, char_pos: usize) -> Option<(usize, usize, usize)> {
    // Convert char pos to byte offset
    let byte_pos = text
        .char_indices()
        .nth(char_pos)
        .map(|(i, _)| i)
        .unwrap_or(text.len());

    let lines: Vec<&str> = text.lines().collect();
    // Map byte_pos to a line index
    let mut offset = 0;
    let mut cursor_line = 0;
    for (i, line) in lines.iter().enumerate() {
        let line_end = offset + line.len();
        if byte_pos <= line_end {
            cursor_line = i;
            break;
        }
        offset = line_end + 1; // +1 for '\n'
        cursor_line = i;
    }

    // Check if cursor line looks like a table line
    if !is_table_line(lines.get(cursor_line).unwrap_or(&"")) {
        return None;
    }

    // Expand upward to find first table line
    let mut first = cursor_line;
    while first > 0 && is_table_line(lines[first - 1]) {
        first -= 1;
    }
    // Need at least 2 lines (header + delimiter) for a valid table
    if cursor_line < first + 1 {
        // Could be header line — check if next line is delimiter
        if cursor_line + 1 < lines.len() && is_delimiter_line(lines[cursor_line + 1]) {
            // OK, we're on the header
        } else {
            return None;
        }
    }

    // Expand downward to find last table line
    let mut last = cursor_line;
    while last + 1 < lines.len() && is_table_line(lines[last + 1]) {
        last += 1;
    }

    // Must have at least header + delimiter (2 lines)
    if last - first < 1 {
        return None;
    }

    // Verify line at first+1 is a delimiter
    if !is_delimiter_line(lines[first + 1]) {
        return None;
    }

    // Calculate byte offsets
    let start_byte: usize = lines[..first].iter().map(|l| l.len() + 1).sum();
    let mut end_byte: usize = lines[..=last].iter().map(|l| l.len() + 1).sum();
    // Don't include the trailing newline if we're at the end of file
    if end_byte > text.len() {
        end_byte = text.len();
    } else {
        end_byte -= 1; // exclude the trailing \n that comes after the last table line
    }

    Some((start_byte, end_byte, cursor_line - first))
}

fn is_table_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|') || trimmed.contains('|')
}

fn is_delimiter_line(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.contains('|') {
        return false;
    }
    // Every cell should be dashes (optionally with colons for alignment)
    trimmed.trim_matches('|').split('|').all(|cell| {
        let c = cell.trim();
        !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':' || ch == ' ')
    })
}

/// Count the number of columns in a table line.
fn count_columns(line: &str) -> usize {
    let trimmed = line.trim();
    let trimmed = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let trimmed = trimmed.strip_suffix('|').unwrap_or(trimmed);
    trimmed.split('|').count()
}

/// Add a new empty row after the cursor's current row in the table.
fn table_add_row(text: &str, char_pos: usize) -> Option<TableEditResult> {
    let (start, end, cursor_row_in_table) = find_table_bounds(text, char_pos)?;
    let table_text = &text[start..end];
    let table_lines: Vec<&str> = table_text.lines().collect();

    let num_cols = count_columns(table_lines[0]);

    // Build new empty row: | | | ... |
    let empty_cells: Vec<&str> = vec!["  "; num_cols];
    let new_row = format!("|{}|", empty_cells.join("|"));

    // Insert after the current row (but never between header and delimiter)
    let insert_after = if cursor_row_in_table <= 1 {
        1 // After the delimiter row
    } else {
        cursor_row_in_table
    };

    let mut new_lines: Vec<String> = table_lines.iter().map(|l| l.to_string()).collect();
    new_lines.insert(insert_after + 1, new_row);

    let new_table = new_lines.join("\n");
    let mut result = String::with_capacity(text.len() + 20);
    result.push_str(&text[..start]);
    result.push_str(&new_table);
    result.push_str(&text[end..]);

    // Place cursor at start of the new row's first cell
    let cursor_byte = start
        + new_lines[..=insert_after + 1]
            .iter()
            .map(|l| l.len() + 1)
            .sum::<usize>()
        - new_lines[insert_after + 1].len(); // back to start of new row
    let new_cursor = result[..cursor_byte].chars().count() + 1; // after first |

    Some(TableEditResult {
        new_text: result,
        new_cursor,
    })
}

/// Add a new empty column after the column the cursor is in.
fn table_add_column(text: &str, char_pos: usize) -> Option<TableEditResult> {
    let (start, end, _cursor_row_in_table) = find_table_bounds(text, char_pos)?;
    let table_text = &text[start..end];
    let table_lines: Vec<&str> = table_text.lines().collect();

    // Figure out which column the cursor is in on its line
    let byte_pos = text
        .char_indices()
        .nth(char_pos)
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    let line_start = start
        + table_lines[.._cursor_row_in_table]
            .iter()
            .map(|l| l.len() + 1)
            .sum::<usize>();
    let pos_in_line = byte_pos - line_start;
    let cursor_line = table_lines[_cursor_row_in_table];

    // Count which column: count unescaped | before pos_in_line
    let mut col = 0usize;
    let mut prev_backslash = false;
    for (i, ch) in cursor_line.char_indices() {
        if i >= pos_in_line {
            break;
        }
        if ch == '|' && !prev_backslash {
            col += 1;
        }
        prev_backslash = ch == '\\' && !prev_backslash;
    }
    // col is 1-indexed (leading | counts as 1), so the column index is col-1
    let insert_after_col = if col == 0 { 0 } else { col - 1 };

    // Add a new cell after insert_after_col in each row
    let mut new_lines: Vec<String> = Vec::new();
    for (i, line) in table_lines.iter().enumerate() {
        let cells = split_table_cells(line);
        let mut new_cells: Vec<String> = Vec::new();
        for (j, cell) in cells.iter().enumerate() {
            new_cells.push(cell.clone());
            if j == insert_after_col {
                if i == 1 {
                    // delimiter row
                    new_cells.push(" --- ".to_string());
                } else {
                    new_cells.push("  ".to_string());
                }
            }
        }
        // If insert_after_col >= cells.len(), append at end
        if insert_after_col >= cells.len() {
            if i == 1 {
                new_cells.push(" --- ".to_string());
            } else {
                new_cells.push("  ".to_string());
            }
        }
        new_lines.push(format!("|{}|", new_cells.join("|")));
    }

    let new_table = new_lines.join("\n");
    let mut result = String::with_capacity(text.len() + 40);
    result.push_str(&text[..start]);
    result.push_str(&new_table);
    result.push_str(&text[end..]);

    // Keep cursor roughly where it was
    let new_cursor = result[..byte_pos.min(result.len())].chars().count();

    Some(TableEditResult {
        new_text: result,
        new_cursor,
    })
}

/// Split a table line into cells (content between |'s), respecting escaped pipes.
fn split_table_cells(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let inner = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let inner = inner.strip_suffix('|').unwrap_or(inner);

    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut prev_backslash = false;

    for ch in inner.chars() {
        if ch == '|' && !prev_backslash {
            cells.push(cell);
            cell = String::new();
        } else {
            cell.push(ch);
        }
        prev_backslash = ch == '\\' && !prev_backslash;
    }
    cells.push(cell);
    cells
}

// --- Helpers ---

fn apply_egui_visuals(ctx: &egui::Context, theme: &GtkTheme) {
    let mut visuals = if theme.is_dark() {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.override_text_color = Some(theme.view_fg());
    visuals.weak_text_color = Some(theme.muted_fg());
    visuals.panel_fill = theme.view_bg();
    visuals.window_fill = theme.dialog_bg();
    visuals.window_stroke = egui::Stroke::new(1.0, theme.border());
    visuals.extreme_bg_color = theme.view_bg();
    visuals.faint_bg_color = theme.shade();
    visuals.text_edit_bg_color = Some(theme.view_bg());
    visuals.code_bg_color = theme.view_bg();
    visuals.warn_fg_color = theme.warning();
    visuals.error_fg_color = theme.error();
    visuals.selection.bg_fill = theme.selection_bg();
    visuals.selection.stroke = egui::Stroke::new(1.0, theme.selection_stroke());
    visuals.widgets.noninteractive.bg_fill = theme.view_bg();
    visuals.widgets.noninteractive.weak_bg_fill = theme.view_bg();
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, theme.border());
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, theme.view_fg());
    visuals.widgets.inactive.bg_fill = theme.card_bg();
    visuals.widgets.inactive.weak_bg_fill = theme.card_bg();
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, theme.border());
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, theme.card_fg());
    visuals.widgets.hovered.bg_fill = theme.hover_bg();
    visuals.widgets.hovered.weak_bg_fill = theme.hover_bg();
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, theme.accent());
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.2, theme.view_fg());
    visuals.widgets.active.bg_fill = theme.accent();
    visuals.widgets.active.weak_bg_fill = theme.accent();
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, theme.accent());
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.2, theme.accent_fg());
    visuals.widgets.open.bg_fill = theme.popover_bg();
    visuals.widgets.open.weak_bg_fill = theme.popover_bg();
    visuals.widgets.open.bg_stroke = egui::Stroke::new(1.0, theme.border());
    visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0, theme.popover_fg());
    visuals.text_cursor.stroke = egui::Stroke::new(2.0, theme.accent());
    ctx.set_visuals(visuals);
}

fn shadcn_palette_from_gtk(theme: &GtkTheme) -> ColorPalette {
    let mut palette = if theme.is_dark() {
        ColorPalette::dark()
    } else {
        ColorPalette::light()
    };

    let chart_1 = theme.palette("blue", 3).unwrap_or(theme.accent());
    let chart_2 = theme.palette("green", 3).unwrap_or(theme.success());
    let chart_3 = theme.palette("yellow", 3).unwrap_or(theme.warning());
    let chart_4 = theme.palette("orange", 3).unwrap_or(theme.warning());
    let chart_5 = theme.palette("red", 3).unwrap_or(theme.error());

    palette.background = theme.view_bg();
    palette.foreground = theme.view_fg();
    palette.card = theme.card_bg();
    palette.card_foreground = theme.card_fg();
    palette.popover = theme.popover_bg();
    palette.popover_foreground = theme.popover_fg();
    palette.border = theme.border();
    palette.input = theme.border();
    palette.ring = theme.accent();
    palette.primary = theme.accent();
    palette.primary_foreground = theme.accent_fg();
    palette.secondary = theme.hover_bg();
    palette.secondary_foreground = theme.card_fg();
    palette.accent = theme.accent();
    palette.accent_foreground = theme.accent_fg();
    palette.muted = theme.hover_bg();
    palette.muted_foreground = theme.muted_fg();
    palette.destructive = theme.destructive();
    palette.destructive_foreground = theme.destructive_fg();
    palette.chart_1 = chart_1;
    palette.chart_2 = chart_2;
    palette.chart_3 = chart_3;
    palette.chart_4 = chart_4;
    palette.chart_5 = chart_5;
    palette.sidebar = theme.card_bg();
    palette.sidebar_foreground = theme.card_fg();
    palette.sidebar_primary = theme.accent();
    palette.sidebar_primary_foreground = theme.accent_fg();
    palette.sidebar_accent = theme.hover_bg();
    palette.sidebar_accent_foreground = theme.card_fg();
    palette.sidebar_border = theme.border();
    palette.sidebar_ring = theme.accent();

    palette
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

fn unique_path(path: PathBuf) -> PathBuf {
    if !path.exists() {
        return path;
    }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = path.extension().and_then(|e| e.to_str());
    let parent = path.parent().unwrap_or(Path::new("."));
    for i in 1..100 {
        let name = match ext {
            Some(e) => format!("{} ({}).{}", stem, i, e),
            None => format!("{} ({})", stem, i),
        };
        let candidate = parent.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    path
}

fn copy_path(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        copy_dir_recursive(src, dst)
    } else {
        fs::copy(src, dst).map(|_| ())
    }
}

fn move_path(src: &Path, dst: &Path) -> std::io::Result<()> {
    if fs::rename(src, dst).is_ok() {
        return Ok(());
    }

    // rename can fail across filesystems. Copy first, then remove the source;
    // if removal fails, roll back the newly-created destination so "move"
    // never silently becomes a duplicate.
    copy_path(src, dst)?;
    let remove_result = if src.is_dir() {
        fs::remove_dir_all(src)
    } else {
        fs::remove_file(src)
    };
    if let Err(error) = remove_result {
        let _ = if dst.is_dir() {
            fs::remove_dir_all(dst)
        } else {
            fs::remove_file(dst)
        };
        return Err(error);
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn load_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    fonts.font_data.insert(
        "SplineSansMono".to_owned(),
        egui::FontData::from_static(include_bytes!(
            "../fonts/spline-sans-mono-latin-400-normal.ttf"
        ))
        .into(),
    );
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "SplineSansMono".to_owned());

    fonts.font_data.insert(
        "SplineSansMono-Bold".to_owned(),
        egui::FontData::from_static(include_bytes!(
            "../fonts/spline-sans-mono-latin-700-normal.ttf"
        ))
        .into(),
    );
    fonts
        .families
        .entry(egui::FontFamily::Name("MonoBold".into()))
        .or_default()
        .push("SplineSansMono-Bold".to_owned());

    fonts.font_data.insert(
        "SplineSans".to_owned(),
        egui::FontData::from_static(include_bytes!("../fonts/spline-sans-latin-400-normal.ttf"))
            .into(),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "SplineSans".to_owned());

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
