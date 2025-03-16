use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use common::NodeDescriptor;

#[derive(Debug)]
pub struct RouterNode {
    registered_nodes: Arc<Mutex<HashMap<String, NodeDescriptor>>>,
}

impl RouterNode {
    pub fn new() -> Self {
        Self {
            registered_nodes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn register_node(&self, node: NodeDescriptor) {
        let mut nodes = self.registered_nodes.lock().unwrap();
        nodes.insert(node.id.clone(), node);
        println!("✅ Node registered: {:?}", nodes);
    }

    pub fn get_registered_nodes(&self) -> Vec<NodeDescriptor> {
        let nodes = self.registered_nodes.lock().unwrap();
        nodes.values().cloned().collect()
    }

    pub fn get_node_by_id(&self, id: &str) -> Option<NodeDescriptor> {
        let nodes = self.registered_nodes.lock().unwrap();
        nodes.get(id).cloned()
    }
}