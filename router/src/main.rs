use crate::register::RouterNode;
use anyhow::Result;
use common::NodeDescriptor;
use dora_node_api::dora_core::config::DataId;
use dora_node_api::{ArrowData, DoraNode, Event, IntoArrow};
use rig::agent::Agent;
use rig::completion::{Chat, Completion, CompletionModel, Prompt};
use rig::{providers, tool::Tool};
use std::sync::{Arc, Mutex};

mod register;
mod tools;

/// **`RouterApp` 负责 `DoraNode` 生命周期**
pub struct RouterApp {
    router: Arc<RouterNode>,
    agent: Agent<rig::providers::ollama::CompletionModel>,
}

impl RouterApp {
    pub fn new() -> Result<Self> {
        let router = Arc::new(RouterNode::new());

        // ✅ **初始化 `LLM`，让它管理 `Dora` 数据流**
        let openai_client = providers::ollama::Client::new();
        let agent = openai_client
            .agent("qwen2.5-coder:14b")
            .preamble("你是 `Dora` 数据流控制器，根据 `NodeDescriptor` 选择最优数据流路径。")
            .tool(tools::GetNodes {
                router: Arc::clone(&router),
            })
            .tool(tools::SendData {
                router: Arc::clone(&router),
            })
            .build();

        Ok(Self { router, agent })
    }

    pub async fn run(&self) -> Result<()> {
        println!("🛰️ `RouterApp` 启动...");

        let (mut node, mut events) = DoraNode::init_from_env().unwrap(); // ✅ `RouterApp` 统一管理 `DoraNode`
        println!("🚀 `RouterApp` 初始化完成");
        node.send_output(DataId::from("init".to_owned()), Default::default(), "RouterApp 启动".into_arrow()).unwrap();
        while let Some(event) = events.recv() {
            match event {
                Event::Input { id, data, metadata } => {
                    self.handle_input(&mut node, &id, data, metadata).await;
                }
                Event::Stop => {
                    println!("🛑 `Stop` 事件，RouterApp 退出...");
                    break;
                }
                other => {
                    println!("🤷 收到未知事件: {:?}", other);
                }
            }
        }

        Ok(())
    }

    /// **处理 `Dora` 事件**
    async fn handle_input(
        &self,
        dora_node: &mut DoraNode,
        id: &str,
        data: ArrowData,
        metadata: dora_node_api::Metadata,
    ) {
        match id {
            "register" => {
                println!("📥 收到注册事件: {:?}", data);
                let node = NodeDescriptor::try_from(data).unwrap();
                self.router.register_node(node);
            }
            "query" => {
                let input_data :&str= (&data).try_into().unwrap();
                println!("📥 收到输入事件: {}", input_data);

                let prompt = format!(
                    "当前可用 `Node` 信息: {:?}\n如何处理数据: `{}`？",
                    self.router.get_registered_nodes(),
                    input_data
                );
                println!("🧠 `LLM` 提示: {}", prompt);
                let response = self.agent.prompt(prompt.as_str())
                    .await;
                match response {
                    Ok(output) => {
                        println!("🧠 `LLM` 选择的执行方案: {}", output);
                        if let Some((node_id, new_data)) = tools::parse_output(&output) {
                            dora_node
                                .send_output(
                                    DataId::from(node_id),
                                    metadata.parameters,
                                    new_data.into_arrow(),
                                )
                                .unwrap();
                        }
                    }
                    Err(_) => println!("⚠️ `LLM` 处理失败"),
                }
            }
            other => {
                println!("⚠️ 未知的输入事件: {}", other);
            }
        }
    }
}

/// **主入口**
#[tokio::main]
async fn main() -> Result<()> {
    // tracing_subscriber::fmt().init();
    let app = RouterApp::new()?;
    app.run().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use dora_node_api::arrow::array::StringArray;
    use dora_node_api::arrow::datatypes::Field;
    use dora_node_api::dora_core::config::DataId;
    use dora_node_api::Metadata;
    // use mockall::{mock, predicate};
    use std::collections::HashMap;
    use tokio::sync::mpsc;



    // After:
    // mock! {
    //     CompletionModelMock {}
    //
    //     impl Clone for CompletionModelMock {
    //         fn clone(&self) -> Self {
    //             Self {}
    //         }
    //     }
    //
    //     #[async_trait::async_trait]
    //     impl CompletionModel for CompletionModelMock {
    //         type Response = String;
    //
    //         async fn completion(
    //             &self,
    //             request: rig::completion::CompletionRequest,
    //         ) -> Result<rig::providers::ollama::CompletionResponse<Self::Response>, rig::completion::CompletionError> {
    //             Ok(CompletionResponse {
    //                 text: "mock response".to_string(),
    //                 raw: "mock raw response".to_string(),
    //             })
    //         }
    //     }
    // }

    #[test]
    fn router_node_registers_and_retrieves_nodes() {
        let router = RouterNode::new();
        let node = NodeDescriptor {
            id: "test-node".to_string(),
            description: "Test node".to_string(),
            inputs: r#"{"type": "string"}"#.to_string(),
            outputs: r#"{"type": "string"}"#.to_string(),
        };

        // Register node
        router.register_node(node.clone());

        // Get all nodes
        let nodes = router.get_registered_nodes();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].id, "test-node");

        // Get node by id
        let retrieved = router.get_node_by_id("test-node").unwrap();
        assert_eq!(retrieved.id, "test-node");
        assert_eq!(retrieved.description, "Test node");

        // Get non-existent node
        let not_found = router.get_node_by_id("non-existent");
        assert!(not_found.is_none());
    }

    #[test]
    fn node_descriptor_converts_to_arrow() {
        let node = NodeDescriptor {
            id: "test-node".to_string(),
            description: "Test node".to_string(),
            inputs: r#"{"type": "string"}"#.to_string(),
            outputs: r#"{"type": "string"}"#.to_string(),
        };

        let arrow_data = node.into_arrow();

        // assert_eq!(arrow_data.num_rows(), 1);
        assert_eq!(arrow_data.num_columns(), 4);

        let fields = arrow_data.fields();
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[0].name(), "id");
        assert_eq!(fields[1].name(), "description");
        assert_eq!(fields[2].name(), "inputs");
        assert_eq!(fields[3].name(), "outputs");
    }

    #[tokio::test]
    async fn get_nodes_tool_returns_registered_nodes() {
        let router = Arc::new(RouterNode::new());
        let node = NodeDescriptor {
            id: "test-node".to_string(),
            description: "Test node".to_string(),
            inputs: r#"{"type": "string"}"#.to_string(),
            outputs: r#"{"type": "string"}"#.to_string(),
        };
        router.register_node(node);

        let tool = tools::GetNodes {
            router: Arc::clone(&router),
        };

        let result = tool.call(()).await.unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "test-node");
    }

    #[tokio::test]
    async fn send_data_tool_returns_success_message() {
        let router = Arc::new(RouterNode::new());
        let tool = tools::SendData { router };

        let args = tools::SendDataArgs {
            node_id: "test-node".to_string(),
            data: "test data".to_string(),
        };

        let result = tool.call(args).await.unwrap();

        assert_eq!(result, "✅ `Node` `test-node` 处理成功");
    }

    #[test]
    fn parse_output_extracts_node_id_and_data_from_json() {
        let json = r#"{"next_node": "processor", "data": "hello world"}"#;

        let result = tools::parse_output(json);

        assert!(result.is_some());
        let (node_id, data) = result.unwrap();
        assert_eq!(node_id, "processor");
        assert_eq!(data, "hello world");
    }

    #[test]
    fn parse_output_returns_none_for_invalid_json() {
        let invalid_json = "not a json";
        assert!(tools::parse_output(invalid_json).is_none());

        let missing_fields = r#"{"other": "value"}"#;
        assert!(tools::parse_output(missing_fields).is_none());
    }
}
