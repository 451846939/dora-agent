use std::collections::HashSet;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;
use anyhow::Result;
use rig::{
    completion::{Prompt, ToolDefinition},
    providers,
    tool::Tool,
};
use rig::embeddings::EmbeddingModel;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use thirtyfour::{By, ChromiumLikeCapabilities, DesiredCapabilities, WebDriver, WindowHandle};
use tokio::sync::Mutex;
use tokio::task;
use tokio::time;
use tracing::{error, info};
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
        info!("[tool-call] 执行搜索: {}", args.query);

        // **确保 chromedriver 正在运行**
        ensure_chromedriver().await?;

        // let results = task::spawn_blocking(move || {
        //     tokio::runtime::Runtime::new()
        //         .unwrap()
        //         .block_on(run_web_search(args))
        // })
        // .await
        // .map_err(|e| SearchError(e.to_string()))??;

        // **使用 `spawn_blocking` 让 `run_web_search` 运行在 tokio 线程池中**
        let results = tokio::task::spawn_blocking(move || {
            // **使用当前 tokio 运行时 `Handle`，避免新建 `Runtime`**
            let handle = tokio::runtime::Handle::current();
            handle.block_on(run_web_search(args))
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

async fn extract_search_results(driver: &WebDriver) -> Vec<(String, String)> {
    let mut results = Vec::new();

    let elements = driver.find_all(By::Css("h3")).await.unwrap_or(vec![]);
    for elem in elements.iter().take(5) {
        if let Ok(title) = elem.text().await {
            if let Ok(parent) = elem.find(By::XPath("ancestor::a")).await {
                if let Ok(link) = parent.attr("href").await {
                    results.push((title, link.unwrap_or_default()));
                }
            }
        }
    }

    results
}

async fn run_web_search(args: SearchWebArgs) -> Result<Vec<SearchResult>, ToolsError> {
    let mut caps = DesiredCapabilities::chrome();
    caps.add_arg("--disable-blink-features=AutomationControlled").unwrap(); // 关闭自动化检测
    caps.add_arg("--disable-gpu").unwrap(); // 关闭 GPU 加速
    caps.add_arg("--no-sandbox").unwrap(); // 允许 root 运行
    // caps.add_arg("--disable-dev-shm-usage").unwrap(); // 解决 /dev/shm 共享内存不足
    // caps.add_arg("--disable-infobars").unwrap(); // 禁用 “Chrome 正在被自动化控制” 信息栏
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

    tokio::time::sleep(Duration::from_secs(2)).await; // **等待搜索结果加载**


    // let elements = driver
    //     .find_all(By::Css("h3"))
    //     .await
    //     .map_err(|e| SearchError(e.to_string()))?;
    //
    // let mut results = Vec::new();
    // for elem in elements.iter().take(4) {
    //     if let Ok(title) = elem.text().await {
    //         if let Ok(parent) = elem.find(By::XPath("ancestor::a")).await {
    //             if let Ok(link) = parent.attr("href").await {
    //                 results.push(SearchResult {
    //                     title,
    //                     link: link.unwrap_or_default(),
    //                     content: None,
    //                 });
    //             }
    //         }
    //     }
    // }
    // info!("args.click: {:?}", args.click);
    //
    // // if args.click.unwrap_or(true) && !results.is_empty() {
    // //     for result in &mut results {
    // //         info!("[DEBUG] 点击搜索结果: {}", result.link);
    // //         driver.goto(&result.link).await.map_err(|e| SearchError(e.to_string()))?;
    // //
    // //         // **等待正文加载**
    // //         time::sleep(Duration::from_secs(2)).await;
    // //
    // //         result.content = extract_article_content(&driver).await;
    // //
    // //         if result.content.is_none() {
    // //             info!("[DEBUG] 未找到正文，当前页面: {}", driver.current_url().await.unwrap());
    // //         } else {
    // //             info!("[DEBUG] 提取成功: {}", result.content.as_ref().unwrap().chars().take(100).collect::<String>());
    // //         }
    // //
    // //         // **返回 Google 搜索页面**
    // //         driver.back().await.map_err(|e| SearchError(e.to_string()))?;
    // //         time::sleep(Duration::from_secs(1)).await; // **等待返回搜索结果**
    // //     }
    // // }
    // if args.click.unwrap_or(true) && !results.is_empty() {
    //     for result in &mut results {
    //         driver.goto(&result.link).await.map_err(|e| SearchError(e.to_string()))?;
    //
    //         click_read_more_if_needed(&driver).await; // **尝试点击「阅读更多」**
    //         tokio::time::sleep(Duration::from_secs(2)).await; // 等待加载
    //
    //         result.content = extract_article_content(&driver).await; // **用 LLM 提取正文**
    //     }
    // }



    // 提取搜索结果 (title, link)
    let search_results = extract_search_results(&driver).await;
    info!("[DEBUG] 搜索结果: {:?}", search_results);

    // 记录原始标签页句柄
    let original_handle = driver.window().await
        .map_err(|e| SearchError(format!("无法获取原始标签页句柄: {}", e)))?;
    info!("[DEBUG] 原始标签页句柄: {}", original_handle);

    let visited = Arc::new(Mutex::new(HashSet::new()));

    // 计算搜索关键词的嵌入（用于相关性判断，此处简化）
    let query_embedding = get_text_embedding(&args.query).await.unwrap_or_default();

    let mut results = Vec::new();
    // 对每个搜索结果逐个进行爬取（依次打开新标签页、提取内容、关闭标签页）
    for (_title, link) in search_results {
        let res = crawl_page_with_parent(
            &driver,
            link.clone(),
            0,
            2,  // 最大递归深度
            query_embedding.clone(),
            Arc::clone(&visited),
            &original_handle,
        ).await?;
        results.extend(res);
    }


    driver.quit().await.map_err(|e| SearchError(e.to_string()))?;

    Ok(results)
}
/// **确保 chromedriver 在运行**
async fn ensure_chromedriver() -> Result<(), ToolsError> {
    // 先检查 chromedriver 是否已在运行
    match reqwest::get("http://localhost:9515/status").await {
        Ok(response) if response.status().is_success() => {
            info!("[Chromedriver] 已经在运行");
            return Ok(());
        }
        _ => {
            info!("[Chromedriver] 未运行，尝试启动...");
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
                info!("[Chromedriver] 启动成功");
                return Ok(());
            }
        }
    }

    // **如果 5 秒后仍然无法访问，则报错**
    child.kill().ok();
    Err(SearchError("chromedriver 启动失败".to_string()))
}


/// **解析新闻内容**
// async fn extract_article_content(driver: &WebDriver) -> Option<String> {
//     let possible_selectors = vec![
//         "article",                    // 文章正文
//         ".article-content",            // 一般网站
//         ".news-article",               // 适用于一些新闻站点
//         "div.content",                 // 一些新闻站点
//         ".post-content",               // 博客文章
//         ".entry-content",              // WordPress 博客
//         "#main-content",               // 常见正文 ID
//         ".story-body",                 // 适用于 BBC
//         ".body-copy",                  // 适用于 CNN
//         "div#article",                 // CSDN
//         "div.article"                  // 其他新闻网站
//     ];
//
//     for selector in possible_selectors {
//         if let Ok(elem) = driver.find(By::Css(selector)).await {
//             if let Ok(text) = elem.text().await {
//                 info!("[DEBUG] 正文提取成功: {}", text.chars().take(100).collect::<String>());
//                 return Some(text);
//             }
//         }
//     }
//
//     None
// }

/// 让 LLM 解析网页 HTML 结构，自动提取正文
async fn extract_article_content(driver: &WebDriver) -> Option<String> {
    tokio::time::sleep(Duration::from_secs(3)).await;

    let current_url = driver.current_url().await.unwrap();

    let html = driver.source().await.ok()?;
    info!("[DEBUG] 当前 URL: {}, HTML 代码前 500 字符:\n{}", current_url, html.chars().take(500).collect::<String>());

    let openai_client = providers::ollama::Client::new();
    let agent = openai_client
        .agent("qwen2.5:14b")
        .preamble("你是一个网页解析助手，输入 HTML 代码，返回正文的 CSS 选择器")
        .max_tokens(64)
        .build();

    let prompt = format!(
        "你是一个网页解析助手。请分析以下 HTML，并返回该页面正文的 **CSS 选择器**。\n\
        **注意**：\n\
        - 只返回 CSS 选择器，不要解释 HTML 结构。\n\
        - 选择器必须是 `.class` 或 `#id` 形式。\n\
        - **不要返回 JSON，不要返回 HTML 代码，不要写解释性文字**。\n\
        - 如果正文不存在，返回 `NONE`。\n\
        \n\
        **示例 1:**\n\
        - 输入: HTML 代码\n\
        - 输出: `.article-body`\n\
        \n\
        **示例 2:**\n\
        - 输入: HTML 代码\n\
        - 输出: `#post-content`\n\
        \n\
        **示例 3:**\n\
        - 输入: HTML 代码（无正文）\n\
        - 输出: `NONE`\n\
        \n\
        **HTML 代码:**\n\n{}",
        html
    );

    let response = agent.prompt(prompt).await.ok()?;
    let selector = response.trim();

    if selector == "NONE" {
        error!("[ERROR] LLM 解析失败: 该页面没有正文");
        return None;
    }

    if !selector.starts_with('.') && !selector.starts_with('#') && !selector.contains(" ") {
        error!("[ERROR] LLM 解析失败: 预期返回 CSS 选择器，但返回: {}", selector);
        return None;
    }

    if let Ok(elem) = driver.find(By::Css(selector)).await {
        if let Ok(text) = elem.text().await {
            info!("[DEBUG] 成功提取正文: {}", text.chars().take(100).collect::<String>());
            return Some(text);
        }
    }

    error!("[ERROR] 选择器 `{}` 无法找到正文，尝试常见选择器", selector);
        let common_selectors = vec![
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
            "div.article",                  // 其他新闻网站
            "#content",
            "main",

        ];

    // **回退到常见的正文选择器**
    // let common_selectors = ["article", "main", ".post", "#content", ".entry-content"];
    for &sel in &common_selectors {
        if let Ok(elem) = driver.find(By::Css(sel)).await {
            if let Ok(text) = elem.text().await {
                info!("[DEBUG] 使用 `{}` 成功提取正文: {}", sel, text.chars().take(100).collect::<String>());
                return Some(text);
            }
        }
    }

    error!("[ERROR] 尝试所有选择器后仍然无法提取正文");
    None
}


async fn click_read_more_if_needed(driver: &WebDriver) -> bool {
    info!("[DEBUG] 尝试查找并点击“阅读更多”按钮...");

    let html = driver.source().await.ok().unwrap();
    let openai_client = providers::ollama::Client::new();
    let agent = openai_client
        .agent("qwen2.5:14b")
        .preamble("你是一个网页解析助手，输入 HTML 代码，返回 '阅读更多' 按钮的 CSS 选择器")
        .max_tokens(64)
        .build();

    let prompt = format!(
        "请从以下 HTML 中提取 **'阅读更多' 按钮** 的 CSS 选择器：\n\
        - 仅返回 **.class** 或 **#id**，不返回 HTML 代码。\n\
        - 不能返回 JSON，只能返回一个 CSS 选择器。\n\
        - 如果找不到按钮，请返回 `NONE`。\n\n\
        **HTML 代码:**\n\n{}",
        html
    );

    let response = agent.prompt(prompt).await.ok().unwrap();
    let selector = response.trim();

    if selector == "NONE" {
        info!("[DEBUG] 没有找到 '阅读更多' 按钮");
        return false;
    }

    if !selector.starts_with('.') && !selector.starts_with('#') {
        error!("[ERROR] LLM 返回了无效的选择器: `{}`", selector);
        return false;
    }

    info!("[DEBUG] LLM 解析出的 '阅读更多' 按钮选择器: `{}`", selector);

    // **尝试滚动到底部，确保按钮可见**
    let _ = driver.execute_script("window.scrollTo(0, document.body.scrollHeight);", vec![]).await;

    // **查找按钮并点击**
    if let Ok(elem) = driver.find(By::Css(selector)).await {
        if let Ok(true) = elem.is_displayed().await {
            if let Ok(_) = elem.click().await {
                info!("[DEBUG] 成功点击 '阅读更多' 按钮");
                return true;
            } else {
                error!("[ERROR] 点击 '阅读更多' 按钮失败，尝试 `execute_script`");
                let script = format!("document.querySelector('{}').click();", selector);
                let _ = driver.execute_script(&script, vec![]).await;
                return true;
            }
        } else {
            error!("[ERROR] 按钮 `{}` 存在，但不可见", selector);
        }
    } else {
        error!("[ERROR] 无法找到按钮 `{}`，尝试常见选择器", selector);
    }

    // **尝试常见的 '阅读更多' 按钮选择器**
    let common_selectors = [".read-more", ".more-button", "#readMore", ".btn-load-more"];
    for &sel in &common_selectors {
        if let Ok(elem) = driver.find(By::Css(sel)).await {
            if let Ok(true) = elem.is_displayed().await {
                if let Ok(_) = elem.click().await {
                    info!("[DEBUG] 使用 `{}` 成功点击 '阅读更多' 按钮", sel);
                    return true;
                }
            }
        }
    }

    error!("[ERROR] 尝试所有方法后仍然无法点击 '阅读更多' 按钮");
    false
}


/// 递归爬取页面，每个页面都在独立的标签页中处理，然后关闭该标签页并返回父标签页。
async fn crawl_page_with_parent(
    driver: &WebDriver,
    url: String,
    depth: u8,
    max_depth: u8,
    query_embedding: Vec<f32>,
    visited: Arc<Mutex<HashSet<String>>>,
    parent_handle: &WindowHandle,
) -> Result<Vec<SearchResult>, ToolsError> {
    if depth > max_depth {
        return Ok(vec![]);
    }

    // 去重
    {
        let mut visited_set = visited.lock().await;
        if visited_set.contains(&url) {
            return Ok(vec![]);
        }
        visited_set.insert(url.clone());
    }

    // 在新标签页中打开目标 URL
    info!("[DEBUG] 父标签页句柄: {}", parent_handle);
    info!("[DEBUG] 在新标签页中打开: {}", url);
    driver.execute("window.open(arguments[0], '_blank');", vec![url.clone().into()])
        .await
        .map_err(|e| SearchError(format!("无法打开新标签页: {}", e)))?;

    // 等待新标签页出现
    tokio::time::sleep(Duration::from_secs(2)).await;
    let handles = driver.window_handles().await
        .map_err(|e| SearchError(format!("无法获取标签页句柄: {}", e)))?;

    // 找到新标签页句柄（排除父标签页）
    let new_handle = handles.into_iter()
        .find(|h| h != parent_handle)
        .ok_or(SearchError("未找到新建的标签页".to_string()))?;

    info!("[DEBUG] 新标签页句柄: {}", new_handle);

    // 切换到新标签页
    driver.switch_to_window(new_handle.clone()).await
        .map_err(|e| SearchError(format!("无法切换到新标签页: {}", e)))?;

    // 打开目标 URL（确保页面正确加载）
    driver.goto(&url).await
        .map_err(|e| SearchError(format!("新标签页无法打开页面: {}", e)))?;
    tokio::time::sleep(Duration::from_secs(3)).await;

    info!("[DEBUG] 新标签页加载完成，当前 URL: {}", driver.current_url().await.unwrap_or_else(|_| "http://127.0.0.1".to_string().parse().unwrap()));

    // 尝试点击“阅读更多”
    let _ = click_read_more_if_needed(driver).await;
    tokio::time::sleep(Duration::from_secs(2)).await;

    // 提取正文
    let content_opt = extract_article_content(driver).await;
    let mut results = Vec::new();
    if let Some(content) = content_opt {
        let result = SearchResult {
            title: driver.title().await.unwrap_or_else(|_| "Untitled".to_string()),
            link: url.clone(),
            content: Some(content),
        };
        results.push(result);
    } else {
        info!("[DEBUG] 当前页面无正文: {}", url);
    }

    // 如果当前页面没有正文且深度未达上限，可考虑递归爬取相关链接
    if results.is_empty() && depth < max_depth {
        info!("[DEBUG] 当前页面无正文，尝试提取相关链接进行递归爬取");
        let links = extract_relevant_links(driver, &query_embedding, &query_embedding).await;
        let mut tasks = Vec::new();
        for link in links {
            let visited_clone = Arc::clone(&visited);
            let query_embedding_clone = query_embedding.clone();
            tasks.push(crawl_page_with_parent(
                driver,
                link.clone(),
                depth + 1,
                max_depth,
                query_embedding_clone,
                visited_clone,
                // 当前新标签页作为父标签页传递下去
                &new_handle,
            ));
        }
        let results_vec = futures::future::join_all(tasks).await;
        for res in results_vec {
            if let Ok(mut sub_results) = res {
                results.append(&mut sub_results);
            }
        }
    }

    // 关闭当前新标签页
    driver.close().await
        .map_err(|e| SearchError(format!("无法关闭新标签页: {}", e)))?;
    // 切换回父标签页
    driver.switch_to_window(parent_handle.clone()).await
        .map_err(|e| SearchError(format!("无法切换回父标签页: {}", e)))?;

    Ok(results)
}

/// **在新标签页打开 URL 并切换**
async fn open_in_new_tab(driver: &WebDriver, url: &str) -> Option<String> {
    driver.execute("window.open();", vec![]).await.ok()?; // **新建标签页**
    let handles = driver.window_handles().await.ok()?; // **获取所有标签**
    let new_handle: WindowHandle = handles.last()?.clone(); // **获取最新的标签**
    driver.switch_to_window(new_handle).await.ok()?; // **切换到新标签**

    driver.goto(url).await.ok()?; // **访问 URL**
    tokio::time::sleep(Duration::from_secs(2)).await; // **等待页面加载**

    // **自动点击「阅读更多」**
    click_read_more_if_needed(driver).await;

    // **提取正文**
    extract_article_content(driver).await
}

/// **提取页面内的相关链接**
async fn extract_relevant_links(
    driver: &WebDriver,
    query_embedding: &[f32],
    content_embedding: &[f32]
) -> Vec<String> {
    let mut links = Vec::new();
    let elements = driver.find_all(By::Css("a")).await.unwrap_or(vec![]);

    for elem in elements {
        if let Ok(Some(href)) = elem.attr("href").await {
            if !href.starts_with("http") {
                continue; // **过滤无效链接**
            }

            // **计算相似度**
            let link_embedding = get_text_embedding(&href).await.unwrap_or_default();
            let similarity = cosine_similarity(query_embedding, &link_embedding);
            let content_similarity = cosine_similarity(content_embedding, &link_embedding);

            if similarity > 0.7 || content_similarity > 0.7 {
                links.push(href);
            }
        }
    }

    links
}

use rig::providers::ollama::Client as OllamaClient;

/// **计算文本向量嵌入**
async fn get_text_embedding(text: &str) -> Option<Vec<f32>> {
    let client = OllamaClient::new();
    let response = client
        .embedding_model("nomic-embed-text")
        .embed_texts([text.to_string()]) // **单文本转数组**
        .await
        .ok()?;

    let embedding = response.first()?; // **取第一个结果**
    Some(embedding.vec.iter().map(|&x| x as f32).collect()) // **转换 `Vec<f64>` -> `Vec<f32>`**
}

/// **计算余弦相似度**
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x.powi(2)).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x.powi(2)).sum::<f32>().sqrt();
    dot / (norm_a * norm_b)
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

    info!("查询 Rust async runtime 的最新信息");
    info!(
        "AI 搜索助手: {}",
        agent.prompt("查询 Rust async runtime 的最新信息").await?
    );

    Ok(())
}
