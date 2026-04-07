use crate::cli::{parse_args, CommandDef};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
pub struct DiscoveryReport {
    pub root: String,
    pub metadata: ProjectMetadata,
    pub languages: HashMap<String, usize>,
    pub high_signal_files: Vec<HighSignalFile>,
    pub potential_features: Vec<SuggestedFeature>,
    pub tasks: Vec<DiscoveryTask>,
    pub total_files: usize,
}

#[derive(Serialize)]
pub struct DiscoveryTask {
    pub id: String,
    pub prompt: String,
    pub tool_hint: String,
}

#[derive(Serialize, Default)]
pub struct ProjectMetadata {
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub tech_stack: Vec<String>,
}

#[derive(Serialize)]
pub struct HighSignalFile {
    pub path: String,
    pub kind: String, // "Documentation", "Manifest", "EntryPoint"
}

#[derive(Serialize)]
pub struct SuggestedFeature {
    pub suggested_id: String,
    pub suggested_name: String,
    pub suggested_domain: String,
    pub files: Vec<String>,
}

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

const SOURCE_ROOTS: &[&str] = &["src", "lib", "app", "pkg"];

const DOC_FILES: &[&str] = &[
    "README.md",
    "ARCHITECTURE.md",
    "VISION.md",
    "PLAN.md",
    "GEMINI.md",
    "CLAUDE.md",
    "CODEX.md",
    "PRD.md",
];

use std::process::Command;

/// Scan a directory tree and return a discovery report.
pub fn run_discovery(root: &Path) -> Result<DiscoveryReport, Box<dyn std::error::Error>> {
    let root_path = fs::canonicalize(root)?;
    let mut languages: HashMap<String, usize> = HashMap::new();
    let mut all_files: Vec<PathBuf> = Vec::new();

    walk_directory(&root_path, &root_path, &mut languages, &mut all_files)?;

    let high_signal_files = scan_high_signal(&root_path, &all_files);
    let metadata = extract_metadata(&root_path, &high_signal_files);
    let potential_features = detect_features(&root_path, &all_files);
    let tasks = generate_tasks(&root_path, &high_signal_files, &potential_features);

    Ok(DiscoveryReport {
        root: root_path.to_string_lossy().to_string(),
        metadata,
        languages,
        high_signal_files,
        potential_features,
        tasks,
        total_files: all_files.len(),
    })
}

fn generate_tasks(
    root: &Path,
    high_signal: &[HighSignalFile],
    features: &[SuggestedFeature],
) -> Vec<DiscoveryTask> {
    let mut tasks = Vec::new();

    // 1. Documentation Task
    if let Some(readme) = high_signal
        .iter()
        .find(|f| f.path == "README.md" || f.path == "README")
    {
        tasks.push(DiscoveryTask {
            id: "read_readme".to_string(),
            prompt: "The project has a README.md. Read it to understand the core goals and onboarding flow.".to_string(),
            tool_hint: format!("read_file {{ \"file_path\": \"{}\" }}", readme.path),
        });
    }

    // 2. Git History Task
    let git_log = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("log")
        .arg("-n")
        .arg("10")
        .arg("--oneline")
        .output();

    if let Ok(out) = git_log {
        if out.status.success() {
            let log = String::from_utf8_lossy(&out.stdout);
            let mut high_signal_commits = Vec::new();
            for line in log.lines() {
                let lower = line.to_lowercase();
                if [
                    "refactor", "arch", "decision", "change", "major", "move", "fix",
                ]
                .iter()
                .any(|k| lower.contains(k))
                {
                    high_signal_commits.push(line.to_string());
                }
            }

            if !high_signal_commits.is_empty() {
                tasks.push(DiscoveryTask {
                    id: "git_history".to_string(),
                    prompt: format!(
                        "I found several interesting commits in the git history:\n{}\nPick the most significant ones and investigate their impact.",
                        high_signal_commits.join("\n")
                    ),
                    tool_hint: "git log -p -n 5".to_string(),
                });
            }
        }
    }

    // 3. Feature Investigation
    if !features.is_empty() {
        let feature_names: Vec<String> = features
            .iter()
            .take(3)
            .map(|f| f.suggested_name.clone())
            .collect();
        tasks.push(DiscoveryTask {
            id: "feature_deep_dive".to_string(),
            prompt: format!(
                "I've identified potential features like {}. Choose one and trace its implementation through the codebase.",
                feature_names.join(", ")
            ),
            tool_hint: format!("ls -R {}", features[0].files[0].split('/').next().unwrap_or("src")),
        });
    }

    tasks
}

/// Handle the discover CLI command: JSON to stdout, summary to stderr.
///
/// NOTE: `legend discover` is deprecated. Use `legend init` instead, which
/// includes discovery automatically on first init.
pub fn handle_discover(
    args: &[String],
    def: &CommandDef,
) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("[Note: `legend discover` is deprecated. Use `legend init` which includes discovery automatically.]");

    let parsed = parse_args(args, def);
    let apply = parsed.has("apply");
    let path = parsed
        .positional
        .first()
        .map(|s| PathBuf::from(s))
        .unwrap_or_else(|| PathBuf::from("."));

    let report = run_discovery(&path)?;

    if apply {
        onboard_project(&path, &report)?;
        println!("✓ Project onboarding complete. High-signal context ingested into Legend.");
        println!("\nNext Steps for AI Agent:");
        println!("1. Run 'legend memory start' to see the ingested context.");
        println!("2. Use 'legend memory query' to explore specific modules or features.");
        println!("3. If significant architectural details are missing, manually 'tick' them.");
    } else {
        let json = serde_json::to_string_pretty(&report)?;
        println!("{}", json);

        eprintln!(
            "\nDiscovered {} files in {}",
            report.total_files, report.root
        );
        eprintln!(
            "Project: {} (v{})",
            report.metadata.name,
            report.metadata.version.as_deref().unwrap_or("unknown")
        );
        eprintln!("Languages: {}", format_language_summary(&report.languages));
        eprintln!("High-signal files: {}", report.high_signal_files.len());
        eprintln!("Suggested features: {}", report.potential_features.len());

        if !Path::new(".legend").exists() {
            eprintln!("\nTip: Run 'legend init' or 'legend discover --apply' to start tracking this project.");
        } else {
            eprintln!(
                "\nTip: Run 'legend discover --apply' to ingest this context into Legend's memory."
            );
        }
    }

    Ok(())
}

/// Ingest high-signal context into Legend's memory.
pub(crate) fn onboard_project(
    root: &Path,
    report: &DiscoveryReport,
) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(".legend")?;
    let mut memory = crate::memory::load_or_default()?;

    // 1. Ingest metadata
    let mut metadata_text = format!(
        "ONBOARDING: Discovery report for {}\n",
        report.metadata.name
    );
    if let Some(v) = &report.metadata.version {
        metadata_text.push_str(&format!("Version: {}\n", v));
    }
    if let Some(d) = &report.metadata.description {
        metadata_text.push_str(&format!("Description: {}\n", d));
    }
    if !report.metadata.tech_stack.is_empty() {
        metadata_text.push_str(&format!(
            "Tech Stack: {}\n",
            report.metadata.tech_stack.join(", ")
        ));
    }
    crate::memory::tick(&mut memory, &metadata_text);

    // 2. Ingest high-signal files (Documentation & Manifests)
    for file in &report.high_signal_files {
        if file.kind == "Documentation" || file.kind == "Manifest" {
            let full_path = root.join(&file.path);
            if let Ok(content) = fs::read_to_string(&full_path) {
                eprintln!("  Ingesting {}...", file.path);
                let tick_content = format!(
                    "CONTEXT: High-signal file '{}' ({})\n\n{}",
                    file.path, file.kind, content
                );
                crate::memory::tick(&mut memory, &tick_content);
            }
        }
    }

    // 3. Ingest Discovery Tasks
    if !report.tasks.is_empty() {
        let mut tasks_text = String::from("ONBOARDING TASKS: The following investigations are recommended to complete the mental model:\n");
        for task in &report.tasks {
            tasks_text.push_str(&format!(
                "- [{}] {}. Tool hint: {}\n",
                task.id, task.prompt, task.tool_hint
            ));
        }
        crate::memory::tick(&mut memory, &tasks_text);
    }

    crate::memory::save(&memory)?;

    Ok(())
}

fn scan_high_signal(root: &Path, all_files: &[PathBuf]) -> Vec<HighSignalFile> {
    let mut high_signal = Vec::new();

    for path in all_files {
        let rel_path = path.strip_prefix(root).unwrap_or(path);
        let filename = rel_path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // Root docs
        if rel_path
            .parent()
            .map(|p| p.as_os_str().is_empty())
            .unwrap_or(true)
            && DOC_FILES.contains(&filename)
        {
            high_signal.push(HighSignalFile {
                path: rel_path.to_string_lossy().to_string(),
                kind: "Documentation".to_string(),
            });
            continue;
        }

        // Manifests
        if is_manifest(filename) {
            high_signal.push(HighSignalFile {
                path: rel_path.to_string_lossy().to_string(),
                kind: "Manifest".to_string(),
            });
            continue;
        }

        // Entry points
        if is_entry_point(filename) {
            high_signal.push(HighSignalFile {
                path: rel_path.to_string_lossy().to_string(),
                kind: "EntryPoint".to_string(),
            });
            continue;
        }

        // Subdirectory READMEs are also high signal
        if filename == "README.md" {
            high_signal.push(HighSignalFile {
                path: rel_path.to_string_lossy().to_string(),
                kind: "Documentation".to_string(),
            });
        }
    }

    high_signal
}

fn is_manifest(filename: &str) -> bool {
    matches!(
        filename,
        "Cargo.toml"
            | "package.json"
            | "go.mod"
            | "requirements.txt"
            | "pyproject.toml"
            | "Gemfile"
            | "Makefile"
    )
}

fn is_entry_point(filename: &str) -> bool {
    matches!(
        filename,
        "main.rs" | "lib.rs" | "main.py" | "app.py" | "index.ts" | "index.js" | "main.go"
    )
}

fn extract_metadata(root: &Path, high_signal: &[HighSignalFile]) -> ProjectMetadata {
    let mut meta = ProjectMetadata {
        name: root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unknown Project".to_string()),
        ..Default::default()
    };

    for file in high_signal {
        if file.kind == "Manifest" {
            let path = root.join(&file.path);
            if let Ok(content) = fs::read_to_string(path) {
                if file.path.ends_with("Cargo.toml") {
                    parse_cargo_toml(&content, &mut meta);
                } else if file.path.ends_with("package.json") {
                    parse_package_json(&content, &mut meta);
                }
            }
        }
    }

    meta
}

fn parse_cargo_toml(content: &str, meta: &mut ProjectMetadata) {
    let mut in_package = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[package]" {
            in_package = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_package = false;
            continue;
        }
        if in_package {
            if trimmed.starts_with("name =") {
                meta.name = trimmed
                    .split('=')
                    .nth(1)
                    .unwrap_or("")
                    .trim()
                    .trim_matches('"')
                    .to_string();
            } else if trimmed.starts_with("version =") {
                meta.version = Some(
                    trimmed
                        .split('=')
                        .nth(1)
                        .unwrap_or("")
                        .trim()
                        .trim_matches('"')
                        .to_string(),
                );
            } else if trimmed.starts_with("description =") {
                meta.description = Some(
                    trimmed
                        .split('=')
                        .nth(1)
                        .unwrap_or("")
                        .trim()
                        .trim_matches('"')
                        .to_string(),
                );
            }
        }
    }
    meta.tech_stack.push("Rust".to_string());
}

fn parse_package_json(content: &str, meta: &mut ProjectMetadata) {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(content) {
        if let Some(name) = v["name"].as_str() {
            meta.name = name.to_string();
        }
        if let Some(version) = v["version"].as_str() {
            meta.version = Some(version.to_string());
        }
        if let Some(desc) = v["description"].as_str() {
            meta.description = Some(desc.to_string());
        }
    }
    meta.tech_stack.push("JavaScript/TypeScript".to_string());
}

/// Recursively walk a directory tree, collecting file paths and counting extensions.
fn walk_directory(
    _root: &Path,
    dir: &Path,
    languages: &mut HashMap<String, usize>,
    files: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            let name = entry.file_name();
            if !SKIP_DIRS.contains(&name.to_string_lossy().as_ref())
                && !name.to_string_lossy().starts_with('.')
            {
                walk_directory(_root, &path, languages, files)?;
            }
        } else if path.is_file() {
            if let Some(ext) = path.extension() {
                *languages
                    .entry(ext.to_string_lossy().to_lowercase())
                    .or_insert(0) += 1;
            }
            files.push(path);
        }
    }
    Ok(())
}

/// Detect features from subdirectories under source roots.
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
            if dir_name.starts_with('.') || SKIP_DIRS.contains(&dir_name.as_str()) {
                continue;
            }

            let dir_files: Vec<String> = all_files
                .iter()
                .filter(|f| f.starts_with(&path))
                .filter_map(|f| {
                    f.strip_prefix(root)
                        .ok()
                        .map(|p| p.to_string_lossy().to_string())
                })
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
    if ["auth", "login", "session"]
        .iter()
        .any(|k| name.contains(k))
    {
        "security".to_string()
    } else if ["api", "routes", "endpoints"]
        .iter()
        .any(|k| name.contains(k))
    {
        "api".to_string()
    } else if ["db", "storage", "models", "schema"]
        .iter()
        .any(|k| name.contains(k))
    {
        "storage".to_string()
    } else if ["ui", "components", "views", "pages"]
        .iter()
        .any(|k| name.contains(k))
    {
        "ui".to_string()
    } else if ["test", "spec"].iter().any(|k| name.contains(k)) {
        "testing".to_string()
    } else {
        name
    }
}

/// Convert a snake_case or kebab-case string to Title Case.
fn title_case(s: &str) -> String {
    s.split(['_', '-'])
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
    sorted
        .iter()
        .take(5)
        .map(|(ext, count)| format!("{} ({})", ext, count))
        .collect::<Vec<_>>()
        .join(", ")
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
