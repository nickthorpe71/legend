use super::helpers::read_stdin;
use crate::cli::{parse_args, CommandDef};
use crate::commands::daemon::{client::try_over_ipc, handlers::render_tick, ipc::Command};

fn parse_tick_text(
    args: &[String],
    def: &CommandDef,
) -> Result<(String, bool), Box<dyn std::error::Error>> {
    let parsed = parse_args(args, def);
    let blocker = parsed.flags.contains("--blocker") || parsed.flags.contains("-b");

    let text = if parsed.positional.is_empty() {
        read_stdin()?
    } else {
        parsed.positional.join(" ")
    };
    Ok((text, blocker))
}

pub(super) fn handle_tick(
    args: &[String],
    def: &CommandDef,
) -> Result<(), Box<dyn std::error::Error>> {
    let (text, blocker) = parse_tick_text(args, def)?;

    if text.trim().is_empty() {
        return Err("No input provided for tick".into());
    }

    // Try IPC first; daemon keeps ONNX + state warm across invocations.
    match try_over_ipc(Command::Tick {
        text: text.clone(),
        blocker,
    })? {
        Some(stdout) => {
            print!("{}", stdout);
            Ok(())
        }
        None => {
            // Fallback: daemon unavailable, execute in-process. Same render path
            // so stdout matches byte-for-byte.
            let mut memory = crate::memory::load_or_default()?;
            let stdout =
                render_tick(&mut memory, &text, blocker).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            crate::memory::save(&memory)?;
            print!("{}", stdout);
            Ok(())
        }
    }
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
        let (text, blocker) = parse_tick_text(&args, &TEST_TICK_DEF).unwrap();
        assert_eq!(text, "hello world");
        assert!(!blocker);
    }
}
