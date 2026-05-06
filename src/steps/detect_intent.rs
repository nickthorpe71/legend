use crate::types::Intent;

pub fn detect_intent(input_text: &str) -> Intent {
    Intent {
        conviction: 0.0,
        prediction_error: 0.0,
        arousal: 0.0,
        curiosity: 0.0,
    }
}
