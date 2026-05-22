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

use crate::types::{
    ConsciousAttentionFrame, ElementId, Hypergraph, RelationId, RelationStatus, ResolvedRelation,
    ResolvedTerm, Term,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as FmtWrite;

/// Flatten a `ConsciousAttentionFrame` into a single pipe-joined
/// string of element names — the same view the bench harness scores
/// against. Walks `active_frame`, `focused_relations`, `current_state`,
/// `history`, and `supporting_claims` in that order, resolving each
/// relation's attribute names and values through the substrate to
/// human-readable text. Relation-valued attributes degrade to `R<id>`
/// rather than recursing.
///
/// Relations that appear in multiple buckets (e.g. a `current_state`
/// entry that is also in `focused_relations`) are emitted once,
/// first-bucket-wins. The ordering above is the priority — appearing
/// in `focused_relations` keeps the entry there, and the relation is
/// skipped when re-encountered later.
///
/// Useful when you want to see exactly what the substrate surfaces
/// for a query — the same string a downstream LLM verbalizer (or
/// SubEM bench scorer) would consume. For a human-readable variant
/// with section headers and per-relation grouping, see
/// [`render_flat_frame_annotated`].
pub fn flatten_attention_frame(frame: &ConsciousAttentionFrame) -> String {
    let mut parts: Vec<String> = vec![frame.input_echo.clone()];
    if let Some(e) = &frame.active_frame {
        parts.push(e.name.clone());
    }
    let mut seen: HashSet<RelationId> = HashSet::new();
    let mut push_rel = |r: &ResolvedRelation, parts: &mut Vec<String>| {
        if !seen.insert(r.id) {
            return;
        }
        for a in &r.attributes {
            parts.push(a.name.name.clone());
            match &a.value {
                ResolvedTerm::Element(e) => parts.push(e.name.clone()),
                ResolvedTerm::Relation(rid) => parts.push(format!("R{}", rid.0)),
            }
        }
    };
    for ra in &frame.focused_relations {
        push_rel(&ra.relation, &mut parts);
    }
    for r in &frame.current_state {
        push_rel(r, &mut parts);
    }
    for r in &frame.history {
        push_rel(r, &mut parts);
    }
    for r in &frame.supporting_claims {
        push_rel(r, &mut parts);
    }
    parts.join(" | ")
}

/// Human-readable variant of [`flatten_attention_frame`]. Same content,
/// same walk order, same first-bucket-wins dedupe — but with section
/// headers and per-relation grouping so a human can see what's in
/// each part of the focused subgraph. Index ranges are kept aligned
/// with the raw flat string so you can correlate "this line is at
/// positions \[N..M\]".
///
/// Output shape:
///
/// ```text
/// flat frame (NNN chars, K tokens):
/// ─ input
///   [0] "<query text>"
/// ─ active_frame
///   [1] <element name>
/// ─ focused_relations  (N entries)
///   R621  subject=March 15th  instance_of=month                  [2..5]
///   R632  subject=I  to make=sure I wait outside                 [6..9]
/// ─ current_state  (N entries, M already shown above)
///   …
/// ─ history       (empty)
/// ─ supporting_claims  (empty)
/// ```
pub fn render_flat_frame_annotated(frame: &ConsciousAttentionFrame) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    let mut idx: usize = 0;

    // First compute headline length+token counts from the raw flat
    // string so we don't lie about size after grouping changes
    // presentation.
    let raw = flatten_attention_frame(frame);
    let total_chars = raw.len();
    let total_tokens = raw.split(" | ").count();

    let _ = writeln!(
        out,
        "flat frame ({total_chars} chars, {total_tokens} tokens):"
    );

    // ── input ──────────────────────────────────────────────────
    let _ = writeln!(out, "─ input");
    let _ = writeln!(out, "  [{idx}] {:?}", frame.input_echo);
    idx += 1;

    // ── active_frame ───────────────────────────────────────────
    if let Some(e) = &frame.active_frame {
        let _ = writeln!(out, "─ active_frame");
        let _ = writeln!(out, "  [{idx}] {}", e.name);
        idx += 1;
    } else {
        let _ = writeln!(out, "─ active_frame  (none)");
    }

    // For each bucket: walk relations with first-bucket-wins
    // dedupe. Emit (rid, pairs, range) tuples to feed the bucket-
    // section formatter so dedupe and offset-tracking stay in
    // one place.
    // (relation, key=value pairs, start token idx, end token idx)
    type BucketRow = (RelationId, Vec<(String, String)>, usize, usize);

    let mut seen: HashSet<RelationId> = HashSet::new();
    let mut emit_bucket =
        |label: &str, rels: &[ResolvedRelation], idx: &mut usize, out: &mut String| {
            let mut shown: Vec<BucketRow> = Vec::new();
            let mut dup_count = 0usize;
            for r in rels {
                if !seen.insert(r.id) {
                    dup_count += 1;
                    continue;
                }
                let mut pairs = Vec::with_capacity(r.attributes.len());
                let start = *idx;
                for a in &r.attributes {
                    let value = match &a.value {
                        ResolvedTerm::Element(e) => e.name.clone(),
                        ResolvedTerm::Relation(rid) => format!("R{}", rid.0),
                    };
                    pairs.push((a.name.name.clone(), value));
                    *idx += 2;
                }
                let end = idx.saturating_sub(1);
                shown.push((r.id, pairs, start, end));
            }

            // Header line.
            let _ = match (shown.len(), dup_count) {
                (0, 0) => writeln!(out, "─ {label}  (empty)"),
                (0, n) => writeln!(out, "─ {label}  ({n} already shown above)"),
                (n, 0) => writeln!(out, "─ {label}  ({n})"),
                (n, d) => writeln!(out, "─ {label}  ({n} new, {d} already shown above)"),
            };
            for (rid, pairs, start, end) in shown {
                let pair_str = pairs
                    .iter()
                    .map(|(n, v)| format!("{n}={v}"))
                    .collect::<Vec<_>>()
                    .join("  ");
                let _ = writeln!(out, "  R{:<5} {pair_str}    [{start}..{end}]", rid.0);
            }
        };

    let focused: Vec<ResolvedRelation> = frame
        .focused_relations
        .iter()
        .map(|ra| ra.relation.clone())
        .collect();
    emit_bucket("focused_relations", &focused, &mut idx, &mut out);
    emit_bucket("current_state", &frame.current_state, &mut idx, &mut out);
    emit_bucket("history", &frame.history, &mut idx, &mut out);
    emit_bucket(
        "supporting_claims",
        &frame.supporting_claims,
        &mut idx,
        &mut out,
    );

    out
}

/// Compact header for a tick — input echo, intent vector, bucket
/// counts, uncertainty signals, next actions. Frame-only.
pub fn render_tick_summary(frame: &ConsciousAttentionFrame) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "tick {} \"{}\"",
        frame.tick.0,
        truncate(&frame.input_echo, 60),
    );
    let _ = writeln!(
        out,
        "  intent  conv={:.2}  pe={:.2}  arous={:.2}  curio={:.2}",
        frame.intent.conviction,
        frame.intent.prediction_error,
        frame.intent.arousal,
        frame.intent.curiosity,
    );
    let _ = writeln!(
        out,
        "  frame  active_regions={}  focused={}  supporting={}  history={}  current_state={}  uncertainty={:?}",
        frame.active_regions.len(),
        frame.focused_relations.len(),
        frame.supporting_claims.len(),
        frame.history.len(),
        frame.current_state.len(),
        frame.uncertainty,
    );
    if !frame.next_actions.is_empty() {
        let _ = writeln!(out, "  next_actions:");
        for a in &frame.next_actions {
            match a {
                crate::types::AttentionAction::EnqueueReplay { kind } => {
                    let _ = writeln!(out, "    EnqueueReplay {{ kind: {kind:?} }}");
                }
                crate::types::AttentionAction::FollowUpQuery(t) => {
                    let _ = writeln!(out, "    FollowUpQuery({t:?})");
                }
            }
        }
    }
    out
}

/// Per-relation render of every bucket the frame populated.
/// Frame-only — names + status + confidence live on each
/// `ResolvedRelation`.
pub fn render_frame_contents(frame: &ConsciousAttentionFrame) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    if !frame.focused_relations.is_empty() {
        let _ = writeln!(out, "  focused:");
        for ra in &frame.focused_relations {
            let _ = writeln!(out, "{}", render_relation_line(&ra.relation, Some(ra.activation)));
        }
    }
    if !frame.current_state.is_empty() {
        let _ = writeln!(out, "  current_state:");
        for r in &frame.current_state {
            let _ = writeln!(out, "{}", render_relation_line(r, None));
        }
    }
    if !frame.supporting_claims.is_empty() {
        let _ = writeln!(out, "  supporting:");
        for r in &frame.supporting_claims {
            let _ = writeln!(out, "{}", render_relation_line(r, None));
        }
    }
    if !frame.history.is_empty() {
        let _ = writeln!(out, "  history:");
        for r in &frame.history {
            let _ = writeln!(out, "{}", render_relation_line(r, None));
        }
    }
    out
}

/// One-line render of a resolved relation: subject → attribute → object
/// plus status and (optionally) activation score. The attribute named
/// `"subject"` or `"target"` is treated as the subject slot.
fn render_relation_line(r: &ResolvedRelation, activation: Option<f32>) -> String {
    let is_subject = |attr_name: &str| attr_name == "subject" || attr_name == "target";
    let mut subject: Option<String> = None;
    let mut other: Option<(String, String)> = None;
    for attr in &r.attributes {
        let value = match &attr.value {
            ResolvedTerm::Element(e) => e.name.clone(),
            ResolvedTerm::Relation(rid) => format!("→R{}", rid.0),
        };
        if is_subject(&attr.name.name) {
            subject = Some(value);
        } else if other.is_none() {
            other = Some((attr.name.name.clone(), value));
        }
    }
    let subject = subject.unwrap_or_else(|| "?".to_string());
    let (attr_name, attr_value) = other.unwrap_or_else(|| ("?".to_string(), "?".to_string()));
    let act = activation
        .map(|a| format!(" act={a:.3}"))
        .unwrap_or_default();
    format!(
        "    R{:<5} [{:?}] conf={:.2}{act}  {subject} → {attr_name} → {attr_value}",
        r.id.0, r.status, r.confidence,
    )
}

/// The full tick output a consumer sees: header summary, structured
/// per-bucket render, then the bench-scored flat-frame view. One
/// pure function over the frame; no `Hypergraph` parameter.
pub fn render_frame(frame: &ConsciousAttentionFrame) -> String {
    let mut out = String::new();
    out.push_str(&render_tick_summary(frame));
    out.push_str(&render_frame_contents(frame));
    out.push('\n');
    out.push_str(&render_flat_frame_annotated(frame));
    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

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
