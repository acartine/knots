use std::io::{self, Write};
use std::path::PathBuf;

use clap_complete::{generate, Shell};

pub fn generate_completions(shell: Shell, buf: &mut dyn Write) {
    let mut cmd = crate::cli::styled_command();
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

fn completions_install_path_for_home(shell: Shell, home: &std::path::Path) -> Option<PathBuf> {
    match shell {
        Shell::Bash => {
            let dir = home.join(".local/share/bash-completion/completions");
            Some(dir.join("kno"))
        }
        Shell::Zsh => {
            let dir = home.join(".config/knots/completions");
            Some(dir.join("kno.zsh"))
        }
        Shell::Fish => {
            let dir = home.join(".config/fish/completions");
            Some(dir.join("kno.fish"))
        }
        _ => None,
    }
}

pub fn install_completions(shell: Shell) -> io::Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|e| io::Error::new(io::ErrorKind::NotFound, e))?;
    let home = PathBuf::from(home);

    let path = completions_install_path_for_home(shell, &home).ok_or_else(|| {
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

    if shell == Shell::Zsh {
        patch_zshrc(&home, &path)?;
    }

    Ok(path)
}

fn patch_zshrc(home: &std::path::Path, completions_path: &std::path::Path) -> io::Result<()> {
    let zshrc = home.join(".zshrc");
    let source_line = format!("source \"{}\"", completions_path.display());

    if zshrc.exists() {
        let content = std::fs::read_to_string(&zshrc)?;
        if content.contains(&source_line) {
            return Ok(());
        }
    }

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&zshrc)?;
    writeln!(file)?;
    writeln!(file, "# kno shell completions")?;
    writeln!(file, "{source_line}")?;
    Ok(())
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
        let home = PathBuf::from("/tmp/test-home");
        let bash = completions_install_path_for_home(Shell::Bash, &home);
        assert!(bash.unwrap().to_str().unwrap().contains("bash-completion"));
        let zsh = completions_install_path_for_home(Shell::Zsh, &home);
        assert!(zsh.unwrap().to_str().unwrap().contains("kno.zsh"));
        let fish = completions_install_path_for_home(Shell::Fish, &home);
        assert!(fish.unwrap().to_str().unwrap().contains("kno.fish"));
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
    fn install_and_run_completions_with_zshrc_patching() {
        let prev_home = std::env::var_os("HOME");
        let dir = std::env::temp_dir().join(format!("knots-comp-all-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("dir should be creatable");
        unsafe { std::env::set_var("HOME", &dir) };

        // Bash install writes file
        let path = install_completions(Shell::Bash).expect("bash install should succeed");
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).expect("should read file");
        assert!(content.contains("kno"));

        // Zsh install writes completions file and patches .zshrc
        let zsh_path = install_completions(Shell::Zsh).expect("zsh install should succeed");
        assert!(zsh_path.exists());
        let zshrc = dir.join(".zshrc");
        assert!(zshrc.exists(), ".zshrc should be created");
        let rc_content = std::fs::read_to_string(&zshrc).expect("should read .zshrc");
        assert!(
            rc_content.contains("source"),
            ".zshrc should source completions"
        );
        assert!(
            rc_content.contains("kno.zsh"),
            ".zshrc should reference kno.zsh"
        );

        // Second install is idempotent — no duplicate source line
        install_completions(Shell::Zsh).expect("second zsh install should succeed");
        let rc_after = std::fs::read_to_string(&zshrc).expect("should read .zshrc again");
        assert_eq!(
            rc_content.matches("source").count(),
            rc_after.matches("source").count(),
            "source line should not be duplicated"
        );

        // Fish install
        let fish_path = install_completions(Shell::Fish).expect("fish install should succeed");
        assert!(fish_path.exists());

        // Unsupported shell returns error
        assert!(install_completions(Shell::Elvish).is_err());

        // run_completions_command install mode
        assert!(run_completions_command(Some("bash"), true).is_ok());
        assert!(run_completions_command(Some("zsh"), true).is_ok());
        assert!(run_completions_command(Some("fish"), true).is_ok());

        if let Some(val) = prev_home {
            unsafe { std::env::set_var("HOME", val) };
        } else {
            unsafe { std::env::remove_var("HOME") };
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn run_completions_command_print_and_error_modes() {
        let result = run_completions_command(Some("bash"), false);
        assert!(result.is_ok());
        let result2 = run_completions_command(None, false);
        assert!(result2.is_ok());
        let result3 = run_completions_command(Some("nonsense"), false);
        assert!(result3.is_err());
    }

    #[test]
    fn completions_install_path_returns_none_for_unsupported_shell() {
        let home = PathBuf::from("/tmp/test-home");
        assert!(completions_install_path_for_home(Shell::Elvish, &home).is_none());
        assert!(completions_install_path_for_home(Shell::PowerShell, &home).is_none());
    }

    #[test]
    fn detect_elvish_and_powershell() {
        let prev = std::env::var_os("SHELL");
        unsafe { std::env::set_var("SHELL", "/usr/bin/elvish") };
        assert_eq!(detect_current_shell(), Some(Shell::Elvish));
        unsafe { std::env::set_var("SHELL", "/usr/bin/pwsh") };
        assert_eq!(detect_current_shell(), Some(Shell::PowerShell));
        unsafe { std::env::set_var("SHELL", "/usr/bin/csh") };
        assert_eq!(detect_current_shell(), None);
        if let Some(val) = prev {
            unsafe { std::env::set_var("SHELL", val) };
        } else {
            unsafe { std::env::remove_var("SHELL") };
        }
    }
}
