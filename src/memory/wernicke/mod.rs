/// Wernicke's Area — Language comprehension subsystem.
///
/// Named after the brain region responsible for understanding language,
/// this module handles all text-to-meaning transformations:
///
/// - **extract** — Entity extraction: identifies named entities, code symbols,
///   and semantic relationships from raw text.
/// - **lexicon** — Static vocabulary: innate keyword dictionaries for classification
///   (decisions, bugs, architecture, etc.). Like hardwired neural circuits.
/// - **cache** — Dynamic keyword cache: graph-backed, runtime-refreshed keyword lists
///   with semantic priming from the knowledge graph.
pub mod cache;
pub mod extract;
pub mod lexicon;

// Re-export primary types at the wernicke:: level for convenience.
pub use cache::KeywordCache;
pub use extract::{extract_dates, extract_entities, is_stopword};
