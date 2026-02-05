// Discover command - walk a project directory and produce a bootstrap report
//
// Scans the filesystem to detect languages, directory patterns, and suggest
// features that Claude can use to help the user set up their Legend state.
//
// Rust concepts in this file:
// - Recursive directory traversal with std::fs::read_dir
// - HashMap for counting/aggregating
// - Path, PathBuf, OsStr for path manipulation
// - Pattern matching on file extensions
// - Building nested data structures

use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The full discovery report, output as JSON to stdout
#[derive(Serialize)]
pub struct DiscoveryReport {
    root: String,
    languages: HashMap<String, usize>,
    directories: Vec<String>,
    potential_features: Vec<SuggestedFeature>,
    total_files: usize,
}

/// A suggested feature inferred from directory structure
#[derive(Serialize)]
pub struct SuggestedFeature {
    suggested_id: String,
    suggested_name: String,
    suggested_domain: String,
    files: Vec<String>,
}

/// Directories to skip during traversal
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".legend",
    "target",
    "node_modules",
    ".vscode",
    ".idea",
    "build",
    "bin",
];

/// Common source root directories where we look for feature subdirectories
const SOURCE_ROOTS: &[&str] = &["src", "lib", "app", "pkg"];

/// Handle the discover command
///
/// Walks the given directory (or ".") and prints a JSON discovery report
/// to stdout with a human-readable summary to stderr.
pub fn handle_discover(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    // Use first argument as path, default to "."
    let root_path = if args.is_empty() {
        PathBuf::from(".")
    } else {
        PathBuf::from(&args[0])
    };

    // Canonicalize so the report shows an absolute path
    let root_path = fs::canonicalize(&root_path)?;

    let mut languages: HashMap<String, usize> = HashMap::new();
    let mut all_files: Vec<PathBuf> = Vec::new();
    let mut top_dirs: Vec<String> = Vec::new();

    // Walk the directory tree recursively
    walk_directory(&root_path, &root_path, &mut languages, &mut all_files)?;

    // Collect notable top-level directories (skip hidden/ignored ones)
    if let Ok(entries) = fs::read_dir(&root_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with('.') && !SKIP_DIRS.contains(&name.as_str()) {
                    top_dirs.push(name);
                }
            }
        }
    }
    top_dirs.sort();

    // Detect potential features from source root subdirectories
    let potential_features = detect_features(&root_path, &all_files);

    let report = DiscoveryReport {
        root: root_path.to_string_lossy().to_string(),
        languages,
        directories: top_dirs,
        potential_features,
        total_files: all_files.len(),
    };

    // JSON to stdout (for Claude)
    let json = serde_json::to_string_pretty(&report)?;
    println!("{}", json);

    // Summary to stderr (for the user)
    eprintln!("Discovered {} files in {}", report.total_files, report.root);
    eprintln!(
        "Languages: {}",
        format_language_summary(&report.languages)
    );
    eprintln!(
        "Suggested features: {}",
        report.potential_features.len()
    );

    Ok(())
}

/// Recursively walk a directory, collecting file extensions and paths
///
/// `root` is the original scan root (for computing relative paths)
/// `dir` is the current directory being scanned
fn walk_directory(
    root: &Path,
    dir: &Path,
    languages: &mut HashMap<String, usize>,
    files: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    // read_dir returns an iterator of Result<DirEntry>
    let entries = fs::read_dir(dir)?;

    for entry in entries {
        // Each entry is Result<DirEntry> - ? unwraps the Ok case
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            // Check if we should skip this directory
            // file_name() returns Option<&OsStr> - the last component of the path
            let dir_name = entry.file_name();
            let dir_name_str = dir_name.to_string_lossy();

            if SKIP_DIRS.contains(&dir_name_str.as_ref()) {
                continue;
            }

            // Recurse into subdirectory
            walk_directory(root, &path, languages, files)?;
        } else if path.is_file() {
            // Count file extensions for language detection
            // extension() returns Option<&OsStr>
            if let Some(ext) = path.extension() {
                let ext_str = ext.to_string_lossy().to_lowercase();
                // HashMap::entry gives us an Entry enum for in-place mutation
                // or_insert(0) sets default to 0 if key doesn't exist
                // then we dereference and increment
                *languages.entry(ext_str).or_insert(0) += 1;
            }

            files.push(path);
        }
    }

    Ok(())
}

/// Detect potential features from subdirectories under source roots
///
/// Looks for directories like src/commands/, src/storage/, lib/auth/ etc.
/// Each subdirectory with 2+ files becomes a suggested feature.
fn detect_features(root: &Path, all_files: &[PathBuf]) -> Vec<SuggestedFeature> {
    let mut features: Vec<SuggestedFeature> = Vec::new();

    for source_root in SOURCE_ROOTS {
        let source_dir = root.join(source_root);
        if !source_dir.is_dir() {
            continue;
        }

        // Read first-level subdirectories under the source root
        let entries = match fs::read_dir(&source_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let dir_name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden directories
            if dir_name.starts_with('.') {
                continue;
            }

            // Collect files that live under this subdirectory
            let dir_files: Vec<String> = all_files
                .iter()
                .filter(|f| f.starts_with(&path))
                .filter_map(|f| {
                    // Make paths relative to root for cleaner output
                    f.strip_prefix(root).ok().map(|p| p.to_string_lossy().to_string())
                })
                .collect();

            // Only suggest if there are 2+ files
            if dir_files.len() < 2 {
                continue;
            }

            let domain = infer_domain(&dir_name);
            let suggested_name = title_case(&dir_name);

            features.push(SuggestedFeature {
                suggested_id: dir_name.clone(),
                suggested_name,
                suggested_domain: domain,
                files: dir_files,
            });
        }
    }

    // Sort by id for consistent output
    features.sort_by(|a, b| a.suggested_id.cmp(&b.suggested_id));
    features
}

/// Infer a domain from a directory name using keyword heuristics
fn infer_domain(dir_name: &str) -> String {
    let name = dir_name.to_lowercase();

    // Check against known patterns
    let security_keywords = ["auth", "login", "session"];
    let api_keywords = ["api", "routes", "endpoints"];
    let storage_keywords = ["db", "storage", "models", "schema"];
    let ui_keywords = ["ui", "components", "views", "pages"];
    let testing_keywords = ["test", "spec"];

    if security_keywords.iter().any(|k| name.contains(k)) {
        "security".to_string()
    } else if api_keywords.iter().any(|k| name.contains(k)) {
        "api".to_string()
    } else if storage_keywords.iter().any(|k| name.contains(k)) {
        "storage".to_string()
    } else if ui_keywords.iter().any(|k| name.contains(k)) {
        "ui".to_string()
    } else if testing_keywords.iter().any(|k| name.contains(k)) {
        "testing".to_string()
    } else {
        // Fallback: use the directory name itself as the domain
        name
    }
}

/// Convert a snake_case or lowercase name to Title Case
fn title_case(s: &str) -> String {
    s.split(|c: char| c == '_' || c == '-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    let upper: String = first.to_uppercase().collect();
                    upper + chars.as_str()
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Format language counts into a compact summary string
fn format_language_summary(languages: &HashMap<String, usize>) -> String {
    if languages.is_empty() {
        return "none detected".to_string();
    }

    // Sort by count descending, then take top entries
    let mut sorted: Vec<_> = languages.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));

    sorted
        .iter()
        .take(5)
        .map(|(ext, count)| format!("{} ({})", ext, count))
        .collect::<Vec<_>>()
        .join(", ")
}
