use schemars::JsonSchema;
use serde_derive::{Deserialize, Serialize};
use super::*;



#[derive(Serialize, Deserialize, Debug,JsonSchema)]
pub struct ReactInput {
    /// 工作流查询文本，描述当前任务的信息
    pub query: String,
}

#[derive(Serialize, Deserialize, Debug,JsonSchema)]
pub struct ReactOutput {
    /// 详细的链式思考过程
    pub chain_of_thought: String,
    /// 最终得出的结论
    pub final_conclusion: String,
    /// 最终结果
    pub content: String,
    /// 自我评分，范围 0.0 ~ 1.0，1.0 表示输出完美符合要求
    pub score: f64,
}
