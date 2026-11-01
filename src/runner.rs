//! Runs git, and reports its failures.

use std::ffi::OsStr;
use std::io;
use std::process::{Command, ExitStatus, Output};

/// A program that could not be run, or that failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not run {command}: {source}")]
    Spawn { command: String, source: io::Error },
    #[error("{command} failed ({status}){}", stderr_suffix(.stderr))]
    Failed {
        command: String,
        status: ExitStatus,
        stderr: String,
    },
}

fn stderr_suffix(stderr: &str) -> String {
    if stderr.is_empty() {
        String::new()
    } else {
        format!(": {stderr}")
    }
}

/// A git command with the given arguments.
pub fn git<I, S>(args: I) -> Command
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new("git");
    command.args(args);
    command
}

/// Runs a command to its end, with its output captured, whatever its exit status.
pub fn run(command: &mut Command) -> Result<Output, Error> {
    command.output().map_err(|source| Error::Spawn {
        command: describe(command),
        source,
    })
}

/// Runs a command, and returns its standard output when it succeeds.
pub fn bytes(command: &mut Command) -> Result<Vec<u8>, Error> {
    let output = run(command)?;
    success(command, &output)?;
    Ok(output.stdout)
}

/// Fails unless the command that gave `output` succeeded.
pub fn success(command: &Command, output: &Output) -> Result<(), Error> {
    if output.status.success() {
        return Ok(());
    }
    Err(Error::Failed {
        command: describe(command),
        status: output.status,
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    })
}

/// A command as the shell would show it, for messages.
fn describe(command: &Command) -> String {
    std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|arg| arg.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ")
}
