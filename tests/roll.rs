//! Runs the hook in scratch repositories, as git runs it after a checkout or a merge.

use std::fs;
use std::path::Path;
use std::process::Command;

const HOOK: &str = env!("CARGO_BIN_EXE_githook-submodule-roll");

/// A git command in `repo`, isolated from the configuration around the test.
fn command(repo: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(repo)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        // The submodules are cloned from paths.
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "protocol.file.allow")
        .env("GIT_CONFIG_VALUE_0", "always")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env_remove("GIT_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_WORK_TREE");
    command
}

/// Runs git, and returns its standard output and standard error.
fn git(repo: &Path, args: &[&str]) -> (String, String) {
    let output = command(repo, args).output().unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn rev(repo: &Path, rev: &str) -> String {
    git(repo, &["rev-parse", rev]).0
}

fn init(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q", "-b", "main"]);
}

/// Commits a change to a file named `name`, and returns the commit.
fn commit(repo: &Path, name: &str) -> String {
    fs::write(repo.join(name), name).unwrap();
    git(repo, &["add", "--", name]);
    git(repo, &["commit", "-q", "-m", name]);
    rev(repo, "HEAD")
}

fn tag(repo: &Path, name: &str) {
    git(repo, &["tag", name]);
}

/// Registers the hook in `repo`'s config, as `install.sh` does.
fn install(repo: &Path) {
    for event in ["post-checkout", "post-merge", "post-rewrite"] {
        git(
            repo,
            &["config", &format!("hook.roll-{event}.event"), event],
        );
        git(
            repo,
            &[
                "config",
                &format!("hook.roll-{event}.command"),
                &format!("{HOOK} {event}"),
            ],
        );
    }
}

/// A library with the history `v1.2.3 - a b c pin - v1.4.0 - v1.9.0 - v1.10.0-rc.1 - v2.0.0`,
/// and a branch from `v1.2.3` that is tagged `v1.15.0`. Returns the commit `pin`.
fn library(dir: &Path) -> String {
    init(dir);
    commit(dir, "base");
    tag(dir, "v1.2.3");
    for name in ["a", "b", "c"] {
        commit(dir, name);
    }
    let pin = commit(dir, "pin");
    for name in ["v1.4.0", "v1.9.0", "v1.10.0-rc.1", "v2.0.0"] {
        commit(dir, name);
        tag(dir, name);
    }
    git(dir, &["checkout", "-q", "-b", "side", "v1.2.3"]);
    commit(dir, "side");
    tag(dir, "v1.15.0");
    git(dir, &["checkout", "-q", "main"]);
    pin
}

/// An application that pins the submodule `lib`, cloned from `library`, at `pin`, with the hook
/// installed.
fn application(dir: &Path, library: &Path, pin: &str) {
    init(dir);
    git(
        dir,
        &["submodule", "add", "-q", library.to_str().unwrap(), "lib"],
    );
    git(&dir.join("lib"), &["checkout", "-q", "--detach", pin]);
    git(dir, &["add", "lib"]);
    git(dir, &["commit", "-q", "-m", "app"]);
    install(dir);
}

struct Fixture {
    _tmp: tempfile::TempDir,
    lib: std::path::PathBuf,
    app: std::path::PathBuf,
    pin: String,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("lib");
    let app = tmp.path().join("app");
    let pin = library(&lib);
    application(&app, &lib, &pin);
    Fixture {
        _tmp: tmp,
        lib,
        app,
        pin,
    }
}

#[test]
fn rolls_forward_to_the_highest_release_of_the_major_that_contains_the_pin() {
    let f = fixture();
    let (_, stderr) = git(&f.app, &["checkout", "-q", "-b", "other"]);
    assert_eq!(
        rev(&f.app.join("lib"), "HEAD"),
        rev(&f.lib, "v1.9.0^{commit}")
    );
    assert!(!stderr.contains("warning"), "{stderr}");
    assert!(stderr.contains("lib: rolled forward from"), "{stderr}");
    assert!(stderr.contains(", after v1.2.3, to v1.9.0"), "{stderr}");
}

#[test]
fn leaves_the_pin_in_the_index() {
    let f = fixture();
    git(&f.app, &["checkout", "-q", "-b", "other"]);
    assert_eq!(rev(&f.app, ":lib"), f.pin);
}

#[test]
fn ignores_a_checkout_of_files() {
    let f = fixture();
    fs::write(f.app.join(".gitmodules"), "").unwrap();
    git(&f.app, &["checkout", "-q", "--", ".gitmodules"]);
    assert_eq!(rev(&f.app.join("lib"), "HEAD"), f.pin);
}

#[test]
fn rolls_forward_from_a_tag_before_1_0_within_its_minor() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("lib");
    let app = tmp.path().join("app");
    init(&lib);
    commit(&lib, "base");
    tag(&lib, "v0.3.1");
    let pin = commit(&lib, "pin");
    for name in ["v0.3.5", "v0.4.0"] {
        commit(&lib, name);
        tag(&lib, name);
    }
    application(&app, &lib, &pin);
    git(&app, &["checkout", "-q", "-b", "other"]);
    assert_eq!(rev(&app.join("lib"), "HEAD"), rev(&lib, "v0.3.5^{commit}"));
}

#[test]
fn stays_at_a_pin_without_a_tag_before_it() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("lib");
    let app = tmp.path().join("app");
    init(&lib);
    let pin = commit(&lib, "pin");
    commit(&lib, "v1.0.0");
    tag(&lib, "v1.0.0");
    application(&app, &lib, &pin);
    git(&app, &["checkout", "-q", "-b", "other"]);
    assert_eq!(rev(&app.join("lib"), "HEAD"), pin);
}

#[test]
fn leaves_a_submodule_on_a_branch() {
    let f = fixture();
    git(
        &f.app.join("lib"),
        &["checkout", "-q", "-b", "work", &f.pin],
    );
    git(&f.app, &["checkout", "-q", "-b", "other"]);
    assert_eq!(rev(&f.app.join("lib"), "HEAD"), f.pin);
}

#[test]
fn leaves_a_submodule_with_changes() {
    let f = fixture();
    fs::write(f.app.join("lib/a"), "changed").unwrap();
    let (_, stderr) = git(&f.app, &["checkout", "-q", "-b", "other"]);
    assert_eq!(rev(&f.app.join("lib"), "HEAD"), f.pin);
    assert!(stderr.contains("lib: has uncommitted changes"), "{stderr}");
}

#[test]
fn leaves_a_submodule_that_the_operator_moved() {
    let f = fixture();
    let moved = rev(&f.lib, "v1.4.0^{commit}");
    git(&f.app.join("lib"), &["checkout", "-q", "--detach", &moved]);
    git(&f.app, &["checkout", "-q", "-b", "other"]);
    assert_eq!(rev(&f.app.join("lib"), "HEAD"), moved);
}

#[test]
fn warns_about_a_pin_that_no_remote_branch_or_tag_holds() {
    let f = fixture();
    let lib = f.app.join("lib");
    git(&lib, &["commit", "-q", "--allow-empty", "-m", "unpushed"]);
    git(&f.app, &["commit", "-q", "-am", "unpushed"]);
    let (_, stderr) = git(&f.app, &["checkout", "-q", "-b", "other"]);
    assert!(
        stderr.contains("is on no branch or tag of the submodule's remotes"),
        "{stderr}"
    );
}

#[test]
fn follows_a_new_pin_across_a_checkout_that_does_not_recurse() {
    let f = fixture();
    git(&f.app, &["checkout", "-q", "-b", "two"]);
    let lib = f.app.join("lib");
    git(&lib, &["checkout", "-q", "--detach", "v2.0.0"]);
    git(&f.app, &["commit", "-q", "-am", "two"]);
    git(&f.app, &["checkout", "-q", "main"]);
    assert_eq!(rev(&lib, "HEAD"), rev(&f.lib, "v1.9.0^{commit}"));
    git(&f.app, &["checkout", "-q", "two"]);
    assert_eq!(rev(&lib, "HEAD"), rev(&f.lib, "v2.0.0^{commit}"));
    git(&f.app, &["checkout", "-q", "main"]);
    assert_eq!(rev(&lib, "HEAD"), rev(&f.lib, "v1.9.0^{commit}"));
}

#[test]
fn rolls_forward_after_a_checkout_that_recurses() {
    let f = fixture();
    git(&f.app, &["checkout", "-q", "-b", "other"]);
    git(&f.app, &["checkout", "-q", "--recurse-submodules", "main"]);
    assert_eq!(
        rev(&f.app.join("lib"), "HEAD"),
        rev(&f.lib, "v1.9.0^{commit}")
    );
}

#[test]
fn rolls_forward_after_a_pull() {
    let f = fixture();
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    git(
        tmp.path(),
        &[
            "clone",
            "-q",
            "--recurse-submodules",
            f.app.to_str().unwrap(),
            "clone",
        ],
    );
    install(&clone);
    git(
        &f.app.join("lib"),
        &["checkout", "-q", "--detach", "v1.4.0"],
    );
    git(&f.app, &["commit", "-q", "-am", "v1.4.0"]);
    git(&clone, &["pull", "-q"]);
    assert_eq!(
        rev(&clone.join("lib"), "HEAD"),
        rev(&f.lib, "v1.9.0^{commit}")
    );
}

#[test]
fn follows_a_new_pin_across_a_checkout_that_recurses() {
    let f = fixture();
    git(&f.app, &["checkout", "-q", "-b", "two"]);
    let lib = f.app.join("lib");
    git(&lib, &["checkout", "-q", "--detach", "v2.0.0"]);
    git(&f.app, &["commit", "-q", "-am", "two"]);
    git(&f.app, &["checkout", "-q", "--recurse-submodules", "main"]);
    assert_eq!(rev(&lib, "HEAD"), rev(&f.lib, "v1.9.0^{commit}"));
    git(&f.app, &["checkout", "-q", "--recurse-submodules", "two"]);
    assert_eq!(rev(&lib, "HEAD"), rev(&f.lib, "v2.0.0^{commit}"));
}

#[test]
fn rolls_forward_the_submodules_within_a_submodule() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("lib");
    let mid = tmp.path().join("mid");
    let app = tmp.path().join("app");
    let pin = library(&lib);
    init(&mid);
    git(
        &mid,
        &["submodule", "add", "-q", lib.to_str().unwrap(), "lib"],
    );
    git(&mid.join("lib"), &["checkout", "-q", "--detach", &pin]);
    git(&mid, &["add", "lib"]);
    git(&mid, &["commit", "-q", "-m", "mid"]);
    tag(&mid, "v1.0.0");
    let mid_pin = commit(&mid, "pin");
    commit(&mid, "v1.1.0");
    tag(&mid, "v1.1.0");
    init(&app);
    git(
        &app,
        &["submodule", "add", "-q", mid.to_str().unwrap(), "mid"],
    );
    git(&app.join("mid"), &["checkout", "-q", "--detach", &mid_pin]);
    git(&app, &["add", "mid"]);
    git(
        &app,
        &["submodule", "update", "-q", "--init", "--recursive"],
    );
    git(&app, &["commit", "-q", "-m", "app"]);
    install(&app);
    git(&app, &["checkout", "-q", "-b", "other"]);
    assert_eq!(rev(&app.join("mid"), "HEAD"), rev(&mid, "v1.1.0^{commit}"));
    assert_eq!(
        rev(&app.join("mid/lib"), "HEAD"),
        rev(&lib, "v1.9.0^{commit}")
    );
}

#[test]
fn rolls_forward_when_run_by_hand() {
    let f = fixture();
    let status = Command::new(HOOK)
        .arg("roll")
        .current_dir(&f.app)
        .env_remove("GIT_DIR")
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        rev(&f.app.join("lib"), "HEAD"),
        rev(&f.lib, "v1.9.0^{commit}")
    );
}

/// The refspec that fetches a remote's replaces notes, as `install.sh` adds it.
const NOTES_REFSPEC: &str = "+refs/notes/replaces*:refs/notes/remotes/origin/replaces*";

/// A library with the history `root - v1.2.3 - pin`. Returns `root` and `pin`.
fn library_to_rewrite(dir: &Path) -> (String, String) {
    init(dir);
    let root = commit(dir, "root");
    commit(dir, "base");
    tag(dir, "v1.2.3");
    let pin = commit(dir, "pin");
    (root, pin)
}

/// Amends the library's `pin` as a rewrite would, releases `v1.4.0` after the rewrite, and, when
/// `notes`, records in its replaces notes that the rewrite replaces `pin`. Returns the rewrite.
fn rewrite(dir: &Path, pin: &str, notes: bool) -> String {
    git(dir, &["checkout", "-q", "--detach", pin]);
    git(dir, &["commit", "-q", "--amend", "-m", "pin, amended"]);
    let rewritten = rev(dir, "HEAD");
    git(dir, &["checkout", "-q", "-B", "main", &rewritten]);
    commit(dir, "v1.4.0");
    tag(dir, "v1.4.0");
    if notes {
        let note = format!("replaces: {pin}");
        git(
            dir,
            &["notes", "--ref", "replaces", "add", "-m", &note, &rewritten],
        );
    }
    rewritten
}

/// Fetches the library into the submodule, with the replaces notes.
fn fetch_with_notes(sub: &Path) {
    git(
        sub,
        &["config", "--add", "remote.origin.fetch", NOTES_REFSPEC],
    );
    git(sub, &["fetch", "-q", "origin"]);
}

#[test]
fn rolls_forward_to_a_tag_that_holds_a_rewrite_of_the_pin() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("lib");
    let app = tmp.path().join("app");
    let (_, pin) = library_to_rewrite(&lib);
    application(&app, &lib, &pin);
    rewrite(&lib, &pin, true);
    fetch_with_notes(&app.join("lib"));
    let (_, stderr) = git(&app, &["checkout", "-q", "-b", "other"]);
    assert_eq!(rev(&app.join("lib"), "HEAD"), rev(&lib, "v1.4.0^{commit}"));
    assert!(
        stderr.contains("to v1.4.0, which holds a rewrite of it"),
        "{stderr}"
    );
    assert!(
        stderr.contains("warning: only the replaces notes tie the pin"),
        "{stderr}"
    );
}

#[test]
fn stays_at_a_rewritten_pin_without_notes() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("lib");
    let app = tmp.path().join("app");
    let (_, pin) = library_to_rewrite(&lib);
    application(&app, &lib, &pin);
    rewrite(&lib, &pin, false);
    fetch_with_notes(&app.join("lib"));
    let (_, stderr) = git(&app, &["checkout", "-q", "-b", "other"]);
    assert_eq!(rev(&app.join("lib"), "HEAD"), pin);
    assert!(
        stderr.contains("is on no branch or tag of the submodule's remotes"),
        "{stderr}"
    );
}

#[test]
fn rolls_forward_from_a_rewritten_pin_that_the_submodule_lacks() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("lib");
    let app = tmp.path().join("app");
    let (root, pin) = library_to_rewrite(&lib);
    application(&app, &lib, &root);
    let sub = app.join("lib");
    git(&sub, &["checkout", "-q", "--detach", &pin]);
    git(&app, &["commit", "-q", "-am", "pin"]);
    git(&app, &["checkout", "-q", "--detach", "HEAD^"]);
    git(&sub, &["checkout", "-q", "--detach", &root]);
    rewrite(&lib, &pin, true);
    fetch_with_notes(&sub);
    git(&sub, &["branch", "-q", "-D", "main"]);
    git(&sub, &["reflog", "expire", "--expire=now", "--all"]);
    git(&sub, &["gc", "-q", "--prune=now"]);
    assert!(
        !command(&sub, &["cat-file", "-e", &pin])
            .status()
            .unwrap()
            .success()
    );
    git(&app, &["checkout", "-q", "main"]);
    assert_eq!(rev(&sub, "HEAD"), rev(&lib, "v1.4.0^{commit}"));
}

#[test]
fn leaves_a_submodule_whose_untracked_files_the_target_would_overwrite() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("lib");
    let other = tmp.path().join("other");
    let app = tmp.path().join("app");
    let pin = library(&lib);
    let other_pin = library(&other);
    application(&app, &lib, &pin);
    git(
        &app,
        &["submodule", "add", "-q", other.to_str().unwrap(), "other"],
    );
    git(
        &app.join("other"),
        &["checkout", "-q", "--detach", &other_pin],
    );
    git(&app, &["commit", "-qam", "other"]);
    fs::write(app.join("lib/v1.4.0"), "untracked").unwrap();
    let (_, stderr) = git(&app, &["checkout", "-q", "-b", "next"]);
    assert_eq!(rev(&app.join("lib"), "HEAD"), pin);
    assert_eq!(
        fs::read_to_string(app.join("lib/v1.4.0")).unwrap(),
        "untracked"
    );
    assert!(
        stderr.contains("lib: could not check out v1.9.0"),
        "{stderr}"
    );
    assert_eq!(
        rev(&app.join("other"), "HEAD"),
        rev(&other, "v1.9.0^{commit}")
    );
}

#[test]
fn leaves_a_submodule_within_a_submodule_that_git_would_move_with_it() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("lib");
    let mid = tmp.path().join("mid");
    let app = tmp.path().join("app");
    let pin = library(&lib);
    init(&mid);
    git(
        &mid,
        &["submodule", "add", "-q", lib.to_str().unwrap(), "lib"],
    );
    git(&mid.join("lib"), &["checkout", "-q", "--detach", &pin]);
    git(&mid, &["add", "lib"]);
    git(&mid, &["commit", "-q", "-m", "mid"]);
    tag(&mid, "v1.0.0");
    git(&mid.join("lib"), &["checkout", "-q", "--detach", "v2.0.0"]);
    git(&mid, &["commit", "-qam", "lib v2"]);
    tag(&mid, "v1.1.0");
    init(&app);
    git(
        &app,
        &["submodule", "add", "-q", mid.to_str().unwrap(), "mid"],
    );
    git(&app.join("mid"), &["checkout", "-q", "--detach", "v1.0.0"]);
    git(&app, &["add", "mid"]);
    git(
        &app,
        &["submodule", "update", "-q", "--init", "--recursive"],
    );
    git(&app, &["commit", "-q", "-m", "app"]);
    install(&app);
    // As a global config would set it for every repository.
    git(&app.join("mid"), &["config", "submodule.recurse", "true"]);
    let nested = app.join("mid/lib");
    git(&nested, &["checkout", "-q", "-b", "work", &pin]);
    git(&app, &["checkout", "-q", "-b", "other"]);
    assert_eq!(rev(&app.join("mid"), "HEAD"), rev(&mid, "v1.1.0^{commit}"));
    assert_eq!(git(&nested, &["symbolic-ref", "HEAD"]).0, "refs/heads/work");
    assert_eq!(rev(&nested, "HEAD"), pin);
}

#[test]
fn follows_the_pin_of_the_commits_that_a_rebase_replays() {
    let f = fixture();
    let lib = f.app.join("lib");
    git(&f.app, &["checkout", "-q", "-b", "topic"]);
    git(&lib, &["checkout", "-q", "--detach", "v2.0.0"]);
    git(&f.app, &["commit", "-q", "-am", "two"]);
    git(&f.app, &["checkout", "-q", "main"]);
    commit(&f.app, "unrelated");
    git(&f.app, &["checkout", "-q", "topic"]);
    assert_eq!(rev(&lib, "HEAD"), rev(&f.lib, "v2.0.0^{commit}"));
    git(&f.app, &["rebase", "-q", "main"]);
    assert_eq!(rev(&lib, "HEAD"), rev(&f.lib, "v2.0.0^{commit}"));
}
