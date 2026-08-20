use std::path::PathBuf;
use std::time::Duration;
use std::{io, io::ErrorKind};

use clap::{Args, Parser, Subcommand};

use crate::auth::read_token;
use crate::runner::resolve_default_kno_bin;
use crate::server::{HttpConfig, ServerConfig};

#[derive(Debug, Parser)]
#[command(name = "kno-mcp")]
#[command(version)]
#[command(about = "Expose a Knots checkout through the Model Context Protocol")]
pub struct Cli {
    #[arg(
        short = 'C',
        long = "repo",
        global = true,
        help = "Knots repository root"
    )]
    pub repo: Option<PathBuf>,

    #[arg(long = "kno-bin", global = true, help = "Path to the kno binary")]
    pub kno_bin: Option<PathBuf>,

    #[arg(
        long = "lease-timeout-seconds",
        global = true,
        default_value_t = 600,
        help = "Seconds before MCP-created leases expire"
    )]
    pub lease_timeout_seconds: u64,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    #[command(about = "Serve MCP over stdio")]
    Stdio,
    #[command(about = "Serve MCP over Streamable HTTP")]
    Serve(ServeArgs),
    #[command(about = "Print the kno-mcp version")]
    Version,
}

#[derive(Debug, Args)]
pub struct ServeArgs {
    #[arg(long, default_value = "127.0.0.1:7777", help = "HTTP bind address")]
    pub bind: String,

    #[arg(long = "token-file", help = "File containing the bearer token")]
    pub token_file: Option<PathBuf>,

    #[arg(long = "token-env", default_value = "KNOTS_MCP_TOKEN")]
    pub token_env: String,

    #[arg(long = "sync-interval-seconds", default_value_t = 15)]
    pub sync_interval_seconds: u64,
}

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub mode: CommandMode,
    pub server: ServerConfig,
}

#[derive(Debug, Clone)]
pub enum CommandMode {
    Stdio,
    Serve(HttpConfig),
    Version,
}

impl Cli {
    pub fn into_config(self) -> Result<RuntimeConfig, std::io::Error> {
        let command = self.command.unwrap_or(Commands::Stdio);
        let repo = match command {
            Commands::Version => self.repo.unwrap_or_default(),
            _ => self.repo.ok_or_else(|| {
                io::Error::new(
                    ErrorKind::InvalidInput,
                    "--repo is required unless using version",
                )
            })?,
        };
        let server = ServerConfig {
            repo,
            kno_bin: self.kno_bin.unwrap_or_else(resolve_default_kno_bin),
            lease_timeout_seconds: self.lease_timeout_seconds,
        };
        let mode = match command {
            Commands::Stdio => CommandMode::Stdio,
            Commands::Version => CommandMode::Version,
            Commands::Serve(args) => {
                let token = read_token(args.token_file.as_deref(), &args.token_env)?;
                if token.is_empty() {
                    return Err(io::Error::new(
                        ErrorKind::InvalidInput,
                        "serve requires a bearer token from --token-file or --token-env",
                    ));
                }
                CommandMode::Serve(HttpConfig {
                    bind: args.bind,
                    token,
                    sync_interval: Duration::from_secs(args.sync_interval_seconds),
                })
            }
        };
        Ok(RuntimeConfig { mode, server })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::fs;

    #[test]
    fn defaults_to_stdio() {
        let cli = Cli::parse_from(["kno-mcp", "--repo", "/example/repo"]);
        let config = cli.into_config().expect("config should parse");
        assert!(matches!(config.mode, CommandMode::Stdio));
        assert_eq!(config.server.repo, PathBuf::from("/example/repo"));
    }

    #[test]
    fn version_does_not_require_repo() {
        let cli = Cli::parse_from(["kno-mcp", "version"]);
        let config = cli.into_config().expect("version config should parse");
        assert!(matches!(config.mode, CommandMode::Version));
        assert_eq!(config.server.repo, PathBuf::new());
    }

    #[test]
    fn non_version_requires_repo() {
        let cli = Cli::parse_from(["kno-mcp", "stdio"]);
        let err = cli.into_config().expect_err("repo should be required");
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
        assert!(err.to_string().contains("--repo is required"));
    }

    #[test]
    fn serve_reads_token_file_and_interval() {
        let token_ws = knots_test_support::workspace("knots-kno-mcp-token-serve");
        let token_file = token_ws.path().join("token");
        fs::write(&token_file, " secret \n").expect("write token file");

        let cli = Cli::parse_from([
            "kno-mcp",
            "--repo",
            "/example/repo",
            "serve",
            "--bind",
            "127.0.0.1:8888",
            "--token-file",
            token_file.to_str().expect("utf8 temp path"),
            "--sync-interval-seconds",
            "3",
        ]);
        let config = cli.into_config().expect("serve config should parse");
        let _ = fs::remove_file(token_file);

        match config.mode {
            CommandMode::Serve(http) => {
                assert_eq!(http.bind, "127.0.0.1:8888");
                assert_eq!(http.token, "secret");
                assert_eq!(http.sync_interval, Duration::from_secs(3));
            }
            _ => panic!("expected serve mode"),
        }
    }

    #[test]
    fn serve_rejects_missing_token() {
        let cli = Cli::parse_from([
            "kno-mcp",
            "--repo",
            "/example/repo",
            "serve",
            "--token-env",
            "KNO_MCP_TEST_EMPTY_TOKEN",
        ]);
        let err = cli.into_config().expect_err("missing token should fail");
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
        assert!(err.to_string().contains("requires a bearer token"));
    }
}
