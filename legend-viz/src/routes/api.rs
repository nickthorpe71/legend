use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;
use serde::{Deserialize, Serialize};

use crate::bridge;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct TickRequest {
    pub text: String,
}

#[derive(Deserialize)]
pub struct QueryRequest {
    pub query: String,
}

#[derive(Deserialize)]
pub struct LoadRequest {
    pub path: String,
}

#[derive(Serialize)]
pub struct BrainSnapshot {
    pub clock: u64,
    pub l1_count: usize,
    pub l2_count: usize,
    pub l3_node_count: usize,
    pub l3_edge_count: usize,
    pub ticks_since_consolidation: u32,
    pub consolidation_pressure: f32,
    pub next_id: u64,
    pub keyword_cache_size: usize,
    pub embedding_dim: usize,
    pub immediate_capacity: usize,
}

fn snapshot_from_brain(b: &legend::memory::BrainState) -> BrainSnapshot {
    let effective = legend::memory::neurochemistry::compute_effective(&b.chemistry);
    let kw_size = b.keyword_cache.code.len()
        + b.keyword_cache.decision.len()
        + b.keyword_cache.action.len()
        + b.keyword_cache.architecture.len()
        + b.keyword_cache.bug.len()
        + b.keyword_cache.todo.len()
        + b.keyword_cache.preference.len();
    BrainSnapshot {
        clock: b.clock,
        l1_count: b.working_memory.len(),
        l2_count: b.short_term.len(),
        l3_node_count: b.long_term.nodes.len(),
        l3_edge_count: b.long_term.edges.len(),
        ticks_since_consolidation: b.ticks_since_consolidation,
        consolidation_pressure: effective.consolidation_pressure,
        next_id: b.next_id,
        keyword_cache_size: kw_size,
        embedding_dim: b.config.embedding_dim,
        immediate_capacity: b.config.immediate_capacity,
    }
}

pub async fn tick(
    State(state): State<AppState>,
    Json(req): Json<TickRequest>,
) -> Json<serde_json::Value> {
    let mut mem = state.memory.lock().await;
    let result = bridge::traced_tick(&mut mem.brain, &req.text);
    Json(serde_json::to_value(&result).unwrap_or_default())
}

pub async fn query(
    State(state): State<AppState>,
    Json(req): Json<QueryRequest>,
) -> Json<serde_json::Value> {
    let mut mem = state.memory.lock().await;
    let result = bridge::traced_query(&mut mem.brain, &req.query);
    Json(serde_json::to_value(&result).unwrap_or_default())
}

pub async fn consolidate(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut mem = state.memory.lock().await;
    let result = bridge::traced_consolidate(&mut mem.brain);
    Json(serde_json::to_value(&result).unwrap_or_default())
}

pub async fn get_state(State(state): State<AppState>) -> Json<BrainSnapshot> {
    let mem = state.memory.lock().await;
    Json(snapshot_from_brain(&mem.brain))
}

pub async fn get_l1(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mem = state.memory.lock().await;
    Json(serde_json::to_value(&mem.brain.working_memory).unwrap_or_default())
}

pub async fn get_l2(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mem = state.memory.lock().await;
    Json(serde_json::to_value(&mem.brain.short_term).unwrap_or_default())
}

pub async fn get_l3(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mem = state.memory.lock().await;
    Json(serde_json::to_value(&mem.brain.long_term).unwrap_or_default())
}

pub async fn load(
    State(state): State<AppState>,
    Json(req): Json<LoadRequest>,
) -> Result<Json<BrainSnapshot>, (StatusCode, String)> {
    let loaded = legend::tool::persistence::load_memory_from_path(&req.path)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Load failed: {e}")))?;
    let mut mem = state.memory.lock().await;
    *mem = loaded;
    Ok(Json(snapshot_from_brain(&mem.brain)))
}

pub async fn reset(State(state): State<AppState>) -> Json<BrainSnapshot> {
    let mut mem = state.memory.lock().await;
    *mem = legend::memory::MemoryState::default();
    Json(snapshot_from_brain(&mem.brain))
}
