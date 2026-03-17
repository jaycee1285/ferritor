use super::{SkillEntry, Surface, SyncStatus};

/// Check sync status of all entries against digtwin backup.
pub fn check_sync_status(entries: &mut [SkillEntry]) {
    for entry in entries.iter_mut() {
        entry.sync_status = compute_sync_status(entry);
    }
}

fn compute_sync_status(entry: &SkillEntry) -> SyncStatus {
    // Use the canonical backup path from sync module
    let backup_path = match crate::sync::backup_dest(entry) {
        Some(p) => p,
        None => return SyncStatus::Unbackedup,
    };

    if !backup_path.exists() {
        return SyncStatus::Unbackedup;
    }

    // For hooks, mark as synced if backup file exists
    if entry.surface == Surface::ClaudeHook {
        return SyncStatus::Synced;
    }

    // Compare file contents
    let source_content = match std::fs::read_to_string(&entry.path) {
        Ok(c) => c,
        Err(_) => return SyncStatus::Unknown,
    };

    let backup_content = match std::fs::read_to_string(&backup_path) {
        Ok(c) => c,
        Err(_) => return SyncStatus::Unbackedup,
    };

    if source_content == backup_content {
        SyncStatus::Synced
    } else {
        SyncStatus::Modified
    }
}
