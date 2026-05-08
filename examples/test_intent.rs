//! Batch test of `detect_intent` across a diverse input set.
//!
//! Run: `cargo run --release --example test_intent 2>/dev/null`
//! (Stderr is redirected to silence the per-call debug logits.)
//!
//! Loads the model once, runs through ~50 inputs grouped by expected
//! dimension, and reports both per-input scores and per-group accuracy.

use legend::steps::detect_intent::detect_intent;
use legend::types::Intent;

struct Group {
    label: &'static str,
    expected_dim: Option<&'static str>, // None = neutral / no dim should clearly win
    direction: Direction,
    inputs: &'static [&'static str],
}

#[derive(Copy, Clone)]
enum Direction {
    High, // expected_dim score should be > 0.5
    Low,  // expected_dim score should be < 0.5
    None, // neutral
}

fn main() {
    let groups: &[Group] = &[
        Group {
            label: "HIGH CONVICTION (expect: conviction > 0.5)",
            expected_dim: Some("conviction"),
            direction: Direction::High,
            inputs: &[
                "I am absolutely certain that the meeting is at 3pm",
                "Without a doubt, the deployment succeeded",
                "It's a fact that water boils at 100 degrees",
                "I know for certain the bug is in the parser",
                "There is no question that we shipped on time",
            ],
        },
        Group {
            label: "LOW CONVICTION (expect: conviction < 0.5)",
            expected_dim: Some("conviction"),
            direction: Direction::Low,
            inputs: &[
                "I think maybe the meeting was rescheduled",
                "I'm not sure if the deployment finished",
                "It might be that the server is down",
                "Perhaps we should reconsider the deadline",
                "I could be wrong, but I think the file is in /tmp",
            ],
        },
        Group {
            label: "HIGH PREDICTION_ERROR (expect: prediction_error > 0.5)",
            expected_dim: Some("prediction_error"),
            direction: Direction::High,
            inputs: &[
                "Actually no, the meeting is at 3pm not 2pm",
                "Wait, that's not right — let me check",
                "I take that back, I was wrong",
                "Scratch that, the file is somewhere else",
                "Correction: the database is named prod_v2",
            ],
        },
        Group {
            label: "LOW PREDICTION_ERROR (expect: prediction_error < 0.5)",
            expected_dim: Some("prediction_error"),
            direction: Direction::Low,
            inputs: &[
                "As usual, the meeting started late",
                "Just like before, the test passes",
                "Nothing new to report",
                "Business as usual today",
                "Same as last week",
            ],
        },
        Group {
            label: "HIGH AROUSAL (expect: arousal > 0.5)",
            expected_dim: Some("arousal"),
            direction: Direction::High,
            inputs: &[
                "This is incredibly important — please respond now",
                "I am furious that the deployment broke production",
                "EMERGENCY: the database is down",
                "I'm so excited about the new feature launch",
                "This is a disaster, the system is crashing",
            ],
        },
        Group {
            label: "LOW AROUSAL (expect: arousal < 0.5)",
            expected_dim: Some("arousal"),
            direction: Direction::Low,
            inputs: &[
                "By the way, I noticed a typo",
                "FYI, the package will arrive tomorrow",
                "No rush on the report",
                "Just a minor note about the icon",
                "Whenever you have time, could you look at this",
            ],
        },
        Group {
            label: "HIGH CURIOSITY (expect: curiosity > 0.5)",
            expected_dim: Some("curiosity"),
            direction: Direction::High,
            inputs: &[
                "What time was the meeting?",
                "Find when I last saw Dr. Rao",
                "Where did I put the deployment notes?",
                "Look up whether I sent the report",
                "How many times have I deployed this week?",
            ],
        },
        Group {
            label: "LOW CURIOSITY (expect: curiosity < 0.5)",
            expected_dim: Some("curiosity"),
            direction: Direction::Low,
            inputs: &[
                "I just deployed the feature",
                "The meeting starts at 3pm",
                "Today I worked on the parser",
                "We found a bug in the logs",
                "The team agreed to ship next week",
            ],
        },
        Group {
            label: "NEUTRAL / EDGE",
            expected_dim: None,
            direction: Direction::None,
            inputs: &["The grass is green", "Today is Tuesday", "Hello", "OK", ""],
        },
    ];

    // Warm up the model and classifiers.
    let _ = detect_intent("warm-up");

    let mut total_correct = 0usize;
    let mut total_evaluated = 0usize;

    for group in groups {
        println!("\n{}", group.label);
        println!("  {:<60}  conv  pe    aro   cur   result", "input");
        println!("  {}", "-".repeat(85));

        let mut group_correct = 0;
        let mut group_evaluated = 0;

        for &input in group.inputs {
            let intent = detect_intent(input);
            let (label, result) = grade(&intent, group.expected_dim, group.direction);
            let truncated = truncate(input, 60);
            println!(
                "  {:<60}  {:.2}  {:.2}  {:.2}  {:.2}  {}",
                format!("\"{truncated}\""),
                intent.conviction,
                intent.prediction_error,
                intent.arousal,
                intent.curiosity,
                label
            );

            if matches!(group.direction, Direction::High | Direction::Low) {
                group_evaluated += 1;
                total_evaluated += 1;
                if result {
                    group_correct += 1;
                    total_correct += 1;
                }
            }
        }

        if group_evaluated > 0 {
            println!(
                "  → {}/{} correct ({:.0}%)",
                group_correct,
                group_evaluated,
                100.0 * group_correct as f32 / group_evaluated as f32
            );
        }
    }

    println!("\n=== OVERALL ===");
    if total_evaluated > 0 {
        println!(
            "{}/{} correct ({:.1}%)",
            total_correct,
            total_evaluated,
            100.0 * total_correct as f32 / total_evaluated as f32
        );
    }
}

fn grade(
    intent: &Intent,
    expected_dim: Option<&str>,
    direction: Direction,
) -> (&'static str, bool) {
    match (expected_dim, direction) {
        (Some(dim), Direction::High) => {
            let score = pick(intent, dim);
            if score > 0.5 {
                ("✓", true)
            } else {
                ("✗ (too low)", false)
            }
        }
        (Some(dim), Direction::Low) => {
            let score = pick(intent, dim);
            if score < 0.5 {
                ("✓", true)
            } else {
                ("✗ (too high)", false)
            }
        }
        _ => ("·", true),
    }
}

fn pick(intent: &Intent, dim: &str) -> f32 {
    match dim {
        "conviction" => intent.conviction,
        "prediction_error" => intent.prediction_error,
        "arousal" => intent.arousal,
        "curiosity" => intent.curiosity,
        _ => 0.0,
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(n - 1).collect();
        out.push('…');
        out
    }
}
