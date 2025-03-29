use std::sync::Arc;
use anyhow::Result;
use rig::{completion::ToolDefinition, tool::Tool};
use serde::{Deserialize, Serialize};
use serde_json::json;
use crate::register::RouterNode;
use dora_node_api::{DoraNode, Metadata};
use common::descriptor::NodeDescriptor;
use common::tools::ToolsError;

#[derive(Clone)]
pub struct GetNodes {
    pub router: Arc<RouterNode>,
}

impl Tool for GetNodes {
    const NAME: &'static str = "get_nodes";
    type Error = ToolsError;
    type Args = ();
    type Output = Vec<NodeDescriptor>;

    async fn definition(&self, _prompt: String) -> ToolDefinition {

        ToolDefinition {
            name: "get_nodes".to_string(),
            description: "获取当前可用的所有 `Dora` `Node`".to_string(),
            parameters: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {

        let nodes = self.router.get_registered_nodes();
        Ok(nodes)
    }
}

#[derive(Deserialize)]
pub struct SendDataArgs {
    pub node_id: String,
    pub data: String,
}

#[derive(Clone)]
pub struct SendData {
    pub router: Arc<RouterNode>,
}

impl Tool for SendData {
    const NAME: &'static str = "send_data";
    type Error = ToolsError;
    type Args = SendDataArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "send_data".to_string(),
            description: "发送数据到 `Dora` `Node`".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "node_id": { "type": "string", "description": "目标 Node ID" },
                    "data": { "type": "string", "description": "要发送的数据" }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        println!("🚀 触发 `Node` `{}` 处理数据: {}", args.node_id, args.data);
        Ok(format!("✅ `Node` `{}` 处理成功", args.node_id))
    }
}

pub fn parse_output(output: &str) -> Option<(String, String)> {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(output) {
        let node_id = json["next_node"].as_str()?.to_string();
        let data = json["data"].as_str()?.to_string();
        Some((node_id, data))
    } else {
        None
    }
}