use std::sync::Arc;
use dora_node_api::arrow::array::{Array, ArrayRef, AsArray, BooleanArray, StringArray, StructArray};
use dora_node_api::arrow::datatypes::{DataType, Field};
use dora_node_api::{ArrowData, IntoArrow};
use schemars::JsonSchema;
use serde_derive::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NodeDescriptor {
    pub id: String,          // 唯一 ID
    pub description: String, // 节点作用
    pub inputs: String,      // ✅ 整个 inputs 作为 JSON
    pub outputs: String,     // ✅ 整个 outputs 作为 JSON

    // 新增字段，指示该节点是否需要聚合所有前置结果
    #[serde(default)]
    pub aggregate: bool,
}

/// **实现 `IntoArrow`，让 `NodeDescriptor` 直接转换成 `ArrowData`**
impl IntoArrow for NodeDescriptor {
    type A = StructArray;

    fn into_arrow(self) -> Self::A {
        let id_array = Arc::new(StringArray::from(vec![self.id])) as ArrayRef;
        let description_array = Arc::new(StringArray::from(vec![self.description])) as ArrayRef;
        let inputs_array = Arc::new(StringArray::from(vec![self.inputs])) as ArrayRef; // ✅ JSON 作为 String
        let outputs_array = Arc::new(StringArray::from(vec![self.outputs])) as ArrayRef; // ✅ JSON 作为 String
        let aggregate_array = Arc::new(BooleanArray::from(vec![self.aggregate])) as ArrayRef; //

        StructArray::from(vec![
            (Arc::new(Field::new("id", DataType::Utf8, false)), id_array),
            (
                Arc::new(Field::new("description", DataType::Utf8, false)),
                description_array,
            ),
            (
                Arc::new(Field::new("inputs", DataType::Utf8, false)),
                inputs_array,
            ),
            (
                Arc::new(Field::new("outputs", DataType::Utf8, false)),
                outputs_array,
            ),
            (
                Arc::new(Field::new("aggregate", DataType::Boolean, false)),
                aggregate_array,
            ),
        ])
    }
}

impl TryFrom<ArrowData> for NodeDescriptor {
    type Error = anyhow::Error;

    fn try_from(arrow_data: ArrowData) -> Result<Self, Self::Error> {
        // Convert ArrowData to StructArray
        let struct_array = arrow_data.as_struct();

        // Extract each field as a StringArray
        let id_array = struct_array
            .column_by_name("id")
            .ok_or_else(|| anyhow::anyhow!("Missing 'id' field"))?
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| anyhow::anyhow!("'id' is not a StringArray"))?;

        let description_array = struct_array
            .column_by_name("description")
            .ok_or_else(|| anyhow::anyhow!("Missing 'description' field"))?
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| anyhow::anyhow!("'description' is not a StringArray"))?;

        let inputs_array = struct_array
            .column_by_name("inputs")
            .ok_or_else(|| anyhow::anyhow!("Missing 'inputs' field"))?
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| anyhow::anyhow!("'inputs' is not a StringArray"))?;

        let outputs_array = struct_array
            .column_by_name("outputs")
            .ok_or_else(|| anyhow::anyhow!("Missing 'outputs' field"))?
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| anyhow::anyhow!("'outputs' is not a StringArray"))?;

        let aggregate_array = struct_array
            .column_by_name("aggregate")
            .ok_or_else(|| anyhow::anyhow!("Missing 'aggregate' field"))?
            .as_any()
            .downcast_ref::<BooleanArray>()
            .ok_or_else(|| anyhow::anyhow!("'aggregate' is not a BooleanArray"))?;

        // Assuming we're working with the first row of data
        if struct_array.len() == 0 {
            return Err(anyhow::anyhow!("Empty arrow data"));
        }

        // Get values from the first row
        let id = id_array.value(0).to_string();
        let description = description_array.value(0).to_string();
        let inputs = inputs_array.value(0).to_string();
        let outputs = outputs_array.value(0).to_string();
        let aggregate = aggregate_array.value(0);

        Ok(NodeDescriptor {
            id,
            description,
            inputs,
            outputs,
            aggregate,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::clean_llm_output;
    use super::*;
    #[test]
    pub fn test_clean_llm_output() {
        // 示例1：包含 <think> 块和 Markdown 代码块
        let llm_response = r#"
<think>
模型的内部思考内容，不应出现在最终输出中
</think>
```json
[
  {
    "id": "web_search",
    "description": "使用浏览器执行搜索，并解析搜索结果",
    "inputs": "{\"query\": \"rust for linux\", \"click\": true}",
    "outputs": "{\"title\": \"string\", \"link\": \"string\"}"
  }
]
```"#;

        // 示例2：直接输出纯 JSON，无 Markdown 包裹
        let llm_response_pure = r#"
[
  {
    "id": "web_search",
    "description": "使用浏览器执行搜索，并解析搜索结果",
    "inputs": "{\"query\": \"rust for linux\", \"click\": true}",
    "outputs": "{\"title\": \"string\", \"link\": \"string\"}"
  }
]
"#;

        let cleaned_output = clean_llm_output(llm_response);
        println!("清洗后的输出:\n{}", cleaned_output);

        let nodes: Vec<NodeDescriptor> = serde_json::from_str(&cleaned_output)
            .expect("解析 JSON 失败，请检查返回格式是否符合要求");
        println!("解析后的节点列表:");
        for node in nodes {
            println!("{:?}", node);
        }
    }
}
