use crate::error::AppError;

/// Extracts the latest commit of a branch in a git repository.
pub(super) async fn get_latest_hash(repo_url: String, branch: String) -> Result<String, AppError> {
    tokio::process::Command::new("git")
        .args(["ls-remote", &repo_url, &branch])
        .output()
        .await
        .map_err(AppError::from)
        .and_then(handle_git_output_result)
        .and_then(|stdout| extract_hash(stdout, repo_url, branch))
}

fn handle_git_output_result(output: std::process::Output) -> Result<Vec<u8>, AppError> {
    if output.status.success() {
        Ok(output.stdout)
    } else {
        let msg = format!(
            "Git command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let io_error = std::io::Error::other(msg);
        Err(io_error.into())
    }
}

fn extract_hash(stdout: Vec<u8>, repo_url: String, branch: String) -> Result<String, AppError> {
    String::from_utf8_lossy(&stdout)
        .split_whitespace()
        .next()
        .map(|s| s.to_string())
        .ok_or(AppError::Process(format!(
            "Could not extract hash using `git ls-remote {repo_url} {branch}.\nstdout:\n{}`",
            String::from_utf8_lossy(&stdout)
        )))
}
