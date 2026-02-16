use crate::storage;

/// Load current Legend state and output as JSON to stdout.
pub fn handle_get_state() -> Result<(), Box<dyn std::error::Error>> {
    let state = storage::load_state()?;
    let json = serde_json::to_string_pretty(&state)
        .map_err(|e| format!("Failed to serialize state to JSON: {}", e))?;
    println!("{}", json);
    Ok(())
}
