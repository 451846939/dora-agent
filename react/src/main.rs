mod react;

use anyhow::Context;
use dora_node_api::{DoraNode, Event, IntoArrow};
use dora_node_api::dora_core::config::DataId;
use eyre::Result;
use rig::completion::Prompt;
use schemars::schema_for;
use serde::{Deserialize, Serialize};
use serde_json::{Value};
use common::{register_id, result_id, clean_llm_output};
use std::str::FromStr;
use common::config::AppConfig;
use common::descriptor::NodeDescriptor;
use common::message::FlowMessage;
use crate::react::{ReactInput, ReactOutput};

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 start react-node");
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let app_id = "rect".to_string();
    let (openai_client,config)=AppConfig::from_file_with_appid(&app_id)?;
    // 初始化 LLM 代理，角色预设为链式思考专家
    // let openai_client = providers::ollama::Client::new();
    let agent = openai_client
        .agent(&config.model)
        .preamble("你是一个综合整合专家，擅长使用 ReAct 模式进行链式思考和自我评分，直到生成的输出满足要求。")
        .max_tokens(1024)
        .build();
    // 预先生成 ReactOutput 的 JSON Schema 字符串（动态传入提示词）
    let output_schema_json = serde_json::to_string_pretty(&schema_for!(ReactOutput))?;

    while let Some(event) = events.recv_async().await {
        match event {
            Event::Input { id, metadata, data } => {
                match id.as_str() {
                    "rect" => {
                        // 解析统一 FlowMessage
                        let flow_msg: FlowMessage = FlowMessage::try_from(data)
                            .context("expected FlowMessage").unwrap();
                        println!("📥 react-node received FlowMessage: {:?}", flow_msg);

                        // 将 FlowMessage.input 反序列化为 ReactInput
                        let react_input: ReactInput = if let Value::String(s) = &flow_msg.input {
                            // 如果 input 为简单字符串，则视为 query，资料为空
                            serde_json::from_str(s).context("failed to deserialize react input").unwrap()
                        } else {
                            // 如果 input 为对象，则尝试反序列化成 ReactInput（要求包含 query 和 materials 字段）
                            serde_json::from_value(flow_msg.input.clone())
                                .context("failed to deserialize react input").unwrap()
                        };
                        println!("🔍 ReactInput: {:?}", react_input);

                        let input_text = react_input.query; // 查询内容
                        let materials_text = flow_msg.aggregated.unwrap(); // 整体收集到的资料

                        // 自我评分机制：重试最多 3 次，直到生成结果评分满足阈值
                        let mut output: Option<ReactOutput> = None;
                        let mut iterations = 0;
                        let threshold = 0.8;

                        while iterations < 3 {
                            // 构造详细的 ReAct 链式推理提示词，要求直接以 JSON 格式输出结构化数据，
                            // 且输出必须符合动态传入的 JSON Schema
                            let prompt = format!(r#"
[ReAct 分析]
请对以下工作流查询和收集到的资料进行详细的链式思考和推理，描述你每一步的思考过程，并在最后给出最终结论和生成完整文章的内容：
----------------------------------------
查询:
{}
----------------------------------------
收集到的资料:
{}
----------------------------------------
输出要求：
请以 JSON 格式输出，输出必须符合以下 JSON Schema，不要输出任何多余的内容：
{}
"#, input_text, materials_text, output_schema_json);

                            let react_response = agent.prompt(prompt.as_str()).await?;
                            println!("🧠 第 {} 次 ReAct 输出:\n{}", iterations + 1, react_response);

                            // 构造自我评分提示词，要求 LLM 对上述 ReAct 输出进行评分（只返回数字）
                            let scoring_prompt = format!(r#"
[自我评分]
请对下面的 ReAct 输出进行自我评分，评分范围为 0.0 到 1.0，1.0 表示输出完全符合要求：
{}
请仅返回一个数字，不要附带任何其他说明。
"#, react_response);
                            let score_response = agent.prompt(scoring_prompt.as_str()).await?;
                            println!("🧠 自我评分反馈: {}", score_response);

                            let score = f64::from_str(score_response.trim()).unwrap_or(0.0);
                            if score >= threshold {
                                // 清理 LLM 输出，去除无关信息
                                let cleaned_response = clean_llm_output(&react_response);
                                println!("🧹 Cleaned response:\n{}", cleaned_response);

                                // 直接尝试解析为 JSON 格式
                                match serde_json::from_str::<ReactOutput>(&cleaned_response) {
                                    Ok(mut output_data) => {
                                        output_data.score = score;
                                        output = Some(output_data);
                                        break;
                                    },
                                    Err(e) => {
                                        println!("⚠️ 无法解析 JSON 输出: {}，请检查输出格式。", e);
                                    }
                                }
                            } else {
                                println!("⚠️ 输出评分 {:.2} 低于阈值 {:.2}，重新生成...", score, threshold);
                                iterations += 1;
                            }
                        }


                        // 若重试后仍无合格输出，则构造一个默认输出
                        if output.is_none() {
                            output = Some(ReactOutput {
                                chain_of_thought: "经过多次尝试，未生成满足要求的输出。".to_string(),
                                final_conclusion: "".to_string(),
                                content: "".to_string(),
                                score: 0.0,
                            });
                        }

                        // 构造新的 FlowMessage，将 ReactOutput 序列化后放入 result 字段
                        let new_flow_msg = FlowMessage {
                            workflow_id: flow_msg.workflow_id.clone(),
                            node_id: app_id.clone(),
                            input: flow_msg.input.clone(),
                            prev_result: flow_msg.result.clone(),
                            result: Some(serde_json::to_value(output.unwrap())?),
                            aggregated: None,
                        };

                        // 发送输出给下游节点
                        node.send_output(
                            DataId::from(result_id(app_id.as_str())),
                            metadata.parameters.clone(),
                            new_flow_msg.into_arrow(),
                        )?;
                        println!("✅ react-node finished processing");
                    }
                    "init" => {
                        println!("🚀 react-node started");
                        // 在注册时，利用 schemars::schema_for! 输出结构化的输入输出格式说明
                        node.send_output(
                            DataId::from(register_id(app_id.as_str())),
                            metadata.parameters.clone(),
                            NodeDescriptor {
                                id: app_id.clone(),
                                description: format!("ReAct 节点：使用链式思考整合工作流信息，自我评分直至生成满足要求的输出，必须需要有前置聚合数据，没有前置数据不能单独直接使用,整体输入:{}",serde_json::to_string_pretty(&schema_for!(ReactInput))?),
                                inputs: serde_json::to_string_pretty(&schema_for!(ReactInput))?,
                                outputs: serde_json::to_string_pretty(&schema_for!(ReactOutput))?,
                                aggregate: true,
                            }.into_arrow(),
                        )?;
                        println!("✅ react-node registered");
                    }
                    other => eprintln!("Ignoring unexpected input `{other}`"),
                }
            }
            Event::Stop => {
                println!("收到 stop 事件 rect节点退出");
                break;
            }
            Event::InputClosed { id } => {
                println!("Input `{id}` was closed");
            }
            other => eprintln!("Received unexpected event: {other:?}"),
        }
    }

    Ok(())
}