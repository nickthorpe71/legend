//! Experimental v3 — compare three region-anchor strategies on
//! contextualized span embeddings:
//!
//!   A. **embed_text(region_name)** — the v1 baseline (56% in v1
//!      tests). Region's embedding is bare `embed_text("entities")`.
//!   B. **mean of element-shaped member embeddings** — embed each
//!      member with `embed_text(member)` separately and average.
//!      (What v2 would have measured.)
//!   C. **mean of contextualized member embeddings** — embed each
//!      member inside a short context sentence and take the
//!      contextualized span vector for the member alone.
//!
//! Test inputs are also now context-aware: each test case provides
//! a sentence + a span inside it, and we extract the contextualized
//! embedding of that span. This matches how production inputs work
//! — single bare words are an unfair test for sentence-transformer
//! embeddings.
//!
//! Run: `cargo run --release --example route_element_test_v3`

use legend::embed::{EMBEDDING_DIM, embed_sequence_with_offsets, embed_text};
use legend::math::dot;

fn main() {
    // Hand-picked element-shaped members per region. Same list as
    // experiment v2 but used twice here — once bare for strategy B,
    // once in-context for strategy C.
    let regions: &[(&str, &[&str], &[&str])] = &[
        (
            "entities",
            // bare members
            &[
                "Sarah",
                "John",
                "Maya",
                "Dr. Patel",
                "Maria Reyes",
                "the company Verve",
                "Apollo project",
                "a startup called Pylon",
                "the new MacBook",
                "Tom from accounting",
            ],
            // in-context sentences for the SAME members (member is the
            // span we'll pull contextualized embedding for)
            &[
                "I met Sarah at the office yesterday.",
                "John signed the contract last week.",
                "Maya leads the design team.",
                "Dr. Patel called about the appointment.",
                "Maria Reyes joined the project.",
                "The company Verve was acquired.",
                "The Apollo project shipped on time.",
                "A startup called Pylon raised funding.",
                "She bought the new MacBook on Tuesday.",
                "Tom from accounting sent an email.",
            ],
        ),
        (
            "events",
            &[
                "the meeting started",
                "Sarah arrived",
                "we shipped the feature",
                "the deploy went live",
                "her flight lands",
                "the conference starts",
                "the launch happened",
                "the package arrived",
            ],
            &[
                "The meeting started at 3pm.",
                "Sarah arrived around noon.",
                "We shipped the feature this morning.",
                "The deploy went live on Tuesday.",
                "Her flight lands at 3pm.",
                "The conference starts next week.",
                "The launch happened on Friday.",
                "The package arrived earlier than expected.",
            ],
        ),
        (
            "change_history",
            &[
                "she changed her mind",
                "we updated the doc",
                "the price dropped",
                "moved the meeting",
                "rescheduled the appointment",
                "swapped Tuesday for Friday",
                "switched plans",
            ],
            &[
                "She changed her mind about it.",
                "We updated the doc this morning.",
                "The price dropped overnight.",
                "He moved the meeting to Friday.",
                "She rescheduled the appointment.",
                "We swapped Tuesday for Friday.",
                "They switched plans last minute.",
            ],
        ),
        (
            "relationships",
            &[
                "my brother",
                "her friend Sarah",
                "the team",
                "Sarah's manager",
                "his colleague",
                "their family",
                "my best friend",
            ],
            &[
                "I want you to meet my brother.",
                "Her friend Sarah called yesterday.",
                "The team finished the sprint.",
                "Sarah's manager approved the PR.",
                "His colleague helped with the review.",
                "Their family went on vacation.",
                "My best friend lives in Paris.",
            ],
        ),
        (
            "quantities",
            &[
                "6 pounds",
                "$42",
                "3 years",
                "five miles",
                "two cups",
                "a dozen",
                "eight people",
            ],
            &[
                "The baby weighed 6 pounds at birth.",
                "The book cost $42 yesterday.",
                "He lived there for 3 years.",
                "They walked five miles together.",
                "I drank two cups of coffee.",
                "She bought a dozen eggs.",
                "Eight people attended the meeting.",
            ],
        ),
        (
            "time",
            &[
                "Tuesday",
                "3pm",
                "yesterday",
                "next week",
                "tomorrow morning",
                "an hour ago",
                "in 2026",
            ],
            &[
                "The meeting is on Tuesday.",
                "Her flight lands at 3pm.",
                "He called yesterday afternoon.",
                "The launch is next week.",
                "We'll meet tomorrow morning.",
                "She left an hour ago.",
                "The project ships in 2026.",
            ],
        ),
        (
            "locations",
            &[
                "Berlin",
                "Paris",
                "Brantford",
                "the kitchen",
                "Times Square",
                "Tokyo",
                "the office",
            ],
            &[
                "She moved to Berlin last year.",
                "He flew to Paris on Tuesday.",
                "Nick lived in Brantford for years.",
                "The cat is in the kitchen.",
                "We met at Times Square.",
                "The conference was in Tokyo.",
                "I'll see you at the office.",
            ],
        ),
        (
            "tasks",
            &[
                "finish the report",
                "write the email",
                "review the PR",
                "schedule a call",
                "ship the feature",
                "fix the bug",
                "deploy to prod",
            ],
            &[
                "I need to finish the report today.",
                "She'll write the email this afternoon.",
                "Can you review the PR?",
                "Let's schedule a call for Friday.",
                "We need to ship the feature.",
                "Please fix the bug by Friday.",
                "We should deploy to prod tonight.",
            ],
        ),
        (
            "decisions",
            &[
                "I'll go with option A",
                "we decided to ship",
                "chose the Tesla",
                "picked Tuesday",
                "settled on the date",
                "agreed to the terms",
            ],
            &[
                "I'll go with option A.",
                "We decided to ship on Friday.",
                "He chose the Tesla over the Honda.",
                "We picked Tuesday for the meeting.",
                "They settled on the date last week.",
                "We agreed to the terms yesterday.",
            ],
        ),
        (
            "preferences",
            &[
                "I prefer tea",
                "she likes Italian food",
                "he hates meetings",
                "loves coffee",
                "favorite color is blue",
                "would rather walk",
            ],
            &[
                "I prefer tea over coffee.",
                "She likes Italian food best.",
                "He hates meetings on Mondays.",
                "She loves coffee in the morning.",
                "My favorite color is blue.",
                "I would rather walk than drive.",
            ],
        ),
        (
            "definitions",
            &[
                "a widget is a small part",
                "the term refers to",
                "by which we mean",
                "is defined as",
                "what we call a sprint",
            ],
            &[
                "A widget is a small part of the system.",
                "The term refers to a process.",
                "By which we mean the iteration cycle.",
                "It is defined as a unit of work.",
                "What we call a sprint is two weeks.",
            ],
        ),
        (
            "provenance",
            &[
                "according to Sarah",
                "she told me",
                "the email said",
                "per the document",
                "I read on the website",
            ],
            &[
                "According to Sarah, the meeting moved.",
                "She told me yesterday afternoon.",
                "The email said the deploy was delayed.",
                "Per the document, the release is Friday.",
                "I read on the website that they shipped.",
            ],
        ),
        (
            "domains",
            &[
                "programming",
                "medicine",
                "law",
                "music theory",
                "professional sports",
                "Italian cooking",
            ],
            &[
                "He's been doing programming for years.",
                "She studies medicine at the university.",
                "Law school takes three years.",
                "Music theory underlies composition.",
                "Professional sports demand discipline.",
                "Italian cooking uses fresh ingredients.",
            ],
        ),
        (
            "modal_negated",
            &[
                "not coming",
                "won't be there",
                "didn't happen",
                "shouldn't have",
                "not the case",
                "never went to Paris",
            ],
            &[
                "She is not coming to the meeting.",
                "He won't be there tomorrow.",
                "That didn't happen yesterday.",
                "I shouldn't have said that.",
                "That is not the case.",
                "He never went to Paris.",
            ],
        ),
    ];

    // Context-aware test set: (sentence, span_text, expected_region).
    // Span must appear verbatim in the sentence.
    let cases: &[(&str, &str, Option<&str>)] = &[
        ("Nick lived in Brantford for 3 years.", "Brantford", Some("locations")),
        ("She moved to Berlin last year.", "Berlin", Some("locations")),
        ("He flew to Paris on Tuesday.", "Paris", Some("locations")),
        ("We met at Times Square.", "Times Square", Some("locations")),
        ("Nick lived in Brantford for 3 years.", "Nick", Some("entities")),
        ("Sarah called me yesterday.", "Sarah", Some("entities")),
        ("Dr. Rao changed the appointment.", "Dr. Rao", Some("entities")),
        ("The Apollo project shipped on time.", "Apollo project", Some("entities")),
        ("The meeting is on Tuesday.", "Tuesday", Some("time")),
        ("Her flight lands at 3pm.", "3pm", Some("time")),
        ("He called yesterday afternoon.", "yesterday", Some("time")),
        ("Nick lived in Brantford for 3 years.", "3 years", Some("quantities")),
        ("The baby weighed 6 pounds at birth.", "6 pounds", Some("quantities")),
        ("The book cost $42.", "$42", Some("quantities")),
        ("The dentist saw three patients.", "dentist", Some("entities")),
        ("Sarah is happy today.", "happy", None),
        ("The meeting is scheduled.", "meeting", Some("events")),
        ("She changed her mind about it.", "changed her mind", Some("change_history")),
        ("I prefer tea over coffee.", "prefer tea", Some("preferences")),
    ];

    // Strategy A — embed_text(region_name)
    let anchors_a: Vec<(String, Vec<f32>)> = regions
        .iter()
        .map(|(name, _, _)| {
            let v = embed_text(name);
            (name.to_string(), v)
        })
        .collect();

    // Strategy B — mean of bare-member embeddings
    let anchors_b: Vec<(String, Vec<f32>)> = regions
        .iter()
        .map(|(name, members, _)| {
            let v = mean_normalized(members.iter().map(|m| embed_text(m)));
            (name.to_string(), v)
        })
        .collect();

    // Strategy C — mean of contextualized member-span embeddings.
    // For each member appearing in a context sentence, we pull the
    // contextualized embedding for just the member's span and average
    // those per region.
    let anchors_c: Vec<(String, Vec<f32>)> = regions
        .iter()
        .map(|(name, members, contexts)| {
            let span_vecs = members.iter().zip(contexts.iter()).map(|(member, sentence)| {
                contextualized_span_embedding(sentence, member)
                    .unwrap_or_else(|| embed_text(member)) // fallback if span not found
            });
            let v = mean_normalized(span_vecs);
            (name.to_string(), v)
        })
        .collect();

    // Pre-compute the test-input contextualized span embeddings once.
    let test_vecs: Vec<Vec<f32>> = cases
        .iter()
        .map(|(sentence, span, _)| {
            contextualized_span_embedding(sentence, span)
                .unwrap_or_else(|| {
                    // Fall back to bare embedding if span tokenization
                    // didn't line up — and shout about it so we can
                    // see in the output.
                    eprintln!("[warn] span {span:?} not found in tokenization of {sentence:?}; using bare embedding");
                    embed_text(span)
                })
        })
        .collect();

    println!("Strategy A: embed_text(region_name)");
    print_run(cases, &test_vecs, &anchors_a);
    println!("\nStrategy B: mean of bare-member embeddings");
    print_run(cases, &test_vecs, &anchors_b);
    println!("\nStrategy C: mean of CONTEXTUALIZED member-span embeddings");
    print_run(cases, &test_vecs, &anchors_c);
}

/// Mean-pool a stream of vectors and L2-normalize.
fn mean_normalized<I: Iterator<Item = Vec<f32>>>(iter: I) -> Vec<f32> {
    let mut acc = vec![0.0f32; EMBEDDING_DIM];
    let mut n = 0usize;
    for v in iter {
        debug_assert_eq!(v.len(), EMBEDDING_DIM);
        for i in 0..EMBEDDING_DIM {
            acc[i] += v[i];
        }
        n += 1;
    }
    if n > 0 {
        let inv = 1.0 / n as f32;
        for x in &mut acc {
            *x *= inv;
        }
    }
    let norm: f32 = acc.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-12);
    for x in &mut acc {
        *x /= norm;
    }
    acc
}

/// Run the input forward pass, mean-pool the contextualized embeddings
/// over the tokens whose char offsets fall inside the span, return the
/// L2-normalized result. `None` if the span doesn't match any tokens.
fn contextualized_span_embedding(text: &str, span: &str) -> Option<Vec<f32>> {
    let span_start = text.find(span)?;
    let span_end = span_start + span.len();

    let (sequence, offsets) = embed_sequence_with_offsets(text);
    if sequence.is_empty() {
        return None;
    }

    // Mean-pool over tokens whose offset (char_start, char_end) overlaps
    // the span. Skip special tokens (offsets = (0, 0)).
    let mut acc = vec![0.0f32; EMBEDDING_DIM];
    let mut n = 0usize;
    for (t, &(start, end)) in offsets.iter().enumerate() {
        if start == 0 && end == 0 {
            continue;
        }
        let overlaps = end > span_start && start < span_end;
        if !overlaps {
            continue;
        }
        let base = t * EMBEDDING_DIM;
        for i in 0..EMBEDDING_DIM {
            acc[i] += sequence[base + i];
        }
        n += 1;
    }
    if n == 0 {
        return None;
    }
    let inv = 1.0 / n as f32;
    for x in &mut acc {
        *x *= inv;
    }
    let norm: f32 = acc.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-12);
    for x in &mut acc {
        *x /= norm;
    }
    Some(acc)
}

fn print_run(
    cases: &[(&str, &str, Option<&str>)],
    test_vecs: &[Vec<f32>],
    anchors: &[(String, Vec<f32>)],
) {
    let mut correct = 0usize;
    let mut total = 0usize;

    for ((sentence, span, expected), test_vec) in cases.iter().zip(test_vecs.iter()) {
        let mut scored: Vec<(&str, f32)> = anchors
            .iter()
            .map(|(name, vec)| (name.as_str(), dot(test_vec, vec)))
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        let top1 = scored.first().map(|(n, _)| *n).unwrap_or("?");
        let mark = match expected {
            Some(want) => {
                total += 1;
                if top1 == *want {
                    correct += 1;
                    "✓"
                } else {
                    "✗"
                }
            }
            None => " ",
        };

        let exp = expected.unwrap_or("(no prior)");
        let context_preview = if sentence.chars().count() > 40 {
            let cut: String = sentence.chars().take(37).collect();
            format!("{cut}…")
        } else {
            sentence.to_string()
        };
        print!(
            "  {mark} {span:<20}  in {context_preview:<42}  exp={exp:<14}  top3:",
        );
        for (n, s) in scored.iter().take(3) {
            print!("  {n}({s:+.3})");
        }
        if let Some(want) = expected
            && let Some(r) = scored.iter().position(|(n, _)| n == want).map(|i| i + 1)
            && r > 3
        {
            print!("  [rank {r}]");
        }
        println!();
    }

    if total > 0 {
        println!(
            "  → top-1 accuracy: {correct}/{total} ({:.0}%)",
            100.0 * correct as f32 / total as f32,
        );
    }
}
