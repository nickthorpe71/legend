use crate::commands::update::FeatureUpdate;
use crate::memory::MemoryState;
use crate::storage::{load_state, save_state};
use crate::types::{current_timestamp, Feature, FeatureStatus};

/// Handle 'project' command and its subcommands.
pub fn handle_project(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.is_empty() {
        return handle_project_summary();
    }

    match args[0].as_str() {
        "set" => handle_project_set(&args[1..]),
        "schema" => handle_project_schema(),
        "ls" | "list" => handle_project_summary(),
        _ => {
            // Check if first arg is a feature ID for a quick status update
            if !args[0].starts_with("-") {
                return handle_project_set(args);
            }
            handle_project_summary()
        }
    }
}

fn handle_project_summary() -> Result<(), Box<dyn std::error::Error>> {
    let state = load_state()?;

    println!("# Project: {}", state.project_name);
    println!();

    if state.features.is_empty() {
        println!("No features tracked yet.");
        return Ok(());
    }

    println!("| ID | Name | Status | Domain |");
    println!("|----|------|--------|--------|");

    for f in &state.features {
        println!("| {} | {} | {:?} | {} |", f.id, f.name, f.status, f.domain);
    }

    Ok(())
}

fn handle_project_set(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.is_empty() {
        return Err("Usage: legend project set <id> [options]".into());
    }

    let id = &args[0];
    let mut update = FeatureUpdate {
        id: id.clone(),
        name: None,
        domain: None,
        description: None,
        status: None,
        tags: None,
        context: None,
        files_involved: None,
    };

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--status" => {
                if i + 1 < args.len() {
                    update.status = Some(parse_status(&args[i + 1])?);
                    i += 1;
                }
            }
            "--name" => {
                if i + 1 < args.len() {
                    update.name = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "--domain" => {
                if i + 1 < args.len() {
                    update.domain = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }

    let mut state = load_state()?;
    let now = current_timestamp();

    let mut found = false;
    let mut feature_name = id.clone();

    for f in &mut state.features {
        if f.id == *id {
            feature_name = f.name.clone();
            apply_update(f, update.clone(), now);
            found = true;
            break;
        }
    }

    if !found {
        if let (Some(name), Some(domain)) = (update.name.clone(), update.domain.clone()) {
            feature_name = name.clone();
            let feature = Feature {
                id: id.clone(),
                name,
                domain,
                description: id.clone(),
                status: update.status.unwrap_or(FeatureStatus::Pending),
                tags: update.tags.unwrap_or_default(),
                context: update.context,
                files_involved: update.files_involved.unwrap_or_default(),
                created_at: now,
                last_updated: now,
                recency_score: 1.0,
            };
            state.features.push(feature);
        } else {
            return Err(format!(
                "Feature '{}' not found. To create it, provide --name and --domain.",
                id
            )
            .into());
        }
    }

    save_state(&state)?;

    // Passive experiential logging: status updates are events worth remembering
    if let Some(status) = update.status {
        let mut memory = MemoryState::load_or_default()?;
        memory.tick(&format!(
            "PROGRESS: Feature '{}' status updated to {:?}",
            feature_name, status
        ));
        let _ = memory.save();
    }

    println!("✓ Updated feature '{}'", id);
    Ok(())
}

fn apply_update(feature: &mut Feature, update: FeatureUpdate, now: i64) {
    if let Some(name) = update.name {
        feature.name = name;
    }
    if let Some(domain) = update.domain {
        feature.domain = domain;
    }
    if let Some(status) = update.status {
        feature.status = status;
    }
    feature.last_updated = now;
}

fn parse_status(s: &str) -> Result<FeatureStatus, Box<dyn std::error::Error>> {
    match s.to_lowercase().as_str() {
        "pending" => Ok(FeatureStatus::Pending),
        "in-progress" | "inprogress" | "active" => Ok(FeatureStatus::InProgress),
        "complete" | "done" => Ok(FeatureStatus::Complete),
        "blocked" => Ok(FeatureStatus::Blocked),
        _ => Err(format!(
            "Invalid status '{}'. Valid: Pending, In-Progress, Complete, Blocked",
            s
        )
        .into()),
    }
}

pub fn handle_project_schema() -> Result<(), Box<dyn std::error::Error>> {
    println!("# Legend Project Schema");
    println!();
    println!("## Feature Statuses");
    println!("- `Pending`: Planned but not started");
    println!("- `In-Progress`: Actively being worked on");
    println!("- `Complete`: Finished and verified");
    println!("- `Blocked`: Work stopped due to external factor");
    println!();
    println!("## Automatic Experiential Logging");
    println!("Legend automatically captures experience via hooks:");
    println!("- Session Start: `memory start` primes the brain");
    println!("- User Prompt: `memory query` with prompt text");
    println!("- Tool Execution: captures tool name and status");
    println!("- Turn Completion: summarizes recent actions");
    Ok(())
}
