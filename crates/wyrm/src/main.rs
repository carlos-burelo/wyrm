mod cli;
mod daemon;
mod deploy;
mod ecosystem;
mod ipc;
mod logs;
mod runtime;
mod store;
mod tui;

use clap::Parser;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = cli::Cli::parse();

    if cli.daemon {
        return crate::runtime::service::start_service_dispatcher().map_err(|e| e.into());
    }

    cli::run(cli).await
}
