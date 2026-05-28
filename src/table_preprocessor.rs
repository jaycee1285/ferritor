//! Simple table preprocessor - merges continuation rows in markdown tables
//!
//! A markdown table is defined by:
//! - Header row: | col1 | col2 | col3 |
//! - Separator:  |---|---|---|
//! - Data rows:  | cell1 | cell2 | cell3 |
//!
//! Continuation rows are detected when the first cell is empty (| | or ||)
//! or when the previous row has unclosed formatting (odd number of ** markers).

/// Preprocess markdown tables to merge continuation rows.
/// Returns modified markdown with merged cells.
pub fn merge_table_continuations(md: &str) -> String {
    let lines: Vec<&str> = md.lines().collect();
    let mut result: Vec<String> = Vec::new();

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        // Check if this line starts a table
        if trimmed.starts_with('|') && trimmed.matches('|').count() >= 2 {
            // Find the end of this table
            let table_start = i;
            let mut table_end = i;

            while table_end < lines.len() {
                let tline = lines[table_end].trim();
                if !tline.starts_with('|') {
                    break;
                }
                table_end += 1;
            }

            // Process the table
            let table_lines: Vec<&str> = lines[table_start..table_end].to_vec();
            let processed_table = process_table(&table_lines);
            result.extend(processed_table);

            i = table_end;
        } else {
            result.push(line.to_string());
            i += 1;
        }
    }

    result.join("\n")
}

/// Process a single table, merging continuation rows
fn process_table(lines: &[&str]) -> Vec<String> {
    if lines.is_empty() {
        return Vec::new();
    }

    // Count columns by counting non-empty cells in header row
    let num_cols = lines[0].split('|').skip(1).filter(|s| !s.is_empty()).count();
    if num_cols == 0 {
        return lines.iter().map(|s| s.to_string()).collect();
    }

    let mut result: Vec<String> = Vec::new();
    let mut current_row_group: Vec<Vec<String>> = Vec::new(); // Rows that form one logical row

    for line in lines {
        let trimmed = line.trim();

        // Check if this is a separator row
        let is_separator = trimmed.chars().all(|c| c == '|' || c == '-' || c == ':' || c == ' ')
            && trimmed.contains('-');

        if is_separator {
            // Flush current row group before the separator
            if !current_row_group.is_empty() {
                let merged = merge_row_group(&current_row_group);
                result.push(render_row(&merged));
                current_row_group.clear();
            }
            result.push(line.to_string());
        } else {
            // Parse this row into cells (skip first empty, take non-empty cells up to num_cols)
            let cells: Vec<String> = line.split('|')
                .skip(1)
                .filter(|s| !s.is_empty())
                .take(num_cols)
                .map(|s| s.to_string())
                .collect();
            
            // Check if this is a continuation row
            let first_cell = cells.get(0).map(|s| s.trim()).unwrap_or("");
            let is_continuation = !current_row_group.is_empty() && 
                (first_cell.is_empty() || first_cell.starts_with("**"));
            
            // Check if previous row had incomplete formatting
            let prev_incomplete = !current_row_group.is_empty() && 
                has_incomplete_formatting(&current_row_group[0][0]);
            
            if is_continuation || prev_incomplete {
                // Continue current group
                current_row_group.push(cells);
            } else {
                // New logical row - flush previous group
                if !current_row_group.is_empty() {
                    let merged = merge_row_group(&current_row_group);
                    result.push(render_row(&merged));
                }
                current_row_group = vec![cells];
            }
        }
    }

    // Flush final row group
    if !current_row_group.is_empty() {
        let merged = merge_row_group(&current_row_group);
        result.push(render_row(&merged));
    }

    result
}

/// Merge a group of rows into a single logical row
fn merge_row_group(rows: &[Vec<String>]) -> Vec<String> {
    if rows.is_empty() {
        return Vec::new();
    }

    if rows.len() == 1 {
        return rows[0].clone();
    }

    let num_cols = rows[0].len();
    let mut merged: Vec<String> = vec![String::new(); num_cols];

    for row in rows {
        for (col_idx, cell) in row.iter().enumerate() {
            let trimmed = cell.trim();
            if !trimmed.is_empty() {
                if !merged[col_idx].is_empty() {
                    merged[col_idx].push(' ');
                }
                merged[col_idx].push_str(trimmed);
            }
        }
    }

    merged
}

/// Check if content has incomplete formatting (odd number of ** markers)
fn has_incomplete_formatting(content: &str) -> bool {
    let bold_count = content.matches("**").count();
    bold_count % 2 == 1
}

/// Render a row of cells back to markdown format
fn render_row(cells: &[String]) -> String {
    let mut result = String::from("|");
    for cell in cells {
        result.push(' ');
        result.push_str(cell);
        result.push_str(" |");
    }
    result
}
