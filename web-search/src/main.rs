mod tools;

use dora_node_api::{self, DoraNode, Event, IntoArrow};
use dora_node_api::dora_core::config::DataId;
use eyre::{bail, Context};
use rig::completion::Prompt;
use rig::providers;
use serde_json::json;
use common::NodeDescriptor;
use crate::tools::web_search::{SearchResult, SearchWebArgs};
#[tokio::main]
async fn main() -> eyre::Result<()> {
    println!("🚀 start web-search");
    let (mut node, mut events) = DoraNode::init_from_env()?;
    println!("🔍 web-search started");
    node.send_output(DataId::from("register".to_string()), Default::default(), NodeDescriptor{
        id: "web_search".to_string(),
        description: "使用浏览器执行搜索，并解析搜索结果".to_string(),
        inputs: serde_json::to_string_pretty(&schemars::schema_for!(SearchWebArgs)).unwrap(),
        outputs: serde_json::to_string_pretty(&schemars::schema_for!(SearchResult)).unwrap(),
    }.into_arrow())?;
    println!("🔍 web-search registered");
    println!("sink received init message");
    while let Some(event) = events.recv() {
        match event {
            Event::Input {
                id,
                metadata,
                data,
            } => match id.as_str() {
                "query" => {
                    let received_string: &str =
                        TryFrom::try_from(&data).context("expected string message")?;
                    println!("sink received message: {}", received_string);
                    if !received_string.starts_with("operator received random value ") {
                        bail!("unexpected message format (should start with 'operator received random value')")
                    }
                    if !received_string.ends_with(" ticks") {
                        bail!("unexpected message format (should end with 'ticks')")
                    }

                    let openai_client = providers::ollama::Client::new();

                    let agent = openai_client
                        .agent("qwen2.5-coder:14b")
                        .preamble("你是一个搜索助手，可以使用 search_web 工具来执行搜索任务,你应该判断使用 search_web 并将 click 设置为 true，否则不点击。")
                        .max_tokens(1024)
                        .tool(crate::tools::web_search::SearchWebTool)
                        .build();

                    println!("查询 Rust async runtime 的最新信息");
                    println!(
                        "AI 搜索助手: {}",
                        agent.prompt("查询 Rust async runtime 的最新信息").await?
                    );

                }
                "init" => {
                    println!("🔍 web-search started");
                    node.send_output(DataId::from("register".to_string()), metadata.parameters, NodeDescriptor{
                        id: "web_search".to_string(),
                        description: "使用浏览器执行搜索，并解析搜索结果".to_string(),
                        inputs: serde_json::to_string_pretty(&schemars::schema_for!(SearchWebArgs)).unwrap(),
                        outputs: serde_json::to_string_pretty(&schemars::schema_for!(SearchResult)).unwrap(),
                    }.into_arrow())?;
                    println!("🔍 web-search registered");
                    println!("sink received init message");
                }
                other => eprintln!("Ignoring unexpected input `{other}`"),
            },
            Event::Stop => {
                println!("Received manual stop");
            }
            Event::InputClosed { id } => {
                println!("Input `{id}` was closed");
            }
            other => eprintln!("Received unexpected input: {other:?}"),
        }
    }

    Ok(())
}
