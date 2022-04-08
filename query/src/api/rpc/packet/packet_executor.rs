use std::collections::HashMap;
use std::sync::Arc;
use common_meta_types::NodeInfo;
use crate::api::rpc::packet::packet_fragment::FragmentPacket;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ExecutorPacket {
    pub executor: String,
    pub executors: HashMap<String, String>,
    pub fragments_packets: Vec<FragmentPacket>,
}

impl ExecutorPacket {
    pub fn create(executor: String, fragments_packets: Vec<FragmentPacket>, executors: HashMap<String, String>) -> ExecutorPacket {
        ExecutorPacket { executor, executors, fragments_packets }
    }
}
