use super::event_log::log_event;
use crate::memory;

pub(super) fn handle_task(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let subcommand = args.first().map(|s| s.as_str()).unwrap_or("show");

    match subcommand {
        "show" | "" => {
            let state = crate::memory::load_or_default()?;
            match memory::get_task(&state) {
                Some(task) => println!("Current task: {}", task),
                None => println!("No current task set"),
            }
        }
        "set" => {
            if args.len() < 2 {
                return Err("Usage: legend memory task set <task description>".into());
            }
            let task = args[1..].join(" ");
            let mut state = crate::memory::load_or_default()?;
            memory::set_task(&mut state, &task);
            crate::memory::save(&state)?;
            log_event("task_set", &task);
            println!("✓ Current task set: {}", task);
        }
        "clear" => {
            let mut state = crate::memory::load_or_default()?;
            memory::clear_task(&mut state);
            crate::memory::save(&state)?;
            log_event("task_clear", "task cleared");
            println!("✓ Current task cleared");
        }
        _ => {
            let task = args.join(" ");
            let mut state = crate::memory::load_or_default()?;
            memory::set_task(&mut state, &task);
            crate::memory::save(&state)?;
            log_event("task_set", &task);
            println!("✓ Current task set: {}", task);
        }
    }

    Ok(())
}
