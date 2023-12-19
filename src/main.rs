use std::sync::Arc;
use tokio::runtime::Runtime;

mod broker;
mod control_plane;
mod log;
mod metrics;
mod model;
mod persistent_kv;
mod strategy;
mod utils;

fn main() {
    let _runtime = Arc::new(Runtime::new().unwrap());
    println!("Hello, world!");
}
