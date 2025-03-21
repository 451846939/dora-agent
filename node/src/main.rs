use std::thread::sleep;
use std::time::Duration;
use dora_node_api::{self, dora_core::config::DataId, DoraNode, Event, IntoArrow};


fn main() -> eyre::Result<()> {
    println!("hello");

    let output = DataId::from("query".to_owned());

    let (mut node, mut events) = DoraNode::init_from_env()?;

    sleep(Duration::from_secs(5));

    node.send_output(output.clone(), Default::default(), "根据rust for linux 来写一篇文章，并生成本地文件".into_arrow())?;

    // node.send_output(DataId::from("init"),Default::default(), "init".into_arrow())?;
    // for i in 0..100 {
    //     let event = match events.recv() {
    //         Some(input) => input,
    //         None => Event::Stop,
    //     };
    //
    //     match event {
    //         Event::Input {
    //             id,
    //             metadata,
    //             data: _,
    //         } => match id.as_str() {
    //             "tick" => {
    //                 let random: u64 = rand::random();
    //                 println!("tick {i}, sending {random:#x}");
    //                 node.send_output(output.clone(), metadata.parameters, random.into_arrow())?;
    //             }
    //             other => eprintln!("Ignoring unexpected input `{other}`"),
    //         },
    //         Event::Stop => println!("Received manual stop"),
    //         other => eprintln!("Received unexpected input: {other:?}"),
    //     }
    // }

    Ok(())
}
