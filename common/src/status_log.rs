use std::sync::Arc;
use dora_node_api::{ArrowData, DoraNode, IntoArrow, Metadata};
use dora_node_api::arrow::array::{ArrayRef, StringArray, StructArray, UInt32Array};
use dora_node_api::arrow::datatypes::{DataType, Field};
use dora_node_api::dora_core::config::DataId;
use serde_derive::Serialize;
use serde_json::Value;

#[derive(Serialize,Debug)]
pub struct WorkflowLog {
    pub workflow_id: String,
    pub node_id: String,
    pub step_index: usize,
    pub total_steps: usize,
    pub status: String,
    pub input: serde_json::Value,
    pub output: serde_json::Value,
    pub message: String,
}
pub const WORKFLOW_STATUS:&str= "workflow_status";

pub fn send_status_log(dora_node: &mut DoraNode, metadata: &Metadata, log: WorkflowLog) {
    let _ = dora_node.send_output(
        DataId::from(WORKFLOW_STATUS.to_string()),
        metadata.parameters.clone(),
        log.into_arrow(),
    );
}

impl TryFrom<ArrowData> for WorkflowLog {
    type Error = anyhow::Error;

    fn try_from(data: ArrowData) -> Result<Self, Self::Error> {
        let struct_array = data
            .as_any()
            .downcast_ref::<StructArray>()
            .ok_or_else(|| anyhow::anyhow!("Expected a StructArray"))?;

        let workflow_id = struct_array
            .column_by_name("workflow_id")
            .and_then(|a| a.as_any().downcast_ref::<StringArray>())
            .ok_or_else(|| anyhow::anyhow!("Missing workflow_id"))?
            .value(0)
            .to_string();

        let node_id = struct_array
            .column_by_name("node_id")
            .and_then(|a| a.as_any().downcast_ref::<StringArray>())
            .ok_or_else(|| anyhow::anyhow!("Missing node_id"))?
            .value(0)
            .to_string();

        let step_index = struct_array
            .column_by_name("step_index")
            .and_then(|a| a.as_any().downcast_ref::<UInt32Array>())
            .ok_or_else(|| anyhow::anyhow!("Missing step_index"))?
            .value(0) as usize;

        let total_steps = struct_array
            .column_by_name("total_steps")
            .and_then(|a| a.as_any().downcast_ref::<UInt32Array>())
            .ok_or_else(|| anyhow::anyhow!("Missing total_steps"))?
            .value(0) as usize;

        let status = struct_array
            .column_by_name("status")
            .and_then(|a| a.as_any().downcast_ref::<StringArray>())
            .ok_or_else(|| anyhow::anyhow!("Missing status"))?
            .value(0)
            .to_string();

        let input_str = struct_array
            .column_by_name("input")
            .and_then(|a| a.as_any().downcast_ref::<StringArray>())
            .ok_or_else(|| anyhow::anyhow!("Missing input"))?
            .value(0);
        let input: Value = serde_json::from_str(input_str)?;

        let output_str = struct_array
            .column_by_name("output")
            .and_then(|a| a.as_any().downcast_ref::<StringArray>())
            .ok_or_else(|| anyhow::anyhow!("Missing output"))?
            .value(0);
        let output: Value = serde_json::from_str(output_str)?;

        let message = struct_array
            .column_by_name("message")
            .and_then(|a| a.as_any().downcast_ref::<StringArray>())
            .ok_or_else(|| anyhow::anyhow!("Missing message"))?
            .value(0)
            .to_string();

        Ok(WorkflowLog {
            workflow_id,
            node_id,
            step_index,
            total_steps,
            status,
            input,
            output,
            message,
        })
    }
}

impl IntoArrow for WorkflowLog {
    type A = StructArray;

    fn into_arrow(self) -> Self::A {
        let workflow_id_array = Arc::new(StringArray::from(vec![self.workflow_id])) as ArrayRef;
        let node_id_array = Arc::new(StringArray::from(vec![self.node_id])) as ArrayRef;
        let step_index_array = Arc::new(UInt32Array::from(vec![self.step_index as u32])) as ArrayRef;
        let total_steps_array = Arc::new(UInt32Array::from(vec![self.total_steps as u32])) as ArrayRef;
        let status_array = Arc::new(StringArray::from(vec![self.status])) as ArrayRef;
        let input_array = Arc::new(StringArray::from(vec![self.input.to_string()])) as ArrayRef;
        let output_array = Arc::new(StringArray::from(vec![self.output.to_string()])) as ArrayRef;
        let message_array = Arc::new(StringArray::from(vec![self.message])) as ArrayRef;

        StructArray::from(vec![
            (Arc::new(Field::new("workflow_id", DataType::Utf8, false)), workflow_id_array),
            (Arc::new(Field::new("node_id", DataType::Utf8, false)), node_id_array),
            (Arc::new(Field::new("step_index", DataType::UInt32, false)), step_index_array),
            (Arc::new(Field::new("total_steps", DataType::UInt32, false)), total_steps_array),
            (Arc::new(Field::new("status", DataType::Utf8, false)), status_array),
            (Arc::new(Field::new("input", DataType::Utf8, false)), input_array),
            (Arc::new(Field::new("output", DataType::Utf8, false)), output_array),
            (Arc::new(Field::new("message", DataType::Utf8, false)), message_array),
        ])
    }
}