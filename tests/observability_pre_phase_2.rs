mod common;

#[path = "observability_pre_phase_2/harness.rs"]
mod harness;

#[test]
#[ignore = "Pre-Phase-2 baseline: expected to fail until salience, chunking, graph extraction, and pruning improve"]
fn project_alpha_signal_vs_noise_baseline() {
    harness::run_scenario_in_process(include_str!(
        "observability_pre_phase_2/project_alpha_signal_noise.json"
    ));
}

#[test]
#[ignore = "Phase 8 benchmark: multi-domain white-box gate for memory layers and plan queue"]
fn multi_domain_signal_vs_noise_baseline() {
    harness::run_scenario_in_process(include_str!(
        "observability_pre_phase_2/multi_domain_signal_noise.json"
    ));
}
