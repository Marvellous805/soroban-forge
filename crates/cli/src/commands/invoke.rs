use crate::cli::InvokeArgs;
use anyhow::{Context, Result};

pub fn run(args: InvokeArgs) -> Result<()> {
    // For MVP, we'll use the stellar CLI as the transport layer
    let mut cmd = std::process::Command::new("stellar");
    cmd.arg("contract").arg("invoke");
    cmd.arg("--id").arg(args.contract);

    if let Some(source) = args.source {
        cmd.arg("--source-account").arg(source);
    }

    cmd.arg("--network").arg(args.network);

    // Add the function name
    cmd.arg("--").arg(args.function);

    // Parse and add arguments
    for arg in args.args {
        if let Some((key, value)) = arg.split_once('=') {
            cmd.arg(format!("--{}", key)).arg(value);
        } else {
            // Positional argument
            cmd.arg(arg);
        }
    }

    let status = cmd
        .status()
        .context("failed to run stellar contract invoke")?;

    if !status.success() {
        std::process::exit(1);
    }

    Ok(())
}
