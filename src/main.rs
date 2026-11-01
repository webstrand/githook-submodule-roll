//! Rolls each submodule forward, after a checkout or a merge, from the commit that its parent
//! pins to the highest release tag that contains the pin and is compatible with the highest
//! version tagged at or before it, as cargo reads semver. A submodule that the operator moved off
//! its pin, put on a branch or changed is left as it is. The submodules within a submodule are
//! rolled forward from the pins of the commit that it is left at.

mod repo;
mod roll;
mod runner;
mod tags;
mod version;

use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use crate::repo::Repo;

#[derive(Parser)]
#[command(name = "githook-submodule-roll", about)]
enum Cli {
    /// Runs as the post-checkout hook, with the arguments that git passes it.
    PostCheckout {
        prev: String,
        new: String,
        /// 1 for a checkout of a branch or commit, 0 for one of files.
        branch: String,
    },
    /// Runs as the post-merge hook, with the argument that git passes it.
    PostMerge { squash: String },
    /// Runs as the post-rewrite hook, with the argument that git passes it; only a rebase moves
    /// the submodules.
    PostRewrite { command: String },
    /// Rolls forward the submodules that are at the pins of the index, as after a checkout.
    Roll,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("githook-submodule-roll: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), runner::Error> {
    let prev = match cli {
        Cli::PostCheckout { branch, .. } if branch == "0" => return Ok(()),
        Cli::PostCheckout { prev, .. } if prev.bytes().all(|b| b == b'0') => Vec::new(),
        Cli::PostCheckout { prev, .. } => vec![prev],
        Cli::PostMerge { .. } => vec!["ORIG_HEAD".to_owned()],
        Cli::PostRewrite { command } => {
            // Git writes the rewritten commits to the hook, and may fail when it does not read them.
            io::copy(&mut io::stdin(), &mut io::sink()).ok();
            if command != "rebase" {
                return Ok(());
            }
            let mut prev = vec!["ORIG_HEAD".to_owned()];
            prev.extend(rebase_onto()?);
            prev
        }
        Cli::Roll => Vec::new(),
    };
    let repo = Repo::top()?;
    roll::roll(&repo, &prev)
}

/// The commit that the rebase that ends started from, which git checked out, and rolled the
/// submodules forward from, before it replayed the commits; its state directory is still there
/// when git runs post-rewrite.
fn rebase_onto() -> Result<Option<String>, runner::Error> {
    for dir in ["rebase-merge", "rebase-apply"] {
        let path = runner::bytes(&mut runner::git(["rev-parse", "--git-path", dir]))?;
        let onto = PathBuf::from(String::from_utf8_lossy(&path).trim_end()).join("onto");
        if let Ok(onto) = fs::read_to_string(onto) {
            return Ok(Some(onto.trim().to_owned()));
        }
    }
    Ok(None)
}
