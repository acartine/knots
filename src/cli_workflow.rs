use clap::{Args, Subcommand};

#[derive(Debug, Args)]
#[command(about = "Inspect and manage installed workflow bundles.")]
pub struct WorkflowArgs {
    #[command(subcommand)]
    pub command: WorkflowSubcommands,
}

#[derive(Debug, Subcommand)]
pub enum WorkflowSubcommands {
    #[command(about = "List installed workflow bundles.", alias = "ls")]
    List(WorkflowListArgs),
    #[command(about = "Show one installed workflow bundle.")]
    Show(WorkflowShowArgs),
    #[command(about = "Install a Loom-produced workflow bundle.")]
    Install(WorkflowInstallArgs),
    #[command(about = "Select the current repository workflow.")]
    Use(WorkflowUseArgs),
    #[command(about = "Show the current repository workflow.")]
    Current(WorkflowCurrentArgs),
}

#[derive(Debug, Args)]
pub struct WorkflowListArgs {
    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct WorkflowShowArgs {
    #[arg(help = "Workflow id.")]
    pub id: String,

    #[arg(short = 'v', long, help = "Workflow version (defaults to latest).")]
    pub version: Option<u32>,

    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct WorkflowInstallArgs {
    #[arg(help = "Bundle JSON file or workflow directory.")]
    pub source: std::path::PathBuf,

    #[arg(
        long = "set-default",
        value_name = "BOOL",
        help = "Whether to switch to the installed workflow (yes|true|1|no|false|0)."
    )]
    pub set_default: Option<String>,
}

#[derive(Debug, Args)]
pub struct WorkflowUseArgs {
    #[arg(help = "Workflow id.")]
    pub id: String,

    #[arg(short = 'v', long, help = "Workflow version (defaults to latest).")]
    pub version: Option<u32>,

    #[arg(
        short = 'p',
        long = "profile",
        help = "Profile id (defaults to bundle default)."
    )]
    pub profile: Option<String>,
}

#[derive(Debug, Args)]
pub struct WorkflowCurrentArgs {
    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::{Cli, Commands};
    use crate::cli_workflow::WorkflowSubcommands;

    #[test]
    fn workflow_install_parses() {
        let cli = Cli::parse_from([
            "kno",
            "workflow",
            "install",
            "/tmp/bundle.json",
            "--set-default=true",
        ]);
        match cli.command {
            Commands::Workflow(args) => match args.command {
                WorkflowSubcommands::Install(install) => {
                    assert_eq!(install.source, std::path::PathBuf::from("/tmp/bundle.json"));
                    assert_eq!(install.set_default.as_deref(), Some("true"));
                }
                other => panic!("expected install, got {:?}", other),
            },
            other => panic!("expected workflow, got {:?}", other),
        }
    }
}
