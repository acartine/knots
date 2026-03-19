use clap::{Args, Subcommand, ValueEnum};

#[derive(Debug, Args)]
#[command(about = "Run Loom compatibility checks against Knots workflows.")]
pub struct LoomArgs {
    #[command(subcommand)]
    pub command: LoomSubcommands,
}

#[derive(Debug, Subcommand)]
pub enum LoomSubcommands {
    #[command(about = "Exercise the Loom to Knots compatibility contract.")]
    CompatTest(CompatTestArgs),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum CompatTestMode {
    Smoke,
    Matrix,
}

#[derive(Debug, Args)]
pub struct CompatTestArgs {
    #[arg(
        long,
        default_value = "knots_sdlc",
        help = "Loom template name to initialize for smoke mode."
    )]
    pub template: String,

    #[arg(
        long,
        value_enum,
        default_value_t = CompatTestMode::Smoke,
        help = "Compatibility coverage mode."
    )]
    pub mode: CompatTestMode,

    #[arg(
        long,
        help = "Keep generated test artifacts instead of cleaning them up."
    )]
    pub keep_artifacts: bool,

    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::{Cli, Commands};
    use crate::cli_loom::{CompatTestMode, LoomSubcommands};

    #[test]
    fn loom_compat_test_parses_default_shape() {
        let cli = Cli::parse_from(["kno", "loom", "compat-test"]);
        match cli.command {
            Commands::Loom(args) => match args.command {
                LoomSubcommands::CompatTest(compat) => {
                    assert_eq!(compat.template, "knots_sdlc");
                    assert_eq!(compat.mode, CompatTestMode::Smoke);
                    assert!(!compat.keep_artifacts);
                    assert!(!compat.json);
                }
            },
            other => panic!("expected Loom, got {:?}", other),
        }
    }

    #[test]
    fn loom_compat_test_parses_all_flags() {
        let cli = Cli::parse_from([
            "kno",
            "loom",
            "compat-test",
            "--template",
            "custom_flow",
            "--mode",
            "matrix",
            "--keep-artifacts",
            "--json",
        ]);
        match cli.command {
            Commands::Loom(args) => match args.command {
                LoomSubcommands::CompatTest(compat) => {
                    assert_eq!(compat.template, "custom_flow");
                    assert_eq!(compat.mode, CompatTestMode::Matrix);
                    assert!(compat.keep_artifacts);
                    assert!(compat.json);
                }
            },
            other => panic!("expected Loom, got {:?}", other),
        }
    }
}
