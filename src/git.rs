//! git 原生工具：Fux 只调用 git CLI，凭据由用户在节点本地自理。
use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use tokio::process::Command;

pub async fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .await
        .context("无法执行 git（请确认已安装）")?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(anyhow!(
            "git {} 失败: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

pub async fn git_ok(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub async fn is_repo(dir: &Path) -> bool {
    dir.exists() && git_ok(dir, &["rev-parse", "--git-dir"]).await
}

/// 相对路径按进程当前目录解析为绝对路径。
/// git 子进程以仓库目录为 cwd，若把相对 worktree 路径直接交给 git，
/// 会解析到仓库内部而不是调用方期望的 data 目录。
fn absolutize(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

/// 本地非 git 目录：一键 init（在任务工作副本中调用）。
pub async fn init_repo(dir: &Path) -> Result<()> {
    tokio::fs::create_dir_all(dir).await?;
    git(dir, &["init", "-b", "main"]).await?;
    git(dir, &["config", "user.email", "fux@local"]).await.ok();
    git(dir, &["config", "user.name", "Fux"]).await.ok();
    Ok(())
}

pub async fn ensure_clone(remote: &str, dir: &Path) -> Result<()> {
    if is_repo(dir).await {
        git(dir, &["fetch", "--all", "--prune"]).await.ok();
        return Ok(());
    }
    if let Some(parent) = dir.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let out = Command::new("git")
        .args(["clone", remote, &dir.to_string_lossy()])
        .output()
        .await?;
    if !out.status.success() {
        return Err(anyhow!(
            "git clone 失败（请检查节点 git 认证/网络）: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

pub async fn detect_default_branch(dir: &Path) -> String {
    if let Ok(s) = git(dir, &["symbolic-ref", "refs/remotes/origin/HEAD"]).await {
        if let Some(b) = s.rsplit('/').next() {
            return b.to_string();
        }
    }
    if let Ok(b) = git(dir, &["rev-parse", "--abbrev-ref", "HEAD"]).await {
        if !b.is_empty() && b != "HEAD" {
            return b.to_string();
        }
    }
    "main".to_string()
}

/// 为任务准备隔离 worktree：分支 fux/task-<id>。
pub async fn prepare_worktree(
    repo_dir: &Path,
    worktree_dir: &Path,
    branch: &str,
    base: &str,
) -> Result<()> {
    // 统一成绝对路径：git worktree add 在 repo_dir 下执行，相对路径会被解析到仓库内部。
    let worktree_dir = absolutize(worktree_dir);
    if worktree_dir.exists() {
        return Ok(());
    }
    if let Some(parent) = worktree_dir.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    // 清理可能残留的同名分支 worktree 记录
    git(repo_dir, &["worktree", "prune"]).await.ok();
    let base_ref = if git_ok(
        repo_dir,
        &["rev-parse", "--verify", &format!("origin/{base}")],
    )
    .await
    {
        format!("origin/{base}")
    } else if git_ok(repo_dir, &["rev-parse", "--verify", base]).await {
        base.to_string()
    } else {
        "HEAD".to_string()
    };
    let wt = worktree_dir.to_string_lossy().to_string();
    if git_ok(repo_dir, &["rev-parse", "--verify", branch]).await {
        git(repo_dir, &["worktree", "add", &wt, branch]).await?;
    } else {
        git(repo_dir, &["worktree", "add", "-b", branch, &wt, &base_ref]).await?;
    }
    Ok(())
}

pub async fn remove_worktree(repo_dir: &Path, worktree_dir: &Path) {
    // 同 prepare_worktree：git 在 repo_dir 下解析参数，必须先转绝对路径。
    let wt = absolutize(worktree_dir);
    let wt = wt.to_string_lossy().to_string();
    git(repo_dir, &["worktree", "remove", "--force", &wt])
        .await
        .ok();
    git(repo_dir, &["worktree", "prune"]).await.ok();
}

/// 提交全部变更；无变更返回 None。
pub async fn commit_all(dir: &Path, msg: &str) -> Result<Option<String>> {
    git(dir, &["add", "-A"]).await?;
    if git_ok(dir, &["diff", "--cached", "--quiet"]).await {
        return Ok(None);
    }
    git(dir, &["config", "user.email", "fux@local"]).await.ok();
    git(dir, &["config", "user.name", "Fux"]).await.ok();
    git(dir, &["commit", "-m", msg]).await?;
    Ok(Some(git(dir, &["rev-parse", "HEAD"]).await?))
}

pub async fn has_remote(dir: &Path) -> bool {
    git_ok(dir, &["remote", "get-url", "origin"]).await
}

pub async fn push(dir: &Path, branch: &str) -> Result<()> {
    git(dir, &["push", "-u", "origin", branch]).await?;
    Ok(())
}

/// 合并检测：branch 是否已并入 origin/base（git 原生，无平台 API）。
pub async fn is_merged(dir: &Path, branch: &str, base: &str) -> Result<bool> {
    git(dir, &["fetch", "origin", "--prune"]).await.ok();
    let base_ref = format!("origin/{base}");
    if !git_ok(dir, &["rev-parse", "--verify", &base_ref]).await {
        return Ok(false);
    }
    Ok(git_ok(dir, &["merge-base", "--is-ancestor", branch, &base_ref]).await)
}

/// 从 remote URL 推导 PR/MR 创建链接（仅字符串推导，不调用平台 API）。
pub fn pr_create_url(remote: &str, branch: &str, base: &str) -> Option<String> {
    let r = remote.trim().trim_end_matches(".git");
    // git@host:owner/repo 或 https://host/owner/repo
    let (host, path) = if let Some(rest) = r.strip_prefix("git@") {
        let mut it = rest.splitn(2, ':');
        (it.next()?.to_string(), it.next()?.to_string())
    } else if let Some(rest) = r
        .strip_prefix("https://")
        .or_else(|| r.strip_prefix("http://"))
    {
        let mut it = rest.splitn(2, '/');
        (it.next()?.to_string(), it.next()?.to_string())
    } else if let Some(rest) = r.strip_prefix("ssh://git@") {
        let mut it = rest.splitn(2, '/');
        (it.next()?.to_string(), it.next()?.to_string())
    } else {
        return None;
    };
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segs.len() < 2 {
        return None;
    }
    let owner = segs[0];
    let repo = segs[1];
    if host.contains("github") {
        Some(format!(
            "https://{host}/{owner}/{repo}/compare/{base}...{branch}?expand=1"
        ))
    } else if host.contains("gitlab") {
        Some(format!(
            "https://{host}/{owner}/{repo}/-/merge_requests/new?merge_request%5Bsource_branch%5D={branch}&merge_request%5Btarget_branch%5D={base}"
        ))
    } else {
        // 通用 fallback：Gitea 等
        Some(format!(
            "https://{host}/{owner}/{repo}/compare/{base}...{branch}"
        ))
    }
}

/// 递归拷贝目录（非 git 本地项目的任务工作副本）。
pub fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            if entry.file_name() == ".git"
                || entry.file_name() == "node_modules"
                || entry.file_name() == "target"
            {
                continue;
            }
            copy_dir_recursive(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}

/// 项目检出根目录（控制面/节点本地缓存）。
pub fn checkout_dir(data_dir: &Path, project_id: &str) -> PathBuf {
    data_dir.join("checkouts").join(project_id)
}

pub fn worktree_dir(data_dir: &Path, task_id: &str) -> PathBuf {
    data_dir.join("worktrees").join(task_id)
}
/// 工作流：每个节点一个独立 worktree（同一节点重跑复用，便于续跑与返工）。
pub fn run_worktree_dir(data_dir: &Path, task_id: &str, run_id: &str) -> PathBuf {
    data_dir.join("worktrees").join(task_id).join(run_id)
}

/// 工作流节点 worktree：按 nodeKey 稳定复用（parallel 节点互不冲突）。
pub fn stage_worktree_dir(data_dir: &Path, task_id: &str, stage_key: &str) -> PathBuf {
    data_dir
        .join("worktrees")
        .join(task_id)
        .join(format!("node-{stage_key}"))
}

/// 把上游节点产物检出到 workdir/.fux/upstream/<key>（只读参考）。
pub async fn materialize_upstreams(
    workdir: &Path,
    upstream: &[crate::runner::UpstreamSpec],
) -> Result<()> {
    if upstream.is_empty() {
        return Ok(());
    }
    let base = workdir.join(".fux").join("upstream");
    tokio::fs::create_dir_all(&base).await?;
    for u in upstream {
        let dir = base.join(&u.key);
        if dir.exists() {
            continue;
        }
        if let Err(e) = ensure_clone(&u.remote, &dir).await {
            tracing::warn!("上游产物检出失败 {}: {e:#}", u.key);
            continue;
        }
        if !u.sha.is_empty() {
            git(&dir, &["checkout", "--detach", &u.sha]).await.ok();
        } else if !u.branch.is_empty() {
            git(&dir, &["checkout", &u.branch]).await.ok();
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pr_links() {
        assert_eq!(
            pr_create_url("https://github.com/o/r.git", "fux/task-1", "main").unwrap(),
            "https://github.com/o/r/compare/main...fux/task-1?expand=1"
        );
        assert_eq!(
            pr_create_url("git@gitlab.com:o/r.git", "b", "main").unwrap(),
            "https://gitlab.com/o/r/-/merge_requests/new?merge_request%5Bsource_branch%5D=b&merge_request%5Btarget_branch%5D=main"
        );
        assert!(pr_create_url("https://gitea.example.com/o/r", "b", "main").is_some());
        assert!(pr_create_url("/local/path", "b", "main").is_none());
    }

    /// 回归：git 子进程以仓库目录为 cwd，相对 worktree 路径必须按进程 cwd 解析，
    /// 否则会被创建到仓库内部，重试时报「目录已存在」。
    #[tokio::test]
    async fn prepare_worktree_resolves_relative_path_against_process_cwd() {
        let repo = tempfile::tempdir().unwrap();
        git(repo.path(), &["init", "-b", "main"]).await.unwrap();
        std::fs::write(repo.path().join("README.md"), "test").unwrap();
        git(repo.path(), &["add", "-A"]).await.unwrap();
        git(
            repo.path(),
            &[
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-m",
                "init",
            ],
        )
        .await
        .unwrap();

        let id = crate::new_id();
        let rel = PathBuf::from("target").join(format!("fux-wt-test-{id}"));
        prepare_worktree(repo.path(), &rel, "fux/test-rel", "main")
            .await
            .unwrap();

        assert!(
            rel.is_dir(),
            "worktree 应按进程 cwd 创建: {}",
            rel.display()
        );
        assert!(
            !repo.path().join(&rel).exists(),
            "不应在仓库内部创建 worktree"
        );
        let list = git(repo.path(), &["worktree", "list", "--porcelain"])
            .await
            .unwrap();
        // worktree list 输出为正斜杠绝对路径，匹配唯一目录名即可。
        assert!(list.contains(&format!("fux-wt-test-{id}")));

        remove_worktree(repo.path(), &rel).await;
        assert!(!rel.exists(), "remove_worktree 应清理相对路径 worktree");
    }
}
