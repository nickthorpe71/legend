use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
pub struct DiscoveryReport {
    pub root: String,
    pub languages: HashMap<String, usize>,
    pub directories: Vec<String>,
    pub potential_features: Vec<SuggestedFeature>,
    pub total_files: usize,
}

#[derive(Serialize)]
pub struct SuggestedFeature {
    pub suggested_id: String,
    pub suggested_name: String,
    pub suggested_domain: String,
    pub files: Vec<String>,
}

const SKIP_DIRS: &[&str] = &[
    ".git", ".legend", "target", "node_modules", ".vscode", ".idea", "build", "bin",
];

const SOURCE_ROOTS: &[&str] = &["src", "lib", "app", "pkg"];

/// Scan a directory tree and return a discovery report.
pub fn run_discovery(root: &Path) -> Result<DiscoveryReport, Box<dyn std::error::Error>> {
    let root_path = fs::canonicalize(root)?;
    let mut languages: HashMap<String, usize> = HashMap::new();
    let mut all_files: Vec<PathBuf> = Vec::new();

    walk_directory(&root_path, &root_path, &mut languages, &mut all_files)?;

    let mut top_dirs: Vec<String> = Vec::new();
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

    let potential_features = detect_features(&root_path, &all_files);

    Ok(DiscoveryReport {
        root: root_path.to_string_lossy().to_string(),
        languages,
        directories: top_dirs,
        potential_features,
        total_files: all_files.len(),
    })
}

/// Handle the discover CLI command: JSON to stdout, summary to stderr.
pub fn handle_discover(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let root_path = if args.is_empty() { PathBuf::from(".") } else { PathBuf::from(&args[0]) };
    let report = run_discovery(&root_path)?;

    let json = serde_json::to_string_pretty(&report)?;
    println!("{}", json);

    eprintln!("Discovered {} files in {}", report.total_files, report.root);
    eprintln!("Languages: {}", format_language_summary(&report.languages));
    eprintln!("Suggested features: {}", report.potential_features.len());

    Ok(())
}

/// Recursively walk a directory tree, collecting file paths and counting extensions.
fn walk_directory(
    root: &Path,
    dir: &Path,
    languages: &mut HashMap<String, usize>,
    files: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            let name = entry.file_name();
            if !SKIP_DIRS.contains(&name.to_string_lossy().as_ref()) {
                walk_directory(root, &path, languages, files)?;
            }
        } else if path.is_file() {
            if let Some(ext) = path.extension() {
                *languages.entry(ext.to_string_lossy().to_lowercase()).or_insert(0) += 1;
            }
            files.push(path);
        }
    }
    Ok(())
}

/// Detect features from subdirectories under source roots.
/// Detect potential features from subdirectories under known source roots (src/, lib/, etc.).
fn detect_features(root: &Path, all_files: &[PathBuf]) -> Vec<SuggestedFeature> {
    let mut features: Vec<SuggestedFeature> = Vec::new();

    for source_root in SOURCE_ROOTS {
        let source_dir = root.join(source_root);
        if !source_dir.is_dir() {
            continue;
        }

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
            if dir_name.starts_with('.') {
                continue;
            }

            let dir_files: Vec<String> = all_files
                .iter()
                .filter(|f| f.starts_with(&path))
                .filter_map(|f| f.strip_prefix(root).ok().map(|p| p.to_string_lossy().to_string()))
                .collect();

            if dir_files.len() < 2 {
                continue;
            }

            features.push(SuggestedFeature {
                suggested_id: dir_name.clone(),
                suggested_name: title_case(&dir_name),
                suggested_domain: infer_domain(&dir_name),
                files: dir_files,
            });
        }
    }

    features.sort_by(|a, b| a.suggested_id.cmp(&b.suggested_id));
    features
}

/// Map a directory name to a domain category (security, api, storage, ui, testing).
fn infer_domain(dir_name: &str) -> String {
    let name = dir_name.to_lowercase();
    if ["auth", "login", "session"].iter().any(|k| name.contains(k)) {
        "security".to_string()
    } else if ["api", "routes", "endpoints"].iter().any(|k| name.contains(k)) {
        "api".to_string()
    } else if ["db", "storage", "models", "schema"].iter().any(|k| name.contains(k)) {
        "storage".to_string()
    } else if ["ui", "components", "views", "pages"].iter().any(|k| name.contains(k)) {
        "ui".to_string()
    } else if ["test", "spec"].iter().any(|k| name.contains(k)) {
        "testing".to_string()
    } else {
        name
    }
}

/// Convert a snake_case or kebab-case string to Title Case.
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

fn format_language_summary(languages: &HashMap<String, usize>) -> String {
    if languages.is_empty() {
        return "none detected".to_string();
    }
    let mut sorted: Vec<_> = languages.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));
    sorted.iter().take(5).map(|(ext, count)| format!("{} ({})", ext, count)).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_domain_security() {
        assert_eq!(infer_domain("auth"), "security");
        assert_eq!(infer_domain("login_handler"), "security");
        assert_eq!(infer_domain("session"), "security");
    }

    #[test]
    fn test_infer_domain_api() {
        assert_eq!(infer_domain("api"), "api");
        assert_eq!(infer_domain("routes"), "api");
    }

    #[test]
    fn test_infer_domain_fallback() {
        assert_eq!(infer_domain("commands"), "commands");
        assert_eq!(infer_domain("utils"), "utils");
    }

    #[test]
    fn test_title_case() {
        assert_eq!(title_case("hello_world"), "Hello World");
        assert_eq!(title_case("my-feature"), "My Feature");
        assert_eq!(title_case("simple"), "Simple");
    }

    #[test]
    fn test_format_language_summary_empty() {
        let langs = HashMap::new();
        assert_eq!(format_language_summary(&langs), "none detected");
    }

    #[test]
    fn test_format_language_summary() {
        let mut langs = HashMap::new();
        langs.insert("rs".to_string(), 10);
        langs.insert("toml".to_string(), 2);
        let summary = format_language_summary(&langs);
        assert!(summary.contains("rs"));
        assert!(summary.contains("toml"));
    }
}
