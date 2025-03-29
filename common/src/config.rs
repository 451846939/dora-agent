use config::{Config, ConfigError, File};
use serde::Deserialize;
use std::collections::HashMap;
use rig::providers::openai::Client;

/// 节点配置结构体，表示每个节点对应的 OpenAI 配置
#[derive(Debug, Deserialize, Clone)]
pub struct NodeConfig {
    pub key: String,
    pub url: String,
    pub model: String,
}

/// 统一配置，所有节点配置以 HashMap 形式存储，key 为节点标识（对应 TOML 中的表名）
#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub nodes: HashMap<String, NodeConfig>,
}

impl AppConfig {
    /// 从项目根目录下的 config/config.toml 文件加载配置
    /// 注意：程序的当前工作目录需要为项目根目录
    pub fn from_file() -> Result<Self, ConfigError> {
        let mut settings = Config::builder().add_source(File::with_name("config.toml")).build()?;
        let nodes: HashMap<String, NodeConfig> = settings.try_deserialize()?;
        Ok(AppConfig { nodes })
    }

    /// 根据节点标识（appid）查找对应的节点配置
    pub fn get_node_config(&self, appid: &str) -> Option<&NodeConfig> {
        self.nodes.get(appid)
    }

    /// 为指定的节点（根据 appid）创建 OpenAI 客户端
    /// 返回值为 Some((Client, &NodeConfig))，否则返回 None
    pub fn get_client_for_node(&self, appid: &str) -> Option<(Client, &NodeConfig)> {
        self.get_node_config(appid).map(|node_cfg| {
            let client = Client::from_url(&node_cfg.key, &node_cfg.url);
            (client, node_cfg)
        })
    }
    /// 一行式：加载配置并获取某个 appid 的 OpenAI 客户端和配置
    pub fn from_file_with_appid(appid: &str) -> Result<(Client, NodeConfig), ConfigError> {
        let config = AppConfig::from_file()?;
        if let Some(node_cfg) = config.get_node_config(appid) {
            Ok((Client::from_url(&node_cfg.key, &node_cfg.url), node_cfg.clone()))
        } else {
            Err(ConfigError::Message(format!("找不到 appid: {}", appid)))
        }
    }
}