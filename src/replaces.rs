//! Reads the replaces notes that `githook-replaces-index` records, without its programs: the
//! note of a commit that a rewrite made holds a `replaces: <commit>` line for each commit that it
//! stands in place of.

use std::collections::{BTreeMap, BTreeSet};

use crate::repo::Repo;
use crate::runner::{self, Error};

/// The local replaces notes and those fetched from each remote.
const REFS: [&str; 2] = ["refs/notes/replaces", "refs/notes/remotes/*/replaces"];

/// For each commit, the commits that the notes say were made from it.
#[derive(Default)]
pub struct Replaces {
    made_from: BTreeMap<String, BTreeSet<String>>,
}

impl Replaces {
    /// The notes of every notes ref of `repo` that `REFS` matches; none when it has none.
    pub fn read(repo: &Repo) -> Result<Replaces, Error> {
        let refs = repo.stdout(
            ["for-each-ref", "--format=%(refname)"]
                .into_iter()
                .chain(REFS),
        )?;
        let mut notes = BTreeMap::new();
        for notes_ref in refs.lines() {
            let list = repo.stdout(["notes", "--ref", notes_ref, "list"])?;
            for line in list.lines() {
                if let Some((blob, commit)) = line.split_once(' ') {
                    notes
                        .entry(blob.to_owned())
                        .or_insert_with(BTreeSet::new)
                        .insert(commit.to_owned());
                }
            }
        }
        let mut replaces = Replaces::default();
        if notes.is_empty() {
            return Ok(replaces);
        }
        let input: String = notes.keys().map(|blob| format!("{blob}\n")).collect();
        let output =
            runner::bytes_with_input(&mut repo.git(["cat-file", "--batch"]), input.as_bytes())?;
        for (blob, text) in batch(&output) {
            let Some(commits) = notes.get(blob) else {
                continue;
            };
            for old in text
                .lines()
                .filter_map(|line| line.strip_prefix("replaces: "))
            {
                let made_from = replaces.made_from.entry(old.trim().to_owned()).or_default();
                made_from.extend(commits.iter().cloned());
            }
        }
        Ok(replaces)
    }

    /// Every commit that the notes say stands in place of `commit`, following the notes from
    /// each commit to those made from it, as the copies of a note's lines may be missing.
    pub fn versions_of(&self, commit: &str) -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        let mut queue = vec![commit];
        while let Some(old) = queue.pop() {
            for new in self.made_from.get(old).into_iter().flatten() {
                if new != commit && found.insert(new.clone()) {
                    queue.push(new);
                }
            }
        }
        found
    }
}

/// The objects of `git cat-file --batch` output, as their ids and their contents.
fn batch(mut output: &[u8]) -> Vec<(&str, String)> {
    let mut objects = Vec::new();
    while let Some(end) = output.iter().position(|&b| b == b'\n') {
        let header = std::str::from_utf8(&output[..end]).unwrap_or("");
        output = &output[end + 1..];
        let mut fields = header.split(' ');
        let (Some(id), Some(_), Some(size)) = (fields.next(), fields.next(), fields.next()) else {
            // A missing object prints `<id> missing`, and no contents.
            continue;
        };
        let Ok(size) = size.parse::<usize>() else {
            continue;
        };
        let size = size.min(output.len());
        objects.push((id, String::from_utf8_lossy(&output[..size]).into_owned()));
        output = output.get(size + 1..).unwrap_or(&[]);
    }
    objects
}
