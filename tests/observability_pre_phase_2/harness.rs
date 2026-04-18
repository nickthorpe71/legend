use crate::common::{seed_basic_repo, Harness};
use legend::memory::MemoryState;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Scenario {
    name: String,
    ticks: Vec<TickStep>,
    checkpoints: Vec<Checkpoint>,
}

#[derive(Debug, Deserialize)]
struct TickStep {
    id: String,
    text: String,
}

#[derive(Debug, Deserialize)]
struct Checkpoint {
    name: String,
    after_tick: usize,
    #[serde(default)]
    consolidate_before_check: bool,
    #[serde(default)]
    expect: Expectations,
}

#[derive(Debug, Default, Deserialize)]
struct Expectations {
    #[serde(default)]
    working_memory_contains: Vec<String>,
    #[serde(default)]
    short_term_contains: Vec<String>,
    #[serde(default)]
    long_term_contains: Vec<String>,
    #[serde(default)]
    any_memory_layer_contains: Vec<String>,
    #[serde(default)]
    working_memory_not_contains: Vec<String>,
    #[serde(default)]
    short_term_not_contains: Vec<String>,
    #[serde(default)]
    long_term_not_contains: Vec<String>,
    #[serde(default)]
    any_memory_layer_not_contains: Vec<String>,
    #[serde(default)]
    working_memory_occurs_at_most: Vec<OccurrenceExpectation>,
    #[serde(default)]
    short_term_occurs_at_most: Vec<OccurrenceExpectation>,
    #[serde(default)]
    long_term_occurs_at_most: Vec<OccurrenceExpectation>,
    #[serde(default)]
    any_memory_layer_occurs_at_most: Vec<OccurrenceExpectation>,
    #[serde(default)]
    long_term_node_labels_contain: Vec<String>,
    #[serde(default)]
    long_term_node_labels_not_contain: Vec<String>,
    #[serde(default)]
    long_term_edges_contain: Vec<EdgeExpectation>,
    #[serde(default)]
    long_term_edges_not_contain: Vec<EdgeExpectation>,
    #[serde(default)]
    long_term_node_weight_greater_than: Vec<NodeWeightComparison>,
    #[serde(default)]
    long_term_edge_weight_greater_than: Vec<EdgeWeightComparison>,
}

#[derive(Debug, Deserialize)]
struct OccurrenceExpectation {
    text: String,
    max: usize,
}

#[derive(Debug, Deserialize)]
struct EdgeExpectation {
    from: String,
    to: String,
    kind: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NodeWeightComparison {
    heavier: String,
    lighter: String,
}

#[derive(Debug, Deserialize)]
struct EdgeWeightComparison {
    heavier: EdgeExpectation,
    lighter: EdgeExpectation,
}

#[derive(Debug)]
struct LayerText {
    working_memory: String,
    short_term: String,
    long_term: String,
}

impl LayerText {
    fn any_layer(&self) -> String {
        format!(
            "{}\n{}\n{}",
            self.working_memory, self.short_term, self.long_term
        )
    }
}

pub fn run_scenario(scenario_json: &str) {
    let scenario: Scenario =
        serde_json::from_str(scenario_json).expect("observability scenario should parse");

    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let mut failures = Vec::new();
    let mut consolidated_for_checkpoint = std::collections::HashSet::new();

    for tick_index in 1..=scenario.ticks.len() {
        let tick = &scenario.ticks[tick_index - 1];
        let _tick_id = tick.id.as_str();
        harness.cmd_ok(&["memory", "tick", &tick.text]);

        for checkpoint in scenario
            .checkpoints
            .iter()
            .filter(|checkpoint| checkpoint.after_tick == tick_index)
        {
            if checkpoint.consolidate_before_check
                && consolidated_for_checkpoint.insert(checkpoint.name.clone())
            {
                harness.cmd_ok(&["memory", "consolidate"]);
            }

            let state = load_state(&harness);
            let layer_text = collect_layer_text(&state);
            assert_checkpoint(
                &scenario.name,
                tick_index,
                checkpoint,
                &state,
                &layer_text,
                &mut failures,
            );
        }
    }

    if !failures.is_empty() {
        panic!(
            "Pre-Phase-2 observability scenario '{}' failed:\n{}",
            scenario.name,
            failures.join("\n")
        );
    }
}

fn load_state(harness: &Harness) -> MemoryState {
    let path = harness.legend_dir().join("memory.lz4");
    legend::memory::load_memory_from_path(&path)
        .unwrap_or_else(|err| panic!("load memory state from {}: {err}", path.display()))
}

fn collect_layer_text(state: &MemoryState) -> LayerText {
    let working_memory = state
        .brain
        .working_memory
        .iter()
        .map(|entry| entry.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    let short_term = state
        .brain
        .short_term
        .iter()
        .flat_map(|entry| [entry.text.as_str(), entry.summary.as_str()])
        .collect::<Vec<_>>()
        .join("\n");

    let long_term = state
        .brain
        .long_term
        .nodes
        .values()
        .flat_map(|node| {
            let mut parts = vec![node.label.as_str(), node.kind.as_str()];
            parts.extend(node.source_texts.iter().map(String::as_str));
            if let Some(full_text) = node.full_text.as_deref() {
                parts.push(full_text);
            }
            parts
        })
        .collect::<Vec<_>>()
        .join("\n");

    LayerText {
        working_memory: normalize(&working_memory),
        short_term: normalize(&short_term),
        long_term: normalize(&long_term),
    }
}

fn assert_checkpoint(
    scenario_name: &str,
    tick_index: usize,
    checkpoint: &Checkpoint,
    state: &MemoryState,
    layer_text: &LayerText,
    failures: &mut Vec<String>,
) {
    assert_contains(
        scenario_name,
        tick_index,
        &checkpoint.name,
        "working_memory",
        &layer_text.working_memory,
        &checkpoint.expect.working_memory_contains,
        failures,
    );
    assert_contains(
        scenario_name,
        tick_index,
        &checkpoint.name,
        "short_term",
        &layer_text.short_term,
        &checkpoint.expect.short_term_contains,
        failures,
    );
    assert_contains(
        scenario_name,
        tick_index,
        &checkpoint.name,
        "long_term",
        &layer_text.long_term,
        &checkpoint.expect.long_term_contains,
        failures,
    );
    assert_contains(
        scenario_name,
        tick_index,
        &checkpoint.name,
        "any_memory_layer",
        &layer_text.any_layer(),
        &checkpoint.expect.any_memory_layer_contains,
        failures,
    );

    assert_not_contains(
        scenario_name,
        tick_index,
        &checkpoint.name,
        "working_memory",
        &layer_text.working_memory,
        &checkpoint.expect.working_memory_not_contains,
        failures,
    );
    assert_not_contains(
        scenario_name,
        tick_index,
        &checkpoint.name,
        "short_term",
        &layer_text.short_term,
        &checkpoint.expect.short_term_not_contains,
        failures,
    );
    assert_not_contains(
        scenario_name,
        tick_index,
        &checkpoint.name,
        "long_term",
        &layer_text.long_term,
        &checkpoint.expect.long_term_not_contains,
        failures,
    );
    assert_not_contains(
        scenario_name,
        tick_index,
        &checkpoint.name,
        "any_memory_layer",
        &layer_text.any_layer(),
        &checkpoint.expect.any_memory_layer_not_contains,
        failures,
    );

    assert_occurs_at_most(
        scenario_name,
        tick_index,
        &checkpoint.name,
        "working_memory",
        &layer_text.working_memory,
        &checkpoint.expect.working_memory_occurs_at_most,
        failures,
    );
    assert_occurs_at_most(
        scenario_name,
        tick_index,
        &checkpoint.name,
        "short_term",
        &layer_text.short_term,
        &checkpoint.expect.short_term_occurs_at_most,
        failures,
    );
    assert_occurs_at_most(
        scenario_name,
        tick_index,
        &checkpoint.name,
        "long_term",
        &layer_text.long_term,
        &checkpoint.expect.long_term_occurs_at_most,
        failures,
    );
    assert_occurs_at_most(
        scenario_name,
        tick_index,
        &checkpoint.name,
        "any_memory_layer",
        &layer_text.any_layer(),
        &checkpoint.expect.any_memory_layer_occurs_at_most,
        failures,
    );

    assert_long_term_node_labels_contain(
        scenario_name,
        tick_index,
        &checkpoint.name,
        state,
        &checkpoint.expect.long_term_node_labels_contain,
        failures,
    );
    assert_long_term_node_labels_not_contain(
        scenario_name,
        tick_index,
        &checkpoint.name,
        state,
        &checkpoint.expect.long_term_node_labels_not_contain,
        failures,
    );
    assert_long_term_edges_contain(
        scenario_name,
        tick_index,
        &checkpoint.name,
        state,
        &checkpoint.expect.long_term_edges_contain,
        failures,
    );
    assert_long_term_edges_not_contain(
        scenario_name,
        tick_index,
        &checkpoint.name,
        state,
        &checkpoint.expect.long_term_edges_not_contain,
        failures,
    );
    assert_long_term_node_weight_greater_than(
        scenario_name,
        tick_index,
        &checkpoint.name,
        state,
        &checkpoint.expect.long_term_node_weight_greater_than,
        failures,
    );
    assert_long_term_edge_weight_greater_than(
        scenario_name,
        tick_index,
        &checkpoint.name,
        state,
        &checkpoint.expect.long_term_edge_weight_greater_than,
        failures,
    );
}

fn assert_contains(
    scenario_name: &str,
    tick_index: usize,
    checkpoint: &str,
    layer: &str,
    haystack: &str,
    needles: &[String],
    failures: &mut Vec<String>,
) {
    for needle in needles {
        let normalized = normalize(needle);
        if !haystack.contains(&normalized) {
            failures.push(format!(
                "[{scenario_name}::{checkpoint} after tick {tick_index}] missing expected `{needle}` in {layer}"
            ));
        }
    }
}

fn assert_not_contains(
    scenario_name: &str,
    tick_index: usize,
    checkpoint: &str,
    layer: &str,
    haystack: &str,
    needles: &[String],
    failures: &mut Vec<String>,
) {
    for needle in needles {
        let normalized = normalize(needle);
        if haystack.contains(&normalized) {
            failures.push(format!(
                "[{scenario_name}::{checkpoint} after tick {tick_index}] found noise `{needle}` in {layer}"
            ));
        }
    }
}

fn assert_occurs_at_most(
    scenario_name: &str,
    tick_index: usize,
    checkpoint: &str,
    layer: &str,
    haystack: &str,
    expectations: &[OccurrenceExpectation],
    failures: &mut Vec<String>,
) {
    for expectation in expectations {
        let normalized = normalize(&expectation.text);
        let actual = haystack.matches(&normalized).count();
        if actual > expectation.max {
            failures.push(format!(
                "[{scenario_name}::{checkpoint} after tick {tick_index}] `{}` occurred {actual} times in {layer}, expected at most {}",
                expectation.text, expectation.max
            ));
        }
    }
}

fn assert_long_term_node_labels_contain(
    scenario_name: &str,
    tick_index: usize,
    checkpoint: &str,
    state: &MemoryState,
    labels: &[String],
    failures: &mut Vec<String>,
) {
    let actual = long_term_node_labels(state);
    for label in labels {
        let normalized = normalize(label);
        if !actual.contains(&normalized) {
            failures.push(format!(
                "[{scenario_name}::{checkpoint} after tick {tick_index}] missing L3 node label `{label}`"
            ));
        }
    }
}

fn assert_long_term_node_labels_not_contain(
    scenario_name: &str,
    tick_index: usize,
    checkpoint: &str,
    state: &MemoryState,
    labels: &[String],
    failures: &mut Vec<String>,
) {
    let actual = long_term_node_labels(state);
    for label in labels {
        let normalized = normalize(label);
        if actual.contains(&normalized) {
            failures.push(format!(
                "[{scenario_name}::{checkpoint} after tick {tick_index}] unexpected L3 node label `{label}`"
            ));
        }
    }
}

fn assert_long_term_edges_contain(
    scenario_name: &str,
    tick_index: usize,
    checkpoint: &str,
    state: &MemoryState,
    edges: &[EdgeExpectation],
    failures: &mut Vec<String>,
) {
    for edge in edges {
        if !long_term_edge_exists(state, edge) {
            failures.push(format!(
                "[{scenario_name}::{checkpoint} after tick {tick_index}] missing L3 edge {} -{}- {}",
                edge.from,
                edge.kind.as_deref().unwrap_or("*"),
                edge.to
            ));
        }
    }
}

fn assert_long_term_edges_not_contain(
    scenario_name: &str,
    tick_index: usize,
    checkpoint: &str,
    state: &MemoryState,
    edges: &[EdgeExpectation],
    failures: &mut Vec<String>,
) {
    for edge in edges {
        if long_term_edge_exists(state, edge) {
            failures.push(format!(
                "[{scenario_name}::{checkpoint} after tick {tick_index}] unexpected L3 edge {} -{}- {}",
                edge.from,
                edge.kind.as_deref().unwrap_or("*"),
                edge.to
            ));
        }
    }
}

fn long_term_node_labels(state: &MemoryState) -> std::collections::HashSet<String> {
    state
        .brain
        .long_term
        .nodes
        .values()
        .map(|node| normalize(&node.label))
        .collect()
}

fn long_term_edge_exists(state: &MemoryState, expected: &EdgeExpectation) -> bool {
    long_term_edge_weight(state, expected).is_some()
}

fn assert_long_term_node_weight_greater_than(
    scenario_name: &str,
    tick_index: usize,
    checkpoint: &str,
    state: &MemoryState,
    comparisons: &[NodeWeightComparison],
    failures: &mut Vec<String>,
) {
    for comparison in comparisons {
        let heavier = long_term_node_weight(state, &comparison.heavier);
        let lighter = long_term_node_weight(state, &comparison.lighter);

        match (heavier, lighter) {
            (Some(heavier), Some(lighter)) if heavier > lighter => {}
            (Some(heavier), Some(lighter)) => failures.push(format!(
                "[{scenario_name}::{checkpoint} after tick {tick_index}] expected L3 node `{}` weight ({heavier:.3}) > `{}` weight ({lighter:.3})",
                comparison.heavier, comparison.lighter
            )),
            (None, _) => failures.push(format!(
                "[{scenario_name}::{checkpoint} after tick {tick_index}] missing heavier L3 node `{}` for weight comparison",
                comparison.heavier
            )),
            (_, None) => failures.push(format!(
                "[{scenario_name}::{checkpoint} after tick {tick_index}] missing lighter L3 node `{}` for weight comparison",
                comparison.lighter
            )),
        }
    }
}

fn assert_long_term_edge_weight_greater_than(
    scenario_name: &str,
    tick_index: usize,
    checkpoint: &str,
    state: &MemoryState,
    comparisons: &[EdgeWeightComparison],
    failures: &mut Vec<String>,
) {
    for comparison in comparisons {
        let heavier = long_term_edge_weight(state, &comparison.heavier);
        let lighter = long_term_edge_weight(state, &comparison.lighter);

        match (heavier, lighter) {
            (Some(heavier), Some(lighter)) if heavier > lighter => {}
            (Some(heavier), Some(lighter)) => failures.push(format!(
                "[{scenario_name}::{checkpoint} after tick {tick_index}] expected L3 edge {} -{}- {} weight ({heavier:.3}) > {} -{}- {} weight ({lighter:.3})",
                comparison.heavier.from,
                comparison.heavier.kind.as_deref().unwrap_or("*"),
                comparison.heavier.to,
                comparison.lighter.from,
                comparison.lighter.kind.as_deref().unwrap_or("*"),
                comparison.lighter.to
            )),
            (None, _) => failures.push(format!(
                "[{scenario_name}::{checkpoint} after tick {tick_index}] missing heavier L3 edge {} -{}- {} for weight comparison",
                comparison.heavier.from,
                comparison.heavier.kind.as_deref().unwrap_or("*"),
                comparison.heavier.to
            )),
            (_, None) => failures.push(format!(
                "[{scenario_name}::{checkpoint} after tick {tick_index}] missing lighter L3 edge {} -{}- {} for weight comparison",
                comparison.lighter.from,
                comparison.lighter.kind.as_deref().unwrap_or("*"),
                comparison.lighter.to
            )),
        }
    }
}

fn long_term_node_weight(state: &MemoryState, label: &str) -> Option<f32> {
    let label = normalize(label);
    state
        .brain
        .long_term
        .nodes
        .values()
        .find(|node| normalize(&node.label) == label)
        .map(|node| node.weight)
}

fn long_term_edge_weight(state: &MemoryState, expected: &EdgeExpectation) -> Option<f32> {
    let from = normalize(&expected.from);
    let to = normalize(&expected.to);
    let kind = expected.kind.as_deref().map(normalize);

    state
        .brain
        .long_term
        .edges
        .iter()
        .find_map(|edge| {
            let from_node = state.brain.long_term.nodes.get(&edge.from)?;
            let to_node = state.brain.long_term.nodes.get(&edge.to)?;

            let edge_from = normalize(&from_node.label);
            let edge_to = normalize(&to_node.label);
            let labels_match =
                (edge_from == from && edge_to == to) || (edge_from == to && edge_to == from);
            let kind_matches = kind
                .as_ref()
                .is_none_or(|expected_kind| normalize(&edge.kind) == *expected_kind);

            if labels_match && kind_matches {
                Some(edge.weight)
            } else {
                None
            }
        })
}

fn normalize(text: &str) -> String {
    text.to_lowercase()
}
