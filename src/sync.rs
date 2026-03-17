use crate::scanner::{SkillEntry, Surface};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Get the digtwin backup base path.
fn digtwin_base() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join("repos").join("digtwin"))
}

/// Compute the backup destination for an entry, using the new structure:
///   ~/repos/digtwin/claude/skills/{name}/SKILL.md
///   ~/repos/digtwin/claude/hooks/settings-hooks.json
///   ~/repos/digtwin/codex/skills/{name}/SKILL.md
///   ~/repos/digtwin/codex/rules/{filename}
pub fn backup_dest(entry: &SkillEntry) -> Option<PathBuf> {
    let base = digtwin_base()?;
    match entry.surface {
        Surface::ClaudeSkill => Some(
            base.join("claude")
                .join("skills")
                .join(&entry.name)
                .join("SKILL.md"),
        ),
        Surface::ClaudeHook => Some(
            base.join("claude")
                .join("hooks")
                .join("settings-hooks.json"),
        ),
        Surface::CodexSkill => Some(
            base.join("codex")
                .join("skills")
                .join(&entry.name)
                .join("SKILL.md"),
        ),
        Surface::CodexRule => {
            let filename = entry.path.file_name()?;
            Some(base.join("codex").join("rules").join(filename))
        }
    }
}

/// Backup a single entry to digtwin (copy, not symlink).
pub fn backup_entry(entry: &SkillEntry) -> Result<PathBuf, String> {
    let dest = backup_dest(entry).ok_or("Cannot compute backup path")?;

    // Create parent dirs
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create dirs: {}", e))?;
    }

    // For hooks, content is already in-memory (just the JSON portion, not the script)
    if entry.surface == Surface::ClaudeHook {
        if let Some(ref content) = entry.content {
            // Strip the "--- Script content ---" suffix if present
            let json_part = content
                .split("\n\n--- Script content ---")
                .next()
                .unwrap_or(content);
            std::fs::write(&dest, json_part)
                .map_err(|e| format!("Failed to write hook backup: {}", e))?;
            return Ok(dest);
        }
    }

    // For skills: copy the entire directory (scripts, templates, agents, etc.)
    if matches!(entry.surface, Surface::ClaudeSkill | Surface::CodexSkill) {
        let source_dir = entry
            .path
            .parent()
            .ok_or("Cannot determine skill directory")?;
        let dest_dir = dest.parent().ok_or("Cannot determine backup directory")?;
        copy_dir_recursive(source_dir, dest_dir)?;
        copy_hardcoded_skill_scripts(&entry.path, dest_dir)?;
        return Ok(dest);
    }

    // For rules, copy the single file
    std::fs::copy(&entry.path, &dest).map_err(|e| format!("Failed to copy: {}", e))?;

    Ok(dest)
}

/// Backup all entries that match a given group label (e.g. "Personal").
/// Returns (success_count, errors).
pub fn backup_group(entries: &[SkillEntry], group_label: &str) -> (usize, Vec<String>) {
    let mut success = 0;
    let mut errors = Vec::new();

    for entry in entries {
        if entry.group_label() == group_label {
            match backup_entry(entry) {
                Ok(_) => success += 1,
                Err(e) => errors.push(format!("{}: {}", entry.name, e)),
            }
        }
    }

    (success, errors)
}

/// Backup ALL personal entries across all surfaces.
pub fn backup_all_personal(entries: &[SkillEntry]) -> (usize, Vec<String>) {
    backup_group(entries, "Personal")
}

/// Recursively copy a directory's contents to a destination.
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dst)
        .map_err(|e| format!("Failed to create {}: {}", dst.display(), e))?;

    let entries =
        std::fs::read_dir(src).map_err(|e| format!("Failed to read {}: {}", src.display(), e))?;

    for entry in entries.flatten() {
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)
                .map_err(|e| format!("Failed to copy {}: {}", src_path.display(), e))?;
        }
    }
    Ok(())
}

/// Copy hard-coded device-local JS/TS script references mentioned in SKILL.md.
///
/// Contract: only absolute `/home/john/...` paths ending in `.js` or `.ts` are
/// treated as skill scripts. They are mirrored under `_external/home/john/...`
/// inside the backed-up skill directory.
fn copy_hardcoded_skill_scripts(skill_md: &Path, backup_skill_dir: &Path) -> Result<(), String> {
    let content = match std::fs::read_to_string(skill_md) {
        Ok(content) => content,
        Err(err) => {
            return Err(format!(
                "Failed to read {} for external script scan: {}",
                skill_md.display(),
                err
            ))
        }
    };

    for script_path in extract_hardcoded_script_paths(&content) {
        let relative = script_path
            .strip_prefix("/")
            .map_err(|_| format!("Expected absolute path, got {}", script_path.display()))?;
        let dest = backup_skill_dir.join("_external").join(relative);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
        }
        std::fs::copy(&script_path, &dest).map_err(|e| {
            format!(
                "Failed to copy referenced script {}: {}",
                script_path.display(),
                e
            )
        })?;
    }

    Ok(())
}

fn extract_hardcoded_script_paths(content: &str) -> BTreeSet<PathBuf> {
    let mut paths = BTreeSet::new();
    let bytes = content.as_bytes();
    let mut start = 0usize;

    while let Some(offset) = content[start..].find("/home/john/") {
        let absolute_start = start + offset;
        let mut end = absolute_start;
        while end < bytes.len() && !is_skill_path_terminator(bytes[end] as char) {
            end += 1;
        }

        let candidate = &content[absolute_start..end];
        if looks_like_hardcoded_script_path(candidate) {
            let path = PathBuf::from(candidate);
            if path.is_file() {
                paths.insert(path);
            }
        }

        start = end.saturating_add(1);
    }

    paths
}

fn looks_like_hardcoded_script_path(candidate: &str) -> bool {
    candidate.starts_with("/home/john/")
        && (candidate.ends_with(".js") || candidate.ends_with(".ts"))
}

fn is_skill_path_terminator(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            '`' | '"' | '\'' | ')' | '(' | ']' | '[' | '}' | '{' | '<' | '>' | ',' | ';'
        )
}

/// Delete a single entry from disk.
/// For skills: removes the parent directory (e.g. ~/.claude/skills/{name}/).
/// For rules: removes the file.
/// For hooks: not supported (they live inside settings.json).
pub fn delete_entry(entry: &SkillEntry) -> Result<(), String> {
    match entry.surface {
        Surface::ClaudeSkill | Surface::CodexSkill => {
            // path points to SKILL.md, parent is the skill directory
            let skill_dir = entry
                .path
                .parent()
                .ok_or("Cannot determine skill directory")?;
            std::fs::remove_dir_all(skill_dir)
                .map_err(|e| format!("Failed to delete {}: {}", skill_dir.display(), e))
        }
        Surface::CodexRule => std::fs::remove_file(&entry.path)
            .map_err(|e| format!("Failed to delete {}: {}", entry.path.display(), e)),
        Surface::ClaudeHook => {
            Err("Hook deletion not supported — hooks live in settings.json".to_string())
        }
    }
}
