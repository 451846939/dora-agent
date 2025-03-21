// use anyhow::{ Result};
use common::{NodeDescriptor, flow_msg, register_id, FlowMessage, result_id}; // 假设 NodeDescriptor 和 flow_msg 模块已定义
use dora_node_api::{ArrowData, DoraNode, Event, IntoArrow};
use dora_node_api::dora_core::config::DataId;
use rig::providers;
use schemars::schema_for;
use std::convert::TryFrom;
use std::sync::Arc;
use anyhow::{Context, Error};
use rig::completion::Prompt;
use rig::tool::Tool;
use tracing::info;
use regex::Regex;
use crate::tools::file::{FileOperationArgs, FileTool};

mod tools;

// 引入文件操作工具及参数类型（注意根据你项目的路径调整）
// use common::tools::file::{FileTool, FileOperationArgs};

#[tokio::main]
async fn main() -> eyre::Result<()> {
    println!("🚀 启动 file 节点服务");
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let id="file_op";
    while let Some(event) = events.recv_async().await {
        match event {
            Event::Input { id, metadata, data } => match id.as_str() {
                // 当收到 "file_op" 事件时，处理文件操作任务
                "file_op" => {
                    // 直接将接收到的数据转换为 FlowMessage
                    let mut flow_msg: FlowMessage = FlowMessage::try_from(data)
                        .context("file_op 节点期望接收到 FlowMessage").unwrap();
                    println!("file 节点收到 FlowMessage: {:?}", flow_msg);
                    println!("file 节点收到 input: {:?}", flow_msg.input);
                    // 将 flow_msg.input 转换为字符串（用于构造 prompt）
                    let received_input_str = if let Some(s) = flow_msg.input.as_str() {
                        s.to_string()
                    } else {
                        serde_json::to_string(&flow_msg.input)?
                    };
                    println!("file 节点收到 received_input_str 字符串: {:?}", received_input_str);
                    // 调用 LLM 工具重新组装输入参数
                    let schema = serde_json::to_string_pretty(&schema_for!(Vec<FileOperationArgs>))
                        .expect("生成 JSON Schema 失败");
                    let prompt = format!(
                        r#"请根据下面的输入内容生成一个合法的 JSON 参数，该参数必须符合下面的 JSON Schema，且所有文件路径必须位于 `{}` 下，不要生成随机目录。
                        JSON Schema: {}
                        输入内容: {}
                        请只返回合法的 JSON 参数，不要添加任何额外说明。"#,
                        "./",
                        schema,received_input_str
                    );
                    println!("LLM Prompt: {}", prompt);

                    let openai_client = providers::ollama::Client::new();
                    let agent = openai_client
                        .agent("qwen2.5-coder:14b")
                        .tool(FileTool)
                        .preamble("你是一个文件操作输入组装助手，请根据给出的输入和 JSON Schema 重新生成合法的 JSON 参数。")
                        .max_tokens(256)
                        .build();
                    let reassembled = agent
                        .prompt(prompt.as_str())
                        .await
                        .context("调用 LLM 重新组装输入失败");
                    println!("LLM 返回的组装结果: {:?}", reassembled);

                    if reassembled.is_err() {
                        let err = reassembled.unwrap_err();
                        eprintln!("重新组装输入失败: {:?}", err);
                        // let error_msg = format!("重新组装输入失败: {:?}", err);
                        // node.send_output(
                        //     DataId::from("error".to_string()),
                        //     metadata.parameters,
                        //     error_msg.into_arrow(),
                        // )?;
                        continue;
                    }
                    let mut res = reassembled.unwrap();

                    let re = Regex::new(r"(?s)^```json\s*\n(.*)\n```").unwrap();
                    if let Some(captures) = re.captures(&res) {
                        res = captures[1].to_string();
                    }
                    // 将大模型返回的合法 JSON 解析为 FileOperationArgs
                    let file_args = serde_json::from_str(res.as_str())
                        .context("解析重新组装的 JSON 参数失败").unwrap();

                    let file_tool = FileTool;
                    // 调用文件工具执行实际文件操作
                    let result = file_tool.call(file_args).await?;
                    println!("文件操作结果: {:?}", result);
                    // let result=reassembled.unwrap();
                    // 构造 NodeDescriptor 返回结果，outputs 字段放置操作结果
                    let app_id = "file_op";
                    let new_flow_msg = FlowMessage {
                        workflow_id: flow_msg.workflow_id.clone(),
                        node_id: app_id.to_string(),
                        input: flow_msg.input.clone(),
                        prev_result: flow_msg.result.clone(),
                        result: Some(serde_json::to_value(result)?),
                    };
                    node.send_output(
                        result_id(app_id),
                        metadata.parameters,
                        new_flow_msg.into_arrow(),
                    )?;
                }
                // 初始化事件：注册当前节点信息到 router
                "init" => {
                    info!("🔍 file 节点启动");
                    let id = "file_op";
                    let registration = NodeDescriptor {
                        id: id.to_string(),
                        description: "文件操作节点，支持创建文件夹、删除文件夹、创建文件、删除文件、更新文件和读取文件".to_string(),
                        inputs: serde_json::to_string_pretty(&schema_for!(Vec<FileOperationArgs>)).unwrap(),
                        outputs: "字符串类型，操作结果或读取的文件内容".to_string(),
                    };
                    node.send_output(
                        register_id(id),
                        metadata.parameters,
                        registration.into_arrow(),
                    )?;
                    info!("🔍 file 节点已注册");
                }
                other => {
                    eprintln!("忽略未知输入事件: {}", other);
                }
            },
            Event::Stop => {
                println!("收到 Stop 事件，file 节点退出");
                break;
            }
            Event::InputClosed { id } => {
                println!("输入 {} 被关闭", id);
            }
            other => {
                eprintln!("收到未知事件: {:?}", other);
            }
        }
    }
    Ok(())
}