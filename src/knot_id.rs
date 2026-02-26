use std::path::Path;
use std::process::Command;

use sha2::{Digest, Sha256};
use uuid::Uuid;

pub fn generate_knot_id<F>(repo_root: &Path, mut exists: F) -> String
where
    F: FnMut(&str) -> bool,
{
    let slug = repo_slug(repo_root);

    for _ in 0..64 {
        let seed = Uuid::now_v7().to_string();
        let mut hasher = Sha256::new();
        hasher.update(seed.as_bytes());
        let digest = format!("{:x}", hasher.finalize());
        let short = &digest[..4];
        let candidate = format!("{}-{}", slug, short);
        if !exists(&candidate) {
            return candidate;
        }
    }

    format!("{}-{}", slug, &Uuid::now_v7().simple().to_string()[..8])
}

pub fn repo_slug(repo_root: &Path) -> String {
    if let Some(remote_name) = origin_repo_name(repo_root) {
        let normalized = normalize_slug(&remote_name);
        if !normalized.is_empty() {
            return normalized;
        }
    }

    let basename = repo_root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("repo");
    let normalized = normalize_slug(basename);
    if normalized.is_empty() {
        "repo".to_string()
    } else {
        normalized
    }
}

fn origin_repo_name(repo_root: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if url.is_empty() {
        return None;
    }

    let tail = url.rsplit(['/', ':']).next()?;
    Some(tail.trim_end_matches(".git").to_string())
}

fn normalize_slug(raw: &str) -> String {
    raw.chars()
        .map(|ch| ch.to_ascii_lowercase())
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
        .collect::<String>()
}

pub fn display_id(id: &str) -> &str {
    id.rsplit_once('-').map_or(id, |(_, suffix)| suffix)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{display_id, generate_knot_id, repo_slug};

    #[test]
    fn slug_fallbacks_to_repo_name_when_git_remote_missing() {
        let root = std::env::temp_dir().join("knots-id-test-repo");
        std::fs::create_dir_all(&root).expect("temp root should be creatable");
        let slug = repo_slug(&root);
        assert_eq!(slug, "knots-id-test-repo");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn generated_ids_follow_repo_short_hash_shape() {
        let root = std::env::temp_dir().join("knots-id-shape-test");
        std::fs::create_dir_all(&root).expect("temp root should be creatable");
        let seen: HashSet<String> = HashSet::new();
        let id = generate_knot_id(&root, |candidate| seen.contains(candidate));
        assert!(id.starts_with("knots-id-shape-test-"));
        assert_eq!(
            id.split('-')
                .next_back()
                .expect("short hash should exist")
                .len(),
            4
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn display_id_strips_prefix() {
        assert_eq!(display_id("knots-19dc"), "19dc");
        assert_eq!(display_id("my-repo-a1b2"), "a1b2");
        assert_eq!(display_id("nohyphen"), "nohyphen");
    }
}
