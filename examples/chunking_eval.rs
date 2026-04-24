//! Chunking + entity/relation extraction evaluation (#19).
//!
//! Runs `chunk_text`, `extract_entities`, and `extract_relations` over the
//! observability fixtures and reports the pipeline's actual output. The
//! result is a markdown-ish dump designed to be committed as an evidence
//! artifact (`.perf/chunking-eval-<date>.md`) and compared session-to-session
//! as chunking/extraction improve.
//!
//! Why: #20–#23 propose changing the chunking strategy. Before choosing one,
//! we need a concrete picture of what the current strategy produces and
//! where it over/under-chunks.
//!
//! Usage:
//!   cargo run --release --example chunking_eval > .perf/chunking-eval-$(date +%F).md

use std::collections::HashMap;

use legend::memory::entorhinal::chunk_text;
use legend::memory::wernicke::{extract_entities, extract_relations, KeywordCache};

/// A single input to exercise. The label is a short tag used in the report.
struct Sample {
    label: &'static str,
    text: &'static str,
}

const SAMPLES: &[Sample] = &[
    Sample {
        label: "alpha_001_signal_noise_mix",
        text: "Project Alpha uses SQLite as the canonical datastore. The purple stapler was moved beside a humming vending machine. [[[[",
    },
    Sample {
        label: "alpha_002_repeated_and_incidental",
        text: "The SQLite datastore backs Project Alpha's audit log. A ceramic frog near the monitor is named Biscuit. %%@@",
    },
    Sample {
        label: "alpha_004_backup_time",
        text: "Project Alpha verifies SQLite backups at 02:30 UTC. The moon-shaped paperclip belongs in the third drawer. &&&&",
    },
    Sample {
        label: "alpha_005_migration_path",
        text: "Project Alpha keeps SQLite migration files in db/migrations. The brass keychain was sorted by color near the printer. }}}}",
    },
    Sample {
        label: "alpha_025_summary_long",
        text: "Project Alpha depends on SQLite for datastore, backups, migration files, exports, integrity checks, and restore validation. The glitter pinecone was cataloged under office folklore. ====25",
    },
    Sample {
        label: "decision_prefix",
        text: "DECISION: closed #17 (fast encoding path + deferred work queues). Two surgical fixes in tick_impl: skip encoding_activation when compute_context=false, and move auto-consolidation to the daemon's background worker.",
    },
    Sample {
        label: "paragraph_break",
        text: "First sentence about graph dedup.\n\nSecond paragraph on consolidation thresholds. The worker polls every 2 seconds and drains when ticks_since_consolidation crosses the threshold.",
    },
    Sample {
        label: "pipe_separated",
        text: "Task A complete | Task B pending | Task C deferred | Task D done",
    },
    Sample {
        label: "long_single_sentence",
        text: "The daemon spawns a consolidation_worker thread in run_foreground that polls every 2 seconds to check whether ticks_since_consolidation has crossed CONSOLIDATION_SUGGESTION_THRESHOLD or consolidation_pressure exceeds CONSOLIDATION_PRESSURE_THRESHOLD and if so acquires the write lock runs crate::memory::consolidate and takes a full checkpoint.",
    },
    Sample {
        label: "numeric_heavy",
        text: "Latency p50 20ms p95 94ms mean 42ms over 10 samples on the 11k-node master-clone state.",
    },
];

fn main() {
    let kw = KeywordCache::default_from_static();

    println!("# Chunking + extraction evaluation\n");
    println!(
        "Samples: {}. Pipeline: `chunk_text` → per-chunk `extract_entities` and `extract_relations`.\n",
        SAMPLES.len()
    );
    println!("Report format: chunk boundaries, entity labels grouped by kind, relation (subject, kind, object) triples.\n");

    let mut chunk_sizes = Vec::new();
    let mut total_chunks = 0usize;
    let mut total_entities = 0usize;
    let mut total_relations = 0usize;
    let mut kind_tally: HashMap<String, usize> = HashMap::new();
    let mut relation_tally: HashMap<String, usize> = HashMap::new();

    for sample in SAMPLES {
        println!("---\n## `{}`\n", sample.label);
        println!("**Input** ({} chars):\n\n> {}\n", sample.text.len(), sample.text);

        let chunks = chunk_text(sample.text);
        println!("**Chunks:** {}\n", chunks.len());
        for (i, c) in chunks.iter().enumerate() {
            println!("- [{}] ({} chars) {}", i, c.len(), truncate(c, 200));
            chunk_sizes.push(c.len());
        }
        println!();

        for (i, chunk) in chunks.iter().enumerate() {
            let entities = extract_entities(chunk, &kw);
            let relations = extract_relations(chunk, &kw);
            total_chunks += 1;
            total_entities += entities.len();
            total_relations += relations.len();

            println!("**Chunk {} entities** ({}):", i, entities.len());
            if entities.is_empty() {
                println!("  _(none)_");
            } else {
                // group by kind
                let mut by_kind: HashMap<String, Vec<String>> = HashMap::new();
                for e in &entities {
                    *kind_tally.entry(e.kind.clone()).or_insert(0) += 1;
                    by_kind
                        .entry(e.kind.clone())
                        .or_default()
                        .push(e.label.clone());
                }
                let mut kinds: Vec<_> = by_kind.keys().cloned().collect();
                kinds.sort();
                for k in &kinds {
                    let labels = &by_kind[k];
                    println!("  - {}: {}", k, labels.join(", "));
                }
            }

            println!("**Chunk {} relations** ({}):", i, relations.len());
            if relations.is_empty() {
                println!("  _(none)_");
            } else {
                for r in &relations {
                    let key = format!("{}", r.kind);
                    *relation_tally.entry(key).or_insert(0) += 1;
                    println!(
                        "  - ({}, {}, {}) confidence={:.2}",
                        r.subject.label, r.kind, r.object.label, r.confidence
                    );
                }
            }
            println!();
        }
    }

    println!("---\n## Aggregate\n");
    println!("- Total chunks: {}", total_chunks);
    if !chunk_sizes.is_empty() {
        let mut s = chunk_sizes.clone();
        s.sort();
        let min = *s.first().unwrap();
        let max = *s.last().unwrap();
        let median = s[s.len() / 2];
        let mean = s.iter().sum::<usize>() / s.len();
        println!(
            "- Chunk size chars: min={} median={} mean={} max={}",
            min, median, mean, max
        );
    }
    println!("- Total entities extracted: {}", total_entities);
    println!("- Total relations extracted: {}", total_relations);

    println!("\n### Entity kind distribution");
    let mut kinds: Vec<_> = kind_tally.iter().collect();
    kinds.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
    for (kind, n) in kinds {
        println!("- {}: {}", kind, n);
    }

    println!("\n### Relation kind distribution");
    let mut rels: Vec<_> = relation_tally.iter().collect();
    rels.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
    if rels.is_empty() {
        println!("- (none)");
    } else {
        for (kind, n) in rels {
            println!("- {}: {}", kind, n);
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}
