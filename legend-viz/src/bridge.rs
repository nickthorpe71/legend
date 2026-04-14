use legend::memory::{consolidate, retrieve_context, tick_impl, MemoryContext};
use legend::memory::{BrainState, GraphNodeSummary, TickResult};

/// Tick a thought through the brain pipeline.
/// Trace events are emitted automatically by the instrumented functions.
pub fn traced_tick(brain: &mut BrainState, text: &str) -> TickResult {
    tick_impl(brain, text, false)
}

/// Query memory for context.
pub fn traced_query(brain: &mut BrainState, query: &str) -> MemoryContext {
    retrieve_context(brain, query)
}

/// Run consolidation.
pub fn traced_consolidate(brain: &mut BrainState) -> Vec<GraphNodeSummary> {
    consolidate(brain)
}
