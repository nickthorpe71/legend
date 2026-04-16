use std::io::{self, Read};

/// A parsed KEYWORD: directive from tick text.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct KeywordDirective {
    pub category: String,
    pub term: String,
    pub metadata: Vec<String>,
}

/// Parse `KEYWORD:<category>:<term>` directives from tick text.
/// Returns (clean_text_without_directives, Vec<KeywordDirective>).
pub(crate) fn extract_keyword_directives(text: &str) -> (String, Vec<KeywordDirective>) {
    let mut directives = Vec::new();
    let mut clean_lines = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("KEYWORD:") {
            if let Some(colon_pos) = rest.find(':') {
                let category = rest[..colon_pos].trim().to_lowercase();
                let after_category = rest[colon_pos + 1..].trim();
                let (term, description) = match after_category.find(char::is_whitespace) {
                    Some(sp) => (
                        after_category[..sp].to_string(),
                        Some(after_category[sp..].trim().to_string()),
                    ),
                    None => (after_category.to_string(), None),
                };
                if !category.is_empty() && !term.is_empty() {
                    let metadata = description.filter(|d| !d.is_empty()).into_iter().collect();
                    directives.push(KeywordDirective {
                        category,
                        term,
                        metadata,
                    });
                    continue;
                }
            }
        }
        clean_lines.push(line);
    }

    (clean_lines.join("\n"), directives)
}

pub fn truncate_text(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let end = s.floor_char_boundary(max);
        format!("{}...", &s[..end])
    }
}

pub(crate) fn read_stdin() -> Result<String, Box<dyn std::error::Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    Ok(input)
}

/// Convert a unix timestamp (seconds) to an ISO date string (YYYY-MM-DD).
pub(super) fn ts_to_iso_date(ts: u64) -> String {
    let mut remaining = ts / 86400;
    let mut year = 1970u64;
    loop {
        let days_in_year =
            if (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400) {
                366
            } else {
                365
            };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }
    let is_leap = (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
    let month_days: &[u64] = if is_leap {
        &[31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        &[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 1u64;
    for &days in month_days {
        if remaining < days {
            break;
        }
        remaining -= days;
        month += 1;
    }
    let day = remaining + 1;
    format!("{:04}-{:02}-{:02}", year, month, day)
}
