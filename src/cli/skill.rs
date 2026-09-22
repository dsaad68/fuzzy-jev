//! `jev add skill`: writes the Agent Skill bundled into this binary under `.agents/skills/jev`, or
//! `.claude/skills/jev` with `--claude`, so an agent in that project learns to use the command.
//!
//! The skill's files are compiled in ([`SKILL`], [`PATTERNS`]), so an installed `jev` carries them
//! and the source tree doesn't have to be there. Existing folders are used as they are, and a file
//! that is already what we'd write is left alone, so running it twice says so rather than churning.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

/// The skill, from `skills/jev/`. `cargo build` picks up an edit to either.
const SKILL: &str = include_str!("../../skills/jev/SKILL.md");
const PATTERNS: &str = include_str!("../../skills/jev/references/patterns.md");

/// The skill's files, under the folder it's written to.
const FILES: [(&str, &str); 2] = [("SKILL.md", SKILL), ("references/patterns.md", PATTERNS)];

/// Which convention's folder the skill goes in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Home {
    /// `.agents/skills/jev`, the default.
    Agents,
    /// `.claude/skills/jev`.
    Claude,
}

impl Home {
    fn dir(self) -> &'static str {
        match self {
            Home::Agents => ".agents",
            Home::Claude => ".claude",
        }
    }
}

/// What writing one file did. Nothing is ever replaced without `--force`, so `Replaced` only
/// happens when it was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrote {
    Created,
    /// Already exactly what we'd write, so it was left alone.
    Unchanged,
    /// It differed and `--force` said to overwrite it.
    Replaced,
}

impl fmt::Display for Wrote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Wrote::Created => "created",
            Wrote::Unchanged => "unchanged",
            Wrote::Replaced => "replaced",
        })
    }
}

/// `jev add skill [--agents|--claude] [--force]`; `None` for any other command line, which clap
/// then parses as a question to ask. Handled before clap because the state is a positional
/// argument, so `add` would otherwise be the state.
pub fn command(words: &[&str]) -> Option<Result<()>> {
    match plan(words)? {
        Ok((home, force)) => Some(add(home, force)),
        Err(error) => Some(Err(error)),
    }
}

/// What a command line asks for, without doing any of it. Apart from `command`, which runs it,
/// this is what the tests use: calling `command` with a valid line would write to whatever folder
/// the test process happens to be in.
fn plan(words: &[&str]) -> Option<Result<(Home, bool)>> {
    let rest = match words {
        ["add", "skill", rest @ ..] => rest,
        // `jev add` alone is worth naming: a state with no questions can't be asked anyway. Any
        // other `add …` goes to clap, so `jev add --noul 'q=?'` still judges the state `add`.
        ["add"] => return Some(Err(usage("jev add takes `skill`"))),
        _ => return None,
    };
    let (mut home, mut force) = (None, false);
    for flag in rest {
        match *flag {
            "--agents" if home.is_none() => home = Some(Home::Agents),
            "--claude" if home.is_none() => home = Some(Home::Claude),
            // Naming the flag here would name a valid one; having both is the mistake.
            "--agents" | "--claude" => return Some(Err(usage("jev add skill takes one folder, not two"))),
            "--force" => force = true,
            other => return Some(Err(usage(&format!("jev add skill: unexpected {other}")))),
        }
    }
    Some(Ok((home.unwrap_or(Home::Agents), force)))
}

fn usage(what: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "{what}\n\nUsage: jev add skill [--agents|--claude] [--force]\n  \
         --agents  write it to .agents/skills/jev (the default)\n  \
         --claude  write it to .claude/skills/jev\n  \
         --force   replace files that are already there and differ"
    )
}

/// Writes the skill into the current folder, and says where it went.
fn add(home: Home, force: bool) -> Result<()> {
    let root = std::env::current_dir().context("can't tell what folder this is")?;
    let (skill, written) = install(&root, home, force)?;
    let shown = skill.strip_prefix(&root).unwrap_or(&skill).to_path_buf();
    for (path, wrote) in &written {
        println!("{wrote} {}", shown.join(path).display());
    }
    if written.iter().all(|(_, wrote)| *wrote == Wrote::Unchanged) {
        println!("The jev skill was already there, as it is now.");
    } else {
        println!("The jev skill is in {}. Agents that read {} will find it.", shown.display(), home.dir());
    }
    Ok(())
}

/// Writes the skill under `root`: `<root>/<.agents|.claude>/skills/jev/`. Folders that are already
/// there are used as they are, and a file that is already what we'd write is left alone.
///
/// A file that is there and differs stops the whole thing unless `force`: someone edited it, or it
/// came from another version, and quietly overwriting it would lose their work. Every file is
/// checked before any is written, so a refusal leaves nothing half-applied.
pub fn install(root: &Path, home: Home, force: bool) -> Result<(PathBuf, Vec<(&'static str, Wrote)>)> {
    let skill = root.join(home.dir()).join("skills").join("jev");
    let planned: Vec<(&'static str, &'static str, Wrote)> = FILES
        .into_iter()
        .map(|(path, contents)| {
            let wrote = match fs::read_to_string(skill.join(path)) {
                Ok(there) if there == contents => Wrote::Unchanged,
                Ok(_) => Wrote::Replaced,
                Err(_) => Wrote::Created,
            };
            (path, contents, wrote)
        })
        .collect();

    let clashes: Vec<String> = planned
        .iter()
        .filter(|(_, _, wrote)| *wrote == Wrote::Replaced)
        .map(|(path, _, _)| skill.strip_prefix(root).unwrap_or(&skill).join(path).display().to_string())
        .collect();
    if !force && !clashes.is_empty() {
        let (subject, pronoun) = match clashes.len() {
            1 => ("This file is", "It is left as it is"),
            _ => ("These files are", "They are left as they are"),
        };
        let folder = match home {
            Home::Agents => "",
            Home::Claude => " --claude",
        };
        bail!(
            "{subject} already there, and not what this jev would write:\n  {}\n\n\
             {pronoun} — nothing was written.\n\n\
             To replace {}:  jev add skill{folder} --force\n\
             Or move the folder aside and run it again.",
            clashes.join("\n  "),
            if clashes.len() == 1 { "it" } else { "them" },
        );
    }

    // Checked before anything is created: create_dir_all and fs::write both follow symbolic
    // links, so a checkout carrying `.agents/skills -> /somewhere` would have us write outside the
    // project, and --force would write over whatever a link points at.
    for (path, _, _) in &planned {
        refuse_symlinks(root, &skill.join(path))?;
    }

    // Nothing is in the way, so the folders can be made and the files written.
    fs::create_dir_all(skill.join("references")).with_context(|| format!("creating {}", skill.display()))?;
    let mut written = Vec::with_capacity(planned.len());
    for (path, contents, wrote) in planned {
        if wrote != Wrote::Unchanged {
            let file = skill.join(path);
            fs::write(&file, contents).with_context(|| format!("writing {}", file.display()))?;
        }
        written.push((path, wrote));
    }
    Ok((skill, written))
}

/// Refuses a destination reached through a symbolic link. Walks what exists of `file` below
/// `root`, since it is the parts already on disk that could redirect a write; std has no way to
/// open a path without following links, so they are found and refused rather than avoided.
fn refuse_symlinks(root: &Path, file: &Path) -> Result<()> {
    let mut at = root.to_path_buf();
    for part in file.strip_prefix(root).unwrap_or(file) {
        at.push(part);
        let Ok(there) = fs::symlink_metadata(&at) else { break };
        if there.file_type().is_symlink() {
            let shown = at.strip_prefix(root).unwrap_or(&at);
            bail!(
                "{} is a symbolic link, so writing there would write outside this folder.\n\n\
                 Nothing was written. Remove the link, or run this somewhere else.",
                shown.display()
            );
        }
    }
    Ok(())
}

/// The skill's own frontmatter has to name it `jev`, since the folder written is `jev` and the
/// Agent Skills spec requires the two to match. Checked here so a rename can't ship broken.
#[cfg(test)]
fn declared_name(skill: &str) -> Option<&str> {
    skill.lines().find_map(|line| line.strip_prefix("name: ").map(str::trim))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of this test's own, removed when it's dropped.
    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Temp {
            let dir = std::env::temp_dir().join(format!("jev-skill-{name}-{}", std::process::id()));
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

    #[test]
    fn reads_the_command_line() {
        // `plan`, not `command`: running a valid line would write into whatever folder the test
        // process is in, which is the crate root.
        assert!(plan(&["a state", "--noul", "q=?"]).is_none());
        assert!(plan(&[]).is_none());
        assert!(plan(&["add-skill"]).is_none());

        let asked = |argv: &[&str]| plan(argv).unwrap().unwrap();
        assert_eq!(asked(&["add", "skill"]), (Home::Agents, false));
        assert_eq!(asked(&["add", "skill", "--agents"]), (Home::Agents, false));
        assert_eq!(asked(&["add", "skill", "--claude"]), (Home::Claude, false));
        // --force goes with either folder, in either order, and twice is not a mistake.
        assert_eq!(asked(&["add", "skill", "--force"]), (Home::Agents, true));
        assert_eq!(asked(&["add", "skill", "--claude", "--force"]), (Home::Claude, true));
        assert_eq!(asked(&["add", "skill", "--force", "--claude"]), (Home::Claude, true));
        assert_eq!(asked(&["add", "skill", "--force", "--force"]), (Home::Agents, true));

        // `jev add` alone is named, since a state with no questions can't be asked anyway. Any
        // other `add …` is left to clap, so a state that happens to be `add` still works.
        assert!(plan(&["add", "--noul", "q=?"]).is_none());
        assert!(plan(&["add", "nonsense"]).is_none());
        let error = |argv: &[&str]| format!("{:#}", plan(argv).unwrap().unwrap_err());
        assert!(error(&["add"]).contains("jev add takes `skill`"));
        assert!(error(&["add", "skill", "--vscode"]).contains("unexpected --vscode"));
        assert!(error(&["add", "skill", "--claude", "--agents"]).contains("takes one folder, not two"));
        // Every usage message says how to call it, --force included.
        assert!(error(&["add"]).contains("Usage: jev add skill [--agents|--claude] [--force]"));
        assert!(error(&["add"]).contains("--force   replace files"));
    }

    #[test]
    fn no_test_writes_into_the_crate() {
        // `command` runs what `plan` reads, so a test calling it with a valid line would write a
        // skill into the folder `cargo test` runs in. This is that mistake, kept from coming back.
        for stray in [".agents", ".claude"] {
            let here = Path::new(env!("CARGO_MANIFEST_DIR")).join(stray);
            assert!(!here.exists(), "{} was written by a test; use plan() or a Temp", here.display());
        }
    }

    #[test]
    fn writes_the_skill_where_the_flag_says() {
        let temp = Temp::new("where");
        for (home, dir) in [(Home::Agents, ".agents"), (Home::Claude, ".claude")] {
            let (skill, written) = install(&temp.0, home, false).unwrap();
            assert_eq!(skill, temp.0.join(dir).join("skills").join("jev"));
            assert!(written.iter().all(|(_, wrote)| *wrote == Wrote::Created));
            assert_eq!(fs::read_to_string(skill.join("SKILL.md")).unwrap(), SKILL);
            assert_eq!(fs::read_to_string(skill.join("references/patterns.md")).unwrap(), PATTERNS);
        }
    }

    #[test]
    fn uses_the_folders_that_are_already_there() {
        let temp = Temp::new("existing");
        // A project that already has .agents/skills, with another skill in it.
        let other = temp.0.join(".agents/skills/other");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("SKILL.md"), "not ours").unwrap();

        let (skill, _) = install(&temp.0, Home::Agents, false).unwrap();
        assert!(skill.exists());
        // The neighbour is untouched, and no second skills folder was made.
        assert_eq!(fs::read_to_string(other.join("SKILL.md")).unwrap(), "not ours");
        let skills: Vec<_> = fs::read_dir(temp.0.join(".agents/skills")).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(skills.len(), 2, "{skills:?}");
    }

    #[test]
    fn a_second_run_writes_nothing() {
        let temp = Temp::new("again");
        let wrote = |temp: &Temp| install(&temp.0, Home::Agents, false).unwrap().1;
        assert!(wrote(&temp).iter().all(|(_, w)| *w == Wrote::Created));
        // Twice over changes nothing, so it says so rather than claiming to have written.
        assert!(wrote(&temp).iter().all(|(_, w)| *w == Wrote::Unchanged));
    }

    #[test]
    fn refuses_to_overwrite_what_is_already_there() {
        let temp = Temp::new("refuses");
        install(&temp.0, Home::Agents, false).unwrap();
        let edited = temp.0.join(".agents/skills/jev/SKILL.md");
        fs::write(&edited, "someone's own work").unwrap();

        let error = format!("{:#}", install(&temp.0, Home::Agents, false).unwrap_err());
        assert!(error.contains(".agents/skills/jev/SKILL.md"), "{error}");
        assert!(error.contains("nothing was written"), "{error}");
        assert!(error.contains("jev add skill --force"), "{error}");
        // Refused means refused: the edit is still there, and so is the file that did match.
        assert_eq!(fs::read_to_string(&edited).unwrap(), "someone's own work");
        assert_eq!(fs::read_to_string(temp.0.join(".agents/skills/jev/references/patterns.md")).unwrap(), PATTERNS);

        // --force is the way through, and says `replaced` for the one it replaced.
        let written = install(&temp.0, Home::Agents, true).unwrap().1;
        assert_eq!(written[0], ("SKILL.md", Wrote::Replaced));
        assert_eq!(written[1], ("references/patterns.md", Wrote::Unchanged));
        assert_eq!(fs::read_to_string(&edited).unwrap(), SKILL);
    }

    #[test]
    fn a_refusal_leaves_nothing_half_written() {
        let temp = Temp::new("half");
        // Only the reference is there, and it differs: the SKILL.md that would have been created
        // must not be, or a refusal would still have changed the folder.
        let skill = temp.0.join(".agents/skills/jev");
        fs::create_dir_all(skill.join("references")).unwrap();
        fs::write(skill.join("references/patterns.md"), "mine").unwrap();

        assert!(install(&temp.0, Home::Agents, false).is_err());
        assert!(!skill.join("SKILL.md").exists(), "a refused run created a file");
        assert_eq!(fs::read_to_string(skill.join("references/patterns.md")).unwrap(), "mine");
    }

    #[test]
    fn the_error_names_every_file_in_the_way() {
        let temp = Temp::new("both");
        let skill = temp.0.join(".claude/skills/jev");
        fs::create_dir_all(skill.join("references")).unwrap();
        fs::write(skill.join("SKILL.md"), "mine").unwrap();
        fs::write(skill.join("references/patterns.md"), "also mine").unwrap();

        let error = format!("{:#}", install(&temp.0, Home::Claude, false).unwrap_err());
        assert!(error.contains("These files are already there"), "{error}");
        assert!(error.contains(".claude/skills/jev/SKILL.md"), "{error}");
        assert!(error.contains(".claude/skills/jev/references/patterns.md"), "{error}");
        // The suggested command carries the folder it was called with.
        assert!(error.contains("jev add skill --claude --force"), "{error}");
    }

    #[test]
    #[cfg(unix)]
    fn refuses_a_destination_reached_through_a_symbolic_link() {
        let temp = Temp::new("symlink");
        let outside = temp.0.join("outside");
        fs::create_dir_all(&outside).unwrap();
        let precious = outside.join("precious");
        fs::write(&precious, "someone else's file").unwrap();
        let project = temp.0.join("project");

        // A folder in the way: writing through it would land outside the project entirely.
        fs::create_dir_all(project.join(".agents")).unwrap();
        std::os::unix::fs::symlink(&outside, project.join(".agents/skills")).unwrap();
        let error = format!("{:#}", install(&project, Home::Agents, false).unwrap_err());
        assert!(error.contains(".agents/skills is a symbolic link"), "{error}");
        assert!(error.contains("Nothing was written"), "{error}");
        assert!(!outside.join("jev").exists(), "the write escaped the project");

        // A file in the way, with --force, which would otherwise overwrite what it points at.
        fs::remove_file(project.join(".agents/skills")).unwrap();
        fs::create_dir_all(project.join(".agents/skills/jev/references")).unwrap();
        std::os::unix::fs::symlink(&precious, project.join(".agents/skills/jev/SKILL.md")).unwrap();
        let error = format!("{:#}", install(&project, Home::Agents, true).unwrap_err());
        assert!(error.contains(".agents/skills/jev/SKILL.md is a symbolic link"), "{error}");
        assert_eq!(fs::read_to_string(&precious).unwrap(), "someone else's file");
    }

    #[test]
    fn the_bundled_skill_is_the_one_the_folder_is_named_after() {
        // The Agent Skills spec makes the folder name and the `name` field match; we write `jev`.
        assert_eq!(declared_name(SKILL), Some("jev"));
        assert!(SKILL.starts_with("---\n"), "SKILL.md needs YAML frontmatter");
        // The skill points at the reference by the path we write it to.
        assert!(SKILL.contains("references/patterns.md"));
        assert!(!PATTERNS.is_empty());
    }
}
