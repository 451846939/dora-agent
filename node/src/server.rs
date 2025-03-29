use async_stream::stream;
use axum::response::{Html, IntoResponse};
use axum::{
    extract::{Query, State},
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
    Json, Router,
};
use futures::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::process::exit;
use std::{convert::Infallible, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    fs,
    net::TcpListener,
    signal,
    sync::{broadcast, Mutex},
    time::sleep,
};
use tracing::{error, info};
use tracing_subscriber;

use dora_node_api::dora_core::config::DataId;
use dora_node_api::{DoraNode, Event as DoraEvent, IntoArrow};
use common::message::{flow_msg, FlowMessage};
use common::status_log::WorkflowLog;

#[derive(Debug, Deserialize)]
struct SseQuery {
    query: String,
}

/// DoraShared 用 broadcast 通道保存 DoraNode 事件
struct DoraShared {
    node: Mutex<DoraNode>,
    // 使用 broadcast 通道，允许多个订阅者独立接收消息
    broadcaster: Arc<broadcast::Sender<String>>,
}

pub(crate) async fn run() {
    // 初始化 DoraNode，此处假设 DoraNode::init_from_env() 返回 (node, events)
    let (mut node, mut events) = DoraNode::init_from_env().expect("DoraNode 初始化失败");

    // 创建 broadcast 通道用于转发 DoraNode 事件
    let (tx, _) = broadcast::channel::<String>(100);
    let broadcaster = Arc::new(tx);

    // 启动后台任务，将 DoraNode 事件转发到 broadcast 通道
    {
        let broadcaster = Arc::clone(&broadcaster);
        tokio::spawn(async move {
            println!("rust-node DoraNode 事件转发器已启动");
            while let Some(event) = events.recv_async().await {
                match event {
                    DoraEvent::Stop => {
                        println!("接收到 Dora Stop 信号，准备退出");
                        exit(0);
                    }
                    DoraEvent::Error(_) => {
                        println!("接收到 Dora Error 信号，准备退出");
                        exit(0);
                    }
                    DoraEvent::Input {id,metadata,data}=>{

                        println!("接收到 Dora Input 信号: id {:?} metadata: {:?} data:{:?}", id, metadata,data);
                        // let flow_msg: FlowMessage = match flow_msg::try_from(data) {
                        //     Ok(msg) => msg,
                        //     Err(err) => {
                        //         println!("⚠️ 解析 FlowMessage 失败: {}", err);
                        //         continue;
                        //     }
                        // };

                        let workflow = match WorkflowLog::try_from(data) {
                            Ok(msg) => msg,
                            Err(err) => {
                                println!("⚠️ 解析 FlowMessage 失败: {}", err);
                                continue;
                            }
                        };

                        // let s=format!("id: {:?}  data:{:?}", id,flow_msg.result.unwrap().to_string());
                        println!("接收到 Dora Input 信号: {:?}", workflow);
                        if let Err(e) = broadcaster.send(serde_json::to_string(&workflow).unwrap()) {
                            eprintln!("广播发送错误: {}", e);
                        }
                    }
                    _ => {
                        println!("接收到 Dora 其他事件: {:?}", event);
                    }
                }
            }
        });
    }

    // 构造共享状态，包装 DoraNode 和 broadcast 通道
    let node = Mutex::new(node);
    let dora_shared = Arc::new(DoraShared { node, broadcaster });

    // 构造 Axum 路由
    let app = Router::new()
        .route("/sse", get(sse_handler))
        .route("/send", post(send_handler))
        .route("/debug", get(debug_html)) // 加载 HTML 页面
        .with_state(Arc::clone(&dora_shared));

    let addr: SocketAddr = "127.0.0.1:3000".parse().unwrap();
    println!("Server listening on {}", addr);
    let listener = TcpListener::bind(addr).await.unwrap();
    // 延迟 500ms 后打开浏览器
    tokio::spawn(async {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let _ = open::that("http://127.0.0.1:3000/debug");
    });
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .unwrap();
}

async fn sse_handler(
    Query(query_params): Query<SseQuery>,
    State(dora_shared): State<Arc<DoraShared>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    println!("SSE connected, query = {}", query_params.query);

    // 发送 query 消息给 DoraNode
    {
        let output_id = DataId::from("query".to_owned());
        let arrow_data = query_params.query.clone().into_arrow();
        let mut node = dora_shared.node.lock().await;
        if let Err(e) = node.send_output(output_id, Default::default(), arrow_data) {
            eprintln!("发送 query 消息失败: {}", e);
        } else {
            println!("发送 query 消息成功: {}", query_params.query);
        }
    }

    // 每个 SSE 连接独立订阅 broadcast 通道
    let mut rx = dora_shared.broadcaster.subscribe();
    let stream = stream! {
        loop {
            match rx.recv().await {
                Ok(msg) => yield Ok(Event::default().data(msg)),
                Err(_) => break,
            }
        }
    };

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(1))
            .text("keep-alive-text"),
    )
}

async fn send_handler(
    State(dora_shared): State<Arc<DoraShared>>,
    axum::extract::Json(payload): axum::extract::Json<Value>,
) -> Json<Value> {
    let message = payload
        .get("message")
        .and_then(|m| m.as_str())
        .unwrap_or("");
    println!("手动发送消息: {}", message);
    let output_id = DataId::from("query".to_owned());
    let arrow_data = message.into_arrow();
    let mut node = dora_shared.node.lock().await;
    if let Err(e) = node.send_output(output_id, Default::default(), arrow_data) {
        eprintln!("发送消息失败: {}", e);
        return Json(json!({"status": "error", "message": format!("{}", e)}));
    }
    Json(json!({"status": "sent", "message": message}))
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

async fn debug_html() -> impl IntoResponse {
    match fs::read_to_string("node/html/index.html").await {
        Ok(content) => Html(content).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("读取 HTML 文件失败: {}", e),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use eventsource_stream::Eventsource;
    use futures::StreamExt;

    #[tokio::test]
    async fn simple_sse_request() {
        let client = reqwest::Client::new();
        let response = client
            .get("http://127.0.0.1:3000/sse?query=详细写一篇缅甸发生7.9级地震的新闻") // 你本地跑着的服务
            .send()
            .await
            .expect("请求失败");

        let mut stream = response.bytes_stream().eventsource();

        while let Some(event) = stream.next().await {
            match event {
                Ok(ev) => {
                    println!("收到 SSE 消息: {}", ev.data);
                    if ev.data == "[DONE]" {
                        break;
                    }
                }
                Err(e) => {
                    eprintln!("SSE 错误: {:?}", e);
                    break;
                }
            }
        }
    }
}
