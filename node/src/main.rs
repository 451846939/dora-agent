use std::thread::sleep;
use std::time::Duration;
use dora_node_api::{self, dora_core::config::DataId, DoraNode, Event, IntoArrow};


mod server;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    println!("start node server");
    server::run().await;
    Ok(())
}
