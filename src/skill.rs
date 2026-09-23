//! The Agent Skills this crate ships, compiled in: what `jev add skill` writes into a project,
//! and what dx writes into one when it turns jev on.
//!
//! There are two, because there are two ways an agent is given jev and a skill that teaches the
//! wrong one is worse than none. [`Flavour::Command`] teaches the command — `jev -q questions.json
//! -f …`, the shell, `jq` — and [`Flavour::Tool`] teaches the same substance for an agent whose
//! jev is a tool it calls with JSON, which has no shell to run a command line in. Keep the two in
//! step when either changes.
//!
//! They live here rather than in `cli/` so that a crate with only the `command` feature — dx,
//! which offers both shapes — can write them without the CLI's dependencies.

/// Which jev a skill teaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Flavour {
    /// The `jev` command, in a shell. `skills/jev/`.
    #[default]
    Command,
    /// An agent's `jev` tool, called with JSON. `skills/jev-tool/`.
    Tool,
}

/// `cargo build` picks up an edit to any of these.
const COMMAND: [(&str, &str); 2] = [
    ("SKILL.md", include_str!("../skills/jev/SKILL.md")),
    ("references/patterns.md", include_str!("../skills/jev/references/patterns.md")),
];

const TOOL: [(&str, &str); 2] = [
    ("SKILL.md", include_str!("../skills/jev-tool/SKILL.md")),
    ("references/patterns.md", include_str!("../skills/jev-tool/references/patterns.md")),
];

impl Flavour {
    /// The skill's files, each path relative to the skill's own folder.
    ///
    /// Both flavours are named `jev` in their frontmatter and go in a folder called `jev`, so a
    /// project holds one of them at a time: an agent that had both would be told two different
    /// ways to ask, and dx would leave the second out as a skill it already has.
    pub fn files(self) -> [(&'static str, &'static str); 2] {
        match self {
            Flavour::Command => COMMAND,
            Flavour::Tool => TOOL,
        }
    }

    /// The other one: what a project carrying the wrong flavour has in it.
    pub fn other(self) -> Flavour {
        match self {
            Flavour::Command => Flavour::Tool,
            Flavour::Tool => Flavour::Command,
        }
    }
}

/// The folder a skill is written to, under `.agents/skills` or `.claude/skills`. Both flavours
/// share it, since the frontmatter names both `jev`.
pub const FOLDER: &str = "jev";

/// Refuses a destination reached through a symbolic link. Walks what exists of `file` below
/// `root`, since it is the parts already on disk that could redirect a write; std has no way to
/// open a path without following links, so they are found and refused rather than avoided.
///
/// Both writers use it — `jev add skill` and dx, which writes a skill into the project it works
/// on — because `create_dir_all` and `fs::write` follow links, and a checkout carrying
/// `.agents/skills -> /somewhere` would otherwise have the skill written outside the project.
///
/// Not gated to native: dx's writer takes a folder or a `Vfs` and compiles for both, and on
/// `wasm32` there is no file system for a link to point through — `symlink_metadata` says as
/// much and this says yes, which is what the page's files are anyway.
pub fn refuse_symlinks(root: &std::path::Path, file: &std::path::Path) -> Result<(), String> {
    let mut at = root.to_path_buf();
    for part in file.strip_prefix(root).unwrap_or(file) {
        at.push(part);
        let Ok(there) = std::fs::symlink_metadata(&at) else { break };
        if there.file_type().is_symlink() {
            let shown = at.strip_prefix(root).unwrap_or(&at);
            return Err(format!("{} is a symbolic link, so writing there would write outside this folder", shown.display()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frontmatter's `name` has to be the folder's, which the Agent Skills spec requires and
    /// dx's catalog reads. Checked for both, so a rename can't ship broken.
    fn declared_name(skill: &str) -> Option<&str> {
        skill.lines().find_map(|line| line.strip_prefix("name: ").map(str::trim))
    }

    #[test]
    fn both_flavours_are_named_for_the_folder_they_go_in() {
        for flavour in [Flavour::Command, Flavour::Tool] {
            let files = flavour.files();
            assert_eq!(declared_name(files[0].1), Some(FOLDER), "{flavour:?}");
            assert!(!files[1].1.trim().is_empty(), "{flavour:?} has no patterns reference");
        }
    }

    #[test]
    fn the_two_flavours_teach_different_shapes() {
        let command = Flavour::Command.files()[0].1;
        let tool = Flavour::Tool.files()[0].1;
        assert_ne!(command, tool);
        // The command's shape is a command line; the tool's is a call. Each should teach its own
        // and not the other's, which is the whole reason there are two.
        assert!(command.contains("jev -f") && command.contains("jev '"), "the command skill shows no command line");
        assert!(!tool.contains("jev -f") && !tool.contains("jev '"), "the tool skill teaches a command line the tool can't run");
    }

    #[test]
    fn each_flavour_is_the_others_other() {
        assert_eq!(Flavour::Command.other(), Flavour::Tool);
        assert_eq!(Flavour::Tool.other(), Flavour::Command);
        assert_eq!(Flavour::default(), Flavour::Command);
    }
}
