mod bridge;
mod routes;
mod state;

use axum::routing::{get, post};
use axum::Router;
use legend::memory::trace;
use state::AppState;
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

/// Path where the viz keeps its own copy of the memory state.
pub const VIZ_MEMORY_PATH: &str = ".legend/viz-memory.lz4";

#[tokio::main]
async fn main() {
    // Initialize the trace channel — legend's brain modules will send events here
    let trace_rx = trace::init_trace_channel().expect("trace channel already initialized");

    // Broadcast channel for fanning out to multiple WebSocket clients
    let (trace_tx, _) = broadcast::channel::<trace::TraceEvent>(4096);
    let trace_tx_clone = trace_tx.clone();

    // Relay: mpsc receiver → broadcast sender
    tokio::spawn(async move {
        loop {
            match trace_rx.recv() {
                Ok(event) => {
                    let _ = trace_tx_clone.send(event);
                }
                Err(_) => break,
            }
        }
    });

    // Load viz-specific memory, or seed from the main Legend memory on first run
    let memory = if std::path::Path::new(VIZ_MEMORY_PATH).exists() {
        match legend::tool::persistence::load_memory_from_path(VIZ_MEMORY_PATH) {
            Ok(m) => {
                println!("Loaded viz memory from {VIZ_MEMORY_PATH}");
                m
            }
            Err(e) => {
                eprintln!("Failed to load {VIZ_MEMORY_PATH}: {e}, starting fresh");
                legend::memory::MemoryState::default()
            }
        }
    } else if std::path::Path::new(".legend/memory.lz4").exists() {
        match legend::tool::persistence::load_memory_from_path(".legend/memory.lz4") {
            Ok(m) => {
                // Save the copy so future boots use it
                let _ = legend::tool::persistence::save_memory_to_path(&m, VIZ_MEMORY_PATH);
                println!("Seeded viz memory from .legend/memory.lz4 → {VIZ_MEMORY_PATH}");
                m
            }
            Err(e) => {
                eprintln!("Failed to seed from .legend/memory.lz4: {e}, starting fresh");
                legend::memory::MemoryState::default()
            }
        }
    } else {
        println!("No memory found, starting fresh");
        legend::memory::MemoryState::default()
    };

    // Clear stale tick embedding so the first viz tick doesn't
    // false-trigger context switch detection from a prior CLI session.
    let mut memory = memory;
    memory.brain.last_tick_embedding.clear();

    let app_state = AppState::new(memory, trace_tx);

    let api = Router::new()
        .route("/tick", post(routes::api::tick))
        .route("/query", post(routes::api::query))
        .route("/consolidate", post(routes::api::consolidate))
        .route("/state", get(routes::api::get_state))
        .route("/state/l1", get(routes::api::get_l1))
        .route("/state/l2", get(routes::api::get_l2))
        .route("/state/l3", get(routes::api::get_l3))
        .route("/load", post(routes::api::load))
        .route("/reset", post(routes::api::reset));

    let app = Router::new()
        .nest("/api", api)
        .route("/ws", get(routes::ws::handler))
        .fallback_service(ServeDir::new("legend-viz/frontend/dist"))
        .layer(CorsLayer::permissive())
        .with_state(app_state);

    let addr = "127.0.0.1:3333";
    println!("Legend Viz server running at http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
