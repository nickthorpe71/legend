use super::event_log::*;
use crate::memory::ReinforceResult;

pub(super) fn handle_reinforce(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.len() < 2 {
        return Err("Usage: legend memory reinforce <signal> <id1> [id2 ...]\n  signal: float from -1.0 (irrelevant) to 1.0 (very useful)".into());
    }

    let signal: f32 = args[0].parse().map_err(|_| {
        format!(
            "Invalid signal '{}': expected a float like 1.0 or -0.5",
            args[0]
        )
    })?;

    let ids: Result<Vec<u64>, _> = args[1..].iter().map(|s| s.parse()).collect();
    let ids = ids.map_err(|_| "Invalid entry ID: expected integer(s)")?;

    let mut memory = crate::memory::load_or_default()?;
    let result = crate::memory::basal_ganglia::reinforce(&mut memory.brain, &ids, signal);
    crate::memory::save(&memory)?;

    let event_data = EventData::Reinforce(ReinforceEventData {
        signal,
        entries: result
            .reinforced
            .iter()
            .map(|r| ReinforceEntry {
                id: r.id,
                before: r.salience_before,
                after: r.salience_after,
            })
            .collect(),
        graph_nodes_affected: result.graph_nodes_affected,
    });
    log_event_rich(
        "reinforce",
        &format!("signal={} ids={:?}", signal, ids),
        Some(event_data),
    );
    print_reinforce_result(&result);
    Ok(())
}

fn print_reinforce_result(result: &ReinforceResult) {
    let json = serde_json::to_string(result).unwrap_or_else(|_| "{}".to_string());
    println!("{}", json);
}
