use std::process::{Command, Stdio};
use std::time::Duration;
use anyhow::Result;
use rig::{
    completion::{Prompt, ToolDefinition},
    providers,
    tool::Tool,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use thirtyfour::{By, ChromiumLikeCapabilities, DesiredCapabilities, WebDriver};
use tokio::task;
use tokio::time;
use common::tools::ToolsError;
use common::tools::ToolsError::SearchError;

#[derive(Deserialize,JsonSchema)]
pub struct SearchWebArgs {
    query: String,
    site: Option<String>,
    click: Option<bool>,
}

#[derive(Serialize, Deserialize)]
pub struct SearchWebTool;

impl Tool for SearchWebTool {
    const NAME: &'static str = "search_web";

    type Error = ToolsError;
    type Args = SearchWebArgs;
    type Output = Vec<SearchResult>;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "search_web".to_string(),
            description: "使用浏览器执行搜索，并解析搜索结果".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "搜索的关键词"
                    },
                    "site": {
                        "type": "string",
                        "description": "限定搜索的网站 (可选)"
                    },
                    "click": {
                        "type": "boolean",
                        "description": "是否点击搜索结果 (可选)"
                    }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        println!("[tool-call] 执行搜索: {}", args.query);

        // **确保 chromedriver 正在运行**
        ensure_chromedriver().await?;

        let results = task::spawn_blocking(move || {
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(run_web_search(args))
        })
            .await
            .map_err(|e| SearchError(e.to_string()))??;

        Ok(results)
    }
}
#[derive(Serialize, Deserialize,JsonSchema)]
pub struct SearchResult {
    title: String,
    link: String,
    content: Option<String>, // 文章内容
}
async fn run_web_search(args: SearchWebArgs) -> Result<Vec<SearchResult>, ToolsError> {
    let mut caps = DesiredCapabilities::chrome();
    caps.add_arg("--disable-blink-features=AutomationControlled").unwrap(); // 关闭自动化检测
    caps.add_arg("--disable-gpu").unwrap(); // 关闭 GPU 加速
    caps.add_arg("--no-sandbox").unwrap(); // 允许 root 运行
    caps.add_arg("--disable-software-rasterizer").unwrap();
    // caps.add_arg("--disable-dev-shm-usage").unwrap(); // 解决 /dev/shm 共享内存不足
    caps.add_arg("--disable-infobars").unwrap(); // 禁用 “Chrome 正在被自动化控制” 信息栏
    caps.add_arg("--disable-popup-blocking").unwrap(); // 禁用弹出窗口拦截
    caps.add_arg("--disable-features=IsolateOrigins,site-per-process").unwrap(); // 关闭某些隔离功能
    // caps.add_arg("--window-size=1920,1080").unwrap(); // 伪装正常窗口大小
    // caps.add_arg("--start-maximized").unwrap(); // 启动时最大化
    // caps.add_arg("--user-data-dir=/Users/linchenxu/Library/Application Support/Google/Chrome").unwrap();
    // caps.add_arg("--headless=new").unwrap(); // **使用新 headless 模式，绕过检测**
    let driver = WebDriver::new("http://localhost:9515", caps)
        .await
        .map_err(|e| SearchError(e.to_string()))?;

    let search_url = match args.site {
        Some(site) => format!(
            "https://www.google.com/search?q=site:{}+{}",
            site, args.query
        ),
        None => format!("https://www.google.com/search?q={}", args.query),
    };

    driver.goto(&search_url).await.map_err(|e| SearchError(e.to_string()))?;

    tokio::time::sleep(Duration::from_secs(3)).await; // **等待搜索结果加载**


    let elements = driver
        .find_all(By::Css("h3"))
        .await
        .map_err(|e| SearchError(e.to_string()))?;

    let mut results = Vec::new();
    for elem in elements.iter().take(5) {
        if let Ok(title) = elem.text().await {
            if let Ok(parent) = elem.find(By::XPath("ancestor::a")).await {
                if let Ok(link) = parent.attr("href").await {
                    results.push(SearchResult {
                        title,
                        link: link.unwrap_or_default(),
                        content: None,
                    });
                }
            }
        }
    }
    println!("args.click: {:?}", args.click);

    if args.click.unwrap_or(true) && !results.is_empty() {
        for result in &mut results {
            println!("[DEBUG] 点击搜索结果: {}", result.link);
            let res = driver.goto(&result.link).await.map_err(|e| SearchError(e.to_string()));
            if res.is_err() {
                println!("[DEBUG] 无法访问链接: {},err:{}", result.link,res.unwrap_err());
                continue;
            }

            // **等待正文加载**
            time::sleep(Duration::from_secs(2)).await;

            result.content = extract_article_content(&driver).await;

            if result.content.is_none() {
                println!("[DEBUG] 未找到正文，当前页面: {}", driver.current_url().await.unwrap());
            } else {
                println!("[DEBUG] 提取成功: {}", result.content.as_ref().unwrap().chars().take(100).collect::<String>());
            }

            // **返回 Google 搜索页面**
            driver.back().await.map_err(|e| SearchError(e.to_string()))?;
            time::sleep(Duration::from_secs(1)).await; // **等待返回搜索结果**
        }
    }

    driver.quit().await.map_err(|e| SearchError(e.to_string()))?;

    Ok(results)
}
/// **确保 chromedriver 在运行**
async fn ensure_chromedriver() -> Result<(), ToolsError> {
    // 先检查 chromedriver 是否已在运行
    match reqwest::get("http://localhost:9515/status").await {
        Ok(response) if response.status().is_success() => {
            println!("[Chromedriver] 已经在运行");
            return Ok(());
        }
        _ => {
            println!("[Chromedriver] 未运行，尝试启动...");
        }
    }

    // **启动 chromedriver**
    let mut child = Command::new("chromedriver")
        .arg("--port=9515")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| SearchError(format!("无法启动 chromedriver: {}", e)))?;

    // **等待 chromedriver 启动**
    for _ in 0..10 {

        tokio::time::sleep(Duration::from_millis(500)).await;
        if let Ok(response) = reqwest::get("http://localhost:9515/status").await {
            if response.status().is_success() {
                println!("[Chromedriver] 启动成功");
                return Ok(());
            }
        }
    }

    // **如果 5 秒后仍然无法访问，则报错**
    child.kill().ok();
    Err(SearchError("chromedriver 启动失败".to_string()))
}


/// **解析新闻内容**
async fn extract_article_content(driver: &WebDriver) -> Option<String> {
    let possible_selectors = vec![
        "article",                    // 文章正文
        ".article-content",            // 一般网站
        ".news-article",               // 适用于一些新闻站点
        "div.content",                 // 一些新闻站点
        ".post-content",               // 博客文章
        ".entry-content",              // WordPress 博客
        "#main-content",               // 常见正文 ID
        ".story-body",                 // 适用于 BBC
        ".body-copy",                  // 适用于 CNN
        "div#article",                 // CSDN
        "div.article"                  // 其他新闻网站
    ];

    for selector in possible_selectors {
        if let Ok(elem) = driver.find(By::Css(selector)).await {
            if let Ok(text) = elem.text().await {
                println!("[DEBUG] 正文提取成功: {}", text.chars().take(100).collect::<String>());
                return Some(text);
            }
        }
    }

    None
}

#[tokio::test]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().init();

    let openai_client = providers::ollama::Client::new();

    let agent = openai_client
        .agent("qwen2.5-coder:14b")
        .preamble("你是一个搜索助手，可以使用 search_web 工具来执行搜索任务,你应该判断使用 search_web 并将 click 设置为 true，否则不点击。")
        .max_tokens(1024)
        .tool(SearchWebTool)
        .build();

    println!("查询 Rust async runtime 的最新信息");
    println!(
        "AI 搜索助手: {}",
        agent.prompt("查询 Rust async runtime 的最新信息").await?
    );

    Ok(())
}
