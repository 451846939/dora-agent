use dora_node_api::arrow::array::{
    Array, ArrayRef, AsArray, BooleanArray, StringArray, StructArray,
};
use dora_node_api::arrow::datatypes::{DataType, Field, Schema};
use dora_node_api::dora_core::config::DataId;
use dora_node_api::{ArrowData, IntoArrow};
use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use schemars::schema::InstanceType::Boolean;
use thiserror::Error;
pub mod tools;
pub mod config;
pub mod descriptor;
pub mod message;

pub mod status_log;


/// 将 JSON 字符串转换为 Arrow 数组
fn json_to_arrow(json_str: &str) -> ArrayRef {
    let values: Vec<String> = vec![json_str.to_string()];
    Arc::new(StringArray::from(values)) as ArrayRef
}
pub const REGISTER: &str = "register";
pub const RESULT: &str = "result";

pub fn register_id(id: &str) -> DataId {
    DataId::from(format!("{}_{}", REGISTER, id))
}

pub fn result_id(id: &str) -> DataId {
    DataId::from(format!("{}_{}", RESULT, id))
}

pub fn clean_llm_output(output: &str) -> String {
    // 先移除所有 <think> 块
    let re_think = Regex::new(r"(?s)<think>.*?</think>").unwrap();
    let without_think = re_think.replace_all(output, "").to_string();

    // 尝试检测 Markdown 代码块（例如 ```json ... ```）
    let re_md = Regex::new(r"(?s)^```(?:json)?\s*\n?(.*)\n?```$").unwrap();
    if let Some(captures) = re_md.captures(without_think.trim()) {
        return captures.get(1).unwrap().as_str().to_string();
    }
    // 如果没有 Markdown 包裹，则认为输出为纯 JSON
    without_think.trim().to_string()
}


