// xtask/src/main.rs
use clap::{Parser, Subcommand};
use std::path::Path;
use std::process::Command;
use tokio::process::Command as TokioCommand;
use eyre::{bail, Context, Result};


mod tasks;

#[derive(Parser)]
#[command(name = "xtask")]
#[command(about = "Dora Agent xtask runner", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Build,
    Test,
    Format,
    Run,
    BuildDataflow,
    RunDataflow,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Starting xtask...");
    dora_tracing::set_up_tracing("rust-dataflow-runner")
        .wrap_err("failed to set up tracing subscriber")?;
    println!("Starting xtask... dora_tracing ");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    println!("CARGO_MANIFEST_DIR: {:?}", root);
    let file_path = Path::new(file!());
    println!("file!: {:?}", file_path);
    println!("Calculated Path: {:?}", root.join(file_path).parent().unwrap());
    std::env::set_current_dir(root)
        .wrap_err("failed to set working dir")?;

    let cli = Cli::parse();
    let dataflow = &root.join("../dataflow.yml");

    match cli.command {
        Commands::Build => tasks::build_all(),
        Commands::Test => tasks::test_all(),
        Commands::Format => tasks::format(),
        Commands::Run => {
            match tasks::run(dataflow).await {
                Ok(_) => println!("run success"),
                Err(e) => println!("run failed: {}", e),
            }
        },
        Commands::BuildDataflow => tasks::build_dataflow(dataflow).await?,
        Commands::RunDataflow => tasks::run_dataflow(dataflow).await?,
    }

    Ok(())
}