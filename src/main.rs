use anyhow::Result;
use clap::{Parser, Subcommand};
use smart_relay::{ai, api, config::ConfigStore, telemetry::install_tracing};

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    /// Path to config file; defaults to platform config dir
    #[arg(short, long)]
    config: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run HTTP control plane for automation/UI
    Serve {
        #[arg(short, long, default_value = "127.0.0.1:8080")]
        listen: String,
    },
    /// Ask the AI to propose a connection profile
    Propose {
        #[arg(short, long)]
        goal: Option<String>,
    },
    /// Validate current configuration with lightweight checks
    Validate,
}

#[tokio::main]
async fn main() -> Result<()> {
    install_tracing();

    let cli = Cli::parse();
    let store = ConfigStore::load(cli.config.as_deref()).await?;
    let ai = ai::AiClient::default();

    match cli.command {
        Command::Serve { listen } => api::serve(listen, store, ai).await?,
        Command::Propose { goal } => {
            let goal = goal.unwrap_or_else(|| "low-latency global access with privacy".to_string());
            let plan = ai.propose_profile(&goal, None).await?;
            println!("{}", plan);
        }
        Command::Validate => {
            store.validate()?;
            println!("Configuration looks valid");
        }
    }

    Ok(())
}

