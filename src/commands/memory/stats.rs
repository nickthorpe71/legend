use super::event_log::EVENT_LOG_PATH;
use crate::memory::MemoryState;

pub(super) fn handle_stats() -> Result<(), Box<dyn std::error::Error>> {
    let memory = MemoryState::load_or_default()?;
    println!("Memory stats:");
    println!("  Working memory (L1): {}", memory.working_memory.len());
    println!("  Short-term entries: {}", memory.short_term.len());
    println!("  Long-term nodes: {}", memory.long_term.nodes.len());
    println!("  Long-term edges: {}", memory.long_term.edges.len());
    println!(
        "  Ticks since consolidation: {}",
        memory.ticks_since_consolidation
    );
    if let Some(task) = memory.get_task() {
        println!("  Current task: {}", task);
    }

    if let Some(quality) = compute_session_quality() {
        println!();
        println!("Session quality (current session):");
        println!("  Ticks total:      {}", quality.total_ticks);
        println!(
            "  Meaningful ticks: {} ({:.0}%)",
            quality.meaningful_ticks,
            quality.signal_rate * 100.0
        );
        println!("  Queries:          {}", quality.queries);
        println!("  Consolidations:   {}", quality.consolidations);
        println!("  Quality score:    {}/100", quality.score);
        if quality.score < 40 {
            println!("  [LOW] Aim for more meaningful ticks and queries.");
        } else if quality.score < 70 {
            println!("  [OK] Consider querying before new tasks.");
        } else {
            println!("  [GOOD] Strong session signal.");
        }
    }
    Ok(())
}

struct SessionQuality {
    total_ticks: usize,
    meaningful_ticks: usize,
    signal_rate: f32,
    queries: usize,
    consolidations: usize,
    score: u32,
}

fn compute_session_quality() -> Option<SessionQuality> {
    use std::io::BufRead;

    let file = std::fs::File::open(EVENT_LOG_PATH).ok()?;
    let lines: Vec<String> = std::io::BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .collect();

    let last_start_ts: u64 = lines.iter().rev().find_map(|line| {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        if v.get("cmd")?.as_str()? == "start" {
            v.get("ts")?.as_u64()
        } else {
            None
        }
    })?;

    let mut total_ticks = 0usize;
    let mut meaningful_ticks = 0usize;
    let mut queries = 0usize;
    let mut consolidations = 0usize;

    for line in &lines {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let ts = v.get("ts").and_then(|t| t.as_u64()).unwrap_or(0);
        if ts < last_start_ts {
            continue;
        }
        let cmd = v.get("cmd").and_then(|c| c.as_str()).unwrap_or("");
        match cmd {
            "tick" => {
                total_ticks += 1;
                let detail = v.get("detail").and_then(|d| d.as_str()).unwrap_or("");
                if !detail.starts_with("EXPERIENCE:") {
                    meaningful_ticks += 1;
                }
            }
            "query" => queries += 1,
            "auto_consolidate" | "consolidate" => consolidations += 1,
            _ => {}
        }
    }

    let signal_rate = if total_ticks > 0 {
        meaningful_ticks as f32 / total_ticks as f32
    } else {
        0.0
    };

    let signal_score = (signal_rate * 50.0) as u32;
    let query_score = ((queries as f32 / 3.0).min(1.0) * 30.0) as u32;
    let consolidation_score = if consolidations > 0 { 20u32 } else { 0u32 };
    let score = (signal_score + query_score + consolidation_score).min(100);

    Some(SessionQuality {
        total_ticks,
        meaningful_ticks,
        signal_rate,
        queries,
        consolidations,
        score,
    })
}
