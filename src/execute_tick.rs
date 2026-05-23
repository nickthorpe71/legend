use crate::embed::embed_text;
use crate::steps::adjust_policy::adjust_policy;
use crate::steps::apply_region_delta::apply_region_delta;
use crate::steps::build_relations::build_relations;
use crate::steps::decay::focus_radius_decay;
use crate::steps::detect_intent::detect_intent;
use crate::steps::frame::assemble_frame;
use crate::steps::hebbian::{derive_active_frame, hebbian_and_salience};
use crate::steps::route_regions::route_regions;
use crate::steps::run_extractors::run_extractors;
use crate::steps::supersede::supersede;
use crate::steps::topical::topical_neighbors;
use crate::types::{ConsciousAttentionFrame, Hypergraph, Tick};

pub fn run(input_text: &str, hypergraph: &mut Hypergraph) -> ConsciousAttentionFrame {
    hypergraph.clock = Tick(hypergraph.clock.0 + 1);
    let embedding = embed_text(input_text);
    let intent = detect_intent(input_text, &embedding);
    let policy = adjust_policy(&intent, &hypergraph.policy);
    let active_frame = derive_active_frame(hypergraph, &intent, &policy);
    let route = route_regions(&embedding, hypergraph, &policy);
    let out = run_extractors(input_text, &[], &policy, hypergraph, &route.active_regions);
    apply_region_delta(hypergraph, &route.delta, &policy);
    let step8 = build_relations(input_text, hypergraph, &out, &policy, None);
    let step9 = supersede(hypergraph, &step8.minted_relations, &policy);
    let topical_seeds = topical_neighbors(hypergraph, &embedding, 32);
    let step10 = hebbian_and_salience(
        hypergraph,
        &step8,
        &step9,
        active_frame,
        &policy,
        &topical_seeds,
    );
    focus_radius_decay(hypergraph, &step10.reinforced, &policy);
    assemble_frame(
        input_text,
        hypergraph,
        &intent,
        active_frame,
        &route,
        &step8,
        &step9,
        &step10,
        &policy,
    )
}
