//! Experimental — agglomerative DAG routing test.
//!
//! Validates the proposed model: drop the "region" concept entirely,
//! drop Polarity, and let the DAG self-organize purely by embedding
//! similarity. Three thresholds:
//!
//!   merge_threshold   → cosine ≥ this means "duplicate" (don't mint)
//!   descend_threshold → cosine ≥ this means "descend into this branch"
//!   (else)            → attach as a new child at the current node
//!
//! For each seed element (string), we:
//!   1. Embed it with the bundled MiniLM.
//!   2. Walk the DAG from GENESIS, scoring children by cosine.
//!   3. Apply the rule above to place the element.
//!
//! Type-level routing — same surface form would dedup. We just feed
//! distinct strings here.
//!
//! Run: `cargo run --release --example dag_routing_test`

use legend::embed::{EMBEDDING_DIM, embed_text};
use legend::math::dot;

const MERGE_THRESHOLD: f32 = 0.95;
const DESCEND_THRESHOLD: f32 = 0.30;

/// One node in the synthetic DAG.
#[derive(Debug, Clone)]
struct Node {
    id: usize,
    name: String,
    embedding: Vec<f32>, // L2-normalized
}

/// Parent pointer per node id. None = direct child of GENESIS.
type ParentMap = Vec<Option<usize>>;

/// What `route_into_dag` decided to do.
#[derive(Debug, Clone)]
enum Action {
    /// `id` already exists and is essentially identical — caller
    /// should merge rather than mint a new element.
    Merge(usize),
    /// Mint as a new child of this parent id. `None` parent =
    /// attach directly under GENESIS.
    AttachBelow(Option<usize>),
}

fn main() {
    // Synthetic seed: a mix of cities, people, times, quantities,
    // function words, compound entities, common phrases. Order matters
    // — earlier seeds form the structure that later ones route into.
    // We deliberately interleave classes so the algorithm can't cheat
    // by relying on insertion order.
    let seeds: &[&str] = &[
        // Cities (one per class first to seed branches)
        "Berlin",
        // People
        "Sarah",
        // Times
        "Tuesday",
        // Quantities
        "6 pounds",
        // Function words (we expect these to cluster eventually)
        "the",
        // Now denser fill-in:
        "Paris",
        "John",
        "Friday",
        "3 years",
        "of",
        "Brantford",
        "Maya",
        "3pm",
        "$42",
        "and",
        "Tokyo",
        "Nick",
        "yesterday",
        "two cups",
        "in",
        "Times Square",
        "Dr. Patel",
        "Apollo project",
        "the dentist",
        "the meeting",
        "the appointment",
        "for",
        "MacBook",
    ];

    let mut nodes: Vec<Node> = Vec::new();
    let mut parents: ParentMap = Vec::new();

    println!("Routing {} seeds into an initially-empty DAG (only GENESIS).", seeds.len());
    println!("Thresholds: merge≥{:.2}, descend≥{:.2}\n", MERGE_THRESHOLD, DESCEND_THRESHOLD);

    for &name in seeds {
        let emb = embed_normalized(name);
        let action = route_into_dag(&emb, &nodes, &parents);

        match action {
            Action::Merge(existing) => {
                println!(
                    "  '{name}' → MERGE into #{} '{}'",
                    existing, nodes[existing].name
                );
            }
            Action::AttachBelow(parent) => {
                let parent_label = match parent {
                    Some(p) => format!("#{} '{}'", p, nodes[p].name),
                    None => "GENESIS".to_string(),
                };
                let new_id = nodes.len();
                nodes.push(Node {
                    id: new_id,
                    name: name.to_string(),
                    embedding: emb,
                });
                parents.push(parent);
                println!("  '{name}' → ATTACH below {parent_label} as #{new_id}");
            }
        }
    }

    // ── Print the resulting DAG ──────────────────────────────────────
    println!("\nResulting DAG:");
    let children_of_genesis: Vec<usize> = (0..nodes.len())
        .filter(|&i| parents[i].is_none())
        .collect();
    for &top in &children_of_genesis {
        print_subtree(top, &nodes, &parents, 1);
    }

    println!("\nSummary: {} top-level branches, {} total nodes", children_of_genesis.len(), nodes.len());

    // ── Sanity probes ────────────────────────────────────────────────
    println!("\nSanity probes — which top-level branch did each seed land under?");
    let labels = [
        "Berlin",       // city
        "Brantford",    // city
        "Sarah",        // person
        "Nick",         // person
        "Tuesday",      // time
        "yesterday",    // time
        "6 pounds",     // quantity
        "$42",          // quantity
        "the",          // function
        "of",           // function
        "Times Square", // multi-word location
        "Apollo project", // multi-word entity
    ];
    for label in labels {
        if let Some(id) = nodes.iter().position(|n| n.name == label) {
            let root = walk_to_root(id, &parents);
            let depth = depth_of(id, &parents);
            println!(
                "  '{label}' → root '{}' (depth {})",
                nodes[root].name, depth,
            );
        }
    }
}

/// Decide where a new element with `embedding` belongs in the DAG.
/// Walks from GENESIS, descending into children whose cosine with the
/// incoming embedding clears `DESCEND_THRESHOLD`. Returns the action
/// the caller should apply.
fn route_into_dag(emb: &[f32], nodes: &[Node], parents: &ParentMap) -> Action {
    let mut current: Option<usize> = None; // None == GENESIS

    loop {
        let children: Vec<usize> = match current {
            None => (0..nodes.len()).filter(|&i| parents[i].is_none()).collect(),
            Some(c) => (0..nodes.len()).filter(|&i| parents[i] == Some(c)).collect(),
        };

        if children.is_empty() {
            // No children to descend into — attach as new child here.
            return Action::AttachBelow(current);
        }

        // Score children by cosine.
        let mut best_child: Option<usize> = None;
        let mut best_score: f32 = f32::NEG_INFINITY;
        for &c in &children {
            let s = dot(emb, &nodes[c].embedding);
            if s > best_score {
                best_score = s;
                best_child = Some(c);
            }
        }
        let best = best_child.unwrap();

        if best_score >= MERGE_THRESHOLD {
            return Action::Merge(best);
        }
        if best_score >= DESCEND_THRESHOLD {
            // Descend into the best child and recurse.
            current = Some(best);
            continue;
        }
        // No child crosses descend_threshold. Attach here.
        return Action::AttachBelow(current);
    }
}

fn embed_normalized(text: &str) -> Vec<f32> {
    let mut v = embed_text(text);
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    for x in &mut v {
        *x /= norm;
    }
    debug_assert_eq!(v.len(), EMBEDDING_DIM);
    v
}

fn print_subtree(id: usize, nodes: &[Node], parents: &ParentMap, depth: usize) {
    let indent = "  ".repeat(depth);
    println!("{indent}#{} {}", id, nodes[id].name);
    let mut children: Vec<usize> = (0..nodes.len())
        .filter(|&i| parents[i] == Some(id))
        .collect();
    children.sort();
    for c in children {
        print_subtree(c, nodes, parents, depth + 1);
    }
}

fn walk_to_root(id: usize, parents: &ParentMap) -> usize {
    let mut current = id;
    while let Some(p) = parents[current] {
        current = p;
    }
    current
}

fn depth_of(id: usize, parents: &ParentMap) -> usize {
    let mut current = id;
    let mut d = 0;
    while let Some(p) = parents[current] {
        current = p;
        d += 1;
    }
    d
}
