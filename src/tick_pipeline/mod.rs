pub mod adjust_policy;
pub mod apply_region_delta;
pub mod build_relations;
pub mod coref;
pub mod decay;
pub mod detect_intent;
pub mod frame;
pub mod hebbian;
pub mod orthographic;
pub mod relation_patterns;
pub mod route_regions;
pub mod run_extractors;
pub mod supersede;
pub mod temporal;
pub mod topical;
pub mod void_filter;

use crate::embed::embed_text;
use crate::types::{ConsciousAttentionFrame, Hypergraph, Tick};
use self::adjust_policy::adjust_policy;
use self::apply_region_delta::apply_region_delta;
use self::build_relations::build_relations;
use self::decay::focus_radius_decay;
use self::detect_intent::detect_intent;
use self::frame::assemble_frame;
use self::hebbian::{derive_active_frame, hebbian_and_salience};
use self::route_regions::route_regions;
use self::run_extractors::run_extractors;
use self::supersede::supersede;
use self::topical::topical_neighbors;

pub fn execute_tick(input_text: &str, hypergraph: &mut Hypergraph) -> ConsciousAttentionFrame {
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
