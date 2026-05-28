// Markdown table formatter.
//
// Walks a markdown document, locates GFM-style table blocks, and rewrites each
// block so:
//   - column dashes equal the longest cell string in that column, capped at 20
//   - cells whose content exceeds the column width are word-wrapped across
//     continuation rows (with empty cells in the other columns of those rows)
//   - words longer than the cap are placed alone on a line and allowed to
//     overflow rather than broken mid-word
//
// Code-fenced regions (```...``` or ~~~...~~~) are skipped — `|` inside code
// is not a table delimiter. Header rows are not wrapped (markdown can't
// continue a header).
//
// Algorithm builds on html-to-markdown's table renderer
// (calculateMaxCounts + writeRow), with the 20-char cap and wrap added.

const COLUMN_WIDTH_CAP: usize = 20;
const COLUMN_WIDTH_MIN: usize = 1;

/// Format every markdown table in `input`, return the rewritten document.
/// Non-table lines and code-fenced regions pass through unchanged.
pub fn format_tables(input: &str) -> String {
    let lines: Vec<&str> = input.split_inclusive('\n').collect();
    let mut out = String::with_capacity(input.len());

    let mut i = 0;
    let mut in_code_fence = false;
    while i < lines.len() {
        let line = lines[i];
        let stripped = strip_eol(line);

        // Track ``` fences so we don't reflow a `|` inside code.
        if is_code_fence(stripped) {
            in_code_fence = !in_code_fence;
            out.push_str(line);
            i += 1;
            continue;
        }

        if !in_code_fence
            && i + 1 < lines.len()
            && looks_like_table_row(stripped)
            && is_separator_row(strip_eol(lines[i + 1]))
        {
            // Collect: header, separator, body rows.
            let header = stripped;
            let separator = strip_eol(lines[i + 1]);
            let mut body = Vec::new();
            let mut j = i + 2;
            while j < lines.len() {
                let row = strip_eol(lines[j]);
                if !looks_like_table_row(row) {
                    break;
                }
                body.push(row);
                j += 1;
            }

            let formatted = format_block(header, separator, &body);
            out.push_str(&formatted);
            i = j;
            continue;
        }

        out.push_str(line);
        i += 1;
    }

    out
}

fn strip_eol(line: &str) -> &str {
    line.strip_suffix('\n')
        .map(|s| s.strip_suffix('\r').unwrap_or(s))
        .unwrap_or(line)
}

fn is_code_fence(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("```") || t.starts_with("~~~")
}

fn looks_like_table_row(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with('|') && t.contains('|')
}

fn is_separator_row(line: &str) -> bool {
    let cells = split_cells(line);
    if cells.is_empty() {
        return false;
    }
    cells.iter().all(|cell| {
        let inner = cell.trim();
        if inner.is_empty() {
            return false;
        }
        let stripped = inner.trim_start_matches(':').trim_end_matches(':');
        !stripped.is_empty() && stripped.chars().all(|c| c == '-')
    })
}

/// Split a markdown table row into cells. Strips the leading/trailing pipe.
/// Does not handle escaped pipes (`\|`); that's a v2 concern.
fn split_cells(row: &str) -> Vec<String> {
    let trimmed = row.trim();
    let inner = trimmed
        .strip_prefix('|')
        .unwrap_or(trimmed)
        .strip_suffix('|')
        .unwrap_or_else(|| trimmed.strip_prefix('|').unwrap_or(trimmed));
    inner.split('|').map(|s| s.trim().to_string()).collect()
}

#[derive(Clone, Copy)]
enum Align {
    None,
    Left,
    Right,
    Center,
}

fn parse_alignments(separator: &str) -> Vec<Align> {
    split_cells(separator)
        .into_iter()
        .map(|cell| {
            let t = cell.trim();
            let left = t.starts_with(':');
            let right = t.ends_with(':');
            match (left, right) {
                (true, true) => Align::Center,
                (true, false) => Align::Left,
                (false, true) => Align::Right,
                (false, false) => Align::None,
            }
        })
        .collect()
}

fn char_count(s: &str) -> usize {
    s.chars().count()
}

fn pad_cell(cell: &str, width: usize) -> String {
    let len = char_count(cell);
    if len >= width {
        cell.to_string()
    } else {
        let mut s = String::with_capacity(cell.len() + (width - len));
        s.push_str(cell);
        for _ in 0..(width - len) {
            s.push(' ');
        }
        s
    }
}

fn render_separator(widths: &[usize], aligns: &[Align]) -> String {
    let mut out = String::with_capacity(widths.iter().sum::<usize>() + widths.len() * 4);
    out.push('|');
    for (i, w) in widths.iter().enumerate() {
        let align = aligns.get(i).copied().unwrap_or(Align::None);
        // Match html-to-markdown's CellPaddingBehaviorAligned:
        // colon-or-dash, then `width` dashes, then colon-or-dash.
        out.push(match align {
            Align::Left | Align::Center => ':',
            _ => '-',
        });
        for _ in 0..*w {
            out.push('-');
        }
        out.push(match align {
            Align::Right | Align::Center => ':',
            _ => '-',
        });
        out.push('|');
    }
    out.push('\n');
    out
}

fn render_row(cells: &[String], widths: &[usize]) -> String {
    // Single-line render — used for the header. Body rows go through render_row_wrapped.
    let mut out = String::with_capacity(widths.iter().sum::<usize>() + widths.len() * 3);
    out.push('|');
    for (i, w) in widths.iter().enumerate() {
        let empty = String::new();
        let cell = cells.get(i).unwrap_or(&empty);
        out.push(' ');
        out.push_str(&pad_cell(cell, *w));
        out.push(' ');
        out.push('|');
    }
    out.push('\n');
    out
}

fn word_wrap(text: &str, width: usize) -> Vec<String> {
    // Greedy word-wrap into lines of at most `width` chars. A single word
    // longer than `width` gets its own line and is allowed to overflow.
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in words {
        if current.is_empty() {
            current.push_str(word);
        } else if char_count(&current) + 1 + char_count(word) <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

fn render_row_wrapped(cells: &[String], widths: &[usize]) -> String {
    let segments: Vec<Vec<String>> = cells
        .iter()
        .zip(widths.iter())
        .map(|(cell, w)| word_wrap(cell, *w))
        .collect();
    let line_count = segments
        .iter()
        .map(|s| s.len())
        .max()
        .unwrap_or(1)
        .max(1);

    let mut out = String::with_capacity(line_count * (widths.iter().sum::<usize>() + widths.len() * 3));
    for line_idx in 0..line_count {
        out.push('|');
        for (col_idx, w) in widths.iter().enumerate() {
            let empty = String::new();
            let segment = segments
                .get(col_idx)
                .and_then(|s| s.get(line_idx))
                .unwrap_or(&empty);
            out.push(' ');
            out.push_str(&pad_cell(segment, *w));
            out.push(' ');
            out.push('|');
        }
        out.push('\n');
    }
    out
}

fn format_block(header: &str, separator: &str, body: &[&str]) -> String {
    let header_cells = split_cells(header);
    let aligns = parse_alignments(separator);
    let body_cells: Vec<Vec<String>> = body.iter().map(|row| split_cells(row)).collect();

    let column_count = header_cells
        .len()
        .max(body_cells.iter().map(|r| r.len()).max().unwrap_or(0));

    // Column width = min(longest cell, cap), floored at 1.
    let mut widths = vec![COLUMN_WIDTH_MIN; column_count];
    let row_iter = std::iter::once(&header_cells).chain(body_cells.iter());
    for row in row_iter {
        for (i, cell) in row.iter().enumerate() {
            if i >= column_count {
                break;
            }
            let len = char_count(cell);
            let capped = len.min(COLUMN_WIDTH_CAP);
            if capped > widths[i] {
                widths[i] = capped;
            }
        }
    }

    let mut out = String::new();
    // Header is one line — markdown has no header continuation row.
    out.push_str(&render_row(&pad_to_columns(&header_cells, column_count), &widths));
    out.push_str(&render_separator(&widths, &aligns));
    for row in &body_cells {
        out.push_str(&render_row_wrapped(&pad_to_columns(row, column_count), &widths));
    }
    out
}

fn pad_to_columns(cells: &[String], n: usize) -> Vec<String> {
    let mut v = cells.to_vec();
    while v.len() < n {
        v.push(String::new());
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_simple_table() {
        let input = "\
| h1 | h2 |
|---|---|
| a | b |
| longer | x |
";
        let out = format_tables(input);
        assert!(out.contains("| h1     | h2 |"));
        assert!(out.contains("|--------|----|"));
        assert!(out.contains("| a      | b  |"));
        assert!(out.contains("| longer | x  |"));
    }

    #[test]
    fn caps_dashes_at_twenty() {
        let input = "\
| col |
|---|
| this cell is way longer than twenty characters easily |
";
        let out = format_tables(input);
        // Dash row caps at 20 dashes (plus the outer two from html-to-markdown's
        // CellPaddingBehaviorAligned style → 22 total).
        let dashes = "-".repeat(20 + 2);
        assert!(
            out.contains(&format!("|{}|", dashes)),
            "expected 22-dash separator, got:\n{}",
            out
        );
    }

    #[test]
    fn wraps_overflow_into_continuation_rows() {
        let input = "\
| name | description |
|---|---|
| greetd | minimal and flexible login daemon |
";
        let out = format_tables(input);
        // The long body cell wraps across multiple lines; continuation lines
        // have empty cells in the other columns.
        let lines: Vec<&str> = out.lines().filter(|l| l.starts_with('|')).collect();
        // Header + separator + at least 2 body lines from the wrap.
        assert!(
            lines.len() >= 4,
            "expected ≥4 table lines after wrap, got:\n{}",
            out
        );
        // First wrap line carries the name; second has empty name slot.
        assert!(lines[2].contains("greetd"));
        let name_cell_continuation = lines[3].split('|').nth(1).unwrap_or("").trim();
        assert!(
            name_cell_continuation.is_empty(),
            "continuation row should have empty name cell, got: {:?}",
            name_cell_continuation
        );
    }

    #[test]
    fn long_word_does_not_break() {
        let input = "\
| col |
|---|
| supercalifragilisticexpialidocious |
";
        let out = format_tables(input);
        // The single long word stays intact on its own line — we don't
        // break mid-word.
        assert!(out.contains("supercalifragilisticexpialidocious"));
    }

    #[test]
    fn skips_code_fences() {
        let input = "\
```
| not | a | table |
|---|---|---|
| inside | code | block |
```
";
        let out = format_tables(input);
        assert_eq!(out, input);
    }

    #[test]
    fn preserves_alignment_markers() {
        let input = "\
| L | C | R |
|:--|:-:|--:|
| 1 | 2 | 3 |
";
        let out = format_tables(input);
        assert!(out.contains(":--"));
        assert!(out.contains(":-:") || out.contains(":-"));
        assert!(out.contains("--:"));
    }

    #[test]
    fn fills_short_rows() {
        let input = "\
| a | b | c |
|---|---|---|
| 1 |
";
        let out = format_tables(input);
        // Three columns rendered even when a row was short.
        let body_line = out.lines().nth(2).unwrap_or("");
        assert_eq!(body_line.matches('|').count(), 4, "body row got: {body_line}");
    }

    #[test]
    fn passes_through_non_table_text() {
        let input = "# heading\n\nparagraph\n\n- list item\n";
        assert_eq!(format_tables(input), input);
    }
}
