use crate::cli::{parse_args, CommandDef};
use crate::commands::daemon::{client::try_over_ipc, handlers, ipc::Command};

#[derive(Default)]
struct QueryOptions {
    query: String,
    show_reasons: bool,
}

fn parse_query_args(
    args: &[String],
    def: &CommandDef,
) -> Result<QueryOptions, Box<dyn std::error::Error>> {
    let parsed = parse_args(args, def);

    if parsed.positional.is_empty() {
        return Err("Provide a query string".into());
    }

    Ok(QueryOptions {
        query: parsed.positional.join(" "),
        show_reasons: parsed.has("reasons"),
    })
}

pub(super) fn handle_query(
    args: &[String],
    def: &CommandDef,
) -> Result<(), Box<dyn std::error::Error>> {
    let opts = parse_query_args(args, def)?;

    if let Some(stdout) = try_over_ipc(Command::Query {
        text: opts.query.clone(),
        with_reasons: opts.show_reasons,
    })? {
        print!("{}", stdout);
        return Ok(());
    }

    // In-process fallback — daemon unavailable. Same render path, same
    // byte-identical stdout.
    let mut memory = crate::memory::load_or_default()?;
    let stdout = handlers::render_query(&mut memory, &opts.query, opts.show_reasons)
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    // Note: we intentionally do NOT save here — the prior behavior saved
    // unconditionally even in ReadOnly retrieval mode, which Phase 3b's
    // durability note flagged as a bug. The daemon path also skips the save;
    // both paths now match.
    print!("{}", stdout);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::FlagDef;

    static TEST_QUERY_DEF: CommandDef = CommandDef {
        name: "query",
        about: "Query memory",
        usage: "legend memory query [--reasons] <text>",
        flags: &[FlagDef {
            long: "--reasons",
            short: Some('r'),
            about: "Include retrieval reasoning",
            takes_value: false,
        }],
        positionals: &[],
        children: &[],
    };

    #[test]
    fn test_parse_query_args_simple() {
        let args = vec!["memory".to_string(), "system".to_string()];
        let opts = parse_query_args(&args, &TEST_QUERY_DEF).unwrap();
        assert_eq!(opts.query, "memory system");
        assert!(!opts.show_reasons);
    }

    #[test]
    fn test_parse_query_args_reasons_flag() {
        let args = vec!["--reasons".to_string(), "test".to_string()];
        let opts = parse_query_args(&args, &TEST_QUERY_DEF).unwrap();
        assert!(opts.show_reasons);
        assert_eq!(opts.query, "test");
    }

    #[test]
    fn test_parse_query_args_short_reasons() {
        let args = vec!["-r".to_string(), "query".to_string()];
        let opts = parse_query_args(&args, &TEST_QUERY_DEF).unwrap();
        assert!(opts.show_reasons);
    }

    #[test]
    fn test_parse_query_args_empty_rejects() {
        let args: Vec<String> = vec![];
        assert!(parse_query_args(&args, &TEST_QUERY_DEF).is_err());
    }
}
