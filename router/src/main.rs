use crate::register::RouterNode;
use anyhow::Result;
use common::{flow_msg, FlowMessage, NodeDescriptor, REGISTER, RESULT};
use dora_node_api::arrow::array::{ArrayRef, StringArray};
use dora_node_api::dora_core::config::DataId;
use dora_node_api::{ArrowData, DoraNode, Event, IntoArrow};
use regex::Regex;
use rig::agent::Agent;
use rig::completion::{Chat, Completion, CompletionModel, Prompt};
use rig::{providers, tool::Tool};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid; // 需要在 Cargo.toml 中添加 uuid 依赖

mod register;
mod tools;

/// 工作流状态，用于记录每个 query 的处理流程
#[derive(Debug)]
struct Workflow {
    id: String,                 // 工作流 ID
    query: String,              // 初始 query
    steps: Vec<NodeDescriptor>, // 执行步骤（节点列表，由 LLM 规划）
    current_index: usize,       // 当前执行的步骤索引
    results: Vec<Value>,        // 各步骤返回的结果（使用 Value 保存，便于后续扩展）
}

/// **RouterApp** 负责管理 DoraNode 生命周期以及工作流状态
pub struct RouterApp {
    router: Arc<RouterNode>,
    agent: Agent<rig::providers::ollama::CompletionModel>,
    workflow_manager: Arc<Mutex<HashMap<String, Workflow>>>,
}

impl RouterApp {
    pub fn new() -> Result<Self> {
        let router = Arc::new(RouterNode::new());

        // 初始化 LLM，让其管理 Dora 数据流
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

        Ok(Self {
            router,
            agent,
            workflow_manager: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn run(&self) -> Result<()> {
        println!("🛰️ `RouterApp` 启动...");

        let (mut node, mut events) = DoraNode::init_from_env().unwrap();
        println!("🚀 `RouterApp` 初始化完成");
        node.send_output(
            DataId::from("init".to_owned()),
            Default::default(),
            "RouterApp 启动".into_arrow(),
        )
        .unwrap();
        while let Some(event) = events.recv_async().await {
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

    /// 处理 Dora 事件
    async fn handle_input(
        &self,
        dora_node: &mut DoraNode,
        id: &str,
        data: ArrowData,
        metadata: dora_node_api::Metadata,
    ) {
        match id {
            id if id.starts_with(REGISTER) => {
                println!("📥 收到注册事件: {:?}", data);
                let node = NodeDescriptor::try_from(data).unwrap();
                self.router.register_node(node);
            }
            "query" => {
                let input_data: &str = (&data).try_into().unwrap();
                println!("📥 收到输入事件: {}", input_data);

                let prompt = format!(
                    "当前可用的 `Node` 信息:\n{nodes}\n\n\
                     请根据以下输入数据 `{input_data}` 选择合适的 `NodeDescriptor` 进行处理，并且可以多个进行组合完成，并按照 `NodeDescriptor` 的 `inputs` 规范组装入参。\n\n\
                     规则：\n\
                     1. 优先选择匹配的 `NodeDescriptor`；\n\
                     2. 按照 `NodeDescriptor` 的 `inputs` 组装入参；\n\
                     3. 返回 `NodeDescriptor` 列表的 JSON 结构，示例如下：\n\
                     ```json\n\
                     [\n\
                       {{\n\
                         \"id\": \"web_search\",\n\
                         \"description\": \"使用浏览器执行搜索，并解析搜索结果\",\n\
                         \"inputs\": \"{{\\\"query\\\":\\\"rust for linux\\\", \\\"click\\\":true}}\",\n\
                         \"outputs\": \"{{\\\"title\\\":\\\"string\\\", \\\"link\\\":\\\"string\\\"}}\"\n\
                       }}\n\
                     ]\n\
                     ```\n\n\
                     请根据 `{input_data}` 返回 JSON 结果.",
                    nodes = serde_json::to_string_pretty(&self.router.get_registered_nodes()).unwrap(),
                    input_data = input_data
                );
                println!("🧠 `LLM` 提示: {}", prompt);
                let response = self.agent.prompt(prompt.as_str()).await;
                match response {
                    Ok(mut output) => {
                        println!("🧠 `LLM` 选择的执行方案: {}", output);
                        // 去除 Markdown 代码块标记
                        let re = Regex::new(r"(?s)^```json\n(.*)\n```$").unwrap();
                        if let Some(captures) = re.captures(&output) {
                            output = captures[1].to_string();
                        }
                        match serde_json::from_str::<Vec<NodeDescriptor>>(&output) {
                            Ok(nodes) => {
                                if nodes.is_empty() {
                                    println!("⚠️ LLM 返回的节点列表为空");
                                    return;
                                }
                                // 生成工作流 ID
                                let workflow_id = Uuid::new_v4().to_string();
                                // 创建工作流状态（LLM返回的步骤顺序由 route 管理）
                                let workflow = Workflow {
                                    id: workflow_id.clone(),
                                    query: input_data.to_string(),
                                    steps: nodes.clone(),
                                    current_index: 0,
                                    results: vec![],
                                };
                                self.workflow_manager
                                    .lock()
                                    .unwrap()
                                    .insert(workflow_id.clone(), workflow);

                                // 构造统一的 FlowMessage（首次调用 prev_result 为 None，result 为空）
                                let flow_msg = FlowMessage {
                                    workflow_id: workflow_id.clone(),
                                    node_id: nodes[0].id.clone(),
                                    input: serde_json::from_str(&nodes[0].inputs)
                                        .unwrap_or(Value::Null),
                                    prev_result: None,
                                    result: None,
                                };

                                // 调用第一个节点
                                match dora_node.send_output(
                                    DataId::from(nodes[0].id.clone()),
                                    metadata.parameters.clone(),
                                    flow_msg.into_arrow(),
                                ) {
                                    Ok(_) => println!("✅ 任务已成功发送至 Node: {}", nodes[0].id),
                                    Err(err) => println!("⚠️ 发送至 Node 失败: {}", err),
                                }
                            }
                            Err(err) => {
                                println!("⚠️ JSON 解析失败: {}", err);
                            }
                        }
                    }
                    Err(err) => println!("⚠️ `LLM` 处理失败, 错误: {}", err),
                }
            }
            id if id.starts_with(RESULT) => {
                println!("📥 收到结果事件: {:?}", data);
                // 所有节点返回都采用统一结构 FlowMessage
                let flow_msg: FlowMessage = match flow_msg::try_from(data) {
                    Ok(msg) => msg,
                    Err(err) => {
                        println!("⚠️ 解析 FlowMessage 失败: {}", err);
                        return;
                    }
                };
                println!(
                    "📥 收到结果事件，workflow_id: {}, node_id: {}",
                    flow_msg.workflow_id, flow_msg.node_id
                );

                let mut workflows = self.workflow_manager.lock().unwrap();
                if let Some(workflow) = workflows.get_mut(&flow_msg.workflow_id) {
                    // 检查返回节点与当前预期是否匹配
                    let current_node = &workflow.steps[workflow.current_index];
                    if current_node.id != flow_msg.node_id {
                        println!(
                            "⚠️ 返回的节点 id ({}) 与预期的不符 ({})",
                            flow_msg.node_id, current_node.id
                        );
                        // 根据实际业务决定是否中断流程
                    }
                    // 保存当前节点的处理结果
                    if let Some(res) = flow_msg.result.clone() {
                        workflow.results.push(res);
                    } else {
                        workflow.results.push(Value::Null);
                    }
                    workflow.current_index += 1;
                    // 如果还有后续步骤，则调用大模型生成下一个任务，否则工作流完成
                    if workflow.current_index < workflow.steps.len() {
                        let next_node = &workflow.steps[workflow.current_index];
                        // 构造 prompt，要求大模型根据上一步的结果和下一个节点输入模板生成新的任务 JSON
                        let prompt = format!(
                            r#"你是一个智能任务执行器。请根据上一步返回的结果和当前任务要求，构造新的任务输入。

                            上一步返回的结果如下（可包含多个文档或段落）：
                            {}

                            下一个任务的输入要求如下：
                            {}

                            请注意：
                            1. 当前任务是创建一个文件，字段包括：operation、path、content；请确保 content 字段内容合理，结合上一步结果生成。
                            2. 返回必须是一个合法的 JSON 数组，数组中每个元素是一个 JSON 对象。
                            3. JSON 对象必须严格包含字段 operation, path 和 content，且 content 字段中应当包含上一步相关内容的摘要、整合或重写，不得留空。
                            4. 不要包含解释说明、不要使用 markdown 代码块语法（如 ```json）。
                            5. 只返回最终的 JSON 内容，确保可以被 JSON 解析器直接解析。"#,
                            flow_msg.result.as_ref().map(|v| v.to_string()).unwrap_or_else(|| "null".to_string()),
                            next_node.inputs
                        );
                        println!("🧠 调用大模型生成下一个任务输入，prompt: {}", prompt);
                        let llm_response = self.agent.prompt(prompt.as_str()).await;
                        match llm_response {
                            Ok(mut generated_input) => {
                                // 尝试解析大模型返回的 JSON 作为新的输入
                                // 去除 Markdown 代码块标记
                                let re = Regex::new(r"(?s)^```json\s*\n(.*)\n```").unwrap();
                                if let Some(captures) = re.captures(&generated_input) {
                                    generated_input = captures[1].to_string();
                                }
                                println!("🧠 大模型返回的生成输入: {}", generated_input);
                                let new_input: Value = serde_json::from_str(&generated_input)
                                    .unwrap_or_else(|err| {
                                        println!(
                                            "⚠️ 大模型生成的输入解析失败: {},err: {}",
                                            generated_input, err
                                        );
                                        Value::Null
                                    });
                                let next_msg = FlowMessage {
                                    workflow_id: flow_msg.workflow_id.clone(),
                                    node_id: next_node.id.clone(),
                                    input: new_input,
                                    prev_result: flow_msg.result.clone(),
                                    result: None,
                                };
                                match dora_node.send_output(
                                    DataId::from(next_node.id.clone()),
                                    Default::default(),
                                    next_msg.into_arrow(),
                                ) {
                                    Ok(_) => {
                                        println!("✅ 下一任务已成功发送至 Node: {}", next_node.id)
                                    }
                                    Err(err) => println!("⚠️ 发送下一任务至 Node 失败: {}", err),
                                }
                            }
                            Err(err) => {
                                println!("⚠️ 调用大模型生成下一个任务输入失败: {}", err);
                            }
                        }
                    } else {
                        println!(
                            "✅ 工作流 {} 完成, 最终结果: {:?}",
                            flow_msg.workflow_id, workflow.results
                        );
                        workflows.remove(&flow_msg.workflow_id);
                    }
                } else {
                    println!("⚠️ 未找到对应的工作流: {}", flow_msg.workflow_id);
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
    use std::collections::HashMap;
    use tokio::sync::mpsc;

    #[test]
    fn router_node_registers_and_retrieves_nodes() {
        let router = RouterNode::new();
        let node = NodeDescriptor {
            id: "test-node".to_string(),
            description: "Test node".to_string(),
            inputs: r#"{"type": "string"}"#.to_string(),
            outputs: r#"{"type": "string"}"#.to_string(),
        };

        router.register_node(node.clone());
        let nodes = router.get_registered_nodes();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].id, "test-node");

        let retrieved = router.get_node_by_id("test-node").unwrap();
        assert_eq!(retrieved.id, "test-node");
        assert_eq!(retrieved.description, "Test node");

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
}
