//! Responsive markdown table renderer for ferritor
//! 
//! This module provides a custom table rendering implementation that:
//! - Buffers cell content during parsing
//! - Renders tables to fill the available viewport
//! - Uses equal column distribution with proper word-wrapping
//! - Handles continuation rows by merging cell content

use egui::{self, Color32, FontId, FontFamily, TextStyle};
use pulldown_cmark::{Event, Tag, TagEnd, Parser, Options};

// Re-export for use in app
pub use pulldown_cmark;

/// A table cell with buffered content
#[derive(Debug, Clone)]
pub struct TableCell {
    pub content: String,
}

impl TableCell {
    pub fn new() -> Self {
        Self {
            content: String::new(),
        }
    }

    pub fn push_text(&mut self, text: &str) {
        if !self.content.is_empty() {
            self.content.push(' ');
        }
        self.content.push_str(text);
    }

    pub fn finalize(&mut self) {
        // Normalize: remove visual markers, collapse spaces
        self.content = self.content
            .replace(" / ", " ")
            .replace('/', " ")
            .replace("**", "")
            .replace("*", "");
        
        while self.content.contains("  ") {
            self.content = self.content.replace("  ", " ");
        }
        self.content = self.content.trim().to_string();
    }
}

/// A table row containing cells
#[derive(Debug, Clone)]
pub struct TableRow {
    pub cells: Vec<TableCell>,
}

/// Complete table structure
#[derive(Debug, Clone)]
pub struct MarkdownTable {
    pub header: Option<TableRow>,
    pub rows: Vec<TableRow>,
    pub num_cols: usize,
}

impl MarkdownTable {
    pub fn new() -> Self {
        Self {
            header: None,
            rows: Vec::new(),
            num_cols: 0,
        }
    }

    /// Simple preprocessing: merge continuation rows in markdown
    /// Returns processed markdown with merged table rows
    pub fn preprocess_tables(md: &str) -> String {
        let lines: Vec<&str> = md.lines().collect();
        let mut result = Vec::new();
        let mut in_table = false;
        let mut current_row_content: Vec<String> = Vec::new();
        let mut num_cols = 0;
        let mut row_count_in_table = 0;
        
        for line in &lines {
            let trimmed = line.trim();
            
            // Check if this is a table row
            if trimmed.starts_with('|') && trimmed.matches('|').count() >= 2 {
                // Check if it's a separator row (only |, -, :, spaces and must have dashes)
                let is_separator = trimmed.chars().all(|c| c == '|' || c == '-' || c == ':' || c == ' ')
                    && trimmed.contains('-');
                
                if is_separator {
                    in_table = true;
                    // Flush any pending merged row (header) first
                    if !current_row_content.is_empty() {
                        let merged = Self::build_merged_row(&current_row_content, num_cols);
                        result.push(merged);
                        current_row_content.clear();
                    }
                    // Only add separator if it's the first one (row_count is still 0 means we just flushed header)
                    if row_count_in_table == 0 {
                        result.push(line.to_string());
                    }
                    // After separator, any subsequent rows are data rows
                    row_count_in_table += 1;
                } else if !in_table {
                    // First row of table (header)
                    in_table = true;
                    row_count_in_table = 0;
                    num_cols = trimmed.split('|').count().saturating_sub(1);
                    current_row_content.push(line.to_string());
                } else {
                    // Data row - check if continuation
                    let cells: Vec<&str> = trimmed.split('|').skip(1).collect();
                    let first_cell = cells.first().map(|s| s.trim()).unwrap_or("");
                    
                    // Empty first cell or starts with "**" indicates continuation
                    let is_continuation = first_cell.is_empty() || first_cell.starts_with("**");
                    
                    if is_continuation && !current_row_content.is_empty() {
                        // Merge with current row
                        current_row_content.push(line.to_string());
                    } else {
                        // New row - flush previous
                        if !current_row_content.is_empty() {
                            let merged = Self::build_merged_row(&current_row_content, num_cols);
                            result.push(merged);
                            row_count_in_table += 1;
                        }
                        current_row_content.clear();
                        current_row_content.push(line.to_string());
                    }
                }
            } else {
                // Not a table row
                if in_table {
                    // Flush pending table row
                    if !current_row_content.is_empty() {
                        let merged = Self::build_merged_row(&current_row_content, num_cols);
                        result.push(merged);
                        current_row_content.clear();
                    }
                    in_table = false;
                    row_count_in_table = 0;
                }
                result.push(line.to_string());
            }
        }
        
        // Flush any remaining row
        if !current_row_content.is_empty() {
            let merged = Self::build_merged_row(&current_row_content, num_cols);
            result.push(merged);
        }
        
        result.join("\n")
    }
    
    /// Build a merged row from multiple source lines
    fn build_merged_row(lines: &[String], num_cols: usize) -> String {
        if lines.is_empty() {
            return String::new();
        }
        if lines.len() == 1 {
            return lines[0].clone();
        }
        
        // Parse all lines and merge cells
        let mut merged_cells: Vec<String> = vec![String::new(); num_cols];
        
        for line in lines {
            let cells: Vec<&str> = line.split('|').skip(1).take(num_cols).collect();
            for (i, cell) in cells.iter().enumerate() {
                let trimmed = cell.trim();
                if !trimmed.is_empty() {
                    if !merged_cells[i].is_empty() {
                        merged_cells[i].push(' ');
                    }
                    merged_cells[i].push_str(trimmed);
                }
            }
        }
        
        // Clean up merged cells
        for cell in &mut merged_cells {
            // Remove visual markers
            *cell = cell.replace(" / ", " ").replace("**", "").replace('*', "");
            // Collapse spaces
            while cell.contains("  ") {
                *cell = cell.replace("  ", " ");
            }
            *cell = cell.trim().to_string();
        }
        
        // Build output row
        let mut result = String::from("|");
        for cell in merged_cells {
            result.push(' ');
            result.push_str(&cell);
            result.push_str(" |");
        }
        result
    }

    /// Parse markdown content and extract tables
    pub fn parse_from_markdown(md: &str) -> Vec<Self> {
        let mut options = Options::empty();
        options.insert(Options::ENABLE_TABLES);
        
        let parser = Parser::new_ext(md, options);
        let mut tables = Vec::new();
        let mut current_table: Option<MarkdownTable> = None;
        let mut current_row: Option<TableRow> = None;
        let mut current_cell: Option<TableCell> = None;
        let mut in_header = false;

        for event in parser {
            match event {
                Event::Start(Tag::Table(_)) => {
                    current_table = Some(MarkdownTable::new());
                }
                Event::End(TagEnd::Table) => {
                    if let Some(table) = current_table.take() {
                        tables.push(table);
                    }
                }
                Event::Start(Tag::TableHead) => {
                    in_header = true;
                    current_row = Some(TableRow { cells: Vec::new() });
                }
                Event::End(TagEnd::TableHead) => {
                    in_header = false;
                    if let Some(row) = current_row.take() {
                        if let Some(ref mut table) = current_table {
                            table.num_cols = row.cells.len().max(table.num_cols);
                            table.header = Some(row);
                        }
                    }
                }
                Event::Start(Tag::TableRow) => {
                    current_row = Some(TableRow { cells: Vec::new() });
                }
                Event::End(TagEnd::TableRow) => {
                    if let Some(mut row) = current_row.take() {
                        if let Some(ref mut table) = current_table {
                            table.num_cols = row.cells.len().max(table.num_cols);
                            
                            // Check if this row should be merged with previous
                            // (first cell has content that continues from previous row)
                            // Do this BEFORE finalizing so we can detect ** markers
                            if let Some(last_row) = table.rows.last_mut() {
                                if should_merge_rows_raw(last_row, &row) {
                                    merge_rows_raw(last_row, &row);
                                    // Finalize the merged row
                                    for cell in &mut last_row.cells {
                                        cell.finalize();
                                    }
                                    continue;
                                }
                            }
                            
                            // Finalize cells before pushing
                            for cell in &mut row.cells {
                                cell.finalize();
                            }
                            table.rows.push(row);
                        }
                    }
                }
                Event::Start(Tag::TableCell) => {
                    current_cell = Some(TableCell::new());
                }
                Event::End(TagEnd::TableCell) => {
                    if let Some(cell) = current_cell.take() {
                        if let Some(ref mut row) = current_row {
                            row.cells.push(cell);
                        }
                    }
                }
                Event::Text(text) => {
                    if let Some(ref mut cell) = current_cell {
                        cell.push_text(&text);
                    }
                }
                Event::SoftBreak | Event::HardBreak => {
                    if let Some(ref mut cell) = current_cell {
                        cell.push_text(" ");
                    }
                }
                _ => {}
            }
        }

        tables
    }

    /// Render the table to egui
    pub fn render(&self, ui: &mut egui::Ui) {
        let available_width = ui.available_width();
        let num_cols = self.num_cols.max(1);
        // Calculate column width based on current viewport
        let col_width = available_width / num_cols as f32;
        let cell_pad = 8.0;
        
        // Debug output to verify viewport changes
        ui.label(format!("Viewport: {:.0}px, Column: {:.0}px", available_width, col_width));
        
        // Collect all text content first to measure
        let body_size = ui.text_style_height(&TextStyle::Body);
        let font_id = FontId::new(body_size, FontFamily::Proportional);
        let text_color = ui.visuals().text_color();
        
        // Render header if present
        if let Some(ref header) = self.header {
            self.render_row(ui, header, col_width, cell_pad, &font_id, text_color, true);
        }
        
        // Render body rows
        for row in &self.rows {
            self.render_row(ui, row, col_width, cell_pad, &font_id, text_color, false);
        }
    }

    fn render_row(
        &self,
        ui: &mut egui::Ui,
        row: &TableRow,
        col_width: f32,
        cell_pad: f32,
        font_id: &FontId,
        text_color: Color32,
        is_header: bool,
    ) {
        let wrap_width = col_width - (cell_pad * 2.0);
        
        // Calculate row height based on tallest cell
        let mut max_height = 0.0f32;
        let galleys: Vec<_> = row.cells.iter().enumerate().map(|(i, cell)| {
            let wrap_w = if i < self.num_cols { wrap_width.max(10.0) } else { 100.0 };
            let galley = ui.fonts_mut(|fonts| fonts.layout(
                cell.content.clone(),
                font_id.clone(),
                text_color,
                wrap_w,
            ));
            max_height = max_height.max(galley.size().y);
            galley
        }).collect();
        
        let row_height = max_height + (cell_pad * 2.0);
        
        // Allocate row space and render cells
        ui.horizontal(|ui| {
            ui.set_min_size(egui::vec2(ui.available_width(), row_height));
            
            for (i, (_cell, galley)) in row.cells.iter().zip(galleys.iter()).enumerate() {
                let actual_width = if i < self.num_cols { col_width } else { 100.0 };
                
                let (rect, _response) = ui.allocate_exact_size(
                    egui::vec2(actual_width, row_height),
                    egui::Sense::hover(),
                );
                
                // Draw cell background
                let bg_color = if is_header {
                    ui.visuals().faint_bg_color
                } else {
                    Color32::TRANSPARENT
                };
                ui.painter().rect_filled(rect, 0.0, bg_color);
                
                // Draw text
                let text_pos = rect.min + egui::vec2(cell_pad, cell_pad);
                ui.painter().galley(text_pos, galley.clone(), text_color);
                
                // Draw cell border
                ui.painter().rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
                    egui::StrokeKind::Inside,
                );
            }
        });
    }
}

/// Check if a row should be merged with the previous row (raw content, before finalizing)
fn should_merge_rows_raw(prev: &TableRow, current: &TableRow) -> bool {
    // Merge if first cell of current row looks like a continuation
    if let Some(first_cell) = current.cells.first() {
        let content = first_cell.content.trim();
        
        // Empty first cell in current row means it's a continuation
        if content.is_empty() {
            return true;
        }
        
        // Check if previous row's first cell has incomplete bold
        if let Some(prev_first) = prev.cells.first() {
            let prev_trimmed = prev_first.content.trim();
            // Previous has "**" but doesn't end with "**" (incomplete bold)
            let prev_has_incomplete_bold = prev_trimmed.contains("**") && !prev_trimmed.ends_with("**");
            // Current starts with "**" (closing bold) or lowercase letter (continuation)
            let current_continues = content.starts_with("**") || 
                   content.chars().next().map(|c| c.is_ascii_lowercase()).unwrap_or(false);
            
            if prev_has_incomplete_bold && current_continues {
                return true;
            }
            
            // Also merge if previous doesn't end with "**" and current is not empty
            if !prev_trimmed.is_empty() && !prev_trimmed.ends_with("**") && !content.is_empty() {
                // Check if current starts with lowercase (indicates continuation)
                if content.chars().next().map(|c| c.is_ascii_lowercase()).unwrap_or(false) {
                    return true;
                }
            }
        }
    }
    false
}

/// Merge current row into previous row (raw content)
fn merge_rows_raw(prev: &mut TableRow, current: &TableRow) {
    for (i, cell) in current.cells.iter().enumerate() {
        if i < prev.cells.len() {
            if !cell.content.is_empty() {
                if !prev.cells[i].content.is_empty() {
                    prev.cells[i].content.push(' ');
                }
                prev.cells[i].content.push_str(&cell.content);
            }
        } else if !cell.content.is_empty() {
            prev.cells.push(cell.clone());
        }
    }
}

/// Check if a row should be merged with the previous row
fn should_merge_rows(prev: &TableRow, current: &TableRow) -> bool {
    should_merge_rows_raw(prev, current)
}

/// Merge current row into previous row
fn merge_rows(prev: &mut TableRow, current: &TableRow) {
    merge_rows_raw(prev, current);
}
