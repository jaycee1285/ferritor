use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct GtkTheme {
    pub colors: HashMap<String, egui::Color32>,
}

impl GtkTheme {
    pub fn source_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_default()
            .join("gtk-4.0")
            .join("gtk.css")
    }

    pub fn load() -> Self {
        Self::try_load().unwrap_or_else(|| Self {
            colors: HashMap::new(),
        })
    }

    pub fn try_load() -> Option<Self> {
        let theme = Self::from_file(&Self::source_path())?;
        (!theme.colors.is_empty()).then_some(theme)
    }

    fn from_file(path: &Path) -> Option<Self> {
        let content = std::fs::read_to_string(path).ok()?;

        let mut defs = HashMap::<String, String>::new();
        for line in content.lines() {
            if let Some((name, value)) = parse_define_color_raw(line) {
                defs.insert(name, value);
            }
        }

        let mut colors = HashMap::<String, egui::Color32>::new();
        let mut resolving = HashSet::<String>::new();
        let keys: Vec<String> = defs.keys().cloned().collect();
        for key in keys {
            let _ = resolve_named_color(&key, &defs, &mut colors, &mut resolving);
        }

        Some(Self { colors })
    }

    pub fn is_dark(&self) -> bool {
        luminance(self.view_bg()) < 0.5
    }

    pub fn accent(&self) -> egui::Color32 {
        self.get_opt("accent_bg_color")
            .or_else(|| self.get_opt("accent_color"))
            .unwrap_or(egui::Color32::from_rgb(80, 140, 220))
    }

    pub fn accent_fg(&self) -> egui::Color32 {
        self.get_opt("accent_fg_color")
            .unwrap_or_else(|| readable_foreground(self.accent()))
    }

    #[allow(dead_code)]
    pub fn success(&self) -> egui::Color32 {
        self.get_opt("success_color")
            .or_else(|| self.get_opt("success_bg_color"))
            .unwrap_or(egui::Color32::from_rgb(78, 161, 94))
    }

    pub fn warning(&self) -> egui::Color32 {
        self.get_opt("warning_color")
            .or_else(|| self.get_opt("warning_bg_color"))
            .unwrap_or(egui::Color32::from_rgb(230, 165, 10))
    }

    pub fn error(&self) -> egui::Color32 {
        self.get_opt("error_color")
            .or_else(|| self.get_opt("error_bg_color"))
            .unwrap_or(egui::Color32::from_rgb(210, 65, 65))
    }

    #[allow(dead_code)]
    pub fn destructive(&self) -> egui::Color32 {
        self.get_opt("destructive_color")
            .or_else(|| self.get_opt("destructive_bg_color"))
            .unwrap_or_else(|| self.error())
    }

    pub fn destructive_fg(&self) -> egui::Color32 {
        self.get_opt("destructive_fg_color")
            .unwrap_or_else(|| readable_foreground(self.destructive()))
    }

    pub fn window_bg(&self) -> egui::Color32 {
        self.get_opt("window_bg_color")
            .unwrap_or(egui::Color32::from_rgb(32, 32, 32))
    }

    pub fn window_fg(&self) -> egui::Color32 {
        self.get_opt("window_fg_color")
            .unwrap_or_else(|| readable_foreground(self.window_bg()))
    }

    pub fn view_bg(&self) -> egui::Color32 {
        self.get_opt("view_bg_color")
            .unwrap_or_else(|| self.window_bg())
    }

    pub fn view_fg(&self) -> egui::Color32 {
        self.get_opt("view_fg_color")
            .unwrap_or_else(|| self.window_fg())
    }

    pub fn headerbar_bg(&self) -> egui::Color32 {
        self.get_opt("headerbar_bg_color")
            .unwrap_or_else(|| self.window_bg())
    }

    pub fn headerbar_fg(&self) -> egui::Color32 {
        self.get_opt("headerbar_fg_color")
            .unwrap_or_else(|| self.window_fg())
    }

    pub fn headerbar_border(&self) -> egui::Color32 {
        self.get_opt("headerbar_border_color")
            .unwrap_or_else(|| self.shade())
    }

    pub fn card_bg(&self) -> egui::Color32 {
        self.get_opt("card_bg_color")
            .unwrap_or_else(|| self.view_bg())
    }

    pub fn card_fg(&self) -> egui::Color32 {
        self.get_opt("card_fg_color")
            .unwrap_or_else(|| self.view_fg())
    }

    pub fn dialog_bg(&self) -> egui::Color32 {
        self.get_opt("dialog_bg_color")
            .unwrap_or_else(|| self.card_bg())
    }

    #[allow(dead_code)]
    pub fn dialog_fg(&self) -> egui::Color32 {
        self.get_opt("dialog_fg_color")
            .unwrap_or_else(|| self.card_fg())
    }

    pub fn popover_bg(&self) -> egui::Color32 {
        self.get_opt("popover_bg_color")
            .unwrap_or_else(|| self.card_bg())
    }

    pub fn popover_fg(&self) -> egui::Color32 {
        self.get_opt("popover_fg_color")
            .unwrap_or_else(|| self.card_fg())
    }

    pub fn shade(&self) -> egui::Color32 {
        self.get_opt("shade_color").unwrap_or_else(|| {
            if self.is_dark() {
                egui::Color32::from_rgba_unmultiplied(255, 255, 255, 24)
            } else {
                egui::Color32::from_rgba_unmultiplied(0, 0, 0, 24)
            }
        })
    }

    pub fn muted_fg(&self) -> egui::Color32 {
        with_alpha(self.view_fg(), 0.70)
    }

    #[allow(dead_code)]
    pub fn disabled_fg(&self) -> egui::Color32 {
        with_alpha(self.view_fg(), 0.50)
    }

    pub fn hover_bg(&self) -> egui::Color32 {
        alpha_blend(self.card_bg(), self.shade())
    }

    pub fn selection_bg(&self) -> egui::Color32 {
        with_alpha(self.accent(), 0.30)
    }

    pub fn selection_stroke(&self) -> egui::Color32 {
        self.accent()
    }

    pub fn border(&self) -> egui::Color32 {
        self.get_opt("scrollbar_outline_color")
            .or_else(|| self.get_opt("headerbar_border_color"))
            .unwrap_or_else(|| self.shade())
    }

    pub fn palette(&self, name: &str, level: u8) -> Option<egui::Color32> {
        self.get_opt(&format!("{}_{}", name, level))
    }

    /// Pick Ferritor's heading foreground from the colors supplied by the
    /// active GTK theme. The incoming pairings are not trusted: candidates
    /// must remain readable on both the view and its selection overlay.
    pub fn heading_fg(&self) -> egui::Color32 {
        let surfaces = [
            self.view_bg(),
            alpha_blend(self.view_bg(), self.selection_bg()),
        ];
        choose_palette_text_color(self.accent(), &surfaces, self.colors.values().copied())
    }

    fn get_opt(&self, name: &str) -> Option<egui::Color32> {
        self.colors.get(name).copied()
    }
}

fn parse_define_color_raw(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if !line.starts_with("@define-color ") {
        return None;
    }
    let rest = &line["@define-color ".len()..];
    let rest = rest.trim_end_matches(';').trim();
    let space_idx = rest.find(' ')?;
    let name = rest[..space_idx].to_string();
    let value = rest[space_idx..].trim().to_string();
    Some((name, value))
}

fn resolve_named_color(
    name: &str,
    defs: &HashMap<String, String>,
    out: &mut HashMap<String, egui::Color32>,
    resolving: &mut HashSet<String>,
) -> Option<egui::Color32> {
    if let Some(color) = out.get(name).copied() {
        return Some(color);
    }
    if !resolving.insert(name.to_string()) {
        return None;
    }

    let value = defs.get(name)?;
    let color = parse_color_value(value).or_else(|| {
        let ref_name = value.strip_prefix('@').unwrap_or(value.as_str());
        if defs.contains_key(ref_name) {
            resolve_named_color(ref_name, defs, out, resolving)
        } else {
            None
        }
    });

    resolving.remove(name);
    if let Some(color) = color {
        out.insert(name.to_string(), color);
    }
    color
}

fn parse_color_value(value: &str) -> Option<egui::Color32> {
    if value.starts_with('#') {
        parse_hex(value)
    } else if value.starts_with("rgba(") {
        parse_rgba(value)
    } else if value.starts_with("rgb(") {
        parse_rgb(value)
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
            Some(egui::Color32::from_rgba_unmultiplied(r, g, b, a))
        }
        _ => None,
    }
}

fn parse_rgba(value: &str) -> Option<egui::Color32> {
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
        (a.clamp(0.0, 1.0) * 255.0).round() as u8,
    ))
}

fn parse_rgb(value: &str) -> Option<egui::Color32> {
    let inner = value.strip_prefix("rgb(")?.strip_suffix(')')?;
    let parts: Vec<&str> = inner.split(',').map(|s| s.trim()).collect();
    if parts.len() != 3 {
        return None;
    }
    let r: u8 = parts[0].parse().ok()?;
    let g: u8 = parts[1].parse().ok()?;
    let b: u8 = parts[2].parse().ok()?;
    Some(egui::Color32::from_rgb(r, g, b))
}

fn with_alpha(color: egui::Color32, alpha: f32) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(
        color.r(),
        color.g(),
        color.b(),
        (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

fn alpha_blend(base: egui::Color32, overlay: egui::Color32) -> egui::Color32 {
    let oa = overlay.a() as f32 / 255.0;
    let ba = base.a() as f32 / 255.0;
    let out_a = oa + ba * (1.0 - oa);
    if out_a <= f32::EPSILON {
        return egui::Color32::TRANSPARENT;
    }

    let blend = |bc: u8, oc: u8| -> u8 {
        let bc = bc as f32 / 255.0;
        let oc = oc as f32 / 255.0;
        let out = (oc * oa + bc * ba * (1.0 - oa)) / out_a;
        (out.clamp(0.0, 1.0) * 255.0).round() as u8
    };

    egui::Color32::from_rgba_unmultiplied(
        blend(base.r(), overlay.r()),
        blend(base.g(), overlay.g()),
        blend(base.b(), overlay.b()),
        (out_a * 255.0).round() as u8,
    )
}

fn readable_foreground(bg: egui::Color32) -> egui::Color32 {
    if luminance(bg) < 0.5 {
        egui::Color32::WHITE
    } else {
        egui::Color32::BLACK
    }
}

fn luminance(color: egui::Color32) -> f32 {
    fn channel(v: u8) -> f32 {
        let x = v as f32 / 255.0;
        if x <= 0.039_28 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
}

fn contrast_ratio(a: egui::Color32, b: egui::Color32) -> f32 {
    let (lighter, darker) = if luminance(a) >= luminance(b) {
        (luminance(a), luminance(b))
    } else {
        (luminance(b), luminance(a))
    };
    (lighter + 0.05) / (darker + 0.05)
}

fn color_distance_sq(a: egui::Color32, b: egui::Color32) -> u32 {
    let dr = i32::from(a.r()) - i32::from(b.r());
    let dg = i32::from(a.g()) - i32::from(b.g());
    let db = i32::from(a.b()) - i32::from(b.b());
    (dr * dr + dg * dg + db * db) as u32
}

fn choose_palette_text_color(
    preferred: egui::Color32,
    surfaces: &[egui::Color32],
    colors: impl IntoIterator<Item = egui::Color32>,
) -> egui::Color32 {
    const MIN_CONTRAST: f32 = 4.5;

    let mut candidates = Vec::new();
    for color in colors {
        if color.a() == 255 && !candidates.contains(&color) {
            candidates.push(color);
        }
    }
    if candidates.is_empty() {
        return preferred;
    }

    let min_contrast = |color| {
        surfaces
            .iter()
            .map(|surface| contrast_ratio(color, *surface))
            .fold(f32::INFINITY, f32::min)
    };

    candidates
        .iter()
        .copied()
        .filter(|color| min_contrast(*color) >= MIN_CONTRAST)
        .min_by_key(|color| color_distance_sq(*color, preferred))
        .unwrap_or_else(|| {
            candidates
                .into_iter()
                .max_by(|a, b| min_contrast(*a).total_cmp(&min_contrast(*b)))
                .unwrap_or(preferred)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_color_moves_to_nearest_passing_palette_color() {
        let background = egui::Color32::from_rgb(0xff, 0xfb, 0xef);
        let accent = egui::Color32::from_rgb(0x3a, 0x94, 0xc5);
        let darker_blue = egui::Color32::from_rgb(0x1c, 0x71, 0xd8);
        let unrelated_dark = egui::Color32::from_rgb(0x2e, 0x38, 0x3c);

        let chosen =
            choose_palette_text_color(accent, &[background], [accent, darker_blue, unrelated_dark]);

        assert_eq!(chosen, darker_blue);
        assert!(contrast_ratio(chosen, background) >= 4.5);
    }

    #[test]
    fn heading_color_uses_strongest_palette_fallback() {
        let background = egui::Color32::from_rgb(0x80, 0x80, 0x80);
        let preferred = egui::Color32::from_rgb(0x90, 0x90, 0x90);
        let stronger = egui::Color32::from_rgb(0x10, 0x10, 0x10);

        let chosen = choose_palette_text_color(
            preferred,
            &[background, egui::Color32::from_rgb(0x70, 0x70, 0x70)],
            [preferred, stronger],
        );

        assert_eq!(chosen, stronger);
    }
}
