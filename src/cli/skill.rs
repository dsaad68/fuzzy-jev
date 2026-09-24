//! `jev add skill`: writes the Agent Skill bundled into this binary under `.agents/skills/jev`, or
//! `.claude/skills/jev` with `--claude`, so an agent in that project learns to use jev.
//!
//! Which jev it learns is `--tool`: without it the skill teaches this command, and with it the
//! same substance for an agent whose jev is a tool it calls with JSON ([`jev::skill`]). Both go in
//! the folder `jev`, so a project carries one of them.
//!
//! The files are compiled in, so an installed `jev` carries them and the source tree doesn't have
//! to be there. Existing folders are used as they are, and a file that is already what we'd write
//! is left alone, so running it twice says so rather than churning.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use jev::skill::{Flavour, FOLDER};

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
        Ok(asked) => Some(add(asked)),
        Err(error) => Some(Err(error)),
    }
}

/// What a `jev add skill` line asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Asked {
    home: Home,
    flavour: Flavour,
    force: bool,
}

/// What a command line asks for, without doing any of it. Apart from `command`, which runs it,
/// this is what the tests use: calling `command` with a valid line would write to whatever folder
/// the test process happens to be in.
fn plan(words: &[&str]) -> Option<Result<Asked>> {
    let rest = match words {
        ["add", "skill", rest @ ..] => rest,
        // `jev add` alone is worth naming: a state with no questions can't be asked anyway. Any
        // other `add …` goes to clap, so `jev add --noul 'q=?'` still judges the state `add`.
        ["add"] => return Some(Err(usage("jev add takes `skill`"))),
        _ => return None,
    };
    let (mut home, mut flavour, mut force) = (None, Flavour::Command, false);
    for flag in rest {
        match *flag {
            "--agents" if home.is_none() => home = Some(Home::Agents),
            "--claude" if home.is_none() => home = Some(Home::Claude),
            // Naming the flag here would name a valid one; having both is the mistake.
            "--agents" | "--claude" => return Some(Err(usage("jev add skill takes one folder, not two"))),
            "--tool" => flavour = Flavour::Tool,
            "--force" => force = true,
            other => return Some(Err(usage(&format!("jev add skill: unexpected {other}")))),
        }
    }
    Some(Ok(Asked { home: home.unwrap_or(Home::Agents), flavour, force }))
}

fn usage(what: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "{what}\n\nUsage: jev add skill [--agents|--claude] [--tool] [--force]\n  \
         --agents  write it to .agents/skills/jev (the default)\n  \
         --claude  write it to .claude/skills/jev\n  \
         --tool    teach an agent whose jev is a tool it calls, rather than this command\n  \
         --force   replace files that are already there and differ"
    )
}

/// Writes the skill into the current folder, and says where it went.
fn add(asked: Asked) -> Result<()> {
    let Asked { home, flavour, force } = asked;
    let root = std::env::current_dir().context("can't tell what folder this is")?;
    let (skill, written) = install(&root, home, flavour, force)?;
    let shown = skill.strip_prefix(&root).unwrap_or(&skill).to_path_buf();
    for (path, wrote) in &written {
        println!("{wrote} {}", shown.join(path).display());
    }
    let teaches = match flavour {
        Flavour::Command => "the jev command",
        Flavour::Tool => "a jev tool",
    };
    if written.iter().all(|(_, wrote)| *wrote == Wrote::Unchanged) {
        println!("The jev skill was already there, as it is now.");
    } else {
        println!("The jev skill ({teaches}) is in {}. Agents that read {} will find it.", shown.display(), home.dir());
    }
    Ok(())
}

/// Writes the skill under `root`: `<root>/<.agents|.claude>/skills/jev/`. Folders that are already
/// there are used as they are, and a file that is already what we'd write is left alone.
///
/// A file that is there and differs stops the whole thing unless `force`: someone edited it, or it
/// came from another version, and quietly overwriting it would lose their work. Every file is
/// checked before any is written, so a refusal leaves nothing half-applied.
pub fn install(root: &Path, home: Home, flavour: Flavour, force: bool) -> Result<(PathBuf, Vec<(&'static str, Wrote)>)> {
    let skill = root.join(home.dir()).join("skills").join(FOLDER);
    let mut planned: Vec<(&'static str, &'static str, Wrote)> = Vec::new();
    for (path, contents) in flavour.files() {
        // Bytes, not text: a file there that isn't UTF-8 is still someone's file. Only a file that
        // isn't there at all is created; one that can't be read stops it, since it may be in the way.
        let file = skill.join(path);
        let wrote = match fs::read(&file) {
            Ok(there) if there == contents.as_bytes() => Wrote::Unchanged,
            Ok(_) => Wrote::Replaced,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Wrote::Created,
            Err(error) => return Err(error).with_context(|| format!("reading {}; nothing was written", file.display())),
        };
        planned.push((path, contents, wrote));
    }

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
        let shape = match flavour {
            Flavour::Command => "",
            Flavour::Tool => " --tool",
        };
        bail!(
            "{subject} already there, and not what this jev would write:\n  {}\n\n\
             {pronoun} — nothing was written.\n\n\
             To replace {}:  jev add skill{folder}{shape} --force\n\
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

/// Refuses a destination reached through a symbolic link ([`jev::skill::refuse_symlinks`]), with
/// what to do about it: the check is the library's, since dx writes these files too.
fn refuse_symlinks(root: &Path, file: &Path) -> Result<()> {
    jev::skill::refuse_symlinks(root, file)
        .map_err(|why| anyhow::anyhow!("{why}.\n\nNothing was written. Remove the link, or run this somewhere else."))
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
        let plain = Asked { home: Home::Agents, flavour: Flavour::Command, force: false };
        assert_eq!(asked(&["add", "skill"]), plain);
        assert_eq!(asked(&["add", "skill", "--agents"]), plain);
        assert_eq!(asked(&["add", "skill", "--claude"]), Asked { home: Home::Claude, ..plain });
        // --force goes with either folder, in either order, and twice is not a mistake.
        assert_eq!(asked(&["add", "skill", "--force"]), Asked { force: true, ..plain });
        assert_eq!(asked(&["add", "skill", "--claude", "--force"]), Asked { home: Home::Claude, force: true, ..plain });
        assert_eq!(asked(&["add", "skill", "--force", "--claude"]), Asked { home: Home::Claude, force: true, ..plain });
        assert_eq!(asked(&["add", "skill", "--force", "--force"]), Asked { force: true, ..plain });
        // --tool picks the other skill, and goes with the rest in any order.
        assert_eq!(asked(&["add", "skill", "--tool"]), Asked { flavour: Flavour::Tool, ..plain });
        assert_eq!(
            asked(&["add", "skill", "--tool", "--claude", "--force"]),
            Asked { home: Home::Claude, flavour: Flavour::Tool, force: true }
        );

        // `jev add` alone is named, since a state with no questions can't be asked anyway. Any
        // other `add …` is left to clap, so a state that happens to be `add` still works.
        assert!(plan(&["add", "--noul", "q=?"]).is_none());
        assert!(plan(&["add", "nonsense"]).is_none());
        let error = |argv: &[&str]| format!("{:#}", plan(argv).unwrap().unwrap_err());
        assert!(error(&["add"]).contains("jev add takes `skill`"));
        assert!(error(&["add", "skill", "--vscode"]).contains("unexpected --vscode"));
        assert!(error(&["add", "skill", "--claude", "--agents"]).contains("takes one folder, not two"));
        // Every usage message says how to call it, --force included.
        assert!(error(&["add"]).contains("Usage: jev add skill [--agents|--claude] [--tool] [--force]"));
        assert!(error(&["add"]).contains("--force   replace files"));
        assert!(error(&["add"]).contains("--tool    teach an agent whose jev is a tool"));
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
            let (skill, written) = install(&temp.0, home, Flavour::Command, false).unwrap();
            assert_eq!(skill, temp.0.join(dir).join("skills").join("jev"));
            assert!(written.iter().all(|(_, wrote)| *wrote == Wrote::Created));
            assert_eq!(fs::read_to_string(skill.join("SKILL.md")).unwrap(), Flavour::Command.files()[0].1);
            assert_eq!(fs::read_to_string(skill.join("references/patterns.md")).unwrap(), Flavour::Command.files()[1].1);
        }
    }

    #[test]
    fn uses_the_folders_that_are_already_there() {
        let temp = Temp::new("existing");
        // A project that already has .agents/skills, with another skill in it.
        let other = temp.0.join(".agents/skills/other");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("SKILL.md"), "not ours").unwrap();

        let (skill, _) = install(&temp.0, Home::Agents, Flavour::Command, false).unwrap();
        assert!(skill.exists());
        // The neighbour is untouched, and no second skills folder was made.
        assert_eq!(fs::read_to_string(other.join("SKILL.md")).unwrap(), "not ours");
        let skills: Vec<_> = fs::read_dir(temp.0.join(".agents/skills")).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(skills.len(), 2, "{skills:?}");
    }

    #[test]
    fn a_second_run_writes_nothing() {
        let temp = Temp::new("again");
        let wrote = |temp: &Temp| install(&temp.0, Home::Agents, Flavour::Command, false).unwrap().1;
        assert!(wrote(&temp).iter().all(|(_, w)| *w == Wrote::Created));
        // Twice over changes nothing, so it says so rather than claiming to have written.
        assert!(wrote(&temp).iter().all(|(_, w)| *w == Wrote::Unchanged));
    }

    #[test]
    fn refuses_to_overwrite_what_is_already_there() {
        let temp = Temp::new("refuses");
        install(&temp.0, Home::Agents, Flavour::Command, false).unwrap();
        let edited = temp.0.join(".agents/skills/jev/SKILL.md");
        fs::write(&edited, "someone's own work").unwrap();

        let error = format!("{:#}", install(&temp.0, Home::Agents, Flavour::Command, false).unwrap_err());
        assert!(error.contains(".agents/skills/jev/SKILL.md"), "{error}");
        assert!(error.contains("nothing was written"), "{error}");
        assert!(error.contains("jev add skill --force"), "{error}");
        // Refused means refused: the edit is still there, and so is the file that did match.
        assert_eq!(fs::read_to_string(&edited).unwrap(), "someone's own work");
        assert_eq!(fs::read_to_string(temp.0.join(".agents/skills/jev/references/patterns.md")).unwrap(), Flavour::Command.files()[1].1);

        // --force is the way through, and says `replaced` for the one it replaced.
        let written = install(&temp.0, Home::Agents, Flavour::Command, true).unwrap().1;
        assert_eq!(written[0], ("SKILL.md", Wrote::Replaced));
        assert_eq!(written[1], ("references/patterns.md", Wrote::Unchanged));
        assert_eq!(fs::read_to_string(&edited).unwrap(), Flavour::Command.files()[0].1);
    }

    #[test]
    fn a_file_that_isnt_text_is_still_in_the_way() {
        let temp = Temp::new("binary");
        let skill = temp.0.join(".agents/skills/jev");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), [0xff, 0xfe, 0x00]).unwrap();
        let error = format!("{:#}", install(&temp.0, Home::Agents, Flavour::Command, false).unwrap_err());
        assert!(error.contains("nothing was written"), "{error}");
        assert_eq!(fs::read(skill.join("SKILL.md")).unwrap(), [0xff, 0xfe, 0x00]);
    }

    #[test]
    fn a_file_that_cant_be_read_stops_it() {
        // A folder where the file should be can't be read as one, and isn't absent either.
        let temp = Temp::new("unreadable");
        let skill = temp.0.join(".agents/skills/jev");
        fs::create_dir_all(skill.join("SKILL.md")).unwrap();
        let error = format!("{:#}", install(&temp.0, Home::Agents, Flavour::Command, true).unwrap_err());
        assert!(error.contains("reading") && error.contains("nothing was written"), "{error}");
        assert!(!skill.join("references/patterns.md").exists());
    }

    #[test]
    fn a_refusal_leaves_nothing_half_written() {
        let temp = Temp::new("half");
        // Only the reference is there, and it differs: the SKILL.md that would have been created
        // must not be, or a refusal would still have changed the folder.
        let skill = temp.0.join(".agents/skills/jev");
        fs::create_dir_all(skill.join("references")).unwrap();
        fs::write(skill.join("references/patterns.md"), "mine").unwrap();

        assert!(install(&temp.0, Home::Agents, Flavour::Command, false).is_err());
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

        let error = format!("{:#}", install(&temp.0, Home::Claude, Flavour::Command, false).unwrap_err());
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
        let error = format!("{:#}", install(&project, Home::Agents, Flavour::Command, false).unwrap_err());
        assert!(error.contains(".agents/skills is a symbolic link"), "{error}");
        assert!(error.contains("Nothing was written"), "{error}");
        assert!(!outside.join("jev").exists(), "the write escaped the project");

        // A file in the way, with --force, which would otherwise overwrite what it points at.
        fs::remove_file(project.join(".agents/skills")).unwrap();
        fs::create_dir_all(project.join(".agents/skills/jev/references")).unwrap();
        std::os::unix::fs::symlink(&precious, project.join(".agents/skills/jev/SKILL.md")).unwrap();
        let error = format!("{:#}", install(&project, Home::Agents, Flavour::Command, true).unwrap_err());
        assert!(error.contains(".agents/skills/jev/SKILL.md is a symbolic link"), "{error}");
        assert_eq!(fs::read_to_string(&precious).unwrap(), "someone else's file");
    }

    #[test]
    fn either_flavour_can_be_written() {
        // What each teaches is jev::skill's business; this is that both arrive whole. They share
        // the folder, so writing the second over the first is what switching shape does.
        let temp = Temp::new("flavours");
        let (skill, _) = install(&temp.0, Home::Agents, Flavour::Command, false).unwrap();
        assert_eq!(fs::read_to_string(skill.join("SKILL.md")).unwrap(), Flavour::Command.files()[0].1);
        let (again, written) = install(&temp.0, Home::Agents, Flavour::Tool, true).unwrap();
        assert_eq!(again, skill);
        assert!(written.iter().any(|(_, wrote)| *wrote == Wrote::Replaced));
        assert_eq!(fs::read_to_string(skill.join("SKILL.md")).unwrap(), Flavour::Tool.files()[0].1);
    }

    #[test]
    fn the_other_flavour_is_in_the_way_without_force() {
        // Switching shape replaces a file that differs, which is what --force is for: the same
        // check that protects an edited skill protects one the other flavour wrote.
        let temp = Temp::new("switch");
        install(&temp.0, Home::Agents, Flavour::Command, false).unwrap();
        let error = format!("{:#}", install(&temp.0, Home::Agents, Flavour::Tool, false).unwrap_err());
        assert!(error.contains("jev add skill --tool --force"), "{error}");
    }
}
