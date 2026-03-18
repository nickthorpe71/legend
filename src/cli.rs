use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// Command definition types
// ---------------------------------------------------------------------------

pub struct CommandDef {
    pub name: &'static str,
    pub about: &'static str,
    pub usage: &'static str,
    pub flags: &'static [FlagDef],
    pub positionals: &'static [ArgDef],
    pub children: &'static [&'static CommandDef],
}

pub struct FlagDef {
    pub long: &'static str,
    pub short: Option<char>,
    pub about: &'static str,
    pub takes_value: bool,
}

pub struct ArgDef {
    pub name: &'static str,
    pub about: &'static str,
    pub required: bool,
}

// ---------------------------------------------------------------------------
// Parsed args
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct ParsedArgs {
    pub flags: HashSet<String>,
    pub named: HashMap<String, String>,
    pub positional: Vec<String>,
}

impl ParsedArgs {
    pub fn has(&self, flag: &str) -> bool {
        self.flags.contains(flag)
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.named.get(name).map(|s| s.as_str())
    }
}

/// Parse CLI arguments according to a CommandDef.
///
/// Handles:
/// - `--flag` / `-f` boolean flags
/// - `--key value` and `--key=value` named flags (takes_value: true)
/// - `-k value` short named flags
/// - Remaining args collected as positional
pub fn parse_args(args: &[String], def: &CommandDef) -> ParsedArgs {
    let mut result = ParsedArgs::default();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];

        // Handle --flag=value
        if let Some(eq_pos) = arg.find('=') {
            let key = &arg[..eq_pos];
            let value = &arg[eq_pos + 1..];
            if let Some(flag_def) = find_flag_long(def, key) {
                if flag_def.takes_value {
                    result.named.insert(
                        flag_def.long.trim_start_matches('-').to_string(),
                        value.to_string(),
                    );
                    i += 1;
                    continue;
                }
            }
        }

        // Handle --long flags
        if arg.starts_with("--") {
            if let Some(flag_def) = find_flag_long(def, arg) {
                if flag_def.takes_value {
                    if i + 1 < args.len() {
                        result.named.insert(
                            flag_def.long.trim_start_matches('-').to_string(),
                            args[i + 1].clone(),
                        );
                        i += 2;
                        continue;
                    }
                } else {
                    result
                        .flags
                        .insert(flag_def.long.trim_start_matches('-').to_string());
                    i += 1;
                    continue;
                }
            }
            // Unknown --flag, treat as positional
            result.positional.push(arg.clone());
            i += 1;
            continue;
        }

        // Handle -x short flags
        if arg.starts_with('-') && arg.len() == 2 {
            let ch = arg.chars().nth(1).unwrap();
            if let Some(flag_def) = find_flag_short(def, ch) {
                if flag_def.takes_value {
                    if i + 1 < args.len() {
                        result.named.insert(
                            flag_def.long.trim_start_matches('-').to_string(),
                            args[i + 1].clone(),
                        );
                        i += 2;
                        continue;
                    }
                } else {
                    result
                        .flags
                        .insert(flag_def.long.trim_start_matches('-').to_string());
                    i += 1;
                    continue;
                }
            }
        }

        // Positional
        result.positional.push(arg.clone());
        i += 1;
    }

    result
}

fn find_flag_long<'a>(def: &'a CommandDef, arg: &str) -> Option<&'a FlagDef> {
    def.flags.iter().find(|f| f.long == arg)
}

fn find_flag_short(def: &CommandDef, ch: char) -> Option<&FlagDef> {
    def.flags.iter().find(|f| f.short == Some(ch))
}

// ---------------------------------------------------------------------------
// Help generation
// ---------------------------------------------------------------------------

/// Print formatted help for a single command.
pub fn print_command_help(def: &CommandDef) {
    println!(
        "{} - {}",
        title_case(def.name),
        def.about
    );
    println!();
    println!("Usage: {}", def.usage);

    if !def.flags.is_empty() {
        println!();
        println!("Options:");
        for flag in def.flags {
            let short = flag
                .short
                .map(|c| format!("-{}, ", c))
                .unwrap_or_else(|| "    ".to_string());
            let value_hint = if flag.takes_value { " <value>" } else { "" };
            println!(
                "  {}{}{}\t{}",
                short, flag.long, value_hint, flag.about
            );
        }
    }

    if !def.positionals.is_empty() {
        println!();
        println!("Arguments:");
        for arg in def.positionals {
            let req = if arg.required { " (required)" } else { "" };
            println!("  {}{}\t{}", arg.name, req, arg.about);
        }
    }
}

/// Print top-level help from a list of command definitions.
pub fn print_top_level_help(commands: &[(&CommandDef, &str)]) {
    println!("Legend - Lightweight context memory for AI-assisted development");
    println!();
    println!("Usage: legend <command> [options]");
    println!();
    println!("Commands:");
    for (def, usage_hint) in commands {
        println!("  {:<30}{}", usage_hint, def.about);
    }
}

fn title_case(s: &str) -> String {
    s.split(|c: char| c == ' ' || c == '-')
        .filter(|p| !p.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    let upper: String = first.to_uppercase().collect();
                    format!("{}{}", upper, chars.as_str())
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_FLAGS: &[FlagDef] = &[
        FlagDef {
            long: "--compact",
            short: Some('c'),
            about: "Compact output",
            takes_value: false,
        },
        FlagDef {
            long: "--category",
            short: None,
            about: "Filter by category",
            takes_value: true,
        },
        FlagDef {
            long: "--json",
            short: Some('j'),
            about: "JSON output",
            takes_value: false,
        },
    ];

    static TEST_CMD: CommandDef = CommandDef {
        name: "test",
        about: "A test command",
        usage: "legend test [OPTIONS] [TEXT...]",
        flags: TEST_FLAGS,
        positionals: &[ArgDef {
            name: "TEXT",
            about: "Input text",
            required: false,
        }],
        children: &[],
    };

    #[test]
    fn test_parse_boolean_flag_long() {
        let args = vec!["--compact".to_string(), "hello".to_string()];
        let parsed = parse_args(&args, &TEST_CMD);
        assert!(parsed.has("compact"));
        assert_eq!(parsed.positional, vec!["hello"]);
    }

    #[test]
    fn test_parse_boolean_flag_short() {
        let args = vec!["-c".to_string(), "hello".to_string()];
        let parsed = parse_args(&args, &TEST_CMD);
        assert!(parsed.has("compact"));
        assert_eq!(parsed.positional, vec!["hello"]);
    }

    #[test]
    fn test_parse_named_flag_space() {
        let args = vec!["--category".to_string(), "bugs".to_string()];
        let parsed = parse_args(&args, &TEST_CMD);
        assert_eq!(parsed.get("category"), Some("bugs"));
    }

    #[test]
    fn test_parse_named_flag_equals() {
        let args = vec!["--category=architecture".to_string()];
        let parsed = parse_args(&args, &TEST_CMD);
        assert_eq!(parsed.get("category"), Some("architecture"));
    }

    #[test]
    fn test_parse_mixed_flags_and_positional() {
        let args = vec![
            "--compact".to_string(),
            "--category".to_string(),
            "bugs".to_string(),
            "hello".to_string(),
            "world".to_string(),
        ];
        let parsed = parse_args(&args, &TEST_CMD);
        assert!(parsed.has("compact"));
        assert_eq!(parsed.get("category"), Some("bugs"));
        assert_eq!(parsed.positional, vec!["hello", "world"]);
    }

    #[test]
    fn test_parse_no_args() {
        let args: Vec<String> = vec![];
        let parsed = parse_args(&args, &TEST_CMD);
        assert!(parsed.flags.is_empty());
        assert!(parsed.named.is_empty());
        assert!(parsed.positional.is_empty());
    }

    #[test]
    fn test_parse_unknown_flag_becomes_positional() {
        let args = vec!["--unknown".to_string(), "text".to_string()];
        let parsed = parse_args(&args, &TEST_CMD);
        assert_eq!(parsed.positional, vec!["--unknown", "text"]);
    }

    #[test]
    fn test_parse_multiple_boolean_flags() {
        let args = vec!["-c".to_string(), "-j".to_string()];
        let parsed = parse_args(&args, &TEST_CMD);
        assert!(parsed.has("compact"));
        assert!(parsed.has("json"));
    }

    #[test]
    fn test_title_case() {
        assert_eq!(title_case("memory"), "Memory");
        assert_eq!(title_case("mcp-serve"), "Mcp Serve");
        assert_eq!(title_case("git-merge-driver"), "Git Merge Driver");
    }
}
