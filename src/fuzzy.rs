use frizbee::{Config as FrizbeeConfig, Matcher};
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};
use std::time::Instant;

const MAX_INDEXED_FILES: usize = 50_000;
const MAX_RESULTS: usize = 200;

/// Directories to skip during fuzzy search indexing.
/// These are high-volume directories that slow down indexing without
/// being useful for ferritor's markdown file search.
/// Returns true if the entry should be INCLUDED (not skipped).
fn include_dir_during_walk(entry: &ignore::DirEntry) -> bool {
    // Only filter directories (files are always included)
    if !entry.file_type().is_some_and(|ft| ft.is_dir()) {
        return true;
    }
    
    let name = entry.file_name().to_string_lossy();
    let path = entry.path().to_string_lossy();
    
    // Skip by name
    if matches!(name.as_ref(),
        // Cache directories
        | ".cache"
        | ".ccache"
        | "__pycache__"
        | ".pytest_cache"
        | ".parcel-cache"
        | ".turbo"
        | ".grunt"
        | ".sass-cache"
        | ".eslintcache"
        | ".stylelintcache"
        | ".rpt2_cache"
        | ".rts2_cache"
        
        // Build output directories
        | "target"
        | "build"
        | "dist"
        | "out"
        | "output"
        | ".output"
        | ".nuxt"
        | ".next"
        | ".vuepress/dist"
        | ".docusaurus"
        | ".serverless"
        | "coverage"
        | ".coverage"
        | "htmlcov"
        
        // Package manager directories
        | "node_modules"
        | ".yarn"
        | ".pnpm-store"
        | ".pnp"
        | "bower_components"
        | "vendor"
        | "venv"
        | ".venv"
        | "env"
        | ".env"
        | "virtualenv"
        | ".virtualenv"
        | "__pypackages__"
        | ".eggs"
        | ".tox"
        | "*.egg-info"
        
        // Version control
        | ".git"
        | ".svn"
        | ".hg"
        | "CVS"
        
        // IDE and editor directories
        | ".idea"
        | ".vscode"
        | ".vs"
        | ".atom"
        | ".eclipse"
        | ".settings"
        | ".project"
        | ".classpath"
        | "*.xcodeproj"
        | "*.xcworkspace"
        
        // Temporary and backup directories
        | "tmp"
        | "temp"
        | ".tmp"
        | ".temp"
        | ".swp"
        | ".swo"
        | "*~"
        | ".backup"
        | "backups"
        | ".Trash"
        | ".trash"
        | "Trash"
        | "trash"
        
        // System directories
        | ".DS_Store"
        | "System Volume Information"
        | "$RECYCLE.BIN"
        | "lost+found"
        | ".fseventsd"
        | ".Spotlight-V100"
        
        // Large data directories
        | "logs"
        | ".logs"
        | "log"
        | ".log"
    ){
        return false;
    }
    
    // Skip by path patterns (high-volume directories found in analysis)
    let path_str = path.as_ref();
    
    // Skip all icon themes - thousands of PNG/SVG files
    if path_str.contains("/.local/share/icons/") || path_str.contains("/.icons/") || path_str.contains("/icons/"){
        return false;
    }
    
    // Skip pCloud sync directory (has many course files)
    if path_str.contains("/pCloud"){
        return false;
    }
    
    // Skip Rust cargo registry (crate cache)
    if path_str.contains("/.cargo/registry/"){
        return false;
    }
    
    // Skip VSCodium/Electron caches
    if path_str.contains("/VSCodium/WebStorage/") || path_str.contains("/VSCodium/Cache/"){
        return false;
    }
    
    // Skip Zoom emoji data
    if path_str.contains("/.zoom/data/Emojis"){
        return false;
    }
    
    // Skip Downloads directories with lots of non-text files
    if path_str.contains("/Downloads/resumescovers"){
        return false;
    }
    
    // Skip browser caches and extension storage
    if path_str.contains("/.config/"){
        // Browser caches
        if path_str.contains("/Cache/") 
            || path_str.contains("/Cache_Data/")
            || path_str.contains("/Code Cache/")
            || path_str.contains("/Service Worker/CacheStorage/")
            || path_str.contains("/Default/Extensions/"){
            return false;
        }
        
        // Specific app caches
        if path_str.contains("/libreoffice/") && path_str.contains("/cache/"){
            return false;
        }
        
        // Flavours templates (GTK theme files)
        if path_str.contains("/flavours/templates/"){
            return false;
        }
    }
    
    // Skip Android SDK - thousands of drawable files
    if path_str.contains("/.local/share/android-sdk/") || path_str.contains("/Android/Sdk/"){
        return false;
    }
    
    // Skip WebKit caches
    if path_str.contains("/WebKitCache/"){
        return false;
    }
    
    // Skip icon suites in Downloads
    if path_str.contains("/Downloads/") && path_str.contains("/glyphs"){
        return false;
    }
    
    // Skip app data directories with high file counts
    if path_str.contains("/.local/share/"){
        // Skip known high-volume app directories
        if path_str.contains("/com.spredux.app/")
            || path_str.contains("/com.codexu.NoteGen/")
            || path_str.contains("/com.jotter.app/")
            || path_str.contains("/com.joverse.ferrinode/")
            || path_str.contains("/OpenSwarm/")
            || path_str.contains("/WebKitCache/")
            || path_str.contains("/uv/"){
            return false;
        }
    }
    
    // Skip build artifacts and caches
    if path_str.contains("/.local/bin/release/") || path_str.contains("/.local/bin/debug/"){
        return false;
    }
    
    // Skip browser profiles
    if path_str.contains("/.librewolf/") || path_str.contains("/.var/app/"){
        return false;
    }
    
    // Skip JS package manager caches
    if path_str.contains("/.bun/"){
        return false;
    }
    
    // Skip Gradle caches and wrappers
    if path_str.contains("/.gradle/"){
        return false;
    }
    
    // Skip Dart/Flutter package cache
    if path_str.contains("/.pub-cache/"){
        return false;
    }
    
    // Skip Go package cache
    if path_str.contains("/go/pkg/mod/"){
        return false;
    }
    
    // Skip Claude todo directory (high volume)
    if path_str.contains("/.claude/todos"){
        return false;
    }
    
    true
}

#[derive(Clone, Debug)]
pub struct FuzzyResult {
    pub path: PathBuf,
    pub relative: String,
    pub file_name: String,
    pub score: u16,
}

#[derive(Default)]
pub struct FuzzySearch {
    root: PathBuf,
    indexed_for_root: Option<PathBuf>,
    candidates: Vec<PathBuf>,
    labels: Vec<String>,
    results: Vec<FuzzyResult>,
    query: String,
    cursor: usize,
    last_index_ms: u128,
    last_match_ms: u128,
}

impl FuzzySearch {
    pub fn open_for_root(&mut self, root: PathBuf){
        self.root = root.clone();
        if self.indexed_for_root.as_ref() != Some(&root){
            self.rebuild_index();
        } else {
            self.recompute_results();
        }
    }

    pub fn set_query(&mut self, query: String){
        if self.query == query {
            return;
        }
        self.query = query;
        self.recompute_results();
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn results(&self) -> &[FuzzyResult] {
        &self.results
    }

    pub fn move_cursor_up(&mut self){
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_cursor_down(&mut self){
        if self.results.is_empty(){
            self.cursor = 0;
        } else {
            self.cursor = (self.cursor + 1).min(self.results.len() - 1);
        }
    }

    pub fn set_cursor(&mut self, idx: usize){
        self.cursor = idx.min(self.results.len().saturating_sub(1));
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn selected(&self) -> Option<&FuzzyResult> {
        self.results.get(self.cursor)
    }

    fn rebuild_index(&mut self){
        let started = Instant::now();
        self.candidates.clear();
        self.labels.clear();

        let mut walker = WalkBuilder::new(&self.root);
        walker.hidden(false);
        walker.git_ignore(true);
        walker.git_global(true);
        walker.git_exclude(true);
        walker.parents(true);
        walker.filter_entry(include_dir_during_walk);

        for entry in walker.build() {
            let Ok(entry) = entry else { continue };
            if !entry.file_type().is_some_and(|ft| ft.is_file()) { continue }

            let path = entry.into_path();
            if Self::is_probably_binary(&path) { continue }
            let rel = Self::relative_label(&self.root, &path);
            self.candidates.push(path);
            self.labels.push(rel);
            if self.candidates.len() >= MAX_INDEXED_FILES { break }
        }

        self.indexed_for_root = Some(self.root.clone());
        self.last_index_ms = started.elapsed().as_millis();
        self.recompute_results();
    }

    fn recompute_results(&mut self) {
        let started = Instant::now();
        self.results.clear();
        self.cursor = 0;

        let query = self.query.trim();
        if query.is_empty() {
            let mut pairs: Vec<(usize, &str)> = self
                .labels
                .iter()
                .enumerate()
                .map(|(idx, s)| (idx, s.as_str()))
                .collect();
            pairs.sort_by(|a, b| a.1.cmp(b.1));
            for (idx, _) in pairs.into_iter().take(MAX_RESULTS) {
                self.results
                    .push(Self::mk_result(&self.candidates[idx], &self.labels[idx], 0));
            }
            self.last_match_ms = started.elapsed().as_millis();
            return;
        }

        // Parse query: split by spaces, terms starting with '-' are exclusions
        let parts: Vec<&str> = query.split_whitespace().collect();
        let exclusions: Vec<String> = parts
            .iter()
            .filter(|p| p.starts_with('-') && p.len() > 1)
            .map(|p| p[1..].to_lowercase())
            .collect();
        let positive_terms: Vec<&str> = parts
            .into_iter()
            .filter(|p| !p.starts_with('-'))
            .collect();
        
        let positive_query = positive_terms.join(" ");

        // Filter candidates to exclude paths matching exclusion terms
        let filtered_indices: Vec<usize> = self
            .labels
            .iter()
            .enumerate()
            .filter(|(_, label)| {
                let label_lower = label.to_lowercase();
                !exclusions.iter().any(|ex| label_lower.contains(ex))
            })
            .map(|(idx, _)| idx)
            .collect();

        if positive_query.is_empty() {
            // Only exclusions, no positive search - show filtered results alphabetically
            for idx in filtered_indices.into_iter().take(MAX_RESULTS) {
                self.results.push(Self::mk_result(
                    &self.candidates[idx],
                    &self.labels[idx],
                    0,
                ));
            }
            self.last_match_ms = started.elapsed().as_millis();
            return;
        }

        // Build filtered label list for matching
        let filtered_labels: Vec<(usize, &str)> = filtered_indices
            .iter()
            .map(|&idx| (idx, self.labels[idx].as_str()))
            .collect();

        let mut cfg = FrizbeeConfig::default();
        cfg.max_typos = if positive_query.len() >= 6 { Some(1) } else { Some(0) };
        let mut matcher = Matcher::new(&positive_query, &cfg);
        
        // Extract just the labels for matching
        let labels_only: Vec<&str> = filtered_labels.iter().map(|(_, label)| *label).collect();
        let matched = matcher.match_list(&labels_only);

        for m in matched.into_iter().take(MAX_RESULTS) {
            let filtered_idx = m.index as usize;
            if filtered_idx >= filtered_labels.len() {
                continue;
            }
            let original_idx = filtered_labels[filtered_idx].0;
            if original_idx >= self.candidates.len() || original_idx >= self.labels.len() {
                continue;
            }
            self.results.push(Self::mk_result(
                &self.candidates[original_idx],
                &self.labels[original_idx],
                m.score,
            ));
        }

        self.last_match_ms = started.elapsed().as_millis();
    }

    fn mk_result(path: &Path, relative: &str, score: u16) -> FuzzyResult {
        let file_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        FuzzyResult {
            path: path.to_path_buf(),
            relative: relative.to_string(),
            file_name,
            score,
        }
    }

    fn relative_label(root: &Path, path: &Path) -> String {
        path.strip_prefix(root)
            .ok()
            .and_then(|p| p.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| path.display().to_string())
    }

    fn is_probably_binary(path: &Path) -> bool {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase());
        matches!(
            ext.as_deref(),
            Some("png")
                | Some("jpg")
                | Some("jpeg")
                | Some("gif")
                | Some("webp")
                | Some("avif")
                | Some("pdf")
                | Some("zip")
                | Some("gz")
                | Some("xz")
                | Some("7z")
                | Some("tar")
                | Some("mp3")
                | Some("wav")
                | Some("mp4")
                | Some("mov")
                | Some("mkv")
                | Some("ttf")
                | Some("otf")
                | Some("woff")
                | Some("woff2")
                | Some("exe")
                | Some("dll")
                | Some("so")
                | Some("dylib")
        )
    }

}
