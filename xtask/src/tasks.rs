use eyre::{bail, Result};
use serde::Deserialize;
use std::fs;
use std::path::Path;
use std::process::Command;
use tokio::process::Command as TokioCommand;

#[derive(Debug, Deserialize)]
struct Dataflow {
    nodes: Vec<Node>,
}

#[derive(Debug, Deserialize)]
struct Node {
    id: String,
    build: Option<String>,
    path: Option<String>,
}

fn run_command(cmd: &str) {
    let status = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .status()
        .expect("Failed to execute command");
    if !status.success() {
        eprintln!("Command `{}` failed!", cmd);
    }
}

/// 读取 `dataflow.yml` 并解析出所有 `nodes`
fn parse_dataflow() -> Vec<Node> {
    let path = Path::new("dataflow.yml");
    let yaml_str = fs::read_to_string(path).expect("Failed to read dataflow.yml");
    let dataflow: Dataflow = serde_yaml::from_str(&yaml_str).expect("Invalid YAML format");
    dataflow.nodes
}

/// **批量构建所有节点**
pub fn build_all() {
    println!("Building all Dora nodes...");
    let nodes = parse_dataflow();
    for node in nodes {
        if let Some(build_cmd) = node.build {
            println!("Building `{}`...", node.id);
            run_command(&build_cmd);
        }
    }
}

/// **批量运行测试**
pub fn test_all() {
    println!("Running tests...");
    run_command("cargo test");
}

/// **格式化所有代码**
pub fn format() {
    println!("Formatting code...");
    run_command("cargo fmt");
}

pub async fn run(dataflow: &Path) -> eyre::Result<()> {
    build_dataflow(dataflow).await?;
    run_dataflow(dataflow).await?;
    Ok(())
}

/// **运行指定 `node`**
pub fn run_one(node_name: Option<String>) {
    let nodes = parse_dataflow();
    if let Some(node_name) = node_name {
        if let Some(node) = nodes.iter().find(|n| n.id == node_name) {
            if let Some(path) = &node.path {
                println!("Running `{}`...", node_name);
                run_command(&format!("{}", path));
            } else {
                eprintln!("No executable path found for `{}`", node_name);
            }
        } else {
            eprintln!("Node `{}` not found!", node_name);
        }
    } else {
        eprintln!("Please specify a node to run!");
    }
}

/// **构建 Dataflow**
pub async fn build_dataflow(dataflow: &Path) -> Result<()> {
    // let cargo = std::env::var("CARGO").unwrap();
    let mut cmd = TokioCommand::new("dora");
    let path_str = dataflow.to_str().ok_or_else(|| eyre::eyre!("Invalid dataflow path"))?;
    println!("build dataflow path_str: {:?}", path_str);
    cmd.arg("build").arg(path_str);

    // cmd.arg("build").arg(dataflow);
    if !cmd.status().await?.success() {
        bail!("failed to build dataflow");
    };
    Ok(())
}

/// **运行 Dataflow**
pub async fn run_dataflow(dataflow: &Path) -> Result<()> {
    // let cargo = std::env::var("CARGO").unwrap();
    let mut cmd = TokioCommand::new("dora");
    cmd.arg("run").arg(dataflow);
    if !cmd.status().await?.success() {
        bail!("failed to run dataflow");
    };
    Ok(())
}
