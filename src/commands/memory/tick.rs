use super::event_log::*;
use super::helpers::*;
use crate::cli::{parse_args, CommandDef};
use crate::memory::TickResult;

fn parse_tick_text(
    args: &[String],
    def: &CommandDef,
) -> Result<String, Box<dyn std::error::Error>> {
    let parsed = parse_args(args, def);

    if parsed.positional.is_empty() {
        read_stdin()
    } else {
        Ok(parsed.positional.join(" "))
    }
}

pub(super) fn handle_tick(
    args: &[String],
    def: &CommandDef,
) -> Result<(), Box<dyn std::error::Error>> {
    let text = parse_tick_text(args, def)?;

    if text.trim().is_empty() {
        return Err("No input provided for tick".into());
    }

    let (clean_text, keyword_directives) = extract_keyword_directives(&text);
    let text = if keyword_directives.is_empty() {
        text.trim().to_string()
    } else {
        clean_text.trim().to_string()
    };

    let mut memory = crate::memory::load_or_default()?;

    let mut keywords_added = false;
    for directive in &keyword_directives {
        let created = crate::memory::add_keyword_node(
            &mut memory.brain,
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
        crate::memory::rebuild_keyword_cache(&mut memory.brain);
    }

    if text.is_empty() && !keyword_directives.is_empty() {
        crate::memory::save(&memory)?;
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
    }

    let tick_result = crate::memory::tick(&mut memory, &text);

    crate::memory::save(&memory)?;

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

    print_tick_result(&tick_result);

    Ok(())
}

fn print_tick_result(result: &TickResult) {
    let output = serde_json::json!({
        "action": result.action,
        "entry_id": result.entry_id,
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

    static TEST_TICK_DEF: CommandDef = CommandDef {
        name: "tick",
        about: "Record a memory",
        usage: "legend memory tick <text>",
        flags: &[],
        positionals: &[],
        children: &[],
    };

    #[test]
    fn test_parse_tick_text_simple() {
        let args = vec!["hello".to_string(), "world".to_string()];
        let text = parse_tick_text(&args, &TEST_TICK_DEF).unwrap();
        assert_eq!(text, "hello world");
    }
}
