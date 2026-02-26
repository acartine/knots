use std::io::{self, Write};
use std::path::PathBuf;

use clap::CommandFactory;
use clap_complete::{generate, Shell};

use crate::cli::Cli;

pub fn generate_completions(shell: Shell, buf: &mut dyn Write) {
    let mut cmd = Cli::command();
    generate(shell, &mut cmd, "kno", buf);
}

pub fn detect_current_shell() -> Option<Shell> {
    let shell_var = std::env::var("SHELL").ok()?;
    let basename = shell_var.rsplit('/').next()?;
    match basename {
        "bash" => Some(Shell::Bash),
        "zsh" => Some(Shell::Zsh),
        "fish" => Some(Shell::Fish),
        "elvish" => Some(Shell::Elvish),
        "powershell" | "pwsh" => Some(Shell::PowerShell),
        _ => None,
    }
}

pub fn completions_install_path(shell: Shell) -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let home = PathBuf::from(home);
    match shell {
        Shell::Bash => {
            let dir = home.join(".local/share/bash-completion/completions");
            Some(dir.join("kno"))
        }
        Shell::Zsh => {
            let dir = home.join(".zfunc");
            Some(dir.join("_kno"))
        }
        Shell::Fish => {
            let dir = home.join(".config").join("fish").join("completions");
            Some(dir.join("kno.fish"))
        }
        _ => None,
    }
}

pub fn install_completions(shell: Shell) -> io::Result<PathBuf> {
    let path = completions_install_path(shell).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            format!("no install path for {shell:?}"),
        )
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut buf = Vec::new();
    generate_completions(shell, &mut buf);
    std::fs::write(&path, buf)?;
    Ok(path)
}

fn parse_shell(raw: &str) -> Option<Shell> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "bash" => Some(Shell::Bash),
        "zsh" => Some(Shell::Zsh),
        "fish" => Some(Shell::Fish),
        "elvish" => Some(Shell::Elvish),
        "powershell" | "pwsh" => Some(Shell::PowerShell),
        _ => None,
    }
}

pub fn run_completions_command(
    shell_arg: Option<&str>,
    install: bool,
) -> Result<(), crate::app::AppError> {
    let shell = if let Some(name) = shell_arg {
        parse_shell(name).ok_or_else(|| {
            crate::app::AppError::InvalidArgument(format!("unknown shell '{name}'"))
        })?
    } else {
        detect_current_shell().ok_or_else(|| {
            crate::app::AppError::InvalidArgument(
                "unable to detect shell from $SHELL; pass a shell name".to_string(),
            )
        })?
    };

    if install {
        let path = install_completions(shell)?;
        println!("completions installed to {}", path.display());
        match shell {
            Shell::Zsh => {
                println!(
                    "ensure {} is in your $fpath and run: compinit",
                    path.parent()
                        .map_or("~/.zfunc".into(), |p| { p.display().to_string() })
                );
            }
            Shell::Bash => {
                println!("ensure bash-completion is sourced in your profile");
            }
            _ => {}
        }
    } else {
        let mut stdout = io::stdout().lock();
        generate_completions(shell, &mut stdout);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_shell_from_env() {
        let prev = std::env::var_os("SHELL");
        unsafe { std::env::set_var("SHELL", "/bin/zsh") };
        assert_eq!(detect_current_shell(), Some(Shell::Zsh));
        unsafe { std::env::set_var("SHELL", "/usr/bin/bash") };
        assert_eq!(detect_current_shell(), Some(Shell::Bash));
        unsafe { std::env::set_var("SHELL", "/usr/bin/fish") };
        assert_eq!(detect_current_shell(), Some(Shell::Fish));
        if let Some(val) = prev {
            unsafe { std::env::set_var("SHELL", val) };
        } else {
            unsafe { std::env::remove_var("SHELL") };
        }
    }

    #[test]
    fn completions_install_path_for_known_shells() {
        let prev = std::env::var_os("HOME");
        unsafe { std::env::set_var("HOME", "/tmp/test-home") };
        let bash_path = completions_install_path(Shell::Bash);
        assert!(bash_path.is_some());
        assert!(bash_path
            .as_ref()
            .unwrap()
            .to_str()
            .unwrap()
            .contains("bash-completion"));
        let zsh_path = completions_install_path(Shell::Zsh);
        assert!(zsh_path.is_some());
        assert!(zsh_path
            .as_ref()
            .unwrap()
            .to_str()
            .unwrap()
            .contains("_kno"));
        let fish_path = completions_install_path(Shell::Fish);
        assert!(fish_path.is_some());
        assert!(fish_path
            .as_ref()
            .unwrap()
            .to_str()
            .unwrap()
            .contains("kno.fish"));
        if let Some(val) = prev {
            unsafe { std::env::set_var("HOME", val) };
        } else {
            unsafe { std::env::remove_var("HOME") };
        }
    }

    #[test]
    fn generate_completions_produces_non_empty_output() {
        let mut buf = Vec::new();
        generate_completions(Shell::Bash, &mut buf);
        assert!(!buf.is_empty(), "bash completions should be non-empty");
        let text = String::from_utf8_lossy(&buf);
        assert!(
            text.contains("kno"),
            "bash completions should reference kno"
        );
    }

    #[test]
    fn parse_shell_is_case_insensitive() {
        assert_eq!(parse_shell("BASH"), Some(Shell::Bash));
        assert_eq!(parse_shell("Zsh"), Some(Shell::Zsh));
        assert_eq!(parse_shell("Fish"), Some(Shell::Fish));
        assert_eq!(parse_shell("elvish"), Some(Shell::Elvish));
        assert_eq!(parse_shell("powershell"), Some(Shell::PowerShell));
        assert_eq!(parse_shell("pwsh"), Some(Shell::PowerShell));
        assert_eq!(parse_shell("nonsense"), None);
    }

    #[test]
    fn install_completions_writes_file_to_disk() {
        let prev_home = std::env::var_os("HOME");
        let dir = std::env::temp_dir().join(format!("knots-comp-install-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("dir should be creatable");
        unsafe { std::env::set_var("HOME", &dir) };

        let path = install_completions(Shell::Bash).expect("install should succeed");
        assert!(path.exists(), "completions file should exist");
        let content = std::fs::read_to_string(&path).expect("should read file");
        assert!(content.contains("kno"), "completions should reference kno");

        let zsh_path = install_completions(Shell::Zsh).expect("zsh install should succeed");
        assert!(zsh_path.exists());

        let fish_path = install_completions(Shell::Fish).expect("fish install should succeed");
        assert!(fish_path.exists());

        // Unsupported shell returns error
        let err = install_completions(Shell::Elvish);
        assert!(err.is_err());

        if let Some(val) = prev_home {
            unsafe { std::env::set_var("HOME", val) };
        } else {
            unsafe { std::env::remove_var("HOME") };
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn run_completions_command_print_mode_succeeds() {
        // Print mode with explicit shell
        let result = run_completions_command(Some("bash"), false);
        assert!(result.is_ok());

        // Print mode with auto-detect (SHELL is set in most test environments)
        let result2 = run_completions_command(None, false);
        assert!(result2.is_ok());
    }

    #[test]
    fn run_completions_command_unknown_shell_fails() {
        let result = run_completions_command(Some("nonsense"), false);
        assert!(result.is_err());
    }

    #[test]
    fn run_completions_command_install_mode_succeeds() {
        let prev_home = std::env::var_os("HOME");
        let dir = std::env::temp_dir().join(format!("knots-comp-run-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("dir should be creatable");
        unsafe { std::env::set_var("HOME", &dir) };

        // Install bash completions
        let result = run_completions_command(Some("bash"), true);
        assert!(result.is_ok());

        // Install zsh completions (covers Zsh branch in install hints)
        let result = run_completions_command(Some("zsh"), true);
        assert!(result.is_ok());

        // Install fish completions (covers catch-all branch in install hints)
        let result = run_completions_command(Some("fish"), true);
        assert!(result.is_ok());

        if let Some(val) = prev_home {
            unsafe { std::env::set_var("HOME", val) };
        } else {
            unsafe { std::env::remove_var("HOME") };
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn completions_install_path_returns_none_for_unsupported_shell() {
        assert!(completions_install_path(Shell::Elvish).is_none());
        assert!(completions_install_path(Shell::PowerShell).is_none());
    }

    #[test]
    fn detect_elvish_and_powershell() {
        let prev = std::env::var_os("SHELL");
        unsafe { std::env::set_var("SHELL", "/usr/bin/elvish") };
        assert_eq!(detect_current_shell(), Some(Shell::Elvish));
        unsafe { std::env::set_var("SHELL", "/usr/bin/pwsh") };
        assert_eq!(detect_current_shell(), Some(Shell::PowerShell));
        if let Some(val) = prev {
            unsafe { std::env::set_var("SHELL", val) };
        } else {
            unsafe { std::env::remove_var("SHELL") };
        }
    }
}
