use super::event_log::*;
use super::helpers::*;
use crate::cli::{parse_args, CommandDef, FlagDef};
use crate::commands::llm::{auto_trigger_for_text, LlmAutoTriggerSummary};
use crate::memory::{MemoryState, TickResult};

static TICK_CMD: CommandDef = CommandDef {
    name: "tick",
    about: "Record a memory (decision, progress, discovery)",
    usage: "legend memory tick [--blocker] [--passive] <text>",
    flags: &[
        FlagDef { long: "--blocker", short: Some('b'), about: "Mark as blocker", takes_value: false },
        FlagDef { long: "--passive", short: Some('p'), about: "Passive observation", takes_value: false },
    ],
    positionals: &[],
};

struct TickOptions {
    text: String,
    is_blocker: bool,
    is_passive: bool,
}

fn parse_tick_args(args: &[String]) -> Result<TickOptions, Box<dyn std::error::Error>> {
    let parsed = parse_args(args, &TICK_CMD);

    let text = if parsed.positional.is_empty() {
        read_stdin()?
    } else {
        parsed.positional.join(" ")
    };

    Ok(TickOptions {
        text,
        is_blocker: parsed.has("blocker"),
        is_passive: parsed.has("passive"),
    })
}

pub(super) fn handle_tick(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let opts = parse_tick_args(args)?;

    if opts.text.trim().is_empty() {
        return Err("No input provided for tick".into());
    }

    if is_noise_tick(opts.text.trim()) {
        eprintln!("[tick rejected: low-quality content detected]");
        return Ok(());
    }

    let text = if opts.is_blocker {
        format!("BLOCKER: {}", opts.text.trim())
    } else if opts.is_passive {
        format!("EXPERIENCE: {}", opts.text.trim())
    } else {
        opts.text.trim().to_string()
    };

    let (clean_text, keyword_directives) = extract_keyword_directives(&text);
    let text = if keyword_directives.is_empty() {
        text
    } else {
        clean_text.trim().to_string()
    };

    let mut memory = MemoryState::load_or_default()?;

    let mut keywords_added = false;
    for directive in &keyword_directives {
        let created = memory.add_keyword_node(
            &directive.category,
            &directive.term,
            directive.metadata.clone(),
        );
        if created {
            keywords_added = true;
            eprintln!(
                "[keyword registered: kw:{}:{}]",
                directive.category, directive.term
            );
        }
    }
    if keywords_added {
        memory.rebuild_keyword_cache();
    }

    let tick_result = if text.trim().is_empty() && !keyword_directives.is_empty() {
        memory.save()?;
        for directive in &keyword_directives {
            log_event(
                "keyword_register",
                &format!("kw:{}:{}", directive.category, directive.term),
            );
        }
        println!(
            "{{\"action\":\"keyword_only\",\"keywords_registered\":{}}}",
            keyword_directives.len()
        );
        return Ok(());
    } else if opts.is_passive {
        memory.tick_passive(&text)
    } else {
        memory.tick(&text)
    };
    let should_consolidate = memory.should_suggest_consolidation();

    if let Some(entry) = memory
        .short_term
        .iter_mut()
        .find(|e| e.id == tick_result.entry_id)
    {
        if opts.is_blocker {
            entry.salience = (entry.salience + 0.4).min(1.0);
        } else if opts.is_passive {
            entry.salience *= 0.5;
        }
    }

    memory.save()?;

    let _ = std::fs::write(".legend/.pending_ticks", "0");

    let event_data = EventData::Tick(TickEventData {
        entry_id: Some(tick_result.entry_id),
        matches: tick_result
            .context
            .short_term
            .iter()
            .take(5)
            .map(|m| MatchedEntry {
                id: m.id,
                similarity: m.similarity,
                text_preview: truncate_text(&m.text, 80),
            })
            .collect(),
        graph_nodes: tick_result
            .context
            .long_term
            .iter()
            .take(5)
            .map(|n| GraphHit {
                id: n.id,
                label: n.label.clone(),
                kind: n.kind.clone(),
                weight: n.weight,
            })
            .collect(),
    });
    log_event_rich("tick", text.trim(), Some(event_data));
    let llm_trigger = match auto_trigger_for_text(text.trim(), "memory_tick", Some(memory.clock)) {
        Ok(summary) => Some(summary),
        Err(err) => {
            eprintln!("[llm auto-trigger skipped: {}]", err);
            None
        }
    };

    print_tick_result(&tick_result, llm_trigger);

    if text.trim().to_uppercase().starts_with("ARCHITECTURE:") {
        append_to_architecture_md(text.trim());
    }

    if should_consolidate {
        let summaries = memory.consolidate();
        memory.save()?;
        if !summaries.is_empty() {
            let consolidate_data = EventData::Consolidate(ConsolidateEventData {
                groups_merged: summaries.len(),
                summaries: summaries
                    .iter()
                    .map(|s| ConsolidatedGroup {
                        node_id: s.id,
                        label: truncate_text(&s.label, 60),
                    })
                    .collect(),
            });
            log_event_rich(
                "auto_consolidate",
                &format!("{} groups merged", summaries.len()),
                Some(consolidate_data),
            );
            eprintln!(
                "[auto-consolidated {} group(s) into long-term memory]",
                summaries.len()
            );
        }
    }
    Ok(())
}

fn print_tick_result(result: &TickResult, llm_trigger: Option<LlmAutoTriggerSummary>) {
    let output = serde_json::json!({
        "action": result.action,
        "entry_id": result.entry_id,
        "llm_trigger": llm_trigger,
    });
    let json = serde_json::to_string(&output).unwrap_or_else(|_| "{}".to_string());
    println!("{}", json);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tick_args_simple_text() {
        let args = vec!["hello".to_string(), "world".to_string()];
        let opts = parse_tick_args(&args).unwrap();
        assert_eq!(opts.text, "hello world");
        assert!(!opts.is_blocker);
        assert!(!opts.is_passive);
    }

    #[test]
    fn test_parse_tick_args_blocker_flag() {
        let args = vec!["--blocker".to_string(), "can't".to_string(), "proceed".to_string()];
        let opts = parse_tick_args(&args).unwrap();
        assert!(opts.is_blocker);
        assert_eq!(opts.text, "can't proceed");
    }

    #[test]
    fn test_parse_tick_args_passive_flag() {
        let args = vec!["--passive".to_string(), "observed".to_string(), "event".to_string()];
        let opts = parse_tick_args(&args).unwrap();
        assert!(opts.is_passive);
        assert_eq!(opts.text, "observed event");
    }

    #[test]
    fn test_parse_tick_args_short_flags() {
        let args = vec!["-b".to_string(), "stuck".to_string()];
        let opts = parse_tick_args(&args).unwrap();
        assert!(opts.is_blocker);
    }
}
