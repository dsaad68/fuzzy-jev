//! `jev add skill`, through the built command rather than its functions: the folder it picks from
//! the current directory, what it prints, and what it exits with. The unit tests in
//! `src/cli/skill.rs` cover the writing itself; these cover the parts only the binary has.
//!
//! No key and no network: `add skill` writes files, and the one call that would ask something is
//! `--dry-run`.

#![cfg(feature = "cli")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A folder of this test's own, removed when it's dropped.
struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Temp {
        let dir = std::env::temp_dir().join(format!("jev-add-skill-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Temp(dir)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Runs the built `jev` in `dir`.
fn jev(dir: &Path, argv: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jev")).current_dir(dir).args(argv).output().expect("running jev")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The skill's two files, under the folder `add skill` writes.
const FILES: [&str; 2] = ["SKILL.md", "references/patterns.md"];

#[test]
fn writes_the_skill_into_the_folder_it_is_run_in() {
    let temp = Temp::new("cwd");
    let done = jev(&temp.0, &["add", "skill"]);
    assert!(done.status.success(), "{}", stderr(&done));

    // The default is .agents, and the folder is named after the skill, as the spec requires.
    let skill = temp.0.join(".agents/skills/jev");
    for file in FILES {
        assert!(skill.join(file).is_file(), "no {file}");
    }
    assert!(fs::read_to_string(skill.join("SKILL.md")).unwrap().starts_with("---\nname: jev\n"));

    // It says what it did, in paths relative to where it ran, and where to find the result.
    let said = stdout(&done);
    assert!(said.contains("created .agents/skills/jev/SKILL.md"), "{said}");
    assert!(said.contains("created .agents/skills/jev/references/patterns.md"), "{said}");
    assert!(said.contains(".agents/skills/jev"), "{said}");
    // The absolute temporary path is an implementation detail; the output shouldn't carry it.
    assert!(!said.contains(temp.0.to_str().unwrap()), "{said}");
}

#[test]
fn the_flag_picks_the_folder() {
    for (flag, dir, other) in
        [(Some("--agents"), ".agents", ".claude"), (Some("--claude"), ".claude", ".agents"), (None, ".agents", ".claude")]
    {
        let temp = Temp::new(flag.unwrap_or("default").trim_start_matches('-'));
        let argv: Vec<&str> = ["add", "skill"].into_iter().chain(flag).collect();
        let done = jev(&temp.0, &argv);
        assert!(done.status.success(), "{}", stderr(&done));
        assert!(temp.0.join(dir).join("skills/jev/SKILL.md").is_file(), "{flag:?} didn't write {dir}");
        assert!(!temp.0.join(other).exists(), "{flag:?} also made {other}");
    }
}

#[test]
fn a_second_run_changes_nothing() {
    let temp = Temp::new("again");
    assert!(jev(&temp.0, &["add", "skill"]).status.success());
    let before = fs::read_to_string(temp.0.join(".agents/skills/jev/SKILL.md")).unwrap();

    let again = jev(&temp.0, &["add", "skill"]);
    assert!(again.status.success(), "{}", stderr(&again));
    let said = stdout(&again);
    assert!(said.contains("unchanged .agents/skills/jev/SKILL.md"), "{said}");
    assert!(said.contains("was already there"), "{said}");
    assert_eq!(fs::read_to_string(temp.0.join(".agents/skills/jev/SKILL.md")).unwrap(), before);
}

#[test]
fn an_edited_file_is_refused_rather_than_overwritten() {
    let temp = Temp::new("edited");
    assert!(jev(&temp.0, &["add", "skill"]).status.success());
    let skill = temp.0.join(".agents/skills/jev/SKILL.md");
    fs::write(&skill, "someone's own work").unwrap();

    let again = jev(&temp.0, &["add", "skill"]);
    assert_eq!(again.status.code(), Some(1), "overwriting should have been refused");
    let said = stderr(&again);
    assert!(said.contains(".agents/skills/jev/SKILL.md"), "{said}");
    assert!(said.contains("nothing was written"), "{said}");
    assert!(said.contains("jev add skill --force"), "{said}");
    // The work is still there.
    assert_eq!(fs::read_to_string(&skill).unwrap(), "someone's own work");
}

#[test]
fn force_replaces_and_says_so() {
    let temp = Temp::new("force");
    assert!(jev(&temp.0, &["add", "skill"]).status.success());
    let skill = temp.0.join(".agents/skills/jev/SKILL.md");
    fs::write(&skill, "old version").unwrap();

    let forced = jev(&temp.0, &["add", "skill", "--force"]);
    assert!(forced.status.success(), "{}", stderr(&forced));
    let said = stdout(&forced);
    assert!(said.contains("replaced .agents/skills/jev/SKILL.md"), "{said}");
    // The file that already matched is left alone rather than rewritten.
    assert!(said.contains("unchanged .agents/skills/jev/references/patterns.md"), "{said}");
    assert!(fs::read_to_string(&skill).unwrap().starts_with("---\nname: jev\n"));
}

#[test]
fn it_keeps_the_skills_that_are_already_there() {
    let temp = Temp::new("neighbours");
    let other = temp.0.join(".claude/skills/other");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("SKILL.md"), "not ours").unwrap();

    let done = jev(&temp.0, &["add", "skill", "--claude"]);
    assert!(done.status.success(), "{}", stderr(&done));
    assert_eq!(fs::read_to_string(other.join("SKILL.md")).unwrap(), "not ours");
    let mut skills: Vec<String> =
        fs::read_dir(temp.0.join(".claude/skills")).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    skills.sort();
    assert_eq!(skills, ["jev", "other"]);
}

#[test]
fn a_mistake_says_how_to_call_it_and_exits_1() {
    let temp = Temp::new("mistakes");
    for (argv, expected) in [
        (vec!["add"], "jev add takes `skill`"),
        (vec!["add", "skill", "--vscode"], "unexpected --vscode"),
        (vec!["add", "skill", "--claude", "--agents"], "takes one folder, not two"),
    ] {
        let done = jev(&temp.0, &argv);
        assert_eq!(done.status.code(), Some(1), "{argv:?} should fail");
        let said = stderr(&done);
        assert!(said.contains(expected), "{argv:?}: {said}");
        assert!(said.contains("Usage: jev add skill [--agents|--claude] [--force]"), "{argv:?}: {said}");
    }
    // Nothing was written by any of them.
    assert!(!temp.0.join(".agents").exists() && !temp.0.join(".claude").exists());
}

#[test]
fn only_add_skill_is_intercepted() {
    let temp = Temp::new("intercept");
    // `jev add --noul …` judges the state `add`, as it did before the subcommand existed.
    let asked = jev(&temp.0, &["add", "--noul", "q=Is it?", "--dry-run"]);
    assert!(asked.status.success(), "{}", stderr(&asked));
    let request: serde_json::Value = serde_json::from_str(&stdout(&asked)).unwrap();
    assert_eq!(request["state"], "add");
    // Anything else under `add` is clap's to complain about, which it does in its own way.
    let wrong = jev(&temp.0, &["add", "nonsense"]);
    assert!(!wrong.status.success());
    assert!(stderr(&wrong).contains("nonsense"), "{}", stderr(&wrong));
    assert!(!temp.0.join(".agents").exists());
}

#[test]
fn asking_a_question_still_works() {
    let temp = Temp::new("asking");
    // `add` is intercepted before clap, so a state that starts with something else must still
    // parse as a question — and one that is literally "add" is a mistake, not a state.
    let asked = jev(&temp.0, &["a ticket about a refund", "--noul", "urgent=Is this urgent?", "--dry-run"]);
    assert!(asked.status.success(), "{}", stderr(&asked));
    let request: serde_json::Value = serde_json::from_str(&stdout(&asked)).expect("--dry-run prints JSON");
    assert_eq!(request["state"], "a ticket about a refund");
    assert_eq!(request["questions"]["urgent"]["type"], "noul");
    assert!(!temp.0.join(".agents").exists(), "asking wrote a skill");
}

#[test]
fn the_help_says_the_command_exists() {
    let temp = Temp::new("help");
    let help = stdout(&jev(&temp.0, &["--help"]));
    assert!(help.contains("jev add skill"), "{help}");
}
