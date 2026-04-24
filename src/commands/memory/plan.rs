//! `legend memory plan <subcommand>` — thin convenience layer over `PLAN:`
//! ticks for editing individual items without rewriting the whole plan.
//!
//! Queue item #14. Today this ships `set-status`; future verbs (list, show,
//! reorder, add, remove, edit) can land one at a time without changing the
//! dispatch skeleton.

use crate::commands::daemon::{client::try_over_ipc, handlers, ipc::Command};

const DEFAULT_PLAN: &str = "Current Work Queue";

pub(super) fn handle_plan(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("");
    match sub {
        "set-status" => handle_set_status(&args[1..]),
        "" => {
            print_plan_help();
            Ok(())
        }
        other => {
            eprintln!("legend memory plan: unknown subcommand '{}'", other);
            print_plan_help();
            Err(format!("unknown plan subcommand: {}", other).into())
        }
    }
}

/// Parse positional args for `set-status`. Two shapes are accepted:
///   legend memory plan set-status <item-number> <status>
///   legend memory plan set-status <plan-name> <item-number> <status>
/// The first form defaults to the "Current Work Queue" plan.
fn parse_set_status_args(
    args: &[String],
) -> Result<(String, u64, String), Box<dyn std::error::Error>> {
    match args.len() {
        2 => {
            let item_number: u64 = args[0].parse().map_err(|_| {
                format!(
                    "first arg '{}' must be an item number (leading 'N. ' in the plan)",
                    args[0]
                )
            })?;
            let status = args[1].clone();
            Ok((DEFAULT_PLAN.to_string(), item_number, status))
        }
        3 => {
            let plan_name = args[0].clone();
            let item_number: u64 = args[1].parse().map_err(|_| {
                format!("second arg '{}' must be an item number", args[1])
            })?;
            let status = args[2].clone();
            Ok((plan_name, item_number, status))
        }
        _ => Err("Usage: legend memory plan set-status [<plan-name>] <item-number> <status>\n  status: active | pending | deferred | done".into()),
    }
}

fn handle_set_status(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let (plan_name, item_number, status) = parse_set_status_args(args)?;

    if let Some(stdout) = try_over_ipc(Command::PlanSetStatus {
        plan_name: plan_name.clone(),
        item_number,
        status: status.clone(),
    })? {
        print!("{}", stdout);
        return Ok(());
    }

    // In-process fallback: load state, call the shared render function,
    // save. Same user-visible behavior.
    let mut state = crate::memory::load_or_default()?;
    let stdout = handlers::render_plan_set_status(&mut state, &plan_name, item_number, &status)
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    crate::memory::save(&state)?;
    print!("{}", stdout);
    Ok(())
}

fn print_plan_help() {
    println!("Legend Plan - surgical edits to the executive plan queue");
    println!();
    println!("Usage:");
    println!("  legend memory plan set-status <item-number> <status>");
    println!("      Flip an item's status in the default plan (\"Current Work Queue\").");
    println!("      status: active | pending | deferred | done");
    println!();
    println!("  legend memory plan set-status <plan-name> <item-number> <status>");
    println!("      Same, but for a named plan.");
    println!();
    println!("Example:");
    println!("  legend memory plan set-status 15 done");
    println!("  legend memory plan set-status \"Phase 7\" 3 active");
}
