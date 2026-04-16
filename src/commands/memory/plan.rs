use crate::memory::anterior_pfc;

pub(super) fn handle_plan(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let memory = crate::memory::load_or_default()?;

    if args.is_empty() {
        // Show all plans
        let output = anterior_pfc::format_plans_cli(&memory.brain.plans);
        println!("{}", output);
        return Ok(());
    }

    match args[0].as_str() {
        "clear" => {
            let mut memory = memory;
            if args.len() > 1 {
                // Clear specific plan by name
                let name = args[1..].join(" ");
                let name_lower = name.to_lowercase();
                let before = memory.brain.plans.len();
                memory
                    .brain
                    .plans
                    .retain(|p| p.name.to_lowercase() != name_lower);
                if memory.brain.plans.len() < before {
                    crate::memory::save(&memory)?;
                    println!("Cleared plan: {}", name);
                } else {
                    println!("No plan found with name: {}", name);
                }
            } else {
                // Clear all plans
                let count = memory.brain.plans.len();
                memory.brain.plans.clear();
                crate::memory::save(&memory)?;
                println!("Cleared {} plan(s).", count);
            }
            Ok(())
        }
        _ => {
            // Unknown subcommand — show help
            println!("Usage:");
            println!("  legend memory plan              Show all current plans");
            println!("  legend memory plan clear         Clear all plans");
            println!("  legend memory plan clear <name>  Clear a specific plan");
            Ok(())
        }
    }
}
