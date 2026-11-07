use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::Path;

use serde::Deserialize;
use serde::de::IgnoredAny;

use crate::process::{self, Spec};

const GH: &str = "gh";
const HOST: &str = "github.com";
const BRANCH_PREFIX: &str = "summonr/";
const SLUG_MAX: usize = 40;
const SLUG_EMPTY: &str = "change";
const PULL_FIELDS: &str = "number,url,state,reviewDecision,statusCheckRollup";
const CHECK_FIELDS: &str = "name,bucket,link";
const QUIET: [(&str, &str); 1] = [("GH_PROMPT_DISABLED", "1")];

/// What a window asks of the workspace's GitHub repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ask {
    /// Reads the pull request for the current branch and its checks.
    Status,
    /// Moves work off the default branch, commits, pushes, and opens or finds the pull request; holds the topic title.
    Open(String),
    /// Merges the current branch's pull request.
    Merge,
}

/// A pull request on GitHub with its checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pull {
    pub number: u64,
    pub url: String,
    pub state: State,
    pub review: Review,
    pub checks: Vec<Check>,
}

/// Whether a pull request is open, merged, or closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum State {
    Open,
    Merged,
    Closed,
}

/// The review decision on a pull request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Review {
    Approved,
    ChangesRequested,
    ReviewRequired,
    #[default]
    #[serde(other)]
    None,
}

/// One check run on a pull request.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Check {
    pub name: String,
    pub bucket: Bucket,
    #[serde(default)]
    pub link: String,
}

/// The outcome group `gh pr checks` puts a check in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Bucket {
    Pass,
    Fail,
    Pending,
    Skipping,
    Cancel,
}

/// Why a forge request did not finish.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// `gh` is missing or has no signed-in GitHub account.
    SignedOut,
    /// The workspace has no GitHub remote.
    NoRemote,
    /// A git or `gh` command failed; holds its message.
    Failed(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SignedOut => f.write_str("GitHub not signed in"),
            Self::NoRemote => f.write_str("No GitHub remote"),
            Self::Failed(text) => f.write_str(text),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Deserialize)]
struct Auth {
    hosts: BTreeMap<String, Vec<Account>>,
}

#[derive(Deserialize)]
struct Account {
    state: Sign,
    active: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Sign {
    Success,
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Listed {
    number: u64,
    url: String,
    state: State,
    #[serde(default)]
    review_decision: Review,
    #[serde(default)]
    status_check_rollup: Vec<IgnoredAny>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Repo {
    default_branch_ref: Branch,
}

#[derive(Deserialize)]
struct Branch {
    name: String,
}

/// Answers `ask` in the workspace `dir`: the current branch's pull request, or none yet.
pub fn answer(dir: &Path, ask: &Ask) -> Result<Option<Pull>, Error> {
    let remote = remote(dir)?;
    signed_in(dir)?;
    match ask {
        Ask::Status => find(dir, &current(dir)?),
        Ask::Open(title) => open(dir, &remote, title).map(Some),
        Ask::Merge => merge(dir).map(Some),
    }
}

fn open(dir: &Path, remote: &str, title: &str) -> Result<Pull, Error> {
    let branch = own_branch(dir, title)?;
    if !git(dir, &["status", "--porcelain"])?.is_empty() {
        git(dir, &["add", "-A"])?;
        git(dir, &["commit", "-m", title])?;
    }
    git(dir, &["push", "-u", remote, &branch])?;
    if let Some(pull) = find(dir, &branch)? {
        return Ok(pull);
    }
    gh(dir, &["pr", "create", "--fill", "--head", &branch])?;
    find(dir, &branch)?
        .ok_or_else(|| Error::Failed(format!("gh opened no pull request for {branch}")))
}

fn merge(dir: &Path) -> Result<Pull, Error> {
    let branch = current(dir)?;
    let pull = find(dir, &branch)?
        .ok_or_else(|| Error::Failed(format!("{branch} has no open pull request")))?;
    gh(dir, &["pr", "merge", &pull.number.to_string(), "--merge"])?;
    let listed: Listed = json(&gh(
        dir,
        &["pr", "view", &pull.number.to_string(), "--json", PULL_FIELDS],
    )?)?;
    Ok(Pull {
        state: listed.state,
        ..pull
    })
}

fn own_branch(dir: &Path, title: &str) -> Result<String, Error> {
    let branch = current(dir)?;
    if branch != "HEAD" && branch != default_branch(dir)? {
        return Ok(branch);
    }
    let name = format!("{BRANCH_PREFIX}{}", slug(title));
    git(dir, &["switch", "-c", &name])?;
    Ok(name)
}

fn default_branch(dir: &Path) -> Result<String, Error> {
    let repo: Repo = json(&gh(dir, &["repo", "view", "--json", "defaultBranchRef"])?)?;
    Ok(repo.default_branch_ref.name)
}

fn current(dir: &Path) -> Result<String, Error> {
    git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
}

fn find(dir: &Path, branch: &str) -> Result<Option<Pull>, Error> {
    let listed: Vec<Listed> = json(&gh(
        dir,
        &["pr", "list", "--head", branch, "--json", PULL_FIELDS],
    )?)?;
    let Some(listed) = listed.into_iter().next() else {
        return Ok(None);
    };
    let checks = if listed.status_check_rollup.is_empty() {
        Vec::new()
    } else {
        json(&gh(
            dir,
            &["pr", "checks", &listed.number.to_string(), "--json", CHECK_FIELDS],
        )?)?
    };
    Ok(Some(Pull {
        number: listed.number,
        url: listed.url,
        state: listed.state,
        review: listed.review_decision,
        checks,
    }))
}

fn signed_in(dir: &Path) -> Result<(), Error> {
    let auth: Auth = gh(dir, &["auth", "status", "--json", "hosts"])
        .and_then(|text| json(&text))
        .map_err(|_error| Error::SignedOut)?;
    let active = auth.hosts.get(HOST).is_some_and(|accounts| {
        accounts
            .iter()
            .any(|account| account.active && account.state == Sign::Success)
    });
    if active { Ok(()) } else { Err(Error::SignedOut) }
}

fn remote(dir: &Path) -> Result<String, Error> {
    let urls = git(dir, &["config", "--get-regexp", r"^remote\..*\.url$"])
        .map_err(|_error| Error::NoRemote)?;
    urls.lines()
        .filter_map(|line| line.split_once(' '))
        .filter(|(_, url)| host(url) == HOST)
        .filter_map(|(key, _)| key.strip_prefix("remote.")?.strip_suffix(".url"))
        .min_by_key(|name| *name != "origin")
        .map(str::to_owned)
        .ok_or(Error::NoRemote)
}

fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let rest = rest.split_once('@').map_or(rest, |(_, rest)| rest);
    rest.split([':', '/']).next().unwrap_or_default()
}

fn slug(title: &str) -> String {
    let mut slug = String::new();
    for word in title
        .to_lowercase()
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
    {
        if slug.len() + word.len() + 1 > SLUG_MAX && !slug.is_empty() {
            break;
        }
        if !slug.is_empty() {
            slug.push('-');
        }
        slug.push_str(word);
    }
    slug.truncate(SLUG_MAX);
    if slug.is_empty() {
        SLUG_EMPTY.to_owned()
    } else {
        slug
    }
}

fn git(dir: &Path, args: &[&str]) -> Result<String, Error> {
    crate::diff::git_env(dir, &[], args).map_err(|error| Error::Failed(error.to_string()))
}

fn gh(dir: &Path, args: &[&str]) -> Result<String, Error> {
    let mut spec = Spec::new(GH, args.iter().map(OsStr::new)).envs(QUIET);
    spec.cwd = Some(dir.to_path_buf());
    let output = process::output(spec, None).map_err(|_error| Error::SignedOut)?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(Error::Failed(format!(
        "gh {} failed: {}",
        args.iter().take(2).copied().collect::<Vec<_>>().join(" "),
        stderr.trim()
    )))
}

fn json<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, Error> {
    serde_json::from_str(text).map_err(|error| Error::Failed(format!("gh sent unreadable JSON: {error}")))
}
