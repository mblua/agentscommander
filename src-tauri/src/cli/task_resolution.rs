use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct GitHubCommentUrl {
    pub owner: String,
    pub repo: String,
    pub issue_number: u64,
    pub comment_id: u64,
    pub html_url: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedMessageContext {
    pub workgroup: String,
    pub repo_root: PathBuf,
    pub task_path: PathBuf,
    pub task: TaskRecord,
    pub comment: GitHubCommentUrl,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRecord {
    pub schema_version: u32,
    pub id: String,
    pub slug: String,
    pub summary: String,
    pub status: String,
    pub active_workgroup: String,
    pub workgroup_history: Vec<TaskWorkgroupHistory>,
    pub github: TaskGitHubLink,
    pub branch: TaskBranchLink,
    pub messaging: TaskMessagingLink,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskWorkgroupHistory {
    pub workgroup: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub status: String,
    pub branch: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskGitHubLink {
    pub owner: String,
    pub repo: String,
    pub issue_number: u64,
    pub issue_url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskBranchLink {
    pub name: String,
    pub base: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskMessagingLink {
    pub mode: String,
    pub notify_with: String,
}

#[derive(Debug)]
struct TaskCandidate {
    repo_root: PathBuf,
    task_path: PathBuf,
    task: TaskRecord,
}

#[derive(Debug, thiserror::Error)]
pub enum TaskResolutionError {
    #[error("{0}")]
    Message(String),
}

pub fn resolve_message_context(
    root: &str,
    comment_url: &str,
) -> Result<ResolvedMessageContext, TaskResolutionError> {
    let (workgroup_dir, workgroup) = resolve_workgroup_dir(root)?;
    let tasks = load_candidate_tasks(&workgroup_dir, &workgroup)?;
    let active = find_single_active_task(&tasks, &workgroup)?;
    let current_branch = read_current_branch(&active.repo_root)?;
    if current_branch != active.task.branch.name {
        return Err(TaskResolutionError::Message(format!(
            "Current branch '{}' does not match active task branch '{}' ({})",
            current_branch,
            active.task.branch.name,
            active.task_path.display()
        )));
    }

    let comment = validate_issue_comment_url(comment_url, &active.task)?;
    Ok(ResolvedMessageContext {
        workgroup,
        repo_root: active.repo_root.clone(),
        task_path: active.task_path.clone(),
        task: active.task.clone(),
        comment,
    })
}

fn resolve_workgroup_dir(root: &str) -> Result<(PathBuf, String), TaskResolutionError> {
    let canonical = fs::canonicalize(root).unwrap_or_else(|_| PathBuf::from(root));
    let agent_dir = canonical
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            TaskResolutionError::Message(format!(
                "Cannot resolve agent root from '{}'",
                canonical.display()
            ))
        })?;
    if !agent_dir.starts_with("__agent_") {
        return Err(TaskResolutionError::Message(format!(
            "GitHub comment URL messaging requires a workgroup replica root (expected '__agent_*', got '{}')",
            agent_dir
        )));
    }

    let workgroup_dir = canonical.parent().ok_or_else(|| {
        TaskResolutionError::Message(format!(
            "Cannot resolve workgroup directory from '{}'",
            canonical.display()
        ))
    })?;
    let workgroup_name = workgroup_dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            TaskResolutionError::Message(format!(
                "Cannot resolve workgroup name from '{}'",
                workgroup_dir.display()
            ))
        })?;
    if !workgroup_name.starts_with("wg-") {
        return Err(TaskResolutionError::Message(format!(
            "GitHub comment URL messaging requires a workgroup root under 'wg-*' (got '{}')",
            workgroup_name
        )));
    }

    Ok((workgroup_dir.to_path_buf(), workgroup_name.to_string()))
}

fn load_candidate_tasks(
    workgroup_dir: &Path,
    workgroup: &str,
) -> Result<Vec<TaskCandidate>, TaskResolutionError> {
    let read_dir = fs::read_dir(workgroup_dir).map_err(|e| {
        TaskResolutionError::Message(format!(
            "Failed to scan workgroup directory '{}': {}",
            workgroup_dir.display(),
            e
        ))
    })?;

    let mut repo_dirs: Vec<PathBuf> = read_dir
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with("repo-"))
                .unwrap_or(false)
        })
        .collect();
    repo_dirs.sort();

    let mut tasks = Vec::new();
    let mut saw_tasks_dir = false;
    for repo_root in repo_dirs {
        let tasks_dir = repo_root.join("_plans").join("tasks");
        if !tasks_dir.is_dir() {
            continue;
        }
        saw_tasks_dir = true;

        let read_tasks = fs::read_dir(&tasks_dir).map_err(|e| {
            TaskResolutionError::Message(format!(
                "Failed to scan task directory '{}': {}",
                tasks_dir.display(),
                e
            ))
        })?;

        let mut task_files: Vec<PathBuf> = read_tasks
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
            .collect();
        task_files.sort();

        for task_path in task_files {
            if let Some(candidate) = load_task_candidate(&repo_root, &task_path, workgroup)? {
                tasks.push(candidate);
            }
        }
    }

    if !saw_tasks_dir {
        return Err(TaskResolutionError::Message(format!(
            "No sibling repo-* task directories were found under workgroup '{}'",
            workgroup_dir.display()
        )));
    }

    Ok(tasks)
}

fn load_task_candidate(
    repo_root: &Path,
    task_path: &Path,
    workgroup: &str,
) -> Result<Option<TaskCandidate>, TaskResolutionError> {
    let content = fs::read_to_string(task_path).map_err(|e| {
        TaskResolutionError::Message(format!(
            "Failed to read task record '{}': {}",
            task_path.display(),
            e
        ))
    })?;

    if !content_could_match_active_task(&content, workgroup) {
        return Ok(None);
    }

    let task: TaskRecord = serde_json::from_str(&content).map_err(|e| {
        TaskResolutionError::Message(format!(
            "Failed to parse task record '{}': {}",
            task_path.display(),
            e
        ))
    })?;
    validate_task_record(&task, task_path)?;

    Ok(Some(TaskCandidate {
        repo_root: repo_root.to_path_buf(),
        task_path: task_path.to_path_buf(),
        task,
    }))
}

fn content_could_match_active_task(content: &str, workgroup: &str) -> bool {
    let compact: String = content.chars().filter(|c| !c.is_whitespace()).collect();
    compact.contains(&format!(
        "\"status\":\"active\",\"activeWorkgroup\":\"{}\"",
        escape_json_string(workgroup)
    ))
}

fn escape_json_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn validate_task_record(task: &TaskRecord, task_path: &Path) -> Result<(), TaskResolutionError> {
    let stem = task_path
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            TaskResolutionError::Message(format!(
                "Task record '{}' has no valid filename stem",
                task_path.display()
            ))
        })?;

    if task.id != stem {
        return Err(TaskResolutionError::Message(format!(
            "Task record '{}' id '{}' does not match filename '{}'",
            task_path.display(),
            task.id,
            stem
        )));
    }
    if task.summary.trim().is_empty() {
        return Err(TaskResolutionError::Message(format!(
            "Task record '{}' has an empty summary",
            task_path.display()
        )));
    }
    if task.slug.is_empty()
        || !task
            .slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(TaskResolutionError::Message(format!(
            "Task record '{}' has invalid slug '{}'",
            task_path.display(),
            task.slug
        )));
    }

    let expected_issue_url = format!(
        "https://github.com/{}/{}/issues/{}",
        task.github.owner, task.github.repo, task.github.issue_number
    );
    if task.github.issue_url != expected_issue_url {
        return Err(TaskResolutionError::Message(format!(
            "Task record '{}' has issueUrl '{}' but expected '{}'",
            task_path.display(),
            task.github.issue_url,
            expected_issue_url
        )));
    }

    validate_branch_name(&task.branch.name, task.github.issue_number).map_err(|msg| {
        TaskResolutionError::Message(format!("{} ({})", msg, task_path.display()))
    })?;

    if task.status == "active" {
        let open_rows: Vec<&TaskWorkgroupHistory> = task
            .workgroup_history
            .iter()
            .filter(|row| row.ended_at.is_none())
            .collect();
        if open_rows.len() != 1 {
            return Err(TaskResolutionError::Message(format!(
                "Active task '{}' must have exactly one workgroupHistory row with endedAt == null ({})",
                task.id,
                task_path.display()
            )));
        }
        let open = open_rows[0];
        if open.workgroup != task.active_workgroup {
            return Err(TaskResolutionError::Message(format!(
                "Active task '{}' has activeWorkgroup '{}' but open history row workgroup '{}' ({})",
                task.id,
                task.active_workgroup,
                open.workgroup,
                task_path.display()
            )));
        }
        if open.branch != task.branch.name {
            return Err(TaskResolutionError::Message(format!(
                "Active task '{}' has branch '{}' but open history row branch '{}' ({})",
                task.id,
                task.branch.name,
                open.branch,
                task_path.display()
            )));
        }
        if open.status != "active" {
            return Err(TaskResolutionError::Message(format!(
                "Active task '{}' has open history row status '{}' instead of 'active' ({})",
                task.id,
                open.status,
                task_path.display()
            )));
        }
        if task.messaging.mode != "github-issue-comments"
            || task.messaging.notify_with != "issue-comment-url"
        {
            return Err(TaskResolutionError::Message(format!(
                "Active task '{}' must use messaging.mode='github-issue-comments' and notifyWith='issue-comment-url' ({})",
                task.id,
                task_path.display()
            )));
        }
    }

    Ok(())
}

fn find_single_active_task<'a>(
    tasks: &'a [TaskCandidate],
    workgroup: &str,
) -> Result<&'a TaskCandidate, TaskResolutionError> {
    let active: Vec<&TaskCandidate> = tasks
        .iter()
        .filter(|candidate| candidate.task.status == "active")
        .filter(|candidate| candidate.task.active_workgroup == workgroup)
        .collect();

    match active.len() {
        1 => Ok(active[0]),
        0 => Err(TaskResolutionError::Message(format!(
            "No active task record found for workgroup '{}'",
            workgroup
        ))),
        count => Err(TaskResolutionError::Message(format!(
            "Expected exactly one active task record for workgroup '{}' but found {}",
            workgroup, count
        ))),
    }
}

pub fn validate_issue_comment_url(
    comment_url: &str,
    task: &TaskRecord,
) -> Result<GitHubCommentUrl, TaskResolutionError> {
    let trimmed = comment_url.trim();
    let prefix = "https://github.com/";
    let rest = trimmed.strip_prefix(prefix).ok_or_else(|| {
        TaskResolutionError::Message(
            "--message must be a GitHub issue comment URL on https://github.com/".to_string(),
        )
    })?;

    let (path_part, fragment) = rest.split_once("#issuecomment-").ok_or_else(|| {
        TaskResolutionError::Message(
            "--message must include a GitHub issue comment fragment (#issuecomment-<id>)"
                .to_string(),
        )
    })?;

    if path_part.contains("/pull/") {
        return Err(TaskResolutionError::Message(
            "--message must point to a GitHub issue comment, not a pull request URL".to_string(),
        ));
    }

    let mut parts = path_part.split('/');
    let owner = parts.next().unwrap_or_default();
    let repo = parts.next().unwrap_or_default();
    let scope = parts.next().unwrap_or_default();
    let issue_number_raw = parts.next().unwrap_or_default();

    if owner.is_empty()
        || repo.is_empty()
        || scope != "issues"
        || issue_number_raw.is_empty()
        || parts.next().is_some()
    {
        return Err(TaskResolutionError::Message(
            "--message must match https://github.com/<owner>/<repo>/issues/<number>#issuecomment-<id>"
                .to_string(),
        ));
    }

    let issue_number = issue_number_raw.parse::<u64>().map_err(|_| {
        TaskResolutionError::Message(format!(
            "Issue number '{}' in --message is not a valid integer",
            issue_number_raw
        ))
    })?;
    let comment_id = fragment.parse::<u64>().map_err(|_| {
        TaskResolutionError::Message(format!(
            "Comment id '{}' in --message is not a valid integer",
            fragment
        ))
    })?;

    if owner != task.github.owner
        || repo != task.github.repo
        || issue_number != task.github.issue_number
    {
        return Err(TaskResolutionError::Message(format!(
            "--message URL must target {}/{} issue #{}",
            task.github.owner, task.github.repo, task.github.issue_number
        )));
    }

    Ok(GitHubCommentUrl {
        owner: owner.to_string(),
        repo: repo.to_string(),
        issue_number,
        comment_id,
        html_url: trimmed.to_string(),
    })
}

fn validate_branch_name(branch_name: &str, issue_number: u64) -> Result<(), String> {
    let (prefix, slug_with_issue) = branch_name
        .split_once('/')
        .ok_or_else(|| format!("Branch '{}' must contain a '/'", branch_name))?;
    if !matches!(prefix, "feature" | "fix" | "bug") {
        return Err(format!(
            "Branch '{}' must start with feature/, fix/, or bug/",
            branch_name
        ));
    }

    let suffix = format!("-gh{}", issue_number);
    let slug = slug_with_issue
        .strip_suffix(&suffix)
        .ok_or_else(|| format!("Branch '{}' must end with '{}'", branch_name, suffix))?;
    if slug.is_empty() {
        return Err(format!(
            "Branch '{}' must include a slug before '{}'",
            branch_name, suffix
        ));
    }
    if !slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(format!(
            "Branch '{}' must use only lowercase letters, digits, and hyphens in the slug",
            branch_name
        ));
    }

    Ok(())
}

fn read_current_branch(repo_root: &Path) -> Result<String, TaskResolutionError> {
    #[cfg(windows)]
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let mut cmd = Command::new("git");
    cmd.args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(repo_root);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = cmd.output().map_err(|e| {
        TaskResolutionError::Message(format!(
            "Failed to read current branch for '{}': {}",
            repo_root.display(),
            e
        ))
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(TaskResolutionError::Message(format!(
            "git rev-parse failed for '{}': {}",
            repo_root.display(),
            stderr
        )));
    }

    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() || branch == "HEAD" {
        return Err(TaskResolutionError::Message(format!(
            "Current branch for '{}' is detached or empty",
            repo_root.display()
        )));
    }

    Ok(branch)
}
