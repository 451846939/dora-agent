use dora_node_api::{ArrowData, IntoArrow};
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use dora_node_api::arrow::array::{Array, ArrayRef, AsArray, StringArray, StructArray};
use dora_node_api::arrow::datatypes::{DataType, Field, Schema};
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
#[cfg(test)]
mod tests {
    use super::*;

}
