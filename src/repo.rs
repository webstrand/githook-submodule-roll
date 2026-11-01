//! A repository that git commands run in: the one that runs the hook, or a submodule within it.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

use crate::runner::{self, Error};

pub struct Repo {
    /// The working tree, from the top of the hook's working tree; `None` is the hook's own
    /// repository, whose commands keep the environment that git gives the hook.
    dir: Option<PathBuf>,
    /// The variables that `git rev-parse --local-env-vars` names, which point git at the hook's
    /// repository and are removed for the commands of a submodule.
    local_env: Rc<[String]>,
}

impl Repo {
    /// The repository that runs the hook, from the top of its working tree.
    pub fn top() -> Result<Repo, Error> {
        let top = String::from_utf8_lossy(&runner::bytes(&mut runner::git([
            "rev-parse",
            "--show-toplevel",
        ]))?)
        .trim_end()
        .to_owned();
        std::env::set_current_dir(&top).map_err(|source| Error::Spawn {
            command: format!("cd {top}"),
            source,
        })?;
        let local_env = String::from_utf8_lossy(&runner::bytes(&mut runner::git([
            "rev-parse",
            "--local-env-vars",
        ]))?)
        .lines()
        .map(str::to_owned)
        .collect();
        Ok(Repo {
            dir: None,
            local_env,
        })
    }

    /// The submodule at `path` in this repository's working tree.
    pub fn submodule(&self, path: &str) -> Repo {
        Repo {
            dir: Some(self.dir().join(path)),
            local_env: self.local_env.clone(),
        }
    }

    pub fn dir(&self) -> &Path {
        self.dir.as_deref().unwrap_or(Path::new(""))
    }

    /// The working tree's path from the top of the hook's working tree, for messages.
    pub fn path(&self) -> String {
        self.dir().to_string_lossy().into_owned()
    }

    /// Whether the submodule's working tree is checked out, which `git submodule init` and
    /// `git submodule update` do.
    pub fn is_checked_out(&self) -> bool {
        self.dir.is_none() || self.dir().join(".git").exists()
    }

    pub fn git<I, S>(&self, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = runner::git(args);
        if let Some(dir) = &self.dir {
            command.current_dir(dir);
            for var in self.local_env.iter() {
                command.env_remove(var);
            }
        }
        command
    }

    /// The standard output of a git command that must succeed, without its final newline.
    pub fn stdout<I, S>(&self, args: I) -> Result<String, Error>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let stdout = runner::bytes(&mut self.git(args))?;
        Ok(String::from_utf8_lossy(&stdout).trim_end().to_owned())
    }

    /// The standard output of a git command, or `None` when it fails, as `git rev-parse --verify`
    /// does for an object that the repository does not have.
    pub fn stdout_if_succeeds<I, S>(&self, args: I) -> Result<Option<String>, Error>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = runner::run(&mut self.git(args))?;
        Ok(output.status.success().then(|| {
            String::from_utf8_lossy(&output.stdout)
                .trim_end()
                .to_owned()
        }))
    }

    /// The commit that `rev` names, or `None` when the repository does not have it.
    pub fn commit(&self, rev: &str) -> Result<Option<String>, Error> {
        self.stdout_if_succeeds([
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{rev}^{{commit}}"),
        ])
    }

    /// Whether a remote-tracking branch or a tag holds `commit`, as one that the remotes publish.
    pub fn is_published(&self, commit: &str) -> Result<bool, Error> {
        if self.commit(commit)?.is_none() {
            return Ok(false);
        }
        let contains = format!("--contains={commit}");
        let refs = self.stdout([
            "for-each-ref",
            "--count=1",
            &contains,
            "refs/remotes",
            "refs/tags",
        ])?;
        Ok(!refs.is_empty())
    }

    /// Whether the repository has a remote-tracking branch, which it has once it fetched a remote.
    pub fn has_remote_branches(&self) -> Result<bool, Error> {
        Ok(!self
            .stdout(["for-each-ref", "--count=1", "refs/remotes"])?
            .is_empty())
    }
}
