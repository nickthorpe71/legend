mod common;

#[path = "observability_pre_phase_2/harness.rs"]
mod harness;

#[test]
#[ignore = "Pre-Phase-2 baseline: expected to fail until salience, chunking, graph extraction, and pruning improve"]
fn project_alpha_signal_vs_noise_baseline() {
    harness::run_scenario(include_str!(
        "observability_pre_phase_2/project_alpha_signal_noise.json"
    ));
}
