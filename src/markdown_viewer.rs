//! Custom markdown viewer with marked-extended-tables support
//!
//! Syntax supported:
//! - Column spanning: `|||` at end of cell (extra pipes = extra columns)
//! - Row spanning: `^` before closing pipe (merges with cell above)
//! - Multi-row headers
//! - Percentage column widths: `|---10%---|`
//! - Standard GFM alignment markers
//!
//! Ported from marked-extended-tables JS reference implementation.

use egui::{self, Color32, FontFamily, FontId, TextStyle};
use egui_commonmark::CommonMarkCache;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

pub fn show_with_tables(
    ui: &mut egui::Ui,
    cache: &mut CommonMarkCache,
    markdown: &str,
    heading_color: Color32,
) {
    ui.scope(|ui| {
        // egui_commonmark renders headings and **strong text** via
        // RichText::strong(). In egui, strong text ignores
        // override_text_color and instead reads widgets.active.fg_stroke.
        // Override that channel only for the preview so the WCAG-selected
        // heading color reaches the final galley without changing buttons
        // elsewhere in Ferritor.
        ui.visuals_mut().widgets.active.fg_stroke.color = heading_color;
        show_with_tables_inner(ui, cache, markdown, heading_color);
    });
}

fn show_with_tables_inner(
    ui: &mut egui::Ui,
    cache: &mut CommonMarkCache,
    markdown: &str,
    heading_color: Color32,
) {
    let table_regions = find_table_regions(markdown);

    if table_regions.is_empty() {
        show_with_heading_color(ui, cache, markdown, heading_color);
        return;
    }

    let mut last_end = 0;
    for (start, end, table_md) in &table_regions {
        if *start > last_end {
            let before = &markdown[last_end..*start];
            show_with_heading_color(ui, cache, before, heading_color);
        }

        if let Some(table) = parse_extended_table(table_md) {
            render_extended_table(ui, &table);
        } else {
            show_with_heading_color(ui, cache, table_md, heading_color);
        }

        last_end = *end;
    }

    if last_end < markdown.len() {
        let after = &markdown[last_end..];
        show_with_heading_color(ui, cache, after, heading_color);
    }
}

fn show_with_heading_color(
    ui: &mut egui::Ui,
    cache: &mut CommonMarkCache,
    markdown: &str,
    heading_color: Color32,
) {
    let heading_regions = find_heading_regions(markdown);
    if heading_regions.is_empty() {
        egui_commonmark::CommonMarkViewer::new().show(ui, cache, markdown);
        return;
    }

    let mut last_end = 0;
    for (start, end) in heading_regions {
        if start > last_end {
            egui_commonmark::CommonMarkViewer::new().show(ui, cache, &markdown[last_end..start]);
        }
        ui.scope(|ui| {
            ui.visuals_mut().override_text_color = Some(heading_color);
            egui_commonmark::CommonMarkViewer::new().show(ui, cache, &markdown[start..end]);
        });
        last_end = end;
    }

    if last_end < markdown.len() {
        egui_commonmark::CommonMarkViewer::new().show(ui, cache, &markdown[last_end..]);
    }
}

fn find_heading_regions(markdown: &str) -> Vec<(usize, usize)> {
    let mut regions = Vec::new();
    let mut heading_start = None;

    for (event, range) in Parser::new(markdown).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { .. }) => heading_start = Some(range.start),
            Event::End(TagEnd::Heading(_)) => {
                if let Some(start) = heading_start.take() {
                    regions.push((start, range.end));
                }
            }
            _ => {}
        }
    }

    regions
}

// ============= TABLE DETECTION =============

fn find_table_regions(md: &str) -> Vec<(usize, usize, String)> {
    let mut regions = Vec::new();
    let mut in_table = false;
    let mut table_lines = Vec::new();
    let mut table_start_byte = 0;
    let mut byte_offset = 0;

    for line in md.lines() {
        let is_table_line = is_table_region_line(line, in_table);

        if is_table_line && !in_table {
            in_table = true;
            table_start_byte = byte_offset;
            table_lines.clear();
            table_lines.push(line);
        } else if is_table_line && in_table {
            table_lines.push(line);
        } else if !is_table_line && in_table {
            let table_content = table_lines.join("\n");
            let end_byte = table_start_byte + table_content.len();
            regions.push((table_start_byte, end_byte, table_content));
            in_table = false;
        }

        byte_offset += line.len() + 1;
    }

    if in_table {
        let table_content = table_lines.join("\n");
        regions.push((table_start_byte, table_start_byte + table_content.len(), table_content));
    }

    regions
}

fn is_table_region_line(line: &str, in_table: bool) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }

    let pipe_count = trimmed.matches('|').count();
    if in_table {
        pipe_count > 0
    } else {
        pipe_count >= 2
    }
}

// ============= EXTENDED TABLE PARSING =============

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    None,
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone)]
pub struct ExtCell {
    pub text: String,
    pub colspan: usize,
    pub rowspan: usize,
    pub alignment: Alignment,
}

#[derive(Debug, Clone)]
pub struct ExtRow {
    pub cells: Vec<ExtCell>,
    pub is_header: bool,
}

#[derive(Debug, Clone)]
pub struct ExtTable {
    pub rows: Vec<ExtRow>,
    pub num_cols: usize,
    pub col_widths: Vec<Option<u8>>,
}

impl ExtCell {
    pub fn empty() -> Self {
        Self {
            text: String::new(),
            colspan: 1,
            rowspan: 1,
            alignment: Alignment::None,
        }
    }
}

fn parse_extended_table(md: &str) -> Option<ExtTable> {
    let lines: Vec<&str> = md.lines().collect();
    if lines.len() < 2 {
        return None;
    }

    let mut sep_idx = None;
    for (i, line) in lines.iter().enumerate() {
        if is_separator_row(line.trim()) {
            sep_idx = Some(i);
            break;
        }
    }
    let sep_idx = sep_idx?;

    let header_lines = &lines[..sep_idx];
    let body_lines = &lines[sep_idx + 1..];
    let sep_line = lines[sep_idx];

    // Parse alignment row
    let sep_cells = split_row(sep_line);
    let mut col_aligns = Vec::new();
    let mut col_widths = Vec::new();
    for raw in &sep_cells {
        let text = raw.trim_end_matches('|').trim();
        let (align, width) = parse_separator_cell(text);
        col_aligns.push(align);
        col_widths.push(width);
    }

    let num_cols = col_aligns.len().max(1);

    // Parse all rows (without rowspan resolution)
    let mut rows = Vec::new();
    for line in header_lines {
        let cells = split_cells(line, num_cols);
        rows.push(ExtRow { cells, is_header: true });
    }
    for line in body_lines {
        let cells = split_cells(line, num_cols);
        rows.push(ExtRow { cells, is_header: false });
    }

    // Resolve rowspans top-to-bottom
    resolve_rowspans(&mut rows);

    // Apply column alignments
    for row in &mut rows {
        let mut col = 0;
        for cell in &mut row.cells {
            if col < col_aligns.len() {
                cell.alignment = col_aligns[col];
            }
            col += cell.colspan;
        }
    }

    Some(ExtTable { rows, num_cols, col_widths })
}

fn is_separator_row(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|')
        && trimmed.contains('-')
        && trimmed
            .chars()
            .all(|c| c == '|' || c == '-' || c == ':' || c == ' ' || c == '%' || c.is_ascii_digit())
}

/// Split a table row into raw cell strings matching the JS regex behavior:
/// `(?:[^|\]|\.?)+(?:\|+|$)` — content followed by one or more pipes or end-of-string.
/// Leading pipes are skipped; trailing pipes are included in the cell match.
fn split_row(row: &str) -> Vec<String> {
    let trimmed = row.trim();
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut chars = trimmed.chars().peekable();
    let mut escaped = false;
    let mut in_content = false;

    while let Some(ch) = chars.next() {
        if escaped {
            current.push('\\');
            current.push(ch);
            escaped = false;
            in_content = true;
        } else if ch == '\\' {
            escaped = true;
            in_content = true;
        } else if ch == '|' {
            if in_content {
                // End of cell — consume all trailing pipes as part of this match
                current.push('|');
                while chars.peek() == Some(&'|') {
                    current.push(chars.next().unwrap());
                }
                cells.push(current);
                current = String::new();
                in_content = false;
            }
            // Leading pipes (before any content) are skipped
        } else {
            current.push(ch);
            in_content = true;
        }
    }

    if !current.is_empty() {
        cells.push(current);
    }

    cells
}

/// Parse a single separator cell for alignment and width.
/// Matches JS regex logic:
///   right:  `^ *-+: *$`
///   center: `^ *:-+: *$`
///   left:   `^ *:-+ *$`
fn parse_separator_cell(text: &str) -> (Alignment, Option<u8>) {
    let trimmed = text.trim();
    let width = extract_width(trimmed);

    // Strip width percentages before testing alignment (JS: cap[2].replace(widthRegex, ''))
    let align_text = strip_width_percentages(trimmed);
    let align_text = align_text.trim();

    let align = if regex_like_right(align_text) {
        Alignment::Right
    } else if regex_like_center(align_text) {
        Alignment::Center
    } else if regex_like_left(align_text) {
        Alignment::Left
    } else {
        Alignment::None
    };

    (align, width)
}

fn strip_width_percentages(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch.is_ascii_digit() {
            let mut num = String::new();
            num.push(ch);
            while let Some(&c) = chars.peek() {
                if c.is_ascii_digit() {
                    num.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            if chars.peek() == Some(&'%') {
                chars.next(); // consume %
                // Consume trailing spaces that were part of the width pattern
                while chars.peek() == Some(&' ') {
                    chars.next();
                }
                // Strip one preceding space if present
                if result.ends_with(' ') {
                    result.pop();
                }
            } else {
                result.push_str(&num);
            }
        } else {
            result.push(ch);
        }
    }
    // Collapse any remaining spaces (e.g. `:-- --:` → `:----:`)
    result.replace(' ', "")
}

fn regex_like_right(s: &str) -> bool {
    let mut chars = s.chars().peekable();
    while chars.peek() == Some(&' ') { chars.next(); }
    let mut dashes = 0;
    while chars.peek() == Some(&'-') { chars.next(); dashes += 1; }
    if dashes == 0 || chars.next() != Some(':') { return false; }
    while chars.peek() == Some(&' ') { chars.next(); }
    chars.next().is_none()
}

fn regex_like_center(s: &str) -> bool {
    let mut chars = s.chars().peekable();
    while chars.peek() == Some(&' ') { chars.next(); }
    if chars.next() != Some(':') { return false; }
    let mut dashes = 0;
    while chars.peek() == Some(&'-') { chars.next(); dashes += 1; }
    if dashes == 0 || chars.next() != Some(':') { return false; }
    while chars.peek() == Some(&' ') { chars.next(); }
    chars.next().is_none()
}

fn regex_like_left(s: &str) -> bool {
    let mut chars = s.chars().peekable();
    while chars.peek() == Some(&' ') { chars.next(); }
    if chars.next() != Some(':') { return false; }
    let mut dashes = 0;
    while chars.peek() == Some(&'-') { chars.next(); dashes += 1; }
    if dashes == 0 { return false; }
    while chars.peek() == Some(&' ') { chars.next(); }
    chars.next().is_none()
}

fn extract_width(text: &str) -> Option<u8> {
    let mut chars = text.chars().peekable();
    while let Some(&ch) = chars.peek() {
        if ch.is_ascii_digit() {
            let mut num_str = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_ascii_digit() {
                    num_str.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            if chars.peek() == Some(&'%') {
                if let Ok(n) = num_str.parse::<u8>() {
                    return Some(n);
                }
            }
        } else {
            chars.next();
        }
    }
    None
}

/// Split a table row into cells. Empty segments between pipes (`||`) add colspan
/// to the previous cell.
fn split_cells(table_row: &str, count: usize) -> Vec<ExtCell> {
    let raw_cells = split_row(table_row);

    let mut cells: Vec<ExtCell> = Vec::new();
    let mut num_cols = 0;

    for raw in raw_cells {
        // trimmedCell = cells[i].split(/\|+$/)[0]
        let trimmed = raw.trim_end_matches('|');
        let text = trimmed.trim().replace("\\|", "|");
        // colspan = Math.max(cells[i].length - trimmedCell.length, 1)
        let colspan = (raw.len() - trimmed.len()).max(1);

        cells.push(ExtCell {
            text,
            colspan,
            rowspan: 1,
            alignment: Alignment::None,
        });
        num_cols += colspan;
    }

    // Normalize to column count
    if num_cols > count && count > 0 {
        let mut kept_cols = 0;
        let mut i = 0;
        while i < cells.len() && kept_cols < count {
            kept_cols += cells[i].colspan;
            i += 1;
        }
        cells.truncate(i);
        if kept_cols > count {
            if let Some(last) = cells.last_mut() {
                last.colspan -= kept_cols - count;
                if last.colspan == 0 {
                    last.colspan = 1;
                }
            }
        }
    } else {
        while num_cols < count {
            cells.push(ExtCell::empty());
            num_cols += 1;
        }
    }

    cells
}

/// Resolve rowspans top-to-bottom, mutating rows in place.
/// For each cell ending with `^`, finds the matching cell in the row above
/// (at the same column offset with matching colspan) and merges into it.
/// Follows chains through intermediate ghost cells.
fn resolve_rowspans(rows: &mut [ExtRow]) {
    if rows.len() < 2 {
        return;
    }

    for row_idx in 1..rows.len() {
        let mut col_offset = 0;
        for cell_idx in 0..rows[row_idx].cells.len() {
            let cell_text = rows[row_idx].cells[cell_idx].text.clone();
            let cell_colspan = rows[row_idx].cells[cell_idx].colspan;

            if !cell_text.ends_with('^') {
                col_offset += cell_colspan;
                continue;
            }

            if let Some((target_row, target_col)) =
                find_rowspan_target(rows, row_idx - 1, col_offset, cell_colspan)
            {
                let append = cell_text[..cell_text.len().saturating_sub(1)].trim_end();
                if !append.is_empty() {
                    let target = &mut rows[target_row].cells[target_col];
                    if !target.text.is_empty() {
                        target.text.push(' ');
                    }
                    target.text.push_str(append);
                }
                rows[target_row].cells[target_col].rowspan += 1;
                rows[row_idx].cells[cell_idx].rowspan = 0;
            }

            col_offset += cell_colspan;
        }
    }
}

/// Find the ultimate rowspan target for a cell that should merge upward.
/// Starting from `start_row`, finds the cell at `col_offset` with `colspan`,
/// and if that cell is itself a ghost, follows the chain upward.
fn find_rowspan_target(
    rows: &[ExtRow],
    start_row: usize,
    col_offset: usize,
    colspan: usize,
) -> Option<(usize, usize)> {
    let mut row = start_row;

    loop {
        let mut current_offset = 0;
        let mut found_col = None;
        for (col_idx, cell) in rows[row].cells.iter().enumerate() {
            if current_offset == col_offset && cell.colspan == colspan {
                found_col = Some(col_idx);
                break;
            }
            current_offset += cell.colspan;
            if current_offset > col_offset {
                break;
            }
        }

        let col = found_col?;

        if rows[row].cells[col].rowspan > 0 {
            return Some((row, col));
        }

        if row == 0 {
            return None;
        }
        row -= 1;
    }
}

// ============= RENDERING =============

#[derive(Debug, Clone)]
struct CellLayout {
    row: usize,
    start_col: usize,
    rowspan: usize,
    galley: std::sync::Arc<egui::Galley>,
    height: f32,
}

fn render_extended_table(ui: &mut egui::Ui, table: &ExtTable) {
    if table.num_cols == 0 || table.rows.is_empty() {
        return;
    }

    let available_width = ui.available_width();
    let cell_pad = 6.0;
    let min_row_height = ui.text_style_height(&TextStyle::Body) + cell_pad * 2.0;

    let body_size = ui.text_style_height(&TextStyle::Body);
    let body_font = FontId::new(body_size, FontFamily::Proportional);
    let heading_font = FontId::new(
        body_size,
        ui.style()
            .text_styles
            .get(&TextStyle::Heading)
            .map(|f| f.family.clone())
            .unwrap_or(FontFamily::Proportional),
    );

    let text_color = ui.visuals().text_color();
    let stripe_color = ui.visuals().faint_bg_color;
    let border_color = ui.visuals().widgets.noninteractive.bg_stroke.color;

    let col_widths = calculate_column_widths(available_width, table);

    // Compute layouts for all non-ghost cells
    let mut layouts: Vec<CellLayout> = Vec::new();
    for (row_idx, row) in table.rows.iter().enumerate() {
        let mut col_idx = 0;
        for cell in &row.cells {
            if cell.rowspan == 0 {
                col_idx += cell.colspan;
                continue;
            }

            let cell_width = col_widths[col_idx..col_idx + cell.colspan].iter().sum::<f32>();
            let wrap_width = (cell_width - cell_pad * 2.0).max(1.0);
            let font = if row.is_header { &heading_font } else { &body_font };
            let cell_text = markdown_cell_text(&cell.text);
            let galley =
                ui.fonts_mut(|fonts| fonts.layout(cell_text, font.clone(), text_color, wrap_width));

            let height = galley.size().y + cell_pad * 2.0;
            layouts.push(CellLayout {
                row: row_idx,
                start_col: col_idx,
                rowspan: cell.rowspan,
                galley,
                height,
            });

            col_idx += cell.colspan;
        }
    }

    // Calculate row heights
    let num_rows = table.rows.len();
    let mut row_heights = vec![min_row_height; num_rows];

    // Mark all-ghost rows as 0 height initially
    for (row_idx, row) in table.rows.iter().enumerate() {
        if row.cells.iter().all(|c| c.rowspan == 0) {
            row_heights[row_idx] = 0.0;
        }
    }

    // Base heights from non-spanning cells
    for layout in &layouts {
        if layout.rowspan == 1 {
            row_heights[layout.row] = row_heights[layout.row].max(layout.height);
        }
    }

    // Adjust for spanning cells
    for _ in 0..num_rows {
        let mut changed = false;
        for layout in &layouts {
            if layout.rowspan > 1 {
                let spanned: f32 = row_heights[layout.row..layout.row + layout.rowspan]
                    .iter()
                    .sum();
                if layout.height > spanned {
                    let extra = layout.height - spanned;
                    for r in layout.row..layout.row + layout.rowspan {
                        row_heights[r] += extra / layout.rowspan as f32;
                    }
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    let total_height: f32 = row_heights.iter().sum();

    let (rect, _response) = ui.allocate_exact_size(
        egui::vec2(available_width, total_height),
        egui::Sense::hover(),
    );

    let painter = ui.painter_at(rect);
    let mut y = rect.min.y;

    for (row_idx, row) in table.rows.iter().enumerate() {
        let row_height = row_heights[row_idx];
        let mut x = rect.min.x;
        let mut col_idx = 0;

        for cell in &row.cells {
            if cell.rowspan == 0 {
                let w = col_widths[col_idx..col_idx + cell.colspan].iter().sum::<f32>();
                x += w;
                col_idx += cell.colspan;
                continue;
            }

            let cell_width = col_widths[col_idx..col_idx + cell.colspan].iter().sum::<f32>();
            let cell_height = row_heights[row_idx..row_idx + cell.rowspan].iter().sum::<f32>();

            let cell_rect = egui::Rect::from_min_size(
                egui::pos2(x, y),
                egui::vec2(cell_width, cell_height),
            );

            let bg = if row.is_header {
                stripe_color
            } else {
                Color32::TRANSPARENT
            };
            painter.rect_filled(cell_rect, 0.0, bg);

            if let Some(layout) = layouts
                .iter()
                .find(|l| l.row == row_idx && l.start_col == col_idx)
            {
                let text_x = match cell.alignment {
                    Alignment::Right => cell_rect.max.x - cell_pad - layout.galley.size().x,
                    Alignment::Center => cell_rect.center().x - layout.galley.size().x / 2.0,
                    Alignment::None | Alignment::Left => cell_rect.min.x + cell_pad,
                }
                .max(cell_rect.min.x + cell_pad);
                let text_pos = egui::pos2(text_x, cell_rect.min.y + cell_pad);
                painter.galley(text_pos, layout.galley.clone(), text_color);
            }

            painter.rect_stroke(
                cell_rect,
                0.0,
                egui::Stroke::new(1.0_f32, border_color),
                egui::StrokeKind::Inside,
            );

            x += cell_width;
            col_idx += cell.colspan;
        }

        y += row_height;
    }
}

fn markdown_cell_text(markdown: &str) -> String {
    let mut text = String::new();
    for event in Parser::new(markdown) {
        match event {
            Event::Text(value) | Event::Code(value) => text.push_str(&value),
            Event::SoftBreak | Event::HardBreak => text.push(' '),
            _ => {}
        }
    }

    if text.is_empty() {
        markdown.to_string()
    } else {
        text
    }
}

fn calculate_column_widths(available_width: f32, table: &ExtTable) -> Vec<f32> {
    let num_cols = table.num_cols;
    if num_cols == 0 {
        return Vec::new();
    }

    if table.col_widths.iter().all(|w| w.is_none()) {
        let w = available_width / num_cols as f32;
        return vec![w; num_cols];
    }

    let total_specified: u8 = table.col_widths.iter().filter_map(|w| *w).sum();
    let specified_pct = total_specified.min(100);
    let unspecified_cols = table.col_widths.iter().filter(|w| w.is_none()).count();

    let remaining_pct = 100 - specified_pct;
    let default_pct = if unspecified_cols > 0 {
        remaining_pct as f32 / unspecified_cols as f32
    } else {
        0.0
    };

    let mut widths = Vec::with_capacity(num_cols);
    for w in &table.col_widths {
        let pct = w.unwrap_or(default_pct as u8) as f32;
        widths.push(available_width * pct / 100.0);
    }

    widths
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered_text_color(shapes: &[egui::epaint::ClippedShape], needle: &str) -> Option<Color32> {
        fn visit(shape: &egui::Shape, needle: &str) -> Option<Color32> {
            match shape {
                egui::Shape::Text(text) => {
                    let job = &text.galley.job;
                    job.sections.iter().find_map(|section| {
                        let section_text = &job.text[section.byte_range.clone()];
                        section_text.contains(needle).then_some(
                            text.override_text_color
                                .unwrap_or(section.format.color),
                        )
                    })
                }
                egui::Shape::Vec(shapes) => {
                    shapes.iter().find_map(|shape| visit(shape, needle))
                }
                _ => None,
            }
        }

        shapes
            .iter()
            .find_map(|clipped| visit(&clipped.shape, needle))
    }

    #[test]
    fn preview_heading_and_strong_text_use_wcag_color() {
        let ctx = egui::Context::default();
        ctx.style_mut(|style| {
            style.visuals.override_text_color = Some(Color32::BLACK);
            // Reproduce a light GTK theme whose accent foreground is white.
            style.visuals.widgets.active.fg_stroke.color = Color32::WHITE;
        });
        let expected = Color32::from_rgb(0x1c, 0x71, 0xd8);
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut cache = CommonMarkCache::default();
                show_with_tables(
                    ui,
                    &mut cache,
                    "# Preview heading\n\nBody with **bold words**.",
                    expected,
                );
            });
        });

        assert_eq!(
            rendered_text_color(&output.shapes, "Preview heading"),
            Some(expected)
        );
        assert_eq!(
            rendered_text_color(&output.shapes, "bold words"),
            Some(expected)
        );
    }

    #[test]
    fn heading_regions_include_atx_and_setext_headings() {
        let md = "# First\n\nBody\n\nSecond\n------\n\nLast";
        let regions = find_heading_regions(md);
        let headings: Vec<&str> = regions
            .iter()
            .map(|(start, end)| &md[*start..*end])
            .collect();

        assert_eq!(headings, vec!["# First\n", "Second\n------\n"]);
    }

    #[test]
    fn heading_regions_ignore_heading_markers_in_code() {
        let md = "```md\n# Not a heading\n```\n\n## Real heading";
        let regions = find_heading_regions(md);

        assert_eq!(regions.len(), 1);
        assert_eq!(&md[regions[0].0..regions[0].1], "## Real heading");
    }

    #[test]
    fn test_split_row_basic() {
        let cells = split_row("| H1 | H2 | H3 |");
        assert_eq!(cells, vec![" H1 |", " H2 |", " H3 |"]);
    }

    #[test]
    fn test_split_row_colspan() {
        let cells = split_row("| This cell spans 3 columns |||");
        assert_eq!(cells, vec![" This cell spans 3 columns |||"]);
    }

    #[test]
    fn test_split_row_rowspan() {
        let cells = split_row("| spans three ^|");
        assert_eq!(cells, vec![" spans three ^|"]);
    }

    #[test]
    fn test_split_row_multi_row_header() {
        let cells = split_row("| This header spans two   || Header A |");
        assert_eq!(cells, vec![" This header spans two   ||", " Header A |"]);
    }

    #[test]
    fn test_split_row_biology_row() {
        let cells = split_row("^ | 2 NADH | 3--5 ATP |");
        assert_eq!(cells, vec!["^ |", " 2 NADH |", " 3--5 ATP |"]);
    }

    #[test]
    fn test_find_table_region_keeps_extended_rows_without_leading_pipe() {
        let md = "Before\n\n| Stage | Products | ATP |\n|-------|----------|-----|\n| Glycolysis | None | 2 ATP |\n^ | 2 NADH | 3--5 ATP |\n\nAfter";
        let regions = find_table_regions(md);
        assert_eq!(regions.len(), 1);
        assert!(regions[0].2.contains("^ | 2 NADH | 3--5 ATP |"));

        let table = parse_extended_table(&regions[0].2).unwrap();
        assert_eq!(table.rows[1].cells[0].text, "Glycolysis");
        assert_eq!(table.rows[1].cells[0].rowspan, 2);
        assert_eq!(table.rows[2].cells[0].rowspan, 0);
    }

    #[test]
    fn test_restored_catalog_rowspans() {
        let md = "| Model | Base / Params | Format | File Size | Unique |\n|-------|---------------|--------|-----------|--------|\n| Kokoro-82M | StyleTTS2-LJSpeech, | PyTorch (Apache) | -- | TTS decoder-only, no |\n^ | 82M ^ | ^ | ^ | diffusion; 8 langs, ^ |\n^ | ^ | ^ | ^ | 54 voices ^ |\n^ | ^ | ^ | ^ | permissive training audio ^ |";
        let table = parse_extended_table(md).unwrap();

        assert_eq!(table.rows[1].cells[0].text, "Kokoro-82M");
        assert_eq!(table.rows[1].cells[0].rowspan, 4);
        assert_eq!(table.rows[1].cells[1].text, "StyleTTS2-LJSpeech, 82M");
        assert_eq!(table.rows[1].cells[1].rowspan, 4);
        assert_eq!(table.rows[1].cells[2].rowspan, 4);
        assert_eq!(table.rows[1].cells[3].rowspan, 4);
        assert_eq!(
            table.rows[1].cells[4].text,
            "TTS decoder-only, no diffusion; 8 langs, 54 voices permissive training audio"
        );
        assert_eq!(table.rows[1].cells[4].rowspan, 4);
        assert!(table.rows[4].cells.iter().all(|cell| cell.rowspan == 0));
    }

    #[test]
    fn test_markdown_cell_text_removes_inline_markers() {
        assert_eq!(markdown_cell_text("columns *and* two rows"), "columns and two rows");
        assert_eq!(markdown_cell_text("**30--32** ATP"), "30--32 ATP");
        assert_eq!(markdown_cell_text("`cat file | wc -l`"), "cat file | wc -l");
    }

    #[test]
    fn test_split_cells_basic() {
        let cells = split_cells("| H1 | H2 | H3 |", 3);
        assert_eq!(cells.len(), 3);
        assert_eq!(cells[0].text, "H1");
        assert_eq!(cells[0].colspan, 1);
        assert_eq!(cells[1].text, "H2");
        assert_eq!(cells[2].text, "H3");
    }

    #[test]
    fn test_split_cells_colspan() {
        let cells = split_cells("| This cell spans 3 columns |||", 3);
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].text, "This cell spans 3 columns");
        assert_eq!(cells[0].colspan, 3);
    }

    #[test]
    fn test_split_cells_colspan_partial() {
        let cells = split_cells("| spans two |||", 3);
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].text, "spans two");
        assert_eq!(cells[0].colspan, 3);
    }

    #[test]
    fn test_split_cells_escaped_pipe() {
        let cells = split_cells("| foo \\| bar | baz |", 2);
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].text, "foo | bar");
        assert_eq!(cells[1].text, "baz");
    }

    #[test]
    fn test_separator_alignment_none() {
        let (align, width) = parse_separator_cell("---");
        assert_eq!(align, Alignment::None);
        assert_eq!(width, None);
    }

    #[test]
    fn test_separator_alignment_left() {
        let (align, width) = parse_separator_cell(":---");
        assert_eq!(align, Alignment::Left);
        assert_eq!(width, None);
    }

    #[test]
    fn test_separator_alignment_center() {
        let (align, width) = parse_separator_cell(":---:");
        assert_eq!(align, Alignment::Center);
        assert_eq!(width, None);
    }

    #[test]
    fn test_separator_alignment_right() {
        let (align, width) = parse_separator_cell("---:");
        assert_eq!(align, Alignment::Right);
        assert_eq!(width, None);
    }

    #[test]
    fn test_separator_width_only() {
        let (align, width) = parse_separator_cell("--10%--");
        assert_eq!(align, Alignment::None);
        assert_eq!(width, Some(10));
    }

    #[test]
    fn test_separator_left_with_width() {
        let (align, width) = parse_separator_cell(":--10%--");
        assert_eq!(align, Alignment::Left);
        assert_eq!(width, Some(10));
    }

    #[test]
    fn test_separator_center_with_width() {
        let (align, width) = parse_separator_cell(":--20%--:");
        assert_eq!(align, Alignment::Center);
        assert_eq!(width, Some(20));
    }

    #[test]
    fn test_separator_right_with_width() {
        let (align, width) = parse_separator_cell("--50%--:");
        assert_eq!(align, Alignment::Right);
        assert_eq!(width, Some(50));
    }

    #[test]
    fn test_full_table_basic() {
        let md = "| Name | Value |\n|------|-------|\n| Foo  | 42    |";
        let table = parse_extended_table(md).unwrap();
        assert_eq!(table.num_cols, 2);
        assert_eq!(table.rows.len(), 2); // header + 1 body
        assert!(table.rows[0].is_header);
        assert_eq!(table.rows[0].cells[0].text, "Name");
        assert_eq!(table.rows[1].cells[0].text, "Foo");
    }

    #[test]
    fn test_full_table_colspan() {
        let md = "| H1 | H2 | H3 |\n|----|----|----|\n| This cell spans 3 columns |||";
        let table = parse_extended_table(md).unwrap();
        assert_eq!(table.num_cols, 3);
        assert_eq!(table.rows[1].cells.len(), 1);
        assert_eq!(table.rows[1].cells[0].text, "This cell spans 3 columns");
        assert_eq!(table.rows[1].cells[0].colspan, 3);
    }

    #[test]
    fn test_full_table_rowspan() {
        let md = "| H1 | H2 |\n|----|----|\n| This cell    | Cell A |\n| spans three ^| Cell B |\n| rows        ^| Cell C |";
        let table = parse_extended_table(md).unwrap();
        assert_eq!(table.num_cols, 2);
        assert_eq!(table.rows.len(), 4); // header + 3 body

        // Target cell should have merged text and rowspan=3
        assert_eq!(table.rows[1].cells[0].text, "This cell spans three rows");
        assert_eq!(table.rows[1].cells[0].rowspan, 3);

        // Ghost cells
        assert_eq!(table.rows[2].cells[0].rowspan, 0);
        assert_eq!(table.rows[3].cells[0].rowspan, 0);
    }

    #[test]
    fn test_full_table_multirow_header() {
        let md = "| This header spans two   || Header A |\n| columns *and* two rows ^|| Header B |\n|-------------|------------|----------|\n| Cell A      | Cell B     | Cell C   |";
        let table = parse_extended_table(md).unwrap();
        assert_eq!(table.num_cols, 3);
        assert_eq!(table.rows.len(), 3); // 2 header + 1 body

        // First header cell should span 2 cols and 2 rows
        assert_eq!(table.rows[0].cells[0].text, "This header spans two columns *and* two rows");
        assert_eq!(table.rows[0].cells[0].colspan, 2);
        assert_eq!(table.rows[0].cells[0].rowspan, 2);

        // Ghost cell in second header row
        assert_eq!(table.rows[1].cells[0].rowspan, 0);
    }

    #[test]
    fn test_full_table_biology_atp() {
        let md = "| Stage | Products | ATP |\n|-------|----------|-----|\n| Glycolysis | None| 2 ATP |\n^ | 2 NADH | 3--5 ATP |";
        let table = parse_extended_table(md).unwrap();
        assert_eq!(table.num_cols, 3);
        assert_eq!(table.rows.len(), 3);

        // Glycolysis should span 2 rows
        assert_eq!(table.rows[1].cells[0].text, "Glycolysis");
        assert_eq!(table.rows[1].cells[0].rowspan, 2);

        // Ghost cell in second data row
        assert_eq!(table.rows[2].cells[0].rowspan, 0);
        assert_eq!(table.rows[2].cells[1].text, "2 NADH");
        assert_eq!(table.rows[2].cells[2].text, "3--5 ATP");
    }

    #[test]
    fn test_full_table_widths_and_alignment() {
        let md = "| A | B | C |\n|--10%--|:--20%--:|--50%--:|\n| Cell A | Cell B | Cell C |";
        let table = parse_extended_table(md).unwrap();
        assert_eq!(table.num_cols, 3);
        assert_eq!(table.col_widths, vec![Some(10), Some(20), Some(50)]);
        assert_eq!(table.rows[0].cells[0].alignment, Alignment::None);
        assert_eq!(table.rows[0].cells[1].alignment, Alignment::Center);
        assert_eq!(table.rows[0].cells[2].alignment, Alignment::Right);
    }

    #[test]
    fn test_minimal_delimiter() {
        let md = "| A | B | C | D |\n|-|:-|-:|:-:|\n| 1 | 2 | 3 | 4 |";
        let table = parse_extended_table(md).unwrap();
        assert_eq!(table.num_cols, 4);
        assert_eq!(table.rows[0].cells[0].alignment, Alignment::None);
        assert_eq!(table.rows[0].cells[1].alignment, Alignment::Left);
        assert_eq!(table.rows[0].cells[2].alignment, Alignment::Right);
        assert_eq!(table.rows[0].cells[3].alignment, Alignment::Center);
    }
}
