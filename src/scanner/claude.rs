use super::{parse_frontmatter, SkillEntry, SkillMeta, Surface, SyncStatus};
use std::path::PathBuf;

/// Scan ~/.claude/skills/*/SKILL.md for user-level skills.
pub fn scan_skills() -> Vec<SkillEntry> {
    let base = dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
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

                // Parse frontmatter for grouping metadata
                let meta = std::fs::read_to_string(&skill_file)
                    .map(|c| parse_frontmatter(&c))
                    .unwrap_or_default();

                entries.push(SkillEntry {
                    name,
                    surface: Surface::ClaudeSkill,
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

/// Parse hooks from ~/.claude/settings.json.
pub fn scan_hooks() -> Vec<SkillEntry> {
    let settings_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join("settings.json");

    let mut entries = Vec::new();

    let content = match std::fs::read_to_string(&settings_path) {
        Ok(c) => c,
        Err(_) => return entries,
    };

    let parsed: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return entries,
    };

    if let Some(hooks) = parsed.get("hooks").and_then(|h| h.as_object()) {
        for (event_type, matchers) in hooks {
            if let Some(matchers) = matchers.as_array() {
                for (i, matcher_obj) in matchers.iter().enumerate() {
                    let matcher = matcher_obj
                        .get("matcher")
                        .and_then(|m| m.as_str())
                        .unwrap_or("");
                    if let Some(hook_list) = matcher_obj.get("hooks").and_then(|h| h.as_array()) {
                        for (j, hook) in hook_list.iter().enumerate() {
                            let hook_type = hook
                                .get("type")
                                .and_then(|t| t.as_str())
                                .unwrap_or("unknown");
                            let command =
                                hook.get("command").and_then(|c| c.as_str()).unwrap_or("");

                            let name = if matcher.is_empty() {
                                format!("{}[{}][{}]: {} {}", event_type, i, j, hook_type, command)
                            } else {
                                format!(
                                    "{}:{} [{}][{}]: {} {}",
                                    event_type, matcher, i, j, hook_type, command
                                )
                            };

                            // Build content: hook JSON + resolved script content
                            let mut full_content =
                                serde_json::to_string_pretty(hook).unwrap_or_default();

                            // If it's a command hook, try to resolve and inline the script
                            if hook_type == "command" && !command.is_empty() {
                                if let Some(script_content) = resolve_hook_script(command) {
                                    full_content.push_str("\n\n--- Script content ---\n\n");
                                    full_content.push_str(&script_content);
                                }
                            }

                            entries.push(SkillEntry {
                                name,
                                surface: Surface::ClaudeHook,
                                path: settings_path.clone(),
                                content: Some(full_content),
                                sync_status: SyncStatus::Unknown,
                                meta: SkillMeta::default(),
                            });
                        }
                    }
                }
            }
        }
    }

    entries
}

/// Try to resolve a hook command to its script content.
/// Handles patterns like "bash /path/to/script.sh" or direct paths.
fn resolve_hook_script(command: &str) -> Option<String> {
    // Common pattern: "bash /path/to/script.sh" or "sh /path/to/script.sh"
    let path = command
        .strip_prefix("bash ")
        .or_else(|| command.strip_prefix("sh "))
        .unwrap_or(command)
        .trim();

    // Expand ~ to home dir
    let expanded = if path.starts_with('~') {
        let home = dirs::home_dir()?;
        home.join(path.trim_start_matches("~/"))
    } else {
        std::path::PathBuf::from(path)
    };

    std::fs::read_to_string(&expanded).ok()
}

/// Get the path where a skill would be backed up in digtwin.
pub fn backup_path(entry: &SkillEntry) -> Option<PathBuf> {
    let digtwin = dirs::home_dir()?.join("repos").join("digtwin");
    match entry.surface {
        Surface::ClaudeSkill => Some(digtwin.join("skills").join(&entry.name).join("SKILL.md")),
        Surface::ClaudeHook => Some(digtwin.join("hooks").join("claude-settings-hooks.json")),
        _ => None,
    }
}
