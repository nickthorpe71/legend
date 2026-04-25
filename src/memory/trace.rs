//! Instrumentation layer for visualizing Legend's memory pipeline.
//!
//! Gated behind `#[cfg(feature = "instrument")]` — zero cost when disabled.
//! Provides a global trace channel that emits `TraceEvent`s as brain modules
//! process ticks, queries, and consolidations.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, OnceLock};
use std::time::Instant;

// ---------------------------------------------------------------------------
// Trace ID
// ---------------------------------------------------------------------------

/// Monotonically increasing trace ID for grouping events within a single
/// tick/query/consolidate operation.
static TRACE_COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn next_trace_id() -> u64 {
    TRACE_COUNTER.fetch_add(1, Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEvent {
    /// Groups all events from a single tick/query/consolidate call.
    pub trace_id: u64,
    /// Monotonic sequence number within a trace.
    pub clock: u64,
    /// Which pipeline step this event represents.
    pub step: PipelineStep,
    /// Microseconds elapsed since the trace started.
    pub elapsed_us: u64,
    /// Optional payload with step-specific data.
    pub payload: TracePayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PipelineStep {
    // --- Tick pipeline ---
    TickStart,
    ClockIncrement,
    Decay,
    StabilizeLabile,
    Renormalize,
    NormalizeGraphWeights,
    ChunkText,
    EmbedText,
    ComputeSalience,
    ComputeEmotionalValence,
    ExtractMemoryRefs,
    SparseOrthogonalize,
    PushWorkingMemory,
    AttentionGate,
    TryReconsolidate,
    FindBestMatch,
    DiversityGate,
    MergeEntryHigh,
    MergeEntryLow,
    InsertShortTerm,
    UpdateGraph,
    EncodingActivation,
    ConceptClassifierVotes,
    UpdateTermFrequencies,
    PruneL2,
    PruneL3,
    CpebTagging,
    ContextSwitchDetection,
    FlushWorkingMemory,
    TickEnd,

    // --- Query pipeline ---
    QueryStart,
    InferQueryMode,
    L1Scan,
    L2TopKSimilar,
    L3SummaryRetrieval,
    PatternCompletion,
    AssociativePriming,
    HebbianReinforce,
    AutoReinforceTop,
    GraphLookup,
    QueryEnd,

    // --- Consolidation pipeline ---
    ConsolidateStart,
    ReplayConsolidation,
    ClusterGroups,
    SummarizeGroup,
    SystemsConsolidation,
    CreateOrMergeSummaryNode,
    SemanticTopicExtraction,
    EntityLinking,
    MarkConsolidated,
    ConsolidateEnd,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TracePayload {
    None,
    Text(String),
    Number(f64),
    Embedding(Vec<f32>),
    Entries(Vec<TraceEntry>),
    Similarities(Vec<(u64, f32)>),
    Graph(Vec<TraceGraphNode>),
    KeyValue(Vec<(String, String)>),
}

/// Lightweight entry representation for trace payloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEntry {
    pub id: u64,
    pub text: String,
    pub salience: f32,
    pub similarity: Option<f32>,
}

/// Lightweight graph node representation for trace payloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceGraphNode {
    pub id: u64,
    pub label: String,
    pub kind: String,
    pub weight: f32,
}

// ---------------------------------------------------------------------------
// Channel
// ---------------------------------------------------------------------------

static TRACE_TX: OnceLock<mpsc::Sender<TraceEvent>> = OnceLock::new();

/// Initialize the global trace channel. Returns the receiving end.
/// Call once at startup (e.g., in the viz server). Subsequent calls return `None`.
pub fn init_trace_channel() -> Option<mpsc::Receiver<TraceEvent>> {
    let (tx, rx) = mpsc::channel();
    TRACE_TX.set(tx).ok()?;
    Some(rx)
}

/// Emit a trace event. Non-blocking; silently drops if no receiver is attached.
pub fn emit(event: TraceEvent) {
    if let Some(tx) = TRACE_TX.get() {
        let _ = tx.send(event);
    }
}

// ---------------------------------------------------------------------------
// Trace context helper
// ---------------------------------------------------------------------------

/// Holds per-operation timing and sequence state.
pub struct TraceCtx {
    pub trace_id: u64,
    pub start: Instant,
    pub seq: AtomicU64,
}

impl TraceCtx {
    pub fn new() -> Self {
        Self {
            trace_id: next_trace_id(),
            start: Instant::now(),
            seq: AtomicU64::new(0),
        }
    }

    pub fn emit(&self, step: PipelineStep, payload: TracePayload) {
        let clock = self.seq.fetch_add(1, Ordering::Relaxed);
        let elapsed_us = self.start.elapsed().as_micros() as u64;
        emit(TraceEvent {
            trace_id: self.trace_id,
            clock,
            step,
            elapsed_us,
            payload,
        });
    }
}

// ---------------------------------------------------------------------------
// Macro
// ---------------------------------------------------------------------------

/// Emit a trace step. Expands to a `TraceCtx::emit()` call when `instrument`
/// feature is enabled, otherwise compiles to nothing.
///
/// Usage: `trace_step!(ctx, PipelineStep::TickStart, TracePayload::None);`
#[macro_export]
macro_rules! trace_step {
    ($ctx:expr, $step:expr, $payload:expr) => {
        #[cfg(feature = "instrument")]
        {
            $ctx.emit($step, $payload);
        }
        #[cfg(not(feature = "instrument"))]
        {
            let _ = &$ctx; // suppress unused warnings in non-instrument builds
            let _ = $step;
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_id_increments() {
        let a = next_trace_id();
        let b = next_trace_id();
        assert!(b > a);
    }

    #[test]
    fn emit_without_receiver_does_not_panic() {
        // No channel initialized — emit should silently drop
        emit(TraceEvent {
            trace_id: 0,
            clock: 0,
            step: PipelineStep::TickStart,
            elapsed_us: 0,
            payload: TracePayload::None,
        });
    }

    #[test]
    fn trace_ctx_emits_sequenced_events() {
        // This test verifies the TraceCtx helper
        let ctx = TraceCtx::new();
        assert!(ctx.trace_id > 0);
        // Emitting without a channel should not panic
        ctx.emit(PipelineStep::TickStart, TracePayload::None);
        ctx.emit(
            PipelineStep::TickEnd,
            TracePayload::Text("done".to_string()),
        );
    }

    #[test]
    fn trace_event_serializes_to_json() {
        let event = TraceEvent {
            trace_id: 42,
            clock: 0,
            step: PipelineStep::ComputeSalience,
            elapsed_us: 1234,
            payload: TracePayload::Number(0.75),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("ComputeSalience"));
        assert!(json.contains("0.75"));
    }
}
