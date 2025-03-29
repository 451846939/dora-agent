use crate::register::RouterNode;
use anyhow::Result;
use common::config::AppConfig;
use common::descriptor::NodeDescriptor;
use common::message::{flow_msg, FlowMessage, Workflow};
use common::{clean_llm_output, REGISTER, RESULT};
use dora_node_api::dora_core::config::DataId;
use dora_node_api::{ArrowData, DoraNode, Event, IntoArrow, Metadata};
use rig::agent::Agent;
use rig::completion::{Chat, Completion, CompletionModel, Message, Prompt};
use rig::{providers, tool::Tool};
use serde_json::Value;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use uuid::Uuid;
use crate::summary::summarize_results;

mod register;
mod summary;
mod tools;

/// **RouterApp** 负责管理 DoraNode 生命周期以及工作流状态
pub struct RouterApp {
    router: Arc<RouterNode>,
    agent: Agent<rig::providers::openai::CompletionModel>,
    workflow_manager: Arc<Mutex<HashMap<String, Workflow>>>,
    app_id: String,
}

impl RouterApp {
    pub fn new() -> Result<Self> {
        let router = Arc::new(RouterNode::new());
        let app_id = "router".to_string();
        let (openai_client, config) = AppConfig::from_file_with_appid(&app_id)?;

        // 初始化 LLM，让其管理 Dora 数据流
        let agent = openai_client
            .agent(&config.model)
            .preamble("你是 `Dora` 数据流控制器，根据 `NodeDescriptor` 选择最优数据流路径。")
            .build();

        Ok(Self {
            router,
            agent,
            workflow_manager: Arc::new(Mutex::new(HashMap::new())),
            app_id,
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
            "query_state" => {
                let state_json = self.get_workflow_state();
                // 将 workflow 状态作为结果发送到 node-state 节点
                match dora_node.send_output(
                    DataId::from("node-state".to_owned()),
                    metadata.parameters.clone(),
                    state_json.into_arrow(),
                ) {
                    Ok(_) => println!("✅ 已将当前 workflow 状态发送到 node-state"),
                    Err(err) => println!("⚠️ 发送 workflow 状态失败: {}", err),
                }
            }
            id if id.starts_with(REGISTER) => {
                println!("📥 收到注册事件: {:?}", data);
                let node = NodeDescriptor::try_from(data).unwrap();
                self.router.register_node(node);
            }
            "query" => {
                let input_data: &str = (&data).try_into().unwrap();
                println!("📥 收到输入事件: {}", input_data);
                self.execute_node_selection_workflow(input_data, dora_node, metadata)
                    .await;
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

                // 处理工作流节点结果和下一步操作
                self.process_workflow_step_result(flow_msg, dora_node, metadata).await;
            }
            other => {
                println!("⚠️ 未知的输入事件: {}", other);
            }
        }
    }

    /// 处理工作流节点执行结果和后续步骤
    async fn process_workflow_step_result(
        &self,
        flow_msg: FlowMessage,
        dora_node: &mut DoraNode,
        metadata: Metadata
    ) {
        let mut workflows = self.workflow_manager.lock().unwrap();

        // 如果找不到对应工作流，记录错误并返回
        let Some(workflow) = workflows.get_mut(&flow_msg.workflow_id) else {
            println!("⚠️ 未找到对应的工作流: {}", flow_msg.workflow_id);
            return;
        };

        // 验证节点匹配
        let current_node = &workflow.steps[workflow.current_index];
        if current_node.id != flow_msg.node_id {
            println!(
                "⚠️ 返回的节点 id ({}) 与预期的不符 ({})",
                flow_msg.node_id, current_node.id
            );
            // 注意：此处根据实际业务需求可以选择终止流程或继续执行
        }

        // 保存节点处理结果
        let result_value = flow_msg.result.clone().unwrap_or(Value::Null);
        workflow.results.push(result_value);
        workflow.current_index += 1;
        common::status_log::send_status_log(
            dora_node,
            &metadata,
            common::status_log::WorkflowLog {
                workflow_id: flow_msg.workflow_id.clone(),
                node_id: flow_msg.node_id.clone(),
                step_index: workflow.current_index - 1,
                total_steps: workflow.steps.len(),
                status: "completed".to_string(),
                input: flow_msg.input.clone(),
                output: flow_msg.result.clone().unwrap_or(Value::Null),
                message: format!("✅ 节点 {} 执行完成", flow_msg.node_id),
            }
        );
        // 检查是否还有后续步骤
        if workflow.current_index < workflow.steps.len() {
            self.execute_next_workflow_step(workflow, &flow_msg, dora_node, metadata).await;
        } else {
            // 工作流已完成
            println!(
                "✅ 工作流 {} 完成, 最终结果: {:?}",
                flow_msg.workflow_id, workflow.results
            );
            let summary=summarize_results(
                &self.agent,
                workflow,
            ).await.unwrap();
            // let summary = aggregate_results(&workflow.results);
            common::status_log::send_status_log(
                dora_node,
                &metadata,
                common::status_log::WorkflowLog {
                    workflow_id: flow_msg.workflow_id.clone(),
                    node_id: flow_msg.node_id.clone(),
                    step_index: workflow.current_index-1,
                    total_steps: workflow.steps.len(),
                    status: "finished".to_string(),
                    input: Value::Null,
                    output: Value::String(summary.clone()),
                    message: format!("🏁 工作流 {} 完成，结果汇总：{}", flow_msg.workflow_id, summary),
                }
            );
            workflows.remove(&flow_msg.workflow_id);
        }
    }

    /// 执行工作流的下一步骤
    async fn execute_next_workflow_step(
        &self,
        workflow: &mut Workflow,
        flow_msg: &FlowMessage,
        dora_node: &mut DoraNode,
        metadata: Metadata
    ) {

        // let next_node = &workflow.steps[workflow.current_index];
        // let next_node_id =next_node.id.clone();
        // 使用LLM映射变量
        let Some(next_input) = self.map_variables_with_llm(flow_msg,workflow).await else {
            println!("⚠️ 无法为节点 {:?} 生成输入", workflow.steps[workflow.current_index].id);
            return;
        };

        let next_node= &workflow.steps[workflow.current_index];

        // 准备聚合字段（如果需要）
        let aggregated_field = if next_node.aggregate {
            Some(aggregate_results(&workflow.results))
        } else {
            None
        };

        // 构建下一步节点消息
        let next_msg = FlowMessage {
            workflow_id: flow_msg.workflow_id.clone(),
            node_id: next_node.id.clone(),
            input: next_input,
            prev_result: flow_msg.result.clone(),
            result: None,
            aggregated: aggregated_field,
        };

        // 转发到下一个节点
        Self::node_forward(dora_node, metadata, next_node, next_msg);
    }

    /// 查询并执行LLM节点选择流程
    async fn execute_node_selection_workflow(
        &self,
        input_data: &str,
        dora_node: &mut DoraNode,
        metadata: Metadata,
    ) {
        // 1. 准备LLM提示
        let prompt = self.create_node_selection_prompt(input_data);
        println!("🧠 `LLM` 提示: {}", prompt);

        // 2. 请求LLM规划
        match self.agent.prompt(prompt.as_str()).await {
            Ok(output) => {
                self.process_llm_selection_result(output, input_data, dora_node, metadata)
                    .await
            }
            Err(err) => println!("⚠️ `LLM` 处理失败, 错误: {}", err),
        }
    }

    /// 创建节点选择提示
    fn create_node_selection_prompt(&self, input_data: &str) -> String {
        format!(
            r#"
        你是一个严格的任务规划器，请根据当前输入数据 `{input_data}` 和以下 `Node` 列表，从中选择一个或多个节点来完成任务。

        当前可用的节点信息如下：
        {nodes}

        请严格遵守以下规则：

        1. **只返回一个合法的 JSON 字符串数组**，数组元素是节点的 `id`，如：["node_a", "node_b"]；
        2. **严禁输出 `<think>`、解释说明、自然语言推理、格式注释、markdown 代码块等**；
        3. 不要返回任何结构体、对象或其他字段，只返回数组；
        4. 返回的 `id` 必须来自节点列表，不能随意编造；
        5. 如果无法选择，返回一个空数组：[]。

        请只返回一个合法 JSON 数组作为最终结果。
        "#,
            nodes = serde_json::to_string_pretty(&self.router.get_registered_nodes()).unwrap(),
            input_data = input_data
        )
    }

    /// 处理LLM返回的节点选择结果
    async fn process_llm_selection_result(
        &self,
        mut output: String,
        input_data: &str,
        dora_node: &mut DoraNode,
        metadata: Metadata,
    ) {
        println!("🧠 `LLM` 选择的执行方案: {}", output);

        // 1. 清理输出并解析JSON
        output = clean_llm_output(&output);
        let node_ids = match serde_json::from_str::<Vec<String>>(&output) {
            Ok(ids) => ids,
            Err(err) => {
                println!("⚠️ JSON 解析失败: {}", err);
                return;
            }
        };

        // 2. 查找对应的节点描述符
        let nodes: Vec<NodeDescriptor> = node_ids
            .iter()
            .filter_map(|id| self.router.get_node_by_id(id))
            .collect();

        if nodes.is_empty() {
            println!("⚠️ LLM 返回的节点列表为空或找不到对应节点");
            return;
        }

        // 3. 构建工作流并准备首个节点执行
        self.execute_first_workflow_node(input_data, &nodes, dora_node, metadata)
            .await;
    }

    /// 执行工作流的首个节点
    async fn execute_first_workflow_node(
        &self,
        input_data: &str,
        nodes: &Vec<NodeDescriptor>,
        dora_node: &mut DoraNode,
        metadata: Metadata,
    ) {
        // 1. 创建工作流
        let mut workflow = Self::build_workflow(input_data, nodes);
        let first_node = &nodes[0];
        let workflow_id = workflow.id.clone();

        // 2. 构造初始流消息
        let mut flow_msg = FlowMessage {
            workflow_id: workflow_id.clone(),
            node_id: first_node.id.clone(),
            input: serde_json::from_str(&first_node.inputs).unwrap_or(Value::Null),
            prev_result: None,
            result: Some(Value::from(input_data)),
            aggregated: None,
        };

        // 3. 使用LLM映射变量，准备节点输入
        if let Some(input) = self.map_variables_with_llm(&flow_msg,&mut workflow).await {
            flow_msg.input = input;
        } else {
            flow_msg.input = Value::Null;
        }

        common::status_log::send_status_log(
            dora_node,
            &metadata,
            common::status_log::WorkflowLog {
                workflow_id: workflow_id.clone(),
                node_id: first_node.id.clone(),
                step_index: 0,
                total_steps: nodes.len(),
                status: "started".to_string(),
                input: flow_msg.input.clone(),
                output: Value::Null,
                message: format!("🟢 启动工作流: {}", workflow_id),
            });
        self.workflow_manager
            .lock()
            .unwrap()
            .insert(workflow_id.clone(), workflow);
        // 4. 转发到首个节点执行
        Self::node_forward(dora_node, metadata, first_node, flow_msg);
    }

    #[deprecated]
    fn node_descriptor_prompt(self, input_data: &str) -> String {
        let prompt = format!(
            r#"
            你是一个严格的任务规划器，请根据当前输入数据 `{input_data}` 和以下 `Node` 列表，从中选择一个或多个节点来完成任务，并为每个节点按照其 `inputs` 字段规范提供正确的入参,如果最后有深度思考总结信息的节点必须添加该节点在最后进行总结。

            当前可用的节点信息如下：
            {nodes}

            请务必严格遵守以下规则：

            1. 你只能返回一个**合法的 JSON 数组**。数组中每个元素为选中的 `NodeDescriptor`，包含字段 `id`, `description`, `inputs`,`outputs`（结构必须和节点定义匹配）,以及可选的 `aggregate` 和 `agg_field`（表示该节点是否需要聚合前置所有结果，以及聚合结果应存入哪个字段）；；
            2. **严禁输出 `<think>`、解释说明、自然语言推理、格式注释、markdown 代码块（例如 ```json）等**；
            3. 结果必须是纯粹的、没有任何前后缀、可被 JSON 解析器直接解析的 JSON 字符串；
            4. 输入参数结构（`inputs`,`outputs`）必须完全符合节点要求，比如要求结构体时必须是对象，要求数组结构体时必须是数组；
            5. 严禁出现没有的NodeDescriptor，或者臆想一些不存在的NodeDescriptor，还有每个NodeDescriptor不要乱改`id`, `description`, `inputs`,`outputs`。
            6. 如果你不确定返回是否符合规范，请不要返回任何内容。
            7. 对于query你可以对其进行简单的自然语言处理让起更好处理，比如将 `query` 的值从 `rust for linux` 变成 `rust for linux`，但是你不能对 `inputs` 和 `outputs` 的结构进行任何修改。
            8. 如果上下文完全不相关就根据对应的node的功能做对应的事

            示例格式（注意，这只是格式参考，不代表你必须使用这些节点）：
            [
              {{
                "id": "web_search",
                "description": "使用浏览器执行搜索，并解析搜索结果",
                "inputs": "{{\"query\":\"rust for linux\", \"click\":true}}",
                "outputs": "{{\"title\":\"string\", \"link\":\"string\"}}",
                "aggregate": false
              }}
            ]

            请根据 `{input_data}` 和节点列表返回严格合法的 JSON。
             "#,
            nodes = serde_json::to_string_pretty(&self.router.get_registered_nodes()).unwrap(),
            input_data = input_data
        );
        prompt
    }

    fn build_workflow(input_data: &str, nodes: &Vec<NodeDescriptor>) -> Workflow {
        // 生成工作流 ID
        let workflow_id = Uuid::new_v4().to_string();
        // 创建工作流状态（LLM返回的步骤顺序由 route 管理）
        let workflow = Workflow {
            id: workflow_id.clone(),
            query: input_data.to_string(),
            steps: nodes.clone(),
            current_index: 0,
            results: vec![],
            chat_log: vec![],
        };
        workflow
    }

    fn node_forward(
        dora_node: &mut DoraNode,
        metadata: Metadata,
        next_node: &NodeDescriptor,
        next_msg: FlowMessage,
    ) {
        match dora_node.send_output(
            DataId::from(next_node.id.clone()),
            metadata.parameters.clone(),
            next_msg.into_arrow(),
        ) {
            Ok(_) => {
                println!("✅ 下一任务已成功发送至 Node: {}", next_node.id)
            }
            Err(err) => println!("⚠️ 发送下一任务至 Node 失败: {}", err),
        }
    }
    #[deprecated]
    fn node_plan(
        self,
        input_data: &str,
        output: &str,
        dora_node: &mut DoraNode,
        metadata: Metadata,
    ) {
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
                    chat_log: vec![],
                };
                self.workflow_manager
                    .lock()
                    .unwrap()
                    .insert(workflow_id.clone(), workflow);

                // 构造统一的 FlowMessage（首次调用 prev_result 为 None，result 为空）
                let flow_msg = FlowMessage {
                    workflow_id: workflow_id.clone(),
                    node_id: nodes[0].id.clone(),
                    input: serde_json::from_str(&nodes[0].inputs).unwrap_or(Value::Null),
                    prev_result: None,
                    result: None,
                    aggregated: None,
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
        // }
    }

    async fn map_variables_with_llm(
        &self,
        flow_msg: &FlowMessage,
        // next_node: &NodeDescriptor,
        workflow: &mut Workflow
    ) -> Option<Value> {
        let next_node = &workflow.steps[workflow.current_index];
        // 构造 prompt，要求大模型根据上一步的结果和下一个节点输入模板生成新的任务 JSON
        let prompt = format!(
            r#"
你是一个智能任务执行助手。你的唯一目标：生成与 “Next Node 描述信息” 的 `inputs` 结构和字段名字**完全相同**的 JSON 对象或数组。禁止包含其它键值或数组元素，禁止任何注释或解释说明。
1. 认真审读“上一步返回结果”和下游节点的 input；
2. 确定你需要用到哪些信息构造本节点的输入字段；
3. 如果字段语义是“string”，则填写简洁自然语言；若为“object”或“array”，则严格匹配相应结构。
4. 禁止输出任何与节点需求无关的信息、字段或层级。
5. 对于query你可以对其进行简单的自然语言处理让起更好处理，比如将 `query` 的值从 `rust for linux` 变成 `rust for linux`，但是你不能对 `inputs` 和 `outputs` 的结构进行任何修改。
6. 如果上下文完全不相关就根据对应的node的功能做对应的事

【最终输出 - 仅返回符合 Next Node 的 JSON】
- 仅返回一个可被 JSON 解析器直接解析的对象或数组；
- 名称、类型、层级必须与 `Next Node` 的 `inputs` 完全一致，不能添加或删除；
- 不得输出推理过程、解释、Markdown 代码块或任何额外内容。

-----------------------------------------------------------------------
你的总执行计划node_ids为
{node_ids}

(1) Next Node 描述信息：
{node_descriptor}

(2) 上一步返回的原始 JSON 结果：
{previous_result}

(3) 生成规则：
- 如果 Next Node `inputs` 为 {{ "query": "根据搜索结果写一篇文章" }}，
  你必须返回类似：
  {{
    "query": "（此处为最开始的传输信息）"
  }}
- 禁止返回数组，除非 Schema 要求是 array；
- 禁止任何解释、注释或 Markdown 代码块。

请严格遵守上述规则，只输出与 “Next Node” 的 inputs 结构完全相同、可被 JSON 解析器直接解析的纯 JSON。
"#,
            previous_result = flow_msg
                .result
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or_else(|| "null".to_string()),
            // next_input_schema = next_node.inputs,
            node_descriptor = serde_json::to_string_pretty(next_node).unwrap(),
            node_ids=workflow.steps.iter().map(|n| n.id.clone()).collect::<Vec<_>>().join(",")
        );
        println!("🧠 调用大模型生成下一个任务输入，prompt: {}", prompt);

        let llm_response = self.agent.chat(prompt.as_str(), workflow.chat_log.clone()).await;
        match llm_response {
            Ok(mut generated_input) => {
                // 尝试解析大模型返回的 JSON 作为新的输入
                // 去除 Markdown 代码块标记
                generated_input = clean_llm_output(&generated_input);
                insert_workflow_chat_log(workflow, prompt, generated_input.clone());
                println!("🧠 大模型返回的生成输入: {}", generated_input);
                let new_input: Value =
                    serde_json::from_str(&generated_input).unwrap_or_else(|err| {
                        println!(
                            "⚠️ 大模型生成的输入解析失败: {},err: {}",
                            generated_input, err
                        );
                        Value::Null
                    });
                Some(new_input)
            }
            Err(err) => {
                println!("⚠️ 调用大模型生成下一个任务输入失败: {}", err);
                None
            }
        }
    }



    // 新增方法返回当前 workflow 状态的 JSON 字符串
    pub fn get_workflow_state(&self) -> String {
        let workflows = self.workflow_manager.lock().unwrap();
        serde_json::to_string_pretty(&*workflows).unwrap_or_else(|_| "{}".to_string())
    }
}
pub fn insert_workflow_chat_log(workflow: &mut Workflow, input: String, output: String) {
    workflow.chat_log.push(Message::user(input));
    workflow.chat_log.push(Message::assistant(output));
}

/// 聚合所有非空结果为一个字符串，用换行分隔
fn aggregate_results(results: &Vec<Value>) -> String {
    results
        .iter()
        .filter(|r| !r.is_null())
        .map(|r| r.to_string())
        .collect::<Vec<String>>()
        .join("\n")
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
            aggregate: false,
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
            aggregate: false,
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
            aggregate: false,
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
