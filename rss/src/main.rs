use eyre::Result;
use dora_node_api::{DoraNode, Event, IntoArrow};
use dora_node_api::dora_core::config::DataId;
use eyre::Result as EyreResult;
use feed_rs::parser;
use reqwest;
use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use common::{register_id, result_id};
use futures::future::join_all;
use common::config::AppConfig;
use common::descriptor::NodeDescriptor;
use common::message::FlowMessage;

#[derive(Debug)]
pub struct FeedSource {
    pub name: &'static str,
    pub url: &'static str,
}

/// ✅ 固定 RSSHub RSS 源
pub const FIXED_FEED_SOURCES: &[FeedSource] = &[
    // FeedSource {
    //     name: "科技新闻（简中）",
    //     url: "https://rsshub.rssforever.com/google/news/科技/hl=zh-CN&gl=CN&ceid=CN:zh",
    // },
    FeedSource {
        name: "热点新闻（英文）",
        url: "https://rsshub.app/google/news/Top stories/hl=en-US&gl=US&ceid=US:en",
    },
    FeedSource {
        name: "国际新闻（英文）",
        url: "https://rsshub.app/google/news/World/hl=en-US&gl=US&ceid=US:en",
    },
    FeedSource {
        name: "科技（英文）",
        url: "https://rsshub.app/google/news/Technology/hl=en-US&gl=US&ceid=US:en",
    },
    FeedSource {
        name: "财经（英文）",
        url: "https://rsshub.app/google/news/Business/hl=en-US&gl=US&ceid=US:en",
    },
];

/// ✅ 输入只包含关键词
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
pub struct RssInput {
    pub keywords: Vec<String>,
}

/// ✅ RSS 项结构体
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
pub struct RssItem {
    pub title: String,
    pub link: String,
    pub summary: String,
    pub published: String,
}

/// ✅ 输出包含匹配项
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
pub struct RssOutput {
    pub content: Vec<RssItem>,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 start rss-node");
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let app_id = "rss".to_string();

    while let Some(event) = events.recv_async().await {
        match event {
            Event::Input { id, metadata, data } => {
                match id.as_str() {
                    "rss" => {
                        let flow_msg: FlowMessage = FlowMessage::try_from(data)
                            .expect("expected FlowMessage");
                        println!("📥 rss-node received FlowMessage: {:?}", flow_msg);

                        let rss_input: RssInput = if let Value::String(s) = &flow_msg.input {
                            serde_json::from_str(s).expect("failed to deserialize RssInput")
                        } else {
                            serde_json::from_value(flow_msg.input.clone())
                                .expect("failed to deserialize RssInput")
                        };

                        let feeds_results = join_all(
                            FIXED_FEED_SOURCES.iter().map(|source| async {
                                match fetch_feed(source.url).await {
                                    Ok(feed) => {
                                        println!("✅ 成功解析: {}", source.name);
                                        Some(parse_feed_items(&feed, &rss_input.keywords))
                                    }
                                    Err(e) => {
                                        eprintln!("❌ 失败: {} ({})", source.name, e);
                                        None
                                    }
                                }
                            })
                        ).await;
                        println!("✅ rss-node finished fetching feeds :{:?}", feeds_results);
                        let all_items: Vec<RssItem> = feeds_results.into_iter().flatten().flatten().collect();
                        let output = RssOutput { content: all_items };
                        let new_flow_msg = FlowMessage {
                            workflow_id: flow_msg.workflow_id.clone(),
                            node_id: app_id.clone(),
                            input: flow_msg.input.clone(),
                            prev_result: flow_msg.result.clone(),
                            result: Some(serde_json::to_value(output)?),
                            aggregated: None,
                        };
                        node.send_output(
                            DataId::from(result_id(app_id.as_str())),
                            metadata.parameters.clone(),
                            new_flow_msg.into_arrow(),
                        )?;
                        println!("✅ rss-node finished processing");
                    }
                    "init" => {
                        node.send_output(
                            DataId::from(register_id(app_id.as_str())),
                            metadata.parameters.clone(),
                            NodeDescriptor {
                                id: app_id.clone(),
                                description: "RSS 节点：抓取固定 Google News RSS，并根据关键词过滤".to_string(),
                                inputs: serde_json::to_string_pretty(&schema_for!(RssInput))?,
                                outputs: serde_json::to_string_pretty(&schema_for!(RssOutput))?,
                                aggregate: false,
                            }
                                .into_arrow(),
                        )?;
                        println!("✅ rss-node registered");
                    }
                    other => eprintln!("Ignoring unexpected input `{other}`"),
                }
            }
            Event::Stop => {
                println!("🛑 收到 stop 事件，rss 节点退出");
                break;
            }
            Event::InputClosed { id } => println!("Input `{id}` was closed"),
            other => eprintln!("Received unexpected event: {other:?}"),
        }
    }
    Ok(())
}

/// ✅ 拉取并解析 feed
async fn fetch_feed(feed_url: &str) -> EyreResult<feed_rs::model::Feed> {
    let response = reqwest::get(feed_url).await?;

    // 提前复制要用的字段
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if !content_type.contains("xml") {
        let body_text = response.text().await.unwrap_or_else(|_| "<无法读取正文>".into());
        println!(
            "❌ 非 XML 响应：\nStatus: {}\nContent-Type: {}\nBody Preview:\n{}",
            status,
            content_type,
            &body_text[..200.min(body_text.len())]
        );
        return Err(eyre::eyre!("Invalid content type: {}", content_type));
    }

    let body = response.bytes().await?;
    let feed = parser::parse(&body[..])?;
    Ok(feed)
}

/// ✅ 解析并根据关键词过滤项
pub fn parse_feed_items(feed: &feed_rs::model::Feed, keywords: &[String]) -> Vec<RssItem> {
    let mut items = Vec::new();
    for entry in &feed.entries {
        let title = entry.title.as_ref().map(|t| t.content.clone()).unwrap_or_default();
        let summary = entry.summary.as_ref().map(|s| s.content.clone()).unwrap_or_default();
        let published = entry.published.map(|d| d.to_rfc3339()).unwrap_or_default();
        let combined = format!("{} {}", title, summary).to_lowercase();

        let word_list: Vec<&str> = combined
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| !s.is_empty())
            .collect();

        if keywords.iter().any(|kw| {
            let kw_lc = kw.to_lowercase();
            word_list.iter().any(|word| word == &kw_lc)
        }) {
            let link_url = entry.links.get(0).map(|l| l.href.clone()).unwrap_or_default();
            items.push(RssItem {
                title,
                link: link_url,
                summary,
                published,
            });
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio;

    #[tokio::test]
    async fn test_all_fixed_feeds() {
        let keywords = vec!["AI".to_string(), "人工智能".to_string(), "芯片".to_string()];

        let fetches = FIXED_FEED_SOURCES.iter().map(|source| {
        let value = keywords.clone();
        async move {
            println!("\n🌐 测试源: {}", source.name);
            match fetch_feed(source.url).await {
                Ok(feed) => {
                    let items = parse_feed_items(&feed, &value);
                    println!("✅ 成功解析 {} 条匹配项", items.len());
                    for item in &items {
                        println!("🔹 [{}] {}", item.published, item.title);
                        println!("🔗 {}", item.link);
                    }
                    assert!(
                        !items.is_empty(),
                        "❌ [{}] 没有找到关键词匹配项", source.name
                    );
                }
                Err(err) => {
                    eprintln!("❌ [{}] 解析失败: {}", source.name, err);
                    panic!("Feed [{}] failed: {}", source.name, err);
                }
            }
        }
        });

        futures::future::join_all(fetches).await;
    }
}