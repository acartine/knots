use clap::{Args, Subcommand, ValueEnum};

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum SkillTargetArg {
    Codex,
    Claude,
    #[value(name = "opencode")]
    OpenCode,
}

#[derive(Debug, Args)]
#[command(about = "Manage Knots-managed agent skills.")]
pub struct SkillsArgs {
    #[command(subcommand)]
    pub command: SkillsSubcommands,
}

#[derive(Debug, Subcommand)]
pub enum SkillsSubcommands {
    #[command(about = "Install missing managed skills for a target tool.")]
    Install(SkillsInstallArgs),
    #[command(about = "Remove managed skills from a target tool.")]
    Uninstall(SkillsUninstallArgs),
    #[command(about = "Refresh installed managed skills for a target tool.")]
    Update(SkillsUpdateArgs),
}

#[derive(Debug, Args)]
pub struct SkillsInstallArgs {
    #[arg(value_enum, help = "Target tool (codex, claude, opencode).")]
    pub target: SkillTargetArg,
}

#[derive(Debug, Args)]
pub struct SkillsUninstallArgs {
    #[arg(value_enum, help = "Target tool (codex, claude, opencode).")]
    pub target: SkillTargetArg,
}

#[derive(Debug, Args)]
pub struct SkillsUpdateArgs {
    #[arg(value_enum, help = "Target tool (codex, claude, opencode).")]
    pub target: SkillTargetArg,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::{Cli, Commands};

    #[test]
    fn skills_install_parses() {
        let cli = Cli::parse_from(["kno", "skills", "install", "codex"]);
        match cli.command {
            Commands::Skills(args) => match args.command {
                super::SkillsSubcommands::Install(install) => {
                    assert_eq!(install.target, super::SkillTargetArg::Codex);
                }
                other => panic!("expected install, got {:?}", other),
            },
            other => panic!("expected skills, got {:?}", other),
        }
    }

    #[test]
    fn skills_update_parses_opencode() {
        let cli = Cli::parse_from(["kno", "skills", "update", "opencode"]);
        match cli.command {
            Commands::Skills(args) => match args.command {
                super::SkillsSubcommands::Update(update) => {
                    assert_eq!(update.target, super::SkillTargetArg::OpenCode);
                }
                other => panic!("expected update, got {:?}", other),
            },
            other => panic!("expected skills, got {:?}", other),
        }
    }
}
