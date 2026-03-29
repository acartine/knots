use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const CONFIG_DIR_NAME: &str = "knots";
const PROJECTS_DIR_NAME: &str = "projects";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistributionMode {
    Git,
    LocalOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorePaths {
    pub root: PathBuf,
}

impl StorePaths {
    pub fn db_path(&self) -> PathBuf {
        self.root.join("cache").join("state.sqlite")
    }

    pub fn locks_dir(&self) -> PathBuf {
        self.root.join("locks")
    }

    pub fn queue_dir(&self) -> PathBuf {
        self.root.join("queue")
    }

    pub fn repo_lock_path(&self) -> PathBuf {
        self.locks_dir().join("repo.lock")
    }

    pub fn cache_lock_path(&self) -> PathBuf {
        self.root.join("cache").join("cache.lock")
    }

    pub fn write_queue_worker_lock_path(&self) -> PathBuf {
        self.locks_dir().join("write_queue_worker.lock")
    }

    pub fn worktree_path(&self) -> PathBuf {
        self.root.join("_worktree")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectContext {
    pub project_id: Option<String>,
    pub repo_root: PathBuf,
    pub store_paths: StorePaths,
    pub distribution: DistributionMode,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GlobalConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_quick_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_project: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NamedProjectRecord {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<PathBuf>,
}

impl NamedProjectRecord {
    pub fn store_paths(&self, home_override: Option<&Path>) -> Result<StorePaths, String> {
        Ok(StorePaths {
            root: data_dir(home_override)?
                .join(PROJECTS_DIR_NAME)
                .join(self.id.as_str()),
        })
    }
}

pub fn config_path(home_override: Option<&Path>) -> Result<PathBuf, String> {
    Ok(config_dir(home_override)?.join("config.toml"))
}

pub fn config_dir(home_override: Option<&Path>) -> Result<PathBuf, String> {
    let home = home_dir(home_override)?;
    Ok(home.join(".config").join(CONFIG_DIR_NAME))
}

pub fn data_dir(home_override: Option<&Path>) -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    {
        let home = home_dir(home_override)?;
        Ok(home
            .join("Library")
            .join("Application Support")
            .join(CONFIG_DIR_NAME))
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return Ok(PathBuf::from(appdata).join(CONFIG_DIR_NAME));
        }
        if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
            return Ok(PathBuf::from(xdg).join(CONFIG_DIR_NAME));
        }
        let home = home_dir(home_override)?;
        Ok(home.join(".local").join("share").join(CONFIG_DIR_NAME))
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
            return Ok(PathBuf::from(xdg).join(CONFIG_DIR_NAME));
        }
        let home = home_dir(home_override)?;
        Ok(home.join(".local").join("share").join(CONFIG_DIR_NAME))
    }
}

pub fn read_global_config(home_override: Option<&Path>) -> Result<GlobalConfig, String> {
    let path = config_path(home_override)?;
    if !path.exists() {
        return Ok(GlobalConfig::default());
    }
    let raw = fs::read_to_string(path).map_err(|err| err.to_string())?;
    toml::from_str(&raw).map_err(|err| format!("invalid config: {err}"))
}

pub fn write_global_config(
    home_override: Option<&Path>,
    config: &GlobalConfig,
) -> Result<(), String> {
    let path = config_path(home_override)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let rendered = toml::to_string_pretty(config).map_err(|err| err.to_string())?;
    fs::write(path, rendered).map_err(|err| err.to_string())
}

pub fn projects_dir(home_override: Option<&Path>) -> Result<PathBuf, String> {
    Ok(config_dir(home_override)?.join(PROJECTS_DIR_NAME))
}

pub fn list_named_projects(
    home_override: Option<&Path>,
) -> Result<Vec<NamedProjectRecord>, String> {
    let root = projects_dir(home_override)?;
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    for entry in fs::read_dir(root).map_err(|err| err.to_string())? {
        let path = entry.map_err(|err| err.to_string())?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("toml") {
            continue;
        }
        let raw = fs::read_to_string(&path).map_err(|err| err.to_string())?;
        let mut record: NamedProjectRecord =
            toml::from_str(&raw).map_err(|err| format!("invalid project file: {err}"))?;
        if record.id.is_empty() {
            if let Some(stem) = path.file_stem().and_then(|value| value.to_str()) {
                record.id = stem.to_string();
            }
        }
        records.push(record);
    }
    records.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(records)
}

pub fn load_named_project(
    home_override: Option<&Path>,
    id: &str,
) -> Result<NamedProjectRecord, String> {
    validate_project_id(id)?;
    let path = project_file(home_override, id)?;
    if !path.exists() {
        return Err(format!("unknown project '{id}'"));
    }
    let raw = fs::read_to_string(path).map_err(|err| err.to_string())?;
    let mut record: NamedProjectRecord =
        toml::from_str(&raw).map_err(|err| format!("invalid project file: {err}"))?;
    if record.id.is_empty() {
        record.id = id.to_string();
    }
    Ok(record)
}

pub fn create_named_project(
    home_override: Option<&Path>,
    id: &str,
    repo_root: Option<&Path>,
) -> Result<NamedProjectRecord, String> {
    validate_project_id(id)?;
    let path = project_file(home_override, id)?;
    if path.exists() {
        return Err(format!("project '{id}' already exists"));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let record = NamedProjectRecord {
        id: id.to_string(),
        repo_root: repo_root.map(canonical_or_original),
    };
    let rendered = toml::to_string_pretty(&record).map_err(|err| err.to_string())?;
    fs::write(path, rendered).map_err(|err| err.to_string())?;
    let store = record.store_paths(home_override)?;
    fs::create_dir_all(&store.root).map_err(|err| err.to_string())?;
    Ok(record)
}

pub fn delete_named_project(home_override: Option<&Path>, id: &str) -> Result<(), String> {
    let record = load_named_project(home_override, id)?;
    let store = record.store_paths(home_override)?;
    if store.root.exists() {
        fs::remove_dir_all(&store.root).map_err(|err| err.to_string())?;
    }

    let path = project_file(home_override, id)?;
    if path.exists() {
        fs::remove_file(path).map_err(|err| err.to_string())?;
    }

    let mut config = read_global_config(home_override)?;
    if config.active_project.as_deref() == Some(id) {
        config.active_project = None;
        write_global_config(home_override, &config)?;
    }
    Ok(())
}

pub fn set_active_project(home_override: Option<&Path>, id: &str) -> Result<(), String> {
    let _ = load_named_project(home_override, id)?;
    let mut config = read_global_config(home_override)?;
    config.active_project = Some(id.to_string());
    write_global_config(home_override, &config)
}

pub fn clear_active_project(home_override: Option<&Path>) -> Result<(), String> {
    let mut config = read_global_config(home_override)?;
    config.active_project = None;
    write_global_config(home_override, &config)
}

pub fn resolve_context(
    explicit_project: Option<&str>,
    explicit_repo_root: Option<&Path>,
    cwd: &Path,
    home_override: Option<&Path>,
) -> Result<ProjectContext, String> {
    if let Some(id) = explicit_project {
        return named_project_context(home_override, id);
    }
    if let Some(repo_root) = explicit_repo_root {
        return Ok(git_context(repo_root));
    }
    let config = read_global_config(home_override)?;
    if let Some(active) = config.active_project.as_deref() {
        return named_project_context(home_override, active);
    }
    let git_root = find_git_root(cwd)
        .ok_or_else(|| "no active project and not inside a git repository".to_string())?;
    Ok(git_context(&git_root))
}

pub fn prompt_for_project_selection(
    home_override: Option<&Path>,
    repo_root: Option<&Path>,
) -> Result<NamedProjectRecord, String> {
    let projects = list_named_projects(home_override)?;
    let stdin = io::stdin();
    let stdout = io::stdout();
    let stdin_is_tty = stdin.is_terminal();
    let stdout_is_tty = stdout.is_terminal();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    prompt_for_project_selection_from_io(
        &mut input,
        &mut output,
        home_override,
        repo_root,
        stdin_is_tty,
        stdout_is_tty,
        &projects,
    )
}

pub(crate) fn prompt_for_project_selection_from_io<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    home_override: Option<&Path>,
    repo_root: Option<&Path>,
    stdin_is_tty: bool,
    stdout_is_tty: bool,
    projects: &[NamedProjectRecord],
) -> Result<NamedProjectRecord, String> {
    if !stdin_is_tty || !stdout_is_tty {
        return Err("interactive project selection requires a TTY".to_string());
    }
    prompt_for_project_selection_with_io(input, output, home_override, repo_root, projects)
}

pub(crate) fn prompt_for_project_selection_with_io<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    home_override: Option<&Path>,
    repo_root: Option<&Path>,
    projects: &[NamedProjectRecord],
) -> Result<NamedProjectRecord, String> {
    writeln!(output, "Named projects:").map_err(|err| err.to_string())?;
    for (index, project) in projects.iter().enumerate() {
        writeln!(output, "  {}) {}", index + 1, project.id).map_err(|err| err.to_string())?;
    }
    writeln!(output, "  n) create new project").map_err(|err| err.to_string())?;
    write!(output, "Select project: ").map_err(|err| err.to_string())?;
    output.flush().map_err(|err| err.to_string())?;

    let mut line = String::new();
    input.read_line(&mut line).map_err(|err| err.to_string())?;
    let trimmed = line.trim();
    if trimmed.eq_ignore_ascii_case("n") {
        write!(output, "New project id: ").map_err(|err| err.to_string())?;
        output.flush().map_err(|err| err.to_string())?;
        line.clear();
        input.read_line(&mut line).map_err(|err| err.to_string())?;
        let id = line.trim();
        return create_named_project(home_override, id, repo_root);
    }
    let index = trimmed
        .parse::<usize>()
        .map_err(|_| "invalid selection".to_string())?;
    projects
        .get(index.saturating_sub(1))
        .cloned()
        .ok_or_else(|| "selection out of range".to_string())
}

pub fn validate_project_id(id: &str) -> Result<(), String> {
    if id.trim().is_empty() {
        return Err("project id cannot be empty".to_string());
    }
    if id
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_'))
    {
        return Ok(());
    }
    Err("project id must use lowercase letters, digits, '-' or '_'".to_string())
}

pub fn canonical_or_original(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut current = canonical_or_original(start);
    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn named_project_context(home_override: Option<&Path>, id: &str) -> Result<ProjectContext, String> {
    let record = load_named_project(home_override, id)?;
    let store_paths = record.store_paths(home_override)?;
    let repo_root = record
        .repo_root
        .clone()
        .unwrap_or_else(|| store_paths.root.clone());
    Ok(ProjectContext {
        project_id: Some(record.id),
        repo_root,
        store_paths,
        distribution: DistributionMode::LocalOnly,
    })
}

fn git_context(repo_root: &Path) -> ProjectContext {
    let repo_root = canonical_or_original(repo_root);
    ProjectContext {
        project_id: None,
        store_paths: StorePaths {
            root: repo_root.join(".knots"),
        },
        repo_root,
        distribution: DistributionMode::Git,
    }
}

fn home_dir(home_override: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(home) = home_override {
        return Ok(home.to_path_buf());
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "unable to resolve home directory".to_string())
}

fn project_file(home_override: Option<&Path>, id: &str) -> Result<PathBuf, String> {
    Ok(projects_dir(home_override)?.join(format!("{id}.toml")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_home() -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("knots-project-test-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&path).expect("temp home should be creatable");
        path
    }

    #[test]
    fn create_list_and_resolve_named_projects() {
        let home = temp_home();
        let project = create_named_project(Some(&home), "demo", None).expect("create project");
        assert_eq!(project.id, "demo");
        let listed = list_named_projects(Some(&home)).expect("list projects");
        assert_eq!(listed.len(), 1);
        set_active_project(Some(&home), "demo").expect("set active project");
        let context = resolve_context(None, None, &home, Some(&home)).expect("resolve context");
        assert_eq!(context.project_id.as_deref(), Some("demo"));
        assert_eq!(context.distribution, DistributionMode::LocalOnly);
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn explicit_repo_root_beats_active_project() {
        let home = temp_home();
        create_named_project(Some(&home), "demo", None).expect("create project");
        set_active_project(Some(&home), "demo").expect("set active project");
        let repo_root = home.join("repo");
        fs::create_dir_all(repo_root.join(".git")).expect("git dir should exist");
        let context =
            resolve_context(None, Some(&repo_root), &home, Some(&home)).expect("resolve git");
        assert_eq!(context.project_id, None);
        assert_eq!(context.distribution, DistributionMode::Git);
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn delete_project_removes_store_and_clears_active_project() {
        let home = temp_home();
        let project = create_named_project(Some(&home), "demo", None).expect("create project");
        set_active_project(Some(&home), "demo").expect("set active project");

        let store = project
            .store_paths(Some(&home))
            .expect("store paths should resolve");
        fs::write(store.root.join("marker.txt"), "x").expect("marker should be writable");
        delete_named_project(Some(&home), "demo").expect("delete project");

        assert!(!store.root.exists());
        assert!(!projects_dir(Some(&home))
            .expect("projects dir should resolve")
            .join("demo.toml")
            .exists());
        let config = read_global_config(Some(&home)).expect("config should load");
        assert_eq!(config.active_project, None);
        let _ = fs::remove_dir_all(home);
    }
}
