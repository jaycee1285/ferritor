use std::collections::HashMap;
use std::path::Path;

/// Color values parsed from ~/.config/gtk-4.0/gtk.css
/// These are always present — the theme changer guarantees the format.
#[derive(Debug, Clone)]
pub struct GtkTheme {
    pub colors: HashMap<String, egui::Color32>,
}

impl GtkTheme {
    pub fn load() -> Self {
        let path = dirs::config_dir()
            .unwrap_or_default()
            .join("gtk-4.0")
            .join("gtk.css");
        Self::from_file(&path)
    }

    fn from_file(path: &Path) -> Self {
        let mut colors = HashMap::new();
        if let Ok(content) = std::fs::read_to_string(path) {
            for line in content.lines() {
                if let Some(parsed) = parse_define_color(line) {
                    colors.insert(parsed.0, parsed.1);
                }
            }
        }
        Self { colors }
    }

    // Named accessors for the GTK semantic colors
    pub fn accent(&self) -> egui::Color32 {
        self.get("accent_bg_color")
    }

    pub fn accent_fg(&self) -> egui::Color32 {
        self.get("accent_fg_color")
    }

    pub fn success(&self) -> egui::Color32 {
        self.get("success_color")
    }

    pub fn warning(&self) -> egui::Color32 {
        self.get("warning_color")
    }

    pub fn error(&self) -> egui::Color32 {
        self.get("error_color")
    }

    pub fn destructive(&self) -> egui::Color32 {
        self.get("destructive_color")
    }

    pub fn window_bg(&self) -> egui::Color32 {
        self.get("window_bg_color")
    }

    pub fn window_fg(&self) -> egui::Color32 {
        self.get("window_fg_color")
    }

    pub fn view_bg(&self) -> egui::Color32 {
        self.get("view_bg_color")
    }

    pub fn view_fg(&self) -> egui::Color32 {
        self.get("view_fg_color")
    }

    pub fn headerbar_bg(&self) -> egui::Color32 {
        self.get("headerbar_bg_color")
    }

    pub fn headerbar_fg(&self) -> egui::Color32 {
        self.get("headerbar_fg_color")
    }

    pub fn card_bg(&self) -> egui::Color32 {
        self.get("card_bg_color")
    }

    pub fn card_fg(&self) -> egui::Color32 {
        self.get("card_fg_color")
    }

    pub fn shade(&self) -> egui::Color32 {
        self.get("shade_color")
    }

    // Palette colors (blue_1 through blue_5, green_1, etc.)
    pub fn palette(&self, name: &str, level: u8) -> egui::Color32 {
        self.get(&format!("{}_{}", name, level))
    }

    fn get(&self, name: &str) -> egui::Color32 {
        self.colors
            .get(name)
            .copied()
            .unwrap_or(egui::Color32::PLACEHOLDER)
    }
}

/// Parse a line like: @define-color accent_bg_color #399ee6;
/// Also handles rgba() format: @define-color window_fg_color rgba(0, 0, 0, 0.87);
/// Also handles named colors: @define-color accent_fg_color white;
fn parse_define_color(line: &str) -> Option<(String, egui::Color32)> {
    let line = line.trim();
    if !line.starts_with("@define-color ") {
        return None;
    }
    let rest = &line["@define-color ".len()..];
    let rest = rest.trim_end_matches(';').trim();

    let space_idx = rest.find(' ')?;
    let name = rest[..space_idx].to_string();
    let value = rest[space_idx..].trim();

    let color = parse_color_value(value)?;
    Some((name, color))
}

fn parse_color_value(value: &str) -> Option<egui::Color32> {
    if value.starts_with('#') {
        parse_hex(value)
    } else if value.starts_with("rgba(") {
        parse_rgba(value)
    } else {
        match value {
            "white" => Some(egui::Color32::WHITE),
            "black" => Some(egui::Color32::BLACK),
            "transparent" => Some(egui::Color32::TRANSPARENT),
            _ => None,
        }
    }
}

fn parse_hex(hex: &str) -> Option<egui::Color32> {
    let hex = hex.trim_start_matches('#');
    match hex.len() {
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(egui::Color32::from_rgb(r, g, b))
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some(egui::Color32::from_rgba_premultiplied(r, g, b, a))
        }
        _ => None,
    }
}

fn parse_rgba(value: &str) -> Option<egui::Color32> {
    // rgba(0, 0, 0, 0.87)
    let inner = value.strip_prefix("rgba(")?.strip_suffix(')')?;
    let parts: Vec<&str> = inner.split(',').map(|s| s.trim()).collect();
    if parts.len() != 4 {
        return None;
    }
    let r: u8 = parts[0].parse().ok()?;
    let g: u8 = parts[1].parse().ok()?;
    let b: u8 = parts[2].parse().ok()?;
    let a: f32 = parts[3].parse().ok()?;
    Some(egui::Color32::from_rgba_unmultiplied(
        r,
        g,
        b,
        (a * 255.0) as u8,
    ))
}
