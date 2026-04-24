//! `legend memory plan <subcommand>` — thin convenience layer over `PLAN:`
//! ticks for editing individual items without rewriting the whole plan.
//!
//! Queue item #14. Ships: list, show, set-status, reorder, add, remove.
//! `edit` (open $EDITOR with the plan) intentionally skipped — the `PLAN:`
//! tick via heredoc already does the job for bulk edits.

use crate::commands::daemon::{client::try_over_ipc, handlers, ipc::Command};

const DEFAULT_PLAN: &str = "Current Work Queue";

pub(super) fn handle_plan(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("");
    match sub {
        "list" => handle_list(),
        "show" => handle_show(&args[1..]),
        "set-status" => handle_set_status(&args[1..]),
        "reorder" => handle_reorder(&args[1..]),
        "add" => handle_add(&args[1..]),
        "remove" => handle_remove(&args[1..]),
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

// ---------------------------------------------------------------------------
// Read-only verbs
// ---------------------------------------------------------------------------

fn handle_list() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(stdout) = try_over_ipc(Command::PlanList)? {
        print!("{}", stdout);
        return Ok(());
    }
    let state = crate::memory::load_or_default()?;
    let stdout =
        handlers::render_plan_list(&state).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    print!("{}", stdout);
    Ok(())
}

fn handle_show(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let plan_name = if args.is_empty() {
        DEFAULT_PLAN.to_string()
    } else {
        args.join(" ")
    };
    if let Some(stdout) = try_over_ipc(Command::PlanShow {
        plan_name: plan_name.clone(),
    })? {
        print!("{}", stdout);
        return Ok(());
    }
    let state = crate::memory::load_or_default()?;
    let stdout = handlers::render_plan_show(&state, &plan_name)
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    print!("{}", stdout);
    Ok(())
}

// ---------------------------------------------------------------------------
// Mutating verbs
// ---------------------------------------------------------------------------

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
            Ok((DEFAULT_PLAN.to_string(), item_number, args[1].clone()))
        }
        3 => {
            let plan_name = args[0].clone();
            let item_number: u64 = args[1]
                .parse()
                .map_err(|_| format!("second arg '{}' must be an item number", args[1]))?;
            Ok((plan_name, item_number, args[2].clone()))
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
    let mut state = crate::memory::load_or_default()?;
    let stdout = handlers::render_plan_set_status(&mut state, &plan_name, item_number, &status)
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    crate::memory::save(&state)?;
    print!("{}", stdout);
    Ok(())
}

fn parse_reorder_args(
    args: &[String],
) -> Result<(String, usize, usize), Box<dyn std::error::Error>> {
    match args.len() {
        2 => {
            let from: usize = args[0].parse()?;
            let to: usize = args[1].parse()?;
            Ok((DEFAULT_PLAN.to_string(), from, to))
        }
        3 => {
            let from: usize = args[1].parse()?;
            let to: usize = args[2].parse()?;
            Ok((args[0].clone(), from, to))
        }
        _ => Err(
            "Usage: legend memory plan reorder [<plan-name>] <from-position> <to-position>\n  positions are 1-indexed (match the numbers in `memory start`)".into(),
        ),
    }
}

fn handle_reorder(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let (plan_name, from_pos, to_pos) = parse_reorder_args(args)?;
    if let Some(stdout) = try_over_ipc(Command::PlanReorder {
        plan_name: plan_name.clone(),
        from_pos,
        to_pos,
    })? {
        print!("{}", stdout);
        return Ok(());
    }
    let mut state = crate::memory::load_or_default()?;
    let stdout = handlers::render_plan_reorder(&mut state, &plan_name, from_pos, to_pos)
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    crate::memory::save(&state)?;
    print!("{}", stdout);
    Ok(())
}

fn parse_add_args(args: &[String]) -> Result<(String, String, String), Box<dyn std::error::Error>> {
    // legend memory plan add <status> <text...>
    // legend memory plan add --plan "Plan Name" <status> <text...>
    if args.len() >= 3 && args[0] == "--plan" {
        let plan_name = args[1].clone();
        let status = args[2].clone();
        let text = args[3..].join(" ");
        if text.is_empty() {
            return Err("text cannot be empty".into());
        }
        Ok((plan_name, status, text))
    } else if args.len() >= 2 {
        let status = args[0].clone();
        let text = args[1..].join(" ");
        Ok((DEFAULT_PLAN.to_string(), status, text))
    } else {
        Err("Usage: legend memory plan add [--plan <name>] <status> <text...>".into())
    }
}

fn handle_add(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let (plan_name, status, text) = parse_add_args(args)?;
    if let Some(stdout) = try_over_ipc(Command::PlanAdd {
        plan_name: plan_name.clone(),
        status: status.clone(),
        text: text.clone(),
    })? {
        print!("{}", stdout);
        return Ok(());
    }
    let mut state = crate::memory::load_or_default()?;
    let stdout = handlers::render_plan_add(&mut state, &plan_name, &status, &text)
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    crate::memory::save(&state)?;
    print!("{}", stdout);
    Ok(())
}

fn parse_remove_args(args: &[String]) -> Result<(String, u64), Box<dyn std::error::Error>> {
    match args.len() {
        1 => Ok((DEFAULT_PLAN.to_string(), args[0].parse()?)),
        2 => Ok((args[0].clone(), args[1].parse()?)),
        _ => Err("Usage: legend memory plan remove [<plan-name>] <item-number>".into()),
    }
}

fn handle_remove(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let (plan_name, item_number) = parse_remove_args(args)?;
    if let Some(stdout) = try_over_ipc(Command::PlanRemove {
        plan_name: plan_name.clone(),
        item_number,
    })? {
        print!("{}", stdout);
        return Ok(());
    }
    let mut state = crate::memory::load_or_default()?;
    let stdout = handlers::render_plan_remove(&mut state, &plan_name, item_number)
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    crate::memory::save(&state)?;
    print!("{}", stdout);
    Ok(())
}

fn print_plan_help() {
    println!("Legend Plan - surgical edits to the executive plan queue");
    println!();
    println!("Usage:");
    println!("  legend memory plan list                              # all plans + counts");
    println!("  legend memory plan show [<plan-name>]                # one plan's items in order");
    println!("  legend memory plan set-status [<plan-name>] <N> <status>   # flip one item");
    println!("  legend memory plan reorder [<plan-name>] <from> <to>       # move + renumber");
    println!("  legend memory plan add [--plan <name>] <status> <text...>  # append new item");
    println!("  legend memory plan remove [<plan-name>] <N>                # drop item + renumber");
    println!();
    println!("Default plan name is \"Current Work Queue\".");
    println!("<status> is one of: active | pending | deferred | done");
    println!();
    println!("Each mutating command can be followed by `legend daemon checkpoint`");
    println!("to force a disk flush before shutdown.");
}
