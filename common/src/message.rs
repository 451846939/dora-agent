use std::sync::Arc;
use dora_node_api::arrow::array::{ArrayRef, StringArray, StructArray};
use dora_node_api::arrow::datatypes::{DataType, Field};
use dora_node_api::{ArrowData, DoraNode, IntoArrow, Metadata};
use dora_node_api::dora_core::config::DataId;
use rig::message::Message;
use schemars::JsonSchema;
use serde_derive::{Deserialize, Serialize};
use serde_json::Value;
use crate::descriptor::NodeDescriptor;


/// 工作流状态，用于记录每个 query 的处理流程
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub id: String,                 // 工作流 ID
    pub query: String,              // 初始 query
    pub steps: Vec<NodeDescriptor>, // 执行步骤（节点列表，由 LLM 规划）
    pub current_index: usize,       // 当前执行的步骤索引
    pub results: Vec<Value>,        // 各步骤返回的结果（使用 Value 保存，便于后续扩展）
    pub chat_log: Vec<Message>, // 聊天上下文记录
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct FlowMessage {
    pub workflow_id: String,        // 工作流 ID
    pub node_id: String,            // 当前节点 ID（与 NodeDescriptor.id 对应）
    pub input: Value,               // 节点原始输入（NodeDescriptor.inputs 解析后的 JSON）
    pub prev_result: Option<Value>, // 上一步节点的结果（首次调用时为 None）
    pub result: Option<Value>,      // 当前节点处理后的结果（节点返回时填写）

    #[serde(default)]
    pub aggregated: Option<String>,  // 如果存在，表示聚合结果应填入此字段
}

impl IntoArrow for FlowMessage {
    type A = StructArray;

    fn into_arrow(self) -> Self::A {
        // 将 JSON 字段转换为字符串
        let input_str = serde_json::to_string(&self.input).unwrap();
        let prev_result_str = self
            .prev_result
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap())
            .unwrap_or_default();
        let result_str = self
            .result
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap())
            .unwrap_or_default();
        let aggregated_str = self
            .aggregated
            .unwrap_or_default();

        let workflow_id_array = Arc::new(StringArray::from(vec![self.workflow_id])) as ArrayRef;
        let node_id_array = Arc::new(StringArray::from(vec![self.node_id])) as ArrayRef;
        let input_array = Arc::new(StringArray::from(vec![input_str])) as ArrayRef;
        let prev_result_array = Arc::new(StringArray::from(vec![prev_result_str])) as ArrayRef;
        let result_array = Arc::new(StringArray::from(vec![result_str])) as ArrayRef;
        let aggregated_array = Arc::new(StringArray::from(vec![aggregated_str])) as dora_node_api::arrow::array::ArrayRef;


        StructArray::from(vec![
            (
                Arc::new(Field::new("workflow_id", DataType::Utf8, false)),
                workflow_id_array,
            ),
            (
                Arc::new(Field::new("node_id", DataType::Utf8, false)),
                node_id_array,
            ),
            (
                Arc::new(Field::new("input", DataType::Utf8, false)),
                input_array,
            ),
            (
                Arc::new(Field::new("prev_result", DataType::Utf8, true)),
                prev_result_array,
            ),
            (
                Arc::new(Field::new("result", DataType::Utf8, true)),
                result_array,
            ),
            (
                Arc::new(Field::new("aggregated", DataType::Utf8, true)),
                aggregated_array,
            ),
        ])
    }
}

impl TryFrom<ArrowData> for FlowMessage {
    type Error = anyhow::Error;

    fn try_from(data: ArrowData) -> Result<Self, Self::Error> {
        // 假设 ArrowData 内部为 StructArray 类型
        let struct_array = data
            .as_any()
            .downcast_ref::<StructArray>()
            .ok_or_else(|| anyhow::anyhow!("Expected a StructArray"))?;

        let workflow_id_array = struct_array
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| anyhow::anyhow!("Expected StringArray for workflow_id"))?;
        let node_id_array = struct_array
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| anyhow::anyhow!("Expected StringArray for node_id"))?;
        let input_array = struct_array
            .column(2)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| anyhow::anyhow!("Expected StringArray for input"))?;
        let prev_result_array = struct_array
            .column(3)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| anyhow::anyhow!("Expected StringArray for prev_result"))?;
        let result_array = struct_array
            .column(4)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| anyhow::anyhow!("Expected StringArray for result"))?;
        let aggregated_array = struct_array
            .column(5)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| anyhow::anyhow!("Expected StringArray for aggregated"))?;


        let workflow_id = workflow_id_array.value(0).to_string();
        let node_id = node_id_array.value(0).to_string();
        let input: Value = serde_json::from_str(input_array.value(0))?;
        let prev_result: Option<Value> = {
            let s = prev_result_array.value(0);
            if s.is_empty() {
                None
            } else {
                Some(serde_json::from_str(s)?)
            }
        };
        let result: Option<Value> = {
            let s = result_array.value(0);
            if s.is_empty() {
                None
            } else {
                Some(serde_json::from_str(s)?)
            }
        };
        let aggregated = {
            let s = aggregated_array.value(0);
            if s.is_empty() { None } else { Some(s.to_string()) }
        };

        Ok(FlowMessage {
            workflow_id,
            node_id,
            input,
            prev_result,
            result,
            aggregated,
        })
    }
}

/// 为了方便在 "result" 分支中调用 TryFrom，提供一个别名函数
pub mod flow_msg {
    use super::{ArrowData, FlowMessage};
    use std::convert::TryFrom;
    pub fn try_from(data: ArrowData) -> Result<FlowMessage, anyhow::Error> {
        FlowMessage::try_from(data)
    }
}
