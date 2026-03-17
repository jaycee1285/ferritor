use egui::{text::LayoutJob, Color32, FontId, TextFormat};
use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};

static HIGHLIGHTER: OnceLock<SyntaxHighlighter> = OnceLock::new();

pub struct SyntaxHighlighter {
    syntax_set: SyntaxSet,
    theme: Theme,
}

impl SyntaxHighlighter {
    pub fn get() -> &'static Self {
        HIGHLIGHTER.get_or_init(Self::new)
    }

    fn new() -> Self {
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let theme = load_user_theme().unwrap_or_else(|| {
            let ts = ThemeSet::load_defaults();
            ts.themes
                .get("base16-ocean.light")
                .or_else(|| ts.themes.values().next())
                .cloned()
                .expect("syntect must have at least one default theme")
        });
        Self { syntax_set, theme }
    }

    /// Find the best syntax for a file path. Falls back to plain text.
    fn find_syntax(&self, path: &std::path::Path) -> &SyntaxReference {
        if let Some(syntax) = self.find_known_web_syntax(path) {
            return syntax;
        }

        if let Some(file_name) = path.file_name().and_then(|name| name.to_str()) {
            if let Some(syntax) = self.syntax_set.find_syntax_by_token(file_name) {
                return syntax;
            }
        }

        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if let Some(syntax) = self.syntax_set.find_syntax_by_extension(&ext.to_lowercase()) {
                return syntax;
            }
        }

        self.syntax_set
            .find_syntax_for_file(path)
            .ok()
            .flatten()
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text())
    }

    fn find_known_web_syntax(&self, path: &std::path::Path) -> Option<&SyntaxReference> {
        let ext = path.extension().and_then(|e| e.to_str())?.to_ascii_lowercase();
        match ext.as_str() {
            "ts" | "tsx" => self
                .syntax_set
                .find_syntax_by_name("TypeScript")
                .or_else(|| self.syntax_set.find_syntax_by_extension("ts"))
                .or_else(|| self.syntax_set.find_syntax_by_name("JavaScript"))
                .or_else(|| self.syntax_set.find_syntax_by_extension("js")),
            "js" | "jsx" | "mjs" | "cjs" => self
                .syntax_set
                .find_syntax_by_name("JavaScript")
                .or_else(|| self.syntax_set.find_syntax_by_extension("js")),
            _ => None,
        }
    }

    /// Build an egui LayoutJob with syntax-highlighted content.
    pub fn highlight(&self, content: &str, path: &std::path::Path) -> LayoutJob {
        let syntax = self.find_syntax(path);
        let mut h = HighlightLines::new(syntax, &self.theme);
        let mut job = LayoutJob::default();
        let font_id = FontId::monospace(14.0); // matches TextStyle::Monospace

        let default_fg = self
            .theme
            .settings
            .foreground
            .map(syntect_to_egui)
            .unwrap_or(Color32::GRAY);

        for line in syntect::util::LinesWithEndings::from(content) {
            match h.highlight_line(line, &self.syntax_set) {
                Ok(ranges) => {
                    for (style, text) in ranges {
                        let color = syntect_to_egui(style.foreground);
                        let mut format = TextFormat {
                            font_id: font_id.clone(),
                            color,
                            ..Default::default()
                        };
                        if style.font_style.contains(FontStyle::ITALIC) {
                            format.italics = true;
                        }
                        // egui TextFormat doesn't have bold — we just use color
                        job.append(text, 0.0, format);
                    }
                }
                Err(_) => {
                    job.append(
                        line,
                        0.0,
                        TextFormat {
                            font_id: font_id.clone(),
                            color: default_fg,
                            ..Default::default()
                        },
                    );
                }
            }
        }

        job
    }
}

fn syntect_to_egui(c: syntect::highlighting::Color) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

fn load_user_theme() -> Option<Theme> {
    let path = dirs::home_dir()?
        .join(".config")
        .join("syntect")
        .join("current.tmTheme");
    if !path.exists() {
        return None;
    }
    ThemeSet::get_theme(&path).ok()
}
