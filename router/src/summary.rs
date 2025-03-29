use anyhow::Result;
use common::message::{FlowMessage, Workflow};
use serde_json::Value;
use rig::agent::Agent;
use rig::completion::{Chat, Prompt};

/// 对 workflow 执行结果进行总结
pub async fn summarize_results(
    agent: &Agent<rig::providers::openai::CompletionModel>,
    workflow: &Workflow,
) -> Result<String> {
    let prompt = format!(
        r#"
你是一个智能助手，根据query和上下文负责总结整个工作流 id:`{id}` 的执行。以下是每个步骤的输出结果，请综合这些信息，用人类语言描述最终这个query在这个工作流的过程和结果：
query:
{query}
results:
{results}

详细总结整个过程和结果，不要出现任何代码和敏感信息。
        "#,
        id=workflow.id,
        query=workflow.query,
        results=workflow.results
            .iter()
            .enumerate()
            .map(|(i, r)| format!("步骤 {} 输出：{}", i + 1, r))
            .collect::<Vec<_>>()
            .join("\n")
    );

    let response = agent.chat(prompt.as_str(),workflow.chat_log.clone()).await?;
    Ok(response.trim().to_string())
}