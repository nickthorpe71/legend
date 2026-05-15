//! Markdown snapshot renderer for any `&Hypergraph`. Pure string
//! assembly — no I/O. Callable from anywhere with a `&Hypergraph`.
//!
//! Used by:
//! - `examples/dump_hypergraph_md.rs` — regenerates `inspect/seed.md`
//!   for browsing the seed substrate on GitHub.
//! - `lib::run` — writes `inspect/last_run.md` at the end of every
//!   tick so the post-tick substrate state is always inspectable.
//!
//! At v0 scale (~72 elements, ~53 relations) markdown is the fastest
//! way to eyeball the graph; mermaid renders inline on GitHub and
//! diffs cleanly between snapshots. Past ~200 nodes mermaid breaks
//! down — at that point swap in the HTML viewer (planned).

use crate::types::{ElementId, Hypergraph, RelationStatus, Term};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as FmtWrite;

/// Render the full markdown report for `hg`. Pure string assembly —
/// no I/O. Callable from anywhere with a `&Hypergraph`.
pub fn render(hg: &Hypergraph) -> String {
    let cat = categorize(hg);
    let mut s = String::new();

    write_header(&mut s, hg, &cat);
    write_anchors(&mut s, hg);
    write_dag_diagram(&mut s, hg, &cat);
    write_regions_table(&mut s, hg, &cat);
    write_attribute_names(&mut s, hg, &cat);
    write_frames(&mut s, hg, &cat);
    write_prototypes(&mut s, hg, &cat);
    write_relations_grouped(&mut s, hg);

    s
}

// ─────────────────────────────────────────────────────────────────────
// Categorization — derive from relations + anchor IDs (not ID ranges).
// Future-proof against minted elements that land in the same ID space
// post-tick.
// ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Category {
    Anchor,
    Class,
    AttributeName,
    Region,
    Frame,
    Prototype,
    /// Minted by extractors during a tick — not present in v0 seed
    /// dumps, but will appear once tick mutations land.
    Minted,
}

struct Categorized {
    by_element: HashMap<ElementId, Category>,
}

fn categorize(hg: &Hypergraph) -> Categorized {
    let mut by_element: HashMap<ElementId, Category> = HashMap::new();

    by_element.insert(hg.void, Category::Anchor);
    by_element.insert(hg.genesis, Category::Anchor);
    by_element.insert(hg.region_class, Category::Class);
    by_element.insert(hg.reference_frame_class, Category::Class);

    for region in hg.region_parents.keys() {
        by_element.insert(*region, Category::Region);
    }

    for protos in hg.region_prototypes.values() {
        for proto in protos {
            by_element.insert(*proto, Category::Prototype);
        }
    }

    let instance_of = match hg.by_name.get("instance_of") {
        Some(v) if !v.is_empty() => v[0],
        _ => panic!("instance_of attribute name not found in by_name"),
    };
    for r in &hg.relations {
        let mut subj = None;
        let mut points_at_frame_class = false;
        for a in &r.attributes {
            if a.name == hg.subject_attr
                && let Term::Element(s) = a.value
            {
                subj = Some(s);
            }
            if a.name == instance_of
                && let Term::Element(t) = a.value
                && t == hg.reference_frame_class
            {
                points_at_frame_class = true;
            }
        }
        if let (Some(s), true) = (subj, points_at_frame_class) {
            by_element.entry(s).or_insert(Category::Frame);
        }
    }

    let mut attr_name_ids: HashSet<ElementId> = HashSet::new();
    for r in &hg.relations {
        for a in &r.attributes {
            attr_name_ids.insert(a.name);
        }
    }
    for e in &hg.elements {
        if e.names.iter().any(|n| attribute_group(n) != "Other") {
            attr_name_ids.insert(e.id);
        }
    }
    for id in &attr_name_ids {
        by_element.entry(*id).or_insert(Category::AttributeName);
    }

    for e in &hg.elements {
        by_element.entry(e.id).or_insert(Category::Minted);
    }

    Categorized { by_element }
}

// ─────────────────────────────────────────────────────────────────────
// Hardcoded attribute-name → group mapping (per seed_pack.yaml § 2
// commentary). Used only for the attribute-name table grouping; if the
// YAML adds names that aren't listed here, they fall through to
// "Other".
// ─────────────────────────────────────────────────────────────────────

fn attribute_group(name: &str) -> &'static str {
    match name {
        "instance_of" | "subclass_of" => "Ontology",
        "target" | "frame" | "valid_from" | "valid_to" | "source" | "supersedes"
        | "derived_from" | "antecedent_of" => "Meta-relation",
        "member_of" | "parent_region" | "lateral_region" | "prototype" => "Region structural",
        "subject" | "actor" | "from" | "to" | "instrument" | "property" | "reason" => {
            "Generic participant"
        }
        "negated" | "uncertain" | "non_actual" | "general" | "intervened" => "Behavioral modal",
        "caused" | "correlated_with" | "enables" | "prevents" => "Causal-relation",
        _ => "Other",
    }
}

/// Display order for attribute groups — mirrors seed_pack.yaml's
/// declaration order.
const ATTRIBUTE_GROUP_ORDER: &[&str] = &[
    "Ontology",
    "Meta-relation",
    "Region structural",
    "Generic participant",
    "Behavioral modal",
    "Causal-relation",
    "Other",
];

// ─────────────────────────────────────────────────────────────────────
// Section writers
// ─────────────────────────────────────────────────────────────────────

fn write_header(s: &mut String, hg: &Hypergraph, cat: &Categorized) {
    let mut counts: HashMap<Category, usize> = HashMap::new();
    for c in cat.by_element.values() {
        *counts.entry(*c).or_default() += 1;
    }

    let _ = writeln!(s, "# Hypergraph Snapshot");
    let _ = writeln!(s, "Tick {}", hg.clock.0);
    let _ = writeln!(s);
    let _ = writeln!(s, "## Summary");
    let _ = writeln!(
        s,
        "- **{}** elements ({} anchor / {} attribute-name / {} region / {} frame / {} class / {} prototype / {} minted)",
        hg.elements.len(),
        counts.get(&Category::Anchor).copied().unwrap_or(0),
        counts.get(&Category::AttributeName).copied().unwrap_or(0),
        counts.get(&Category::Region).copied().unwrap_or(0),
        counts.get(&Category::Frame).copied().unwrap_or(0),
        counts.get(&Category::Class).copied().unwrap_or(0),
        counts.get(&Category::Prototype).copied().unwrap_or(0),
        counts.get(&Category::Minted).copied().unwrap_or(0),
    );
    let _ = writeln!(s, "- **{}** relations", hg.relations.len());

    let mut counts_by_attr: BTreeMap<ElementId, usize> = BTreeMap::new();
    for r in &hg.relations {
        for a in &r.attributes {
            if a.name == hg.subject_attr {
                continue;
            }
            *counts_by_attr.entry(a.name).or_default() += 1;
        }
    }
    let mut sorted: Vec<_> = counts_by_attr.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.0.cmp(&b.0.0)));
    let _ = writeln!(s, "- **By attribute name:**");
    for (id, count) in sorted {
        let name = canonical_name(hg, id);
        let _ = writeln!(s, "  - `{name}` × {count}");
    }
    let _ = writeln!(s);
}

fn write_anchors(s: &mut String, hg: &Hypergraph) {
    let _ = writeln!(s, "## Anchors");
    let _ = writeln!(s);
    let _ = writeln!(s, "| Symbol | ID | Names |");
    let _ = writeln!(s, "|---|---|---|");
    for (label, id) in [
        ("VOID", hg.void),
        ("GENESIS", hg.genesis),
        ("REGION_CLASS", hg.region_class),
        ("REFERENCE_FRAME_CLASS", hg.reference_frame_class),
    ] {
        let names = element_names(hg, id);
        let _ = writeln!(s, "| {label} | {} | {names} |", id.0);
    }
    let _ = writeln!(s);
}

fn write_dag_diagram(s: &mut String, hg: &Hypergraph, cat: &Categorized) {
    let _ = writeln!(s, "## Region DAG");
    let _ = writeln!(s);
    let _ = writeln!(s, "```mermaid");
    let _ = writeln!(s, "graph TD");
    let _ = writeln!(s, "  {}((GENESIS))", mermaid_id(hg, hg.genesis));

    let mut visited: HashSet<ElementId> = HashSet::new();
    let mut stack = vec![hg.genesis];
    while let Some(node) = stack.pop() {
        if !visited.insert(node) {
            continue;
        }
        if let Some(children) = hg.region_children.get(&node) {
            let mut sorted_children: Vec<_> = children.to_vec();
            sorted_children.sort();
            for child in sorted_children {
                let parent_label = mermaid_id(hg, node);
                let child_label = mermaid_id(hg, child);
                let weight = hg
                    .region_parents
                    .get(&child)
                    .and_then(|parents| parents.iter().find(|(p, _)| *p == node))
                    .map(|(_, w)| *w)
                    .unwrap_or(1.0);
                let _ = writeln!(s, "  {parent_label} -->|{weight:.2}| {child_label}",);
                stack.push(child);
            }
        }
        if let Some(protos) = hg.region_prototypes.get(&node) {
            let mut sorted_protos: Vec<_> = protos.to_vec();
            sorted_protos.sort();
            for proto in sorted_protos {
                let parent_label = mermaid_id(hg, node);
                let proto_label = mermaid_id(hg, proto);
                let _ = writeln!(s, "  {parent_label} -.proto.-> {proto_label}",);
            }
        }
    }

    let _ = writeln!(
        s,
        "  classDef region fill:#1a3050,stroke:#4080c0,color:#fff"
    );
    let _ = writeln!(s, "  classDef proto fill:#103020,stroke:#30c060,color:#cfc");
    let _ = writeln!(
        s,
        "  classDef anchor fill:#403010,stroke:#f0a030,color:#fc8"
    );

    let mut region_ids: Vec<_> = cat
        .by_element
        .iter()
        .filter(|(_, c)| **c == Category::Region)
        .map(|(id, _)| *id)
        .collect();
    region_ids.sort();
    let mut proto_ids: Vec<_> = cat
        .by_element
        .iter()
        .filter(|(_, c)| **c == Category::Prototype)
        .map(|(id, _)| *id)
        .collect();
    proto_ids.sort();

    if !region_ids.is_empty() {
        let labels: Vec<_> = region_ids.iter().map(|id| mermaid_id(hg, *id)).collect();
        let _ = writeln!(s, "  class {} region", labels.join(","));
    }
    if !proto_ids.is_empty() {
        let labels: Vec<_> = proto_ids.iter().map(|id| mermaid_id(hg, *id)).collect();
        let _ = writeln!(s, "  class {} proto", labels.join(","));
    }
    let _ = writeln!(s, "  class {} anchor", mermaid_id(hg, hg.genesis));
    let _ = writeln!(s, "```");
    let _ = writeln!(s);
}

fn write_regions_table(s: &mut String, hg: &Hypergraph, cat: &Categorized) {
    let _ = writeln!(s, "## Regions");
    let _ = writeln!(s);
    let _ = writeln!(s, "| Region | ID | Parent(s) | Prototype(s) |");
    let _ = writeln!(s, "|---|---|---|---|");

    let mut regions: Vec<_> = cat
        .by_element
        .iter()
        .filter(|(_, c)| **c == Category::Region)
        .map(|(id, _)| *id)
        .collect();
    regions.sort_by_key(|id| id.0);

    for id in regions {
        let name = canonical_name(hg, id);
        let parents = hg
            .region_parents
            .get(&id)
            .map(|ps| {
                ps.iter()
                    .map(|(p, w)| format!("{} ({:.2})", canonical_name(hg, *p), w))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_else(|| "—".into());
        let protos = hg
            .region_prototypes
            .get(&id)
            .map(|ps| {
                ps.iter()
                    .map(|p| format!("{} ({})", canonical_name(hg, *p), p.0))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_else(|| "—".into());
        let _ = writeln!(s, "| `{name}` | {} | {parents} | {protos} |", id.0);
    }
    let _ = writeln!(s);
}

fn write_attribute_names(s: &mut String, hg: &Hypergraph, cat: &Categorized) {
    let _ = writeln!(s, "## Attribute Names");
    let _ = writeln!(s);

    let mut grouped: BTreeMap<&str, Vec<ElementId>> = BTreeMap::new();
    for (id, c) in &cat.by_element {
        if *c != Category::AttributeName {
            continue;
        }
        let name = canonical_name(hg, *id);
        let group = attribute_group(&name);
        grouped.entry(group).or_default().push(*id);
    }
    for ids in grouped.values_mut() {
        ids.sort_by_key(|id| id.0);
    }

    for group in ATTRIBUTE_GROUP_ORDER {
        let Some(ids) = grouped.get(group) else {
            continue;
        };
        let _ = writeln!(s, "### {group} ({})", ids.len());
        let _ = writeln!(s);
        let _ = writeln!(s, "| Name | ID |");
        let _ = writeln!(s, "|---|---|");
        for id in ids {
            let _ = writeln!(s, "| `{}` | {} |", canonical_name(hg, *id), id.0);
        }
        let _ = writeln!(s);
    }
}

fn write_frames(s: &mut String, hg: &Hypergraph, cat: &Categorized) {
    let _ = writeln!(s, "## Reference Frames");
    let _ = writeln!(s);
    let _ = writeln!(s, "| Frame | ID | Names |");
    let _ = writeln!(s, "|---|---|---|");

    let mut frames: Vec<_> = cat
        .by_element
        .iter()
        .filter(|(_, c)| **c == Category::Frame)
        .map(|(id, _)| *id)
        .collect();
    frames.sort_by_key(|id| id.0);

    for id in frames {
        let names = element_names(hg, id);
        let canonical = canonical_name(hg, id);
        let _ = writeln!(s, "| `{canonical}` | {} | {names} |", id.0);
    }
    let _ = writeln!(s);
}

fn write_prototypes(s: &mut String, hg: &Hypergraph, cat: &Categorized) {
    let _ = writeln!(s, "## Prototypes");
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "| Prototype | ID | Owning region | Embedding magnitude |"
    );
    let _ = writeln!(s, "|---|---|---|---|");

    let mut proto_to_region: HashMap<ElementId, ElementId> = HashMap::new();
    for (region, protos) in &hg.region_prototypes {
        for p in protos {
            proto_to_region.insert(*p, *region);
        }
    }

    let mut protos: Vec<_> = cat
        .by_element
        .iter()
        .filter(|(_, c)| **c == Category::Prototype)
        .map(|(id, _)| *id)
        .collect();
    protos.sort_by_key(|id| id.0);

    for id in protos {
        let element = &hg.elements[id.0 as usize];
        let mag = (element.embedding.iter().map(|v| v * v).sum::<f32>()).sqrt();
        let owner = proto_to_region
            .get(&id)
            .map(|r| canonical_name(hg, *r))
            .unwrap_or_else(|| "—".into());
        let _ = writeln!(
            s,
            "| `{}` | {} | `{owner}` | {mag:.4} |",
            canonical_name(hg, id),
            id.0,
        );
    }
    let _ = writeln!(s);
}

fn write_relations_grouped(s: &mut String, hg: &Hypergraph) {
    let _ = writeln!(s, "## Relations");
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "Grouped by non-subject attribute name. Each entry is `subject → object (status, conf)`."
    );
    let _ = writeln!(s);

    let mut grouped: BTreeMap<ElementId, Vec<(ElementId, ElementId, RelationStatus, f32)>> =
        BTreeMap::new();

    for r in &hg.relations {
        let mut subject: Option<ElementId> = None;
        for a in &r.attributes {
            if a.name == hg.subject_attr
                && let Term::Element(s) = a.value
            {
                subject = Some(s);
            }
        }
        let Some(subj) = subject else {
            continue;
        };
        for a in &r.attributes {
            if a.name == hg.subject_attr {
                continue;
            }
            let target = match a.value {
                Term::Element(e) => e,
                Term::Relation(_) => continue,
            };
            grouped
                .entry(a.name)
                .or_default()
                .push((subj, target, r.status, r.stats.confidence));
        }
    }

    let mut ordered: Vec<_> = grouped.into_iter().collect();
    ordered.sort_by_key(|a| canonical_name(hg, a.0));

    for (attr_id, mut entries) in ordered {
        let attr_name = canonical_name(hg, attr_id);
        let _ = writeln!(s, "### `{attr_name}` ({})", entries.len());
        let _ = writeln!(s);
        entries.sort_by_key(|(subj, obj, _, _)| (subj.0, obj.0));
        for (subj, obj, status, conf) in entries {
            let subj_name = canonical_name(hg, subj);
            let obj_name = canonical_name(hg, obj);
            let _ = writeln!(
                s,
                "- `{subj_name}` → `{obj_name}`  ({}, conf={:.2})",
                status_str(status),
                conf,
            );
        }
        let _ = writeln!(s);
    }
}

// ─────────────────────────────────────────────────────────────────────
// Small helpers
// ─────────────────────────────────────────────────────────────────────

fn canonical_name(hg: &Hypergraph, id: ElementId) -> String {
    hg.elements
        .get(id.0 as usize)
        .and_then(|e| e.names.first())
        .cloned()
        .unwrap_or_else(|| format!("?{}?", id.0))
}

fn element_names(hg: &Hypergraph, id: ElementId) -> String {
    hg.elements
        .get(id.0 as usize)
        .map(|e| {
            e.names
                .iter()
                .map(|n| format!("`{n}`"))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_else(|| "—".into())
}

/// Sanitize an element name into a mermaid-safe identifier. Mermaid
/// node IDs can't contain dots or slashes; replace with underscores.
/// Prefix with element ID to disambiguate elements that share a name.
fn mermaid_id(hg: &Hypergraph, id: ElementId) -> String {
    let raw = canonical_name(hg, id);
    let safe: String = raw
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("e{}_{safe}", id.0)
}

fn status_str(s: RelationStatus) -> &'static str {
    match s {
        RelationStatus::Asserted => "asserted",
        RelationStatus::Entailed => "entailed",
        RelationStatus::Defeasible => "defeasible",
        RelationStatus::Superseded => "superseded",
        RelationStatus::Retracted => "retracted",
    }
}
