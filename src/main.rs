mod config;
mod consensus;
mod crypto;
mod state;
mod types;
use axum::routing::post;
use axum::Json;
use axum::{extract::DefaultBodyLimit, routing::get, Extension, Router};
use colored::*;
use indicatif::ProgressBar;
use reqwest::Client;
use state::server::{InMemoryBlockStore, InMemoryConsensus, InMemoryTransactionPool};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use types::GenericTransactionData;

struct InMemoryServerState {
    block_state: Arc<Mutex<InMemoryBlockStore>>,
    pool_state: Arc<Mutex<InMemoryTransactionPool>>,
    consensus_state: Arc<Mutex<InMemoryConsensus>>,
}

async fn synchronization_loop(database: Arc<Mutex<InMemoryServerState>>) {
    // todo: synchronize Blocks with other nodes
    // fetch if the height is > this nodes
    // verify the signatures and threshold
    // store valid blocks
    // loop {}
}
#[tokio::main]
async fn main() {
    println!(
        "{}\n{}",
        r#"
██████╗  ██████╗ ██████╗ ██████╗       ███████╗ ██████╗ 
██╔══██╗██╔═══██╗██╔══██╗██╔══██╗      ██╔════╝██╔═══██╗
██████╔╝██║   ██║██████╔╝██║  ██║█████╗███████╗██║   ██║
██╔═══╝ ██║   ██║██╔══██╗██║  ██║╚════╝╚════██║██║▄▄ ██║
██║     ╚██████╔╝██║  ██║██████╔╝      ███████║╚██████╔╝
╚═╝      ╚═════╝ ╚═╝  ╚═╝╚═════╝       ╚══════╝ ╚══▀▀═╝"#
            .blue()
            .bold(),
        "Compact, General Purpose, Semi-Decentralized, Sequencer"
            .bold()
            .italic()
            .magenta()
    );
    let block_state: InMemoryBlockStore = InMemoryBlockStore::empty();
    let pool_state: InMemoryTransactionPool = InMemoryTransactionPool::empty();
    let consensus_state: InMemoryConsensus = InMemoryConsensus::empty();
    let shared_state: Arc<Mutex<InMemoryServerState>> = Arc::new(Mutex::new(InMemoryServerState {
        block_state: Arc::new(Mutex::new(block_state)),
        pool_state: Arc::new(Mutex::new(pool_state)),
        consensus_state: Arc::new(Mutex::new(consensus_state)),
    }));
    tokio::spawn(synchronization_loop(Arc::clone(&shared_state)));
    let api = Router::new()
        .route("/get/pool", get(get_pool))
        .route("/schedule", post(schedule))
        .layer(DefaultBodyLimit::max(10000000))
        .layer(Extension(shared_state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080")
        .await
        .unwrap();
    axum::serve(listener, api).await.unwrap();
}

async fn schedule(
    Extension(shared_state): Extension<Arc<Mutex<InMemoryServerState>>>,
    Json(transaction): Json<GenericTransactionData>,
) -> String {
    let state = shared_state.lock().unwrap();
    let success_response =
        format!("Transaction is being sequenced: {:?}", &transaction).to_string();
    state
        .pool_state
        .lock()
        .unwrap()
        .insert_transaction(transaction);
    success_response
}

async fn get_pool(Extension(shared_state): Extension<Arc<Mutex<InMemoryServerState>>>) -> String {
    let state = shared_state.lock().unwrap();
    let pool_state = state.pool_state.lock().unwrap();
    format!("{:?}", pool_state.transactions)
}

#[tokio::test]
async fn test_schedule_transaction() {
    let client = Client::new();
    let raw_data: String = serde_json::to_string(&vec![1, 2, 3, 4, 5]).unwrap();
    let response = client
        .post("http://127.0.0.1:8080/schedule")
        .header("Content-Type", "application/json")
        .body(raw_data)
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.text().await.unwrap(),
        "Transaction is being sequenced: [1, 2, 3, 4, 5]"
    )
}
