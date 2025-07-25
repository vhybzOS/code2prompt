//! This module handles git operations.

use anyhow::{Context, Result};
use git2::{DiffOptions, Repository};
use log::info;
use std::path::Path;

/// Generates a git diff for the repository at the provided path.
///
/// This function compares the repository's HEAD tree with the index to produce a diff of staged changes.
/// It also checks for unstaged changes (differences between the index and the working directory) and,
/// if found, appends a notification to the output.
///
/// If there are no staged changes, the function returns a message in the format:
/// `"no diff between HEAD and index"`.
///
/// # Arguments
///
/// * `repo_path` - A reference to the path of the git repository.
///
/// # Returns
///
/// * `Result<String>` - On success, returns either the diff (with an appended note if unstaged changes exist)
///   or a message indicating that there is no diff between the compared git objects.
///   In case of error, returns an appropriate error.
pub fn get_git_diff(repo_path: &Path) -> Result<String> {
    info!("Opening repository at path: {:?}", repo_path);
    let repo = Repository::open(repo_path).context("Failed to open repository")?;

    let head = repo.head().context("Failed to get repository head")?;
    let head_tree = head.peel_to_tree().context("Failed to peel to tree")?;

    // Generate diff for staged changes (HEAD vs. index)
    let staged_diff = repo
        .diff_tree_to_index(
            Some(&head_tree),
            None,
            Some(DiffOptions::new().ignore_whitespace(true)),
        )
        .context("Failed to generate diff for staged changes")?;

    let mut staged_diff_text = Vec::new();
    staged_diff
        .print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
            staged_diff_text.extend_from_slice(line.content());
            true
        })
        .context("Failed to print staged diff")?;

    let staged_diff_output = String::from_utf8_lossy(&staged_diff_text).into_owned();

    // If there is no staged diff, return a message indicating so.
    if staged_diff_output.trim().is_empty() {
        return Ok("no diff between HEAD and index".to_string());
    }

    // Generate diff for unstaged changes (index vs. working directory)
    let unstaged_diff = repo
        .diff_index_to_workdir(None, Some(DiffOptions::new().ignore_whitespace(true)))
        .context("Failed to generate diff for unstaged changes")?;

    let mut unstaged_diff_text = Vec::new();
    unstaged_diff
        .print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
            unstaged_diff_text.extend_from_slice(line.content());
            true
        })
        .context("Failed to print unstaged diff")?;

    let unstaged_diff_output = String::from_utf8_lossy(&unstaged_diff_text).into_owned();

    let mut output = staged_diff_output;
    if !unstaged_diff_output.trim().is_empty() {
        output.push_str("\nNote: Some changes are not staged.");
    }

    info!("Generated git diff successfully");
    Ok(output)
}

/// Generates a git diff between two branches for the repository at the provided path
///
/// # Arguments
///
/// * `repo_path` - A reference to the path of the git repository
/// * `branch1` - The name of the first branch
/// * `branch2` - The name of the second branch
///
/// # Returns
///
/// * `Result<String, git2::Error>` - The generated git diff as a string or an error
pub fn get_git_diff_between_branches(
    repo_path: &Path,
    branch1: &str,
    branch2: &str,
) -> Result<String> {
    info!("Opening repository at path: {:?}", repo_path);
    let repo = Repository::open(repo_path).context("Failed to open repository")?;

    for branch in [branch1, branch2].iter() {
        if !branch_exists(&repo, branch) {
            return Err(anyhow::anyhow!("Branch {} doesn't exist!", branch));
        }
    }

    let branch1_commit = repo.revparse_single(branch1)?.peel_to_commit()?;
    let branch2_commit = repo.revparse_single(branch2)?.peel_to_commit()?;

    let branch1_tree = branch1_commit.tree()?;
    let branch2_tree = branch2_commit.tree()?;

    let diff = repo
        .diff_tree_to_tree(
            Some(&branch1_tree),
            Some(&branch2_tree),
            Some(DiffOptions::new().ignore_whitespace(true)),
        )
        .context("Failed to generate diff between branches")?;

    let mut diff_text = Vec::new();
    diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        diff_text.extend_from_slice(line.content());
        true
    })
    .context("Failed to print diff")?;

    info!("Generated git diff between branches successfully");
    Ok(String::from_utf8_lossy(&diff_text).into_owned())
}

/// Retrieves the git log between two branches for the repository at the provided path
///
/// # Arguments
///
/// * `repo_path` - A reference to the path of the git repository
/// * `branch1` - The name of the first branch (e.g., "master")
/// * `branch2` - The name of the second branch (e.g., "migrate-manifest-v3")
///
/// # Returns
///
/// * `Result<String, git2::Error>` - The git log as a string or an error
pub fn get_git_log(repo_path: &Path, branch1: &str, branch2: &str) -> Result<String> {
    info!("Opening repository at path: {:?}", repo_path);
    let repo = Repository::open(repo_path).context("Failed to open repository")?;

    for branch in [branch1, branch2].iter() {
        if !branch_exists(&repo, branch) {
            return Err(anyhow::anyhow!("Branch {} doesn't exist!", branch));
        }
    }

    let branch1_commit = repo.revparse_single(branch1)?.peel_to_commit()?;
    let branch2_commit = repo.revparse_single(branch2)?.peel_to_commit()?;

    let mut revwalk = repo.revwalk().context("Failed to create revwalk")?;
    revwalk
        .push(branch2_commit.id())
        .context("Failed to push branch2 commit to revwalk")?;
    revwalk
        .hide(branch1_commit.id())
        .context("Failed to hide branch1 commit from revwalk")?;
    revwalk.set_sorting(git2::Sort::REVERSE)?;

    let mut log_text = String::new();
    for oid in revwalk {
        let oid = oid.context("Failed to get OID from revwalk")?;
        let commit = repo.find_commit(oid).context("Failed to find commit")?;
        log_text.push_str(&format!(
            "{} - {}\n",
            &commit.id().to_string()[..7],
            commit.summary().unwrap_or("No commit message")
        ));
    }

    info!("Retrieved git log successfully");
    Ok(log_text)
}

/// Checks if a git reference exists in the given repository
///
/// This function can validate any git reference including:
/// - Local and remote branch names
/// - Commit hashes (full or abbreviated)
/// - Tags
/// - Any reference that git rev-parse can resolve
///
/// # Arguments
///
/// * `repo` - A reference to the `Repository` where the reference should be checked
/// * `branch_name` - A string slice that holds the name of the reference to check
///
/// # Returns
///
/// * `bool` - `true` if the reference exists, `false` otherwise
fn branch_exists(repo: &Repository, branch_name: &str) -> bool {
    repo.revparse_single(branch_name).is_ok()
}
