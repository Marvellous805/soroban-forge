use crate::cli::LintArgs;
use anyhow::{Context, Result};

pub fn run(args: LintArgs) -> Result<()> {
    let mut cmd = std::process::Command::new("cargo");
    cmd.args(lint_args(args.fix));
    let status = cmd.status().context("failed to run cargo clippy")?;
    if !status.success() {
        std::process::exit(exit_code_for_status(&status));
    }
    Ok(())
}

fn exit_code_for_status(status: &std::process::ExitStatus) -> i32 {
    if status.success() {
        0
    } else {
        1
    }
}

fn lint_args(fix: bool) -> Vec<String> {
    let mut argv = vec![
        "clippy".to_owned(),
        "--workspace".to_owned(),
        "--all-targets".to_owned(),
    ];
    if fix {
        argv.push("--fix".to_owned());
    }
    argv.extend(["--".to_owned(), "-D".to_owned(), "warnings".to_owned()]);
    argv
}

#[cfg(test)]
mod tests {
    use super::{exit_code_for_status, lint_args};

    #[test]
    fn lint_args_without_fix_uses_workspace_and_clippy_warnings_tail() {
        assert_eq!(
            lint_args(false),
            vec![
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings"
            ]
        );
    }

    #[test]
    fn lint_args_with_fix_includes_fix_before_pass_through_flags() {
        assert_eq!(
            lint_args(true),
            vec![
                "clippy",
                "--workspace",
                "--all-targets",
                "--fix",
                "--",
                "-D",
                "warnings",
            ]
        );
    }

    #[test]
    fn failed_lint_status_maps_to_nonzero_exit_code() {
        let status = std::process::Command::new("false").status().unwrap();
        assert!(!status.success());
        assert_eq!(exit_code_for_status(&status), 1);
    }
}
