use egui::{text::LayoutJob, Color32, FontFamily, FontId, TextFormat};
use std::path::PathBuf;
use std::str::FromStr;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSet};
use syntect::parsing::{ScopeStack, SyntaxReference, SyntaxSet};

pub struct SyntaxHighlighter {
    syntax_set: SyntaxSet,
    theme: Theme,
}

impl SyntaxHighlighter {
    pub fn theme_path() -> Option<PathBuf> {
        Some(
            dirs::home_dir()?
                .join(".config")
                .join("syntect")
                .join("current.tmTheme"),
        )
    }

    pub fn new(heading_color: Color32) -> Self {
        Self::try_new(heading_color).unwrap_or_else(|| {
            let mut theme = default_theme();
            apply_heading_style(&mut theme, heading_color);
            Self {
                syntax_set: SyntaxSet::load_defaults_newlines(),
                theme,
            }
        })
    }

    pub fn try_new(heading_color: Color32) -> Option<Self> {
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let mut theme = load_user_theme()?;
        apply_heading_style(&mut theme, heading_color);
        Some(Self { syntax_set, theme })
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
            if let Some(syntax) = self
                .syntax_set
                .find_syntax_by_extension(&ext.to_lowercase())
            {
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
        let ext = path
            .extension()
            .and_then(|e| e.to_str())?
            .to_ascii_lowercase();
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
        let regular_font = FontId::monospace(14.0); // matches TextStyle::Monospace
        let bold_font = FontId::new(14.0, FontFamily::Name("MonoBold".into()));

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
                            font_id: if style.font_style.contains(FontStyle::BOLD) {
                                bold_font.clone()
                            } else {
                                regular_font.clone()
                            },
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
                            font_id: regular_font.clone(),
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
    let path = SyntaxHighlighter::theme_path()?;
    if !path.exists() {
        return Some(default_theme());
    }
    ThemeSet::get_theme(&path).ok()
}

fn default_theme() -> Theme {
    let ts = ThemeSet::load_defaults();
    ts.themes
        .get("base16-ocean.light")
        .or_else(|| ts.themes.values().next())
        .cloned()
        .expect("syntect must have at least one default theme")
}

fn apply_heading_style(theme: &mut Theme, color: Color32) {
    let Ok(scope) = ScopeSelectors::from_str(
        "markup.heading, markup.heading punctuation.definition.heading, entity.name.section",
    ) else {
        return;
    };
    let style = StyleModifier {
        foreground: Some(syntect::highlighting::Color {
            r: color.r(),
            g: color.g(),
            b: color.b(),
            a: color.a(),
        }),
        background: None,
        font_style: Some(FontStyle::BOLD),
    };
    theme.scopes.push(ThemeItem { scope, style });

    // Syntect keeps the first rule when selectors have equal match power, so
    // merely appending an override is insufficient. Update every strongest
    // rule for representative heading text and punctuation stacks.
    for stack in [
        "text.html.markdown markup.heading.markdown entity.name.section.markdown",
        "text.html.markdown markup.heading.markdown punctuation.definition.heading.markdown",
    ] {
        let Ok(stack) = ScopeStack::from_str(stack) else {
            continue;
        };
        let best = theme
            .scopes
            .iter()
            .filter_map(|item| item.scope.does_match(stack.as_slice()))
            .max();
        let Some(best) = best else {
            continue;
        };
        for item in &mut theme.scopes {
            if item.scope.does_match(stack.as_slice()) == Some(best) {
                item.style.foreground = style.foreground;
                item.style.font_style = style.font_style;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_heading_uses_ferritor_color_and_bold_font() {
        let heading_color = Color32::from_rgb(0x71, 0x28, 0x8f);
        let mut theme = default_theme();
        apply_heading_style(&mut theme, heading_color);
        let highlighter = SyntaxHighlighter {
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme,
        };

        let job = highlighter.highlight("# Ferritor heading\n", std::path::Path::new("test.md"));
        let heading_section = job
            .sections
            .iter()
            .find(|section| job.text[section.byte_range.clone()].contains("Ferritor heading"))
            .expect("heading text should have a highlighted section");

        assert_eq!(heading_section.format.color, heading_color);
        assert_eq!(
            heading_section.format.font_id.family,
            FontFamily::Name("MonoBold".into())
        );
    }
}
