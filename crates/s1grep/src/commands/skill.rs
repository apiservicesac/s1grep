use std::path::PathBuf;

use anyhow::Context;
use clap::Args;

use crate::settings::AgentSettings;

#[derive(Args)]
pub struct SkillCommand {
    /// Install it for Claude Code for this user (~/.claude/skills/s1grep)
    #[arg(long, conflicts_with = "project")]
    install: bool,
    /// Install it in this project only (.claude/skills/s1grep)
    #[arg(long)]
    project: bool,
}

impl SkillCommand {
    /// Prints the agent skill, or installs it, and shows how to add the MCP server to common agents.
    pub fn run(self) -> anyhow::Result<()> {
        let target = if self.install {
            Some(Self::home()?.join(".claude").join("skills").join("s1grep"))
        } else if self.project {
            Some(PathBuf::from(".claude").join("skills").join("s1grep"))
        } else {
            None
        };
        let Some(folder) = target else {
            print!("{}", AgentSettings::SKILL);
            eprintln!("\n(printed only; `s1grep skill --install` installs it for Claude Code)");
            return Ok(());
        };
        std::fs::create_dir_all(&folder).with_context(|| format!("creating {}", folder.display()))?;
        let file = folder.join("SKILL.md");
        std::fs::write(&file, AgentSettings::SKILL).with_context(|| format!("writing {}", file.display()))?;
        eprintln!("Skill installed in {}.", file.display());
        eprintln!();
        eprintln!("To give agents the search_code tool through MCP as well:");
        eprintln!("  Claude Code   claude mcp add s1grep -- s1grep mcp");
        eprintln!("  Codex         codex mcp add s1grep -- s1grep mcp");
        eprintln!("  Others        command `s1grep`, arguments `mcp`, transport stdio");
        Ok(())
    }

    fn home() -> anyhow::Result<PathBuf> {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .context("cannot find the home folder")
    }
}
