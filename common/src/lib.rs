use dora_node_api::{ArrowData, IntoArrow};
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use dora_node_api::arrow::array::{Array, ArrayRef, AsArray, StringArray, StructArray};
use dora_node_api::arrow::datatypes::{DataType, Field, Schema};
use dora_node_api::dora_core::config::DataId;
use schemars::JsonSchema;
use serde_json::Value;
use thiserror::Error;
pub mod tools;

#[derive(Debug, Clone, Serialize, Deserialize,JsonSchema)]
pub struct NodeDescriptor {
    pub id: String,                // 唯一 ID
    pub description: String,        // 节点作用
    pub inputs: String,              // ✅ 整个 inputs 作为 JSON
    pub outputs: String,             // ✅ 整个 outputs 作为 JSON
}


/// **实现 `IntoArrow`，让 `NodeDescriptor` 直接转换成 `ArrowData`**
impl IntoArrow for NodeDescriptor {
    type A = StructArray;

    fn into_arrow(self) -> Self::A {
        let id_array = Arc::new(StringArray::from(vec![self.id])) as ArrayRef;
        let description_array = Arc::new(StringArray::from(vec![self.description])) as ArrayRef;
        let inputs_array = Arc::new(StringArray::from(vec![self.inputs])) as ArrayRef; // ✅ JSON 作为 String
        let outputs_array = Arc::new(StringArray::from(vec![self.outputs])) as ArrayRef; // ✅ JSON 作为 String

        StructArray::from(vec![
            (Arc::new(Field::new("id", DataType::Utf8, false)), id_array),
            (Arc::new(Field::new("description", DataType::Utf8, false)), description_array),
            (Arc::new(Field::new("inputs", DataType::Utf8, false)), inputs_array),
            (Arc::new(Field::new("outputs", DataType::Utf8, false)), outputs_array),
        ])
    }
}

impl TryFrom<ArrowData> for NodeDescriptor {
    type Error = anyhow::Error;

    fn try_from(arrow_data: ArrowData) -> Result<Self, Self::Error> {
        // Convert ArrowData to StructArray
        let struct_array = arrow_data.as_struct();

        // Extract each field as a StringArray
        let id_array = struct_array.column_by_name("id")
            .ok_or_else(|| anyhow::anyhow!("Missing 'id' field"))?
            .as_any().downcast_ref::<StringArray>()
            .ok_or_else(|| anyhow::anyhow!("'id' is not a StringArray"))?;

        let description_array = struct_array.column_by_name("description")
            .ok_or_else(|| anyhow::anyhow!("Missing 'description' field"))?
            .as_any().downcast_ref::<StringArray>()
            .ok_or_else(|| anyhow::anyhow!("'description' is not a StringArray"))?;

        let inputs_array = struct_array.column_by_name("inputs")
            .ok_or_else(|| anyhow::anyhow!("Missing 'inputs' field"))?
            .as_any().downcast_ref::<StringArray>()
            .ok_or_else(|| anyhow::anyhow!("'inputs' is not a StringArray"))?;

        let outputs_array = struct_array.column_by_name("outputs")
            .ok_or_else(|| anyhow::anyhow!("Missing 'outputs' field"))?
            .as_any().downcast_ref::<StringArray>()
            .ok_or_else(|| anyhow::anyhow!("'outputs' is not a StringArray"))?;

        // Assuming we're working with the first row of data
        if struct_array.len() == 0 {
            return Err(anyhow::anyhow!("Empty arrow data"));
        }

        // Get values from the first row
        let id = id_array.value(0).to_string();
        let description = description_array.value(0).to_string();
        let inputs = inputs_array.value(0).to_string();
        let outputs = outputs_array.value(0).to_string();

        Ok(NodeDescriptor {
            id,
            description,
            inputs,
            outputs,
        })
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct FlowMessage {
    pub workflow_id: String,         // 工作流 ID
    pub node_id: String,             // 当前节点 ID（与 NodeDescriptor.id 对应）
    pub input: Value,                // 节点原始输入（NodeDescriptor.inputs 解析后的 JSON）
    pub prev_result: Option<Value>,  // 上一步节点的结果（首次调用时为 None）
    pub result: Option<Value>,       // 当前节点处理后的结果（节点返回时填写）
}

impl IntoArrow for FlowMessage {
    type A = StructArray;

    fn into_arrow(self) -> Self::A {
        // 将 JSON 字段转换为字符串
        let input_str = serde_json::to_string(&self.input).unwrap();
        let prev_result_str = self.prev_result
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap())
            .unwrap_or_default();
        let result_str = self.result
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap())
            .unwrap_or_default();

        let workflow_id_array = Arc::new(StringArray::from(vec![self.workflow_id])) as ArrayRef;
        let node_id_array = Arc::new(StringArray::from(vec![self.node_id])) as ArrayRef;
        let input_array = Arc::new(StringArray::from(vec![input_str])) as ArrayRef;
        let prev_result_array = Arc::new(StringArray::from(vec![prev_result_str])) as ArrayRef;
        let result_array = Arc::new(StringArray::from(vec![result_str])) as ArrayRef;

        StructArray::from(vec![
            (Arc::new(Field::new("workflow_id", DataType::Utf8, false)), workflow_id_array),
            (Arc::new(Field::new("node_id", DataType::Utf8, false)), node_id_array),
            (Arc::new(Field::new("input", DataType::Utf8, false)), input_array),
            (Arc::new(Field::new("prev_result", DataType::Utf8, true)), prev_result_array),
            (Arc::new(Field::new("result", DataType::Utf8, true)), result_array),
        ])
    }
}

impl TryFrom<ArrowData> for FlowMessage {
    type Error = anyhow::Error;

    fn try_from(data: ArrowData) -> Result<Self,Self::Error> {
        // 假设 ArrowData 内部为 StructArray 类型
        let struct_array = data
            .as_any()
            .downcast_ref::<StructArray>()
            .ok_or_else(|| anyhow::anyhow!("Expected a StructArray"))?;

        let workflow_id_array = struct_array.column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| anyhow::anyhow!("Expected StringArray for workflow_id"))?;
        let node_id_array = struct_array.column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| anyhow::anyhow!("Expected StringArray for node_id"))?;
        let input_array = struct_array.column(2)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| anyhow::anyhow!("Expected StringArray for input"))?;
        let prev_result_array = struct_array.column(3)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| anyhow::anyhow!("Expected StringArray for prev_result"))?;
        let result_array = struct_array.column(4)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| anyhow::anyhow!("Expected StringArray for result"))?;

        let workflow_id = workflow_id_array.value(0).to_string();
        let node_id = node_id_array.value(0).to_string();
        let input: Value = serde_json::from_str(input_array.value(0))?;
        let prev_result: Option<Value> = {
            let s = prev_result_array.value(0);
            if s.is_empty() { None } else { Some(serde_json::from_str(s)?) }
        };
        let result: Option<Value> = {
            let s = result_array.value(0);
            if s.is_empty() { None } else { Some(serde_json::from_str(s)?) }
        };

        Ok(FlowMessage {
            workflow_id,
            node_id,
            input,
            prev_result,
            result,
        })
    }
}

/// 将 JSON 字符串转换为 Arrow 数组
fn json_to_arrow(json_str: &str) -> ArrayRef {
    let values: Vec<String> = vec![json_str.to_string()];
    Arc::new(StringArray::from(values)) as ArrayRef
}
pub const REGISTER : &str = "register";
pub const RESULT : &str = "result";

pub fn register_id(id: &str)->DataId{
    DataId::from(format!("{}_{}",REGISTER,id))
}

pub fn result_id(id: &str)->DataId{
    DataId::from(format!("{}_{}",RESULT,id))
}

/// 为了方便在 "result" 分支中调用 TryFrom，提供一个别名函数
pub mod flow_msg {
    use super::{ArrowData, FlowMessage};
    use std::convert::TryFrom;
    pub fn try_from(data: ArrowData) -> Result<FlowMessage, anyhow::Error> {
        FlowMessage::try_from(data)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

}
