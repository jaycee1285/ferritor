use super::{parse_frontmatter, SkillEntry, SkillMeta, Surface, SyncStatus};
use std::path::PathBuf;

/// Scan ~/.codex/skills/*/SKILL.md for codex skills (same structure as Claude Code).
pub fn scan_skills() -> Vec<SkillEntry> {
    let base = dirs::home_dir()
        .unwrap_or_default()
        .join(".codex")
        .join("skills");

    let mut entries = Vec::new();

    if !base.exists() {
        return entries;
    }

    if let Ok(reader) = std::fs::read_dir(&base) {
        for entry in reader.flatten() {
            let dir_path = entry.path();
            if !dir_path.is_dir() {
                continue;
            }
            let skill_file = dir_path.join("SKILL.md");
            if skill_file.exists() {
                let name = dir_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                let meta = std::fs::read_to_string(&skill_file)
                    .map(|c| parse_frontmatter(&c))
                    .unwrap_or_default();

                entries.push(SkillEntry {
                    name,
                    surface: Surface::CodexSkill,
                    path: skill_file,
                    content: None,
                    sync_status: SyncStatus::Unknown,
                    meta,
                });
            }
        }
    }

    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    entries
}

/// Scan ~/.codex/rules/ for codex rules (flat files, not necessarily .md).
pub fn scan_rules() -> Vec<SkillEntry> {
    let dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".codex")
        .join("rules");

    let mut entries = Vec::new();

    if !dir.exists() {
        return entries;
    }

    if let Ok(reader) = std::fs::read_dir(&dir) {
        for entry in reader.flatten() {
            let path = entry.path();
            if path.is_file() {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                entries.push(SkillEntry {
                    name,
                    surface: Surface::CodexRule,
                    path,
                    content: None,
                    sync_status: SyncStatus::Unknown,
                    meta: SkillMeta::default(),
                });
            }
        }
    }

    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    entries
}

/// Get the path where a codex skill/rule would be backed up in digtwin.
pub fn backup_path(entry: &SkillEntry) -> Option<PathBuf> {
    let digtwin = dirs::home_dir()?.join("repos").join("digtwin");
    match entry.surface {
        Surface::CodexSkill => Some(
            digtwin
                .join("skills")
                .join("codex")
                .join(&entry.name)
                .join("SKILL.md"),
        ),
        Surface::CodexRule => {
            let filename = entry.path.file_name()?;
            Some(digtwin.join("rules").join("codex").join(filename))
        }
        _ => None,
    }
}
