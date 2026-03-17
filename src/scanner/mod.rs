pub mod claude;
pub mod codex;
pub mod digtwin;

use std::path::PathBuf;

/// A discovered skill or hook from any surface.
#[derive(Debug, Clone)]
pub struct SkillEntry {
    /// Display name (directory name or derived from filename)
    pub name: String,
    /// Which surface this came from
    pub surface: Surface,
    /// Full path to the skill/hook content file
    pub path: PathBuf,
    /// Raw content (loaded on demand)
    pub content: Option<String>,
    /// Sync status against digtwin backup
    pub sync_status: SyncStatus,
    /// Parsed frontmatter metadata
    pub meta: SkillMeta,
}

impl SkillEntry {
    /// Surface-aware group label.
    pub fn group_label(&self) -> String {
        self.meta.group_label_for(Some(self.surface))
    }
}

/// Metadata parsed from SKILL.md YAML frontmatter.
#[derive(Debug, Clone, Default)]
pub struct SkillMeta {
    pub description: Option<String>,
    pub source: Option<String>,
    pub author: Option<String>,
    pub license: Option<String>,
}

impl SkillMeta {
    /// Derive a grouping label from available metadata.
    /// Priority: source > author > "Personal"
    /// Compute group label. Uses surface from the entry for codex defaults.
    pub fn group_label(&self) -> String {
        self.group_label_for(None)
    }

    pub fn group_label_for(&self, surface: Option<Surface>) -> String {
        if let Some(ref source) = self.source {
            let s = source.trim().trim_matches('"');
            if s.eq_ignore_ascii_case("self") {
                return "Personal".to_string();
            }
            if let Some(repo) = extract_github_repo(s) {
                return repo;
            }
            return s.to_string();
        }
        if let Some(ref author) = self.author {
            let a = author.trim().trim_matches('"');
            if is_personal_author(a) {
                return "Personal".to_string();
            }
            return a.to_string();
        }
        // Codex has no provenance system — skills with no metadata are personal
        if let Some(s) = surface {
            if matches!(s, Surface::CodexSkill | Surface::CodexRule) {
                return "Personal".to_string();
            }
        }
        "Unknown source".to_string()
    }
}

/// Check if an author string matches the repo owner (John Curran).
fn is_personal_author(author: &str) -> bool {
    let lower = author.to_lowercase();
    let lower = lower.replace('-', " ").replace('_', " ");
    lower.contains("john") && lower.contains("curran")
}

/// Extract "owner/repo" from a GitHub URL.
fn extract_github_repo(url: &str) -> Option<String> {
    let url = url.trim_start_matches("https://github.com/");
    let url = url.trim_start_matches("http://github.com/");
    let parts: Vec<&str> = url.split('/').collect();
    if parts.len() >= 2 {
        Some(format!("{}/{}", parts[0], parts[1]))
    } else {
        None
    }
}

/// Parse YAML frontmatter from a SKILL.md file content.
pub fn parse_frontmatter(content: &str) -> SkillMeta {
    let mut meta = SkillMeta::default();

    // Frontmatter is between --- delimiters
    if !content.starts_with("---") {
        return meta;
    }

    let rest = &content[3..];
    let end = match rest.find("\n---") {
        Some(idx) => idx,
        None => return meta,
    };
    let fm = &rest[..end];

    // Simple line-by-line YAML parsing — no serde_yaml needed
    let mut in_metadata = false;
    for line in fm.lines() {
        let trimmed = line.trim();

        if trimmed == "metadata:" {
            in_metadata = true;
            continue;
        }

        if in_metadata {
            if !trimmed.starts_with("author:")
                && !trimmed.starts_with("version:")
                && !line.starts_with(' ')
                && !line.starts_with('\t')
            {
                in_metadata = false;
            }
        }

        if let Some(val) = strip_yaml_key(trimmed, "source") {
            meta.source = Some(val);
        } else if let Some(val) = strip_yaml_key(trimmed, "description") {
            meta.description = Some(val);
        } else if let Some(val) = strip_yaml_key(trimmed, "license") {
            meta.license = Some(val);
        } else if in_metadata {
            if let Some(val) = strip_yaml_key(trimmed, "author") {
                meta.author = Some(val);
            }
        } else if let Some(val) = strip_yaml_key(trimmed, "author") {
            meta.author = Some(val);
        }
    }

    meta
}

fn strip_yaml_key(line: &str, key: &str) -> Option<String> {
    let prefix = format!("{}:", key);
    if line.starts_with(&prefix) {
        let val = line[prefix.len()..].trim();
        let val = val.trim_matches('"').trim_matches('\'');
        if val.is_empty() {
            None
        } else {
            Some(val.to_string())
        }
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    ClaudeSkill,
    ClaudeHook,
    CodexSkill,
    CodexRule,
}

impl Surface {
    pub fn label(&self) -> &'static str {
        match self {
            Surface::ClaudeSkill => "Claude Code Skills",
            Surface::ClaudeHook => "Claude Code Hooks",
            Surface::CodexSkill => "Codex Skills",
            Surface::CodexRule => "Codex Rules",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncStatus {
    /// In sync with digtwin backup
    Synced,
    /// Exists in digtwin but content differs
    Modified,
    /// No backup in digtwin
    Unbackedup,
    /// Not yet checked
    Unknown,
}

/// Scan all surfaces and return combined entries.
pub fn scan_all() -> Vec<SkillEntry> {
    let mut entries = Vec::new();
    entries.extend(claude::scan_skills());
    entries.extend(claude::scan_hooks());
    entries.extend(codex::scan_skills());
    entries.extend(codex::scan_rules());

    // Check sync status against digtwin
    digtwin::check_sync_status(&mut entries);

    entries
}
