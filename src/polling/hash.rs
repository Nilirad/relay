use crate::error::AppError;

/// Extracts the latest commit of a branch in a git repository.
pub(super) async fn get_latest_hash(repo_url: String, branch: String) -> Result<String, AppError> {
    tokio::process::Command::new("git")
        .args(["ls-remote", &repo_url, &branch])
        .output()
        .await
        .map_err(AppError::from)
        .and_then(handle_git_output_result)
        .and_then(extract_hash)
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

fn extract_hash(stdout: Vec<u8>) -> Result<String, AppError> {
    String::from_utf8_lossy(&stdout)
        .split_whitespace()
        .next()
        .map(|s| s.to_string())
        .ok_or(AppError::Process(
            "`git` could not get commit hash".to_string(),
        ))
}
