use crate::cli::EventsArgs;
use anyhow::{Context, Result};

pub fn run(args: EventsArgs) -> Result<()> {
    // For MVP, we'll use the stellar CLI as the transport layer
    let mut cmd = std::process::Command::new("stellar");
    cmd.arg("events").arg("get");

    cmd.arg("--contract-ids").arg(args.contract);
    cmd.arg("--network").arg(args.network);

    if let Some(since) = args.since {
        cmd.arg("--start-ledger").arg(since.to_string());
    }

    if let Some(event_type) = args.event_type {
        // Add type filter - exact format depends on stellar CLI capabilities
        cmd.arg("--type").arg(event_type);
    }

    let status = cmd.status().context("failed to run stellar events get")?;

    if !status.success() {
        std::process::exit(1);
    }

    Ok(())
}
