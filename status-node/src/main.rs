use dora_node_api::{self, dora_core::config::DataId, DoraNode, Event, IntoArrow};
use eyre::Context;
use common::status_log::{WorkflowLog, WORKFLOW_STATUS};

fn main() -> eyre::Result<()> {

    let (mut node, mut events) = DoraNode::init_from_env()?;

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, metadata, data } => match id.as_ref() {
                WORKFLOW_STATUS => {
                    // 尝试解析为 WorkflowLog
                    match WorkflowLog::try_from(data) {
                        Ok(log) => {
                            println!(
                                "✅ 收到 WorkflowLog:\nWorkflow: {}\nStep {}/{}\nNode: {}\nStatus: {}\nMessage: {}\nInput: {}\nOutput: {}\n",
                                log.workflow_id,
                                log.step_index + 1,
                                log.total_steps,
                                log.node_id,
                                log.status,
                                log.message,
                                serde_json::to_string_pretty(&log.input)?,
                                serde_json::to_string_pretty(&log.output)?
                            );
                        }
                        Err(err) => {
                            println!("⚠️ 解析 WorkflowLog 失败: {}", err);
                        }
                    }
                }
                other => eprintln!("ignoring unexpected input {other}"),
            },
            Event::Stop => {
                println!("status-node received stop event");
                break
            }
            Event::InputClosed { id } => {
                println!("input {id} closed");
            }
            other => {
                println!("received unknown event {other:?}");
            }
        }
    }

    Ok(())
}
