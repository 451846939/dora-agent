use anyhow::Result;
use rig::{
    completion::{Prompt, ToolDefinition},
    providers,
    tool::Tool,
};
use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::Path;
use tokio::task;
use common::tools::ToolsError;
use common::tools::ToolsError::FileError; // 假设 ToolsError 定义了 FileError 变体

/// 定义文件操作的参数结构体
#[derive(Deserialize, JsonSchema, Debug)]
pub struct FileOperationArgs {
    /// 操作类型，支持 "create_folder", "delete_folder", "create_file",
    /// "delete_file", "update_file", "read_file"
    pub operation: String,
    /// 文件或文件夹的路径（可以是相对路径或绝对路径）
    pub path: String,
    /// 文件内容（用于创建或更新文件，其他操作可忽略）
    pub content: Option<String>,
}

/// 定义文件工具
#[derive(Serialize, Deserialize)]
pub struct FileTool;

impl Tool for FileTool {
    const NAME: &'static str = "file_op";

    type Error = ToolsError;
    /// 接收文件操作任务数组，每个元素是一个 FileOperationArgs
    type Args = Vec<FileOperationArgs>;
    /// 返回结果为字符串数组，每个元素对应一个操作的执行结果
    type Output = Vec<String>;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        // 使用 Vec<FileOperationArgs> 的 JSON Schema 描述参数格式
        let schema = serde_json::to_value(&schema_for!(Vec<FileOperationArgs>))
            .unwrap_or(json!({}));
        ToolDefinition {
            name: "file_op".to_string(),
            description: "处理文件或文件夹的批量操作，包括创建、删除、更新和读取。文件夹的父节点暂定为当前文件夹".to_string(),
            parameters: schema,
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        println!("执行文件操作批量任务: {:?}", args);
        let mut results = Vec::new();
        for op in args.into_iter() {
            let res = task::spawn_blocking(move || -> Result<String, ToolsError> {
                match op.operation.as_str() {
                    "create_folder" => {
                        fs::create_dir_all(&op.path)
                            .map_err(|e| FileError(format!("创建文件夹 {} 失败: {}", op.path, e)))?;
                        Ok(format!("文件夹 {} 创建成功", op.path))
                    },
                    "delete_folder" => {
                        fs::remove_dir_all(&op.path)
                            .map_err(|e| FileError(format!("删除文件夹 {} 失败: {}", op.path, e)))?;
                        Ok(format!("文件夹 {} 删除成功", op.path))
                    },
                    "create_file" => {
                        let content = op.content.unwrap_or_default();
                        if let Some(parent) = Path::new(&op.path).parent() {
                            fs::create_dir_all(parent)
                                .map_err(|e| FileError(format!("创建父目录失败: {}", e)))?;
                        }
                        fs::write(&op.path, content)
                            .map_err(|e| FileError(format!("创建文件 {} 失败: {}", op.path, e)))?;
                        Ok(format!("文件 {} 创建成功", op.path))
                    },
                    "update_file" => {
                        let content = op.content.unwrap_or_default();
                        if !Path::new(&op.path).exists() {
                            return Err(FileError(format!("文件 {} 不存在，无法更新", op.path)));
                        }
                        fs::write(&op.path, content)
                            .map_err(|e| FileError(format!("更新文件 {} 失败: {}", op.path, e)))?;
                        Ok(format!("文件 {} 更新成功", op.path))
                    },
                    "delete_file" => {
                        fs::remove_file(&op.path)
                            .map_err(|e| FileError(format!("删除文件 {} 失败: {}", op.path, e)))?;
                        Ok(format!("文件 {} 删除成功", op.path))
                    },
                    "read_file" => {
                        let content = fs::read_to_string(&op.path)
                            .map_err(|e| FileError(format!("读取文件 {} 失败: {}", op.path, e)))?;
                        Ok(content)
                    },
                    other => Err(FileError(format!("不支持的操作类型: {}", other))),
                }
            })
                .await
                .map_err(|e| FileError(format!("任务执行失败: {}", e)))??;
            results.push(res);
        }
        Ok(results)
    }
}