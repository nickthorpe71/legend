//! Build the day-zero Hypergraph from `seed_pack.yaml` and serialize it
//! to `src/seed/graph.bin` for the runtime to `include_bytes!`.
//!
//! Run: `cargo run --release --example gen_seed_graph`
//!
//! Why this exists: the v0 substrate boots with ~55 seeded elements
//! (anchors, attribute names, regions, frames) plus eagerly-minted
//! `REGION_CLASS` / `REFERENCE_FRAME_CLASS` plus one prototype Element
//! per region (embedding the YAML's `descriptor` text via MiniLM).
//! Step 4 region routing has nothing to descend into until this graph
//! ships, so this generator is the prerequisite for any meaningful
//! routing work.
//!
//! Pattern matches `gen_intent_classifiers.rs`: build-time tool that
//! reads YAML + runs the embedder + writes a tightly-packed
//! little-endian `.bin` file. The runtime crate carries no `serde`
//! dependency; the `.bin` is parsed by hand in `src/seed.rs`.
//!
//! ── Element minting order (assigns ElementId(0..N) sequentially) ─────
//!
//! 1. VOID at ElementId(0), GENESIS at ElementId(1) — anchor IDs are
//!    fixed by the `Hypergraph` struct's contract.
//! 2. The 30 seeded attribute names in YAML declaration order.
//! 3. The 15 seeded regions.
//! 4. The 8 seeded reference frames.
//! 5. REGION_CLASS, REFERENCE_FRAME_CLASS — the YAML treats these as
//!    lazy (minted on first pin); we mint them eagerly so the runtime
//!    has stable IDs to cache.
//! 6. One prototype Element per region. Name: `<region>_proto_0`.
//!    Embedding: `embed_text(region.descriptor)`.
//!
//! Total: 2 + 30 + 15 + 8 + 2 + 15 = 72 elements.
//!
//! ── Relation minting order ───────────────────────────────────────────
//!
//! 1. 15 region_class_pins — `[REGION_X, instance_of, REGION_CLASS]`.
//! 2. 8 reference_frame_class_pins.
//! 3. 15 region_parent_pins — carry confidence weights from the YAML.
//! 4. 15 prototype-attachment pins — synthesized from the region/proto
//!    pairs minted in step 6 above.
//!
//! Total: 53 relations.
//!
//! ── Binary format (consumed by src/seed.rs) ──────────────────────────
//!
//! All multi-byte integers little-endian. Strings are UTF-8 with a
//! `u32` byte-length prefix.
//!
//! HEADER (10 × u32 = 40 bytes):
//!   format_version            u32  (currently 1)
//!   element_count             u32
//!   relation_count            u32
//!   void_id                   u32
//!   genesis_id                u32
//!   region_class_id           u32
//!   reference_frame_class_id  u32
//!   subject_attr_id           u32
//!   parent_region_attr_id     u32
//!   prototype_attr_id         u32
//!
//! ELEMENTS (element_count × variable):
//!   id            u32
//!   name_count    u32
//!   for each name: u32 byte_len, byte_len UTF-8 bytes
//!   embedding     EMBEDDING_DIM × f32  (no length prefix; constant)
//!   stats         (see MEMORY_STATS layout below)
//!   created_at    u64  (Tick)
//!
//! RELATIONS (relation_count × variable):
//!   id              u32
//!   attribute_count u32
//!   for each attribute:
//!     name_id   u32
//!     term_kind u8   (0 = Element, 1 = Relation)
//!     value     u32
//!   status          u8  (0=Asserted, 1=Entailed, 2=Defeasible,
//!                        3=Superseded, 4=Retracted)
//!   stats           (see MEMORY_STATS layout below)
//!   priority        i8
//!   created_at      u64
//!
//! MEMORY_STATS (49 bytes):
//!   activation           f32
//!   confidence           f32
//!   plasticity           f32
//!   salience             f32
//!   access_count         u32
//!   focus_success_count  u32
//!   support_count        u32
//!   support_diversity    u32
//!   prediction_error     f32
//!   last_seen            u64  (Tick)
//!   last_accessed_some   u8   (0 or 1)
//!   last_accessed_val    u64  (always written; ignored when flag = 0)

#[path = "shared/mod.rs"]
mod shared;

use legend::embed::{EMBEDDING_DIM, embed_text};
use legend::types::{
    Attribute, Element, ElementId, MemoryStats, Relation, RelationId, RelationStatus, Term, Tick,
};
use shared::{RawElement, SeedPack, load_seed_pack};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

const FORMAT_VERSION: u32 = 1;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pack = load_seed_pack(Path::new("seed_pack.yaml"))?;

    let mut elements: Vec<Element> = Vec::new();
    let mut relations: Vec<Relation> = Vec::new();
    let mut symbol_to_id: HashMap<String, ElementId> = HashMap::new();
    // Maps the surface form of a seeded attribute name (e.g. "instance_of",
    // "parent_region") to its element id. Used to resolve YAML relation
    // tuples whose middle slot is a surface-form token rather than a
    // symbolic ID.
    let mut name_to_attr_id: HashMap<String, ElementId> = HashMap::new();

    // ── 1. Anchors ────────────────────────────────────────────────────
    // The YAML lists GENESIS first; we override that order so VOID lands
    // at ElementId(0) and GENESIS at ElementId(1), matching the contract
    // documented on `Hypergraph::void` and `Hypergraph::genesis`.
    let void_raw = find_anchor(&pack, "VOID");
    let void_id = mint_element(
        &mut elements,
        &mut symbol_to_id,
        "VOID",
        void_raw.names.clone(),
        embed_text(&void_raw.names[0]),
    );
    assert_eq!(void_id, ElementId(0), "VOID must land at ElementId(0)");

    let genesis_raw = find_anchor(&pack, "GENESIS");
    let genesis_id = mint_element(
        &mut elements,
        &mut symbol_to_id,
        "GENESIS",
        genesis_raw.names.clone(),
        embed_text(&genesis_raw.names[0]),
    );
    assert_eq!(
        genesis_id,
        ElementId(1),
        "GENESIS must land at ElementId(1)"
    );

    // ── 2. Seeded attribute names ─────────────────────────────────────
    for raw in &pack.seeded_attribute_names {
        let id = mint_element(
            &mut elements,
            &mut symbol_to_id,
            &raw.element_id,
            raw.names.clone(),
            embed_text(&raw.names[0]),
        );
        for name in &raw.names {
            name_to_attr_id.insert(name.clone(), id);
        }
    }

    // ── 3. Regions ────────────────────────────────────────────────────
    for raw in &pack.regions {
        mint_element(
            &mut elements,
            &mut symbol_to_id,
            &raw.element_id,
            raw.names.clone(),
            embed_text(&raw.names[0]),
        );
    }

    // ── 4. Reference frames ───────────────────────────────────────────
    for raw in &pack.reference_frames {
        mint_element(
            &mut elements,
            &mut symbol_to_id,
            &raw.element_id,
            raw.names.clone(),
            embed_text(&raw.names[0]),
        );
    }

    // ── 5. Eager class mints ──────────────────────────────────────────
    let region_class_id = mint_element(
        &mut elements,
        &mut symbol_to_id,
        "REGION_CLASS",
        vec!["region_class".into()],
        embed_text("region_class"),
    );
    let reference_frame_class_id = mint_element(
        &mut elements,
        &mut symbol_to_id,
        "REFERENCE_FRAME_CLASS",
        vec!["reference_frame_class".into()],
        embed_text("reference_frame_class"),
    );

    // ── 6. Prototype elements (one per region, embedding from descriptor) ──
    let mut region_to_proto: HashMap<ElementId, ElementId> = HashMap::new();
    for raw in &pack.regions {
        let region_id = symbol_to_id[&raw.element_id];
        let proto_name = format!("{}_proto_0", raw.names[0]);
        let proto_symbol = format!("{}_PROTO_0", raw.element_id);
        let proto_id = mint_element(
            &mut elements,
            &mut symbol_to_id,
            &proto_symbol,
            vec![proto_name],
            embed_text(&raw.descriptor),
        );
        region_to_proto.insert(region_id, proto_id);
    }

    // Cached attribute IDs the runtime will read from the bin header.
    let subject_attr_id = *name_to_attr_id
        .get("subject")
        .expect("seed pack missing the 'subject' attribute name");
    let parent_region_attr_id = *name_to_attr_id
        .get("parent_region")
        .expect("seed pack missing the 'parent_region' attribute name");
    let prototype_attr_id = *name_to_attr_id
        .get("prototype")
        .expect("seed pack missing the 'prototype' attribute name");

    // ── 7. Seeded relations from the YAML ─────────────────────────────
    for entry in &pack.seeded_relations.region_class_pins.relations {
        let (a, attr, b, w) = unpack_tuple(entry);
        mint_relation(
            &mut relations,
            subject_attr_id,
            symbol_to_id[&a],
            name_to_attr_id[&attr],
            symbol_to_id[&b],
            w.unwrap_or(1.0),
        );
    }
    for entry in &pack.seeded_relations.reference_frame_class_pins.relations {
        let (a, attr, b, w) = unpack_tuple(entry);
        mint_relation(
            &mut relations,
            subject_attr_id,
            symbol_to_id[&a],
            name_to_attr_id[&attr],
            symbol_to_id[&b],
            w.unwrap_or(1.0),
        );
    }
    for entry in &pack.seeded_relations.region_parent_pins.relations {
        let (a, attr, b, w) = unpack_tuple(entry);
        mint_relation(
            &mut relations,
            subject_attr_id,
            symbol_to_id[&a],
            name_to_attr_id[&attr],
            symbol_to_id[&b],
            w.unwrap_or(1.0),
        );
    }

    // ── 8. Prototype-attachment relations ─────────────────────────────
    for raw in &pack.regions {
        let region_id = symbol_to_id[&raw.element_id];
        let proto_id = region_to_proto[&region_id];
        mint_relation(
            &mut relations,
            subject_attr_id,
            region_id,
            prototype_attr_id,
            proto_id,
            1.0,
        );
    }

    // Sanity checks — fail loudly if the YAML's shape drifted.
    assert_eq!(elements.len(), 72, "expected 72 elements");
    assert_eq!(relations.len(), 53, "expected 53 relations");

    // ── 9. Serialize ──────────────────────────────────────────────────
    let mut buf: Vec<u8> = Vec::new();
    write_u32(&mut buf, FORMAT_VERSION);
    write_u32(&mut buf, elements.len() as u32);
    write_u32(&mut buf, relations.len() as u32);
    write_u32(&mut buf, void_id.0);
    write_u32(&mut buf, genesis_id.0);
    write_u32(&mut buf, region_class_id.0);
    write_u32(&mut buf, reference_frame_class_id.0);
    write_u32(&mut buf, subject_attr_id.0);
    write_u32(&mut buf, parent_region_attr_id.0);
    write_u32(&mut buf, prototype_attr_id.0);

    for e in &elements {
        write_element(&mut buf, e);
    }
    for r in &relations {
        write_relation(&mut buf, r);
    }

    // ── 10. Write to disk ─────────────────────────────────────────────
    fs::create_dir_all("src/seed")?;
    let path = Path::new("src/seed/graph.bin");
    let mut file = BufWriter::new(File::create(path)?);
    file.write_all(&buf)?;
    file.flush()?;

    println!(
        "wrote {} elements, {} relations to {} ({} bytes)",
        elements.len(),
        relations.len(),
        path.display(),
        buf.len(),
    );
    println!(
        "  void={} genesis={} region_class={} reference_frame_class={}",
        void_id.0, genesis_id.0, region_class_id.0, reference_frame_class_id.0,
    );
    println!(
        "  subject_attr={} parent_region_attr={} prototype_attr={}",
        subject_attr_id.0, parent_region_attr_id.0, prototype_attr_id.0,
    );

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────
// Helpers — minting
// ─────────────────────────────────────────────────────────────────────

fn find_anchor<'a>(pack: &'a SeedPack, symbol: &str) -> &'a RawElement {
    pack.anchors
        .iter()
        .find(|a| a.element_id == symbol)
        .unwrap_or_else(|| panic!("seed pack missing anchor: {symbol}"))
}

fn mint_element(
    elements: &mut Vec<Element>,
    symbol_to_id: &mut HashMap<String, ElementId>,
    symbol: &str,
    names: Vec<String>,
    embedding: Vec<f32>,
) -> ElementId {
    debug_assert_eq!(
        embedding.len(),
        EMBEDDING_DIM,
        "embedding length mismatch on mint of {symbol}"
    );
    let id = ElementId(elements.len() as u32);
    elements.push(Element {
        id,
        names,
        stats: MemoryStats::default(),
        created_at: Tick(0),
        embedding,
    });
    let prior = symbol_to_id.insert(symbol.to_string(), id);
    assert!(prior.is_none(), "duplicate symbol in seed pack: {symbol}");
    id
}

fn mint_relation(
    relations: &mut Vec<Relation>,
    subject_attr_id: ElementId,
    subject: ElementId,
    attr: ElementId,
    object: ElementId,
    confidence: f32,
) -> RelationId {
    let id = RelationId(relations.len() as u32);
    let stats = MemoryStats {
        confidence,
        ..MemoryStats::default()
    };
    relations.push(Relation {
        id,
        attributes: vec![
            Attribute {
                name: subject_attr_id,
                value: Term::Element(subject),
            },
            Attribute {
                name: attr,
                value: Term::Element(object),
            },
        ],
        status: RelationStatus::Asserted,
        stats,
        priority: 0,
        created_at: Tick(0),
    });
    id
}

/// Unpack a YAML relation entry. Layout is `[A, attr, B]` (3-element)
/// or `[A, attr, B, w]` (4-element with confidence weight). Panics on
/// any other shape — the YAML is schema-stable.
fn unpack_tuple(entry: &[serde_yaml::Value]) -> (String, String, String, Option<f32>) {
    assert!(
        entry.len() == 3 || entry.len() == 4,
        "seeded relation tuple must have 3 or 4 elements, got {}",
        entry.len()
    );
    let a = entry[0]
        .as_str()
        .expect("relation tuple element 0 must be a string")
        .to_string();
    let attr = entry[1]
        .as_str()
        .expect("relation tuple element 1 must be a string")
        .to_string();
    let b = entry[2]
        .as_str()
        .expect("relation tuple element 2 must be a string")
        .to_string();
    let w = entry.get(3).map(|v| {
        v.as_f64()
            .expect("relation tuple element 3 must be a float") as f32
    });
    (a, attr, b, w)
}

// ─────────────────────────────────────────────────────────────────────
// Helpers — binary serialization
// ─────────────────────────────────────────────────────────────────────

fn write_u8(buf: &mut Vec<u8>, x: u8) {
    buf.push(x);
}

fn write_i8(buf: &mut Vec<u8>, x: i8) {
    buf.push(x as u8);
}

fn write_u32(buf: &mut Vec<u8>, x: u32) {
    buf.extend_from_slice(&x.to_le_bytes());
}

fn write_u64(buf: &mut Vec<u8>, x: u64) {
    buf.extend_from_slice(&x.to_le_bytes());
}

fn write_f32(buf: &mut Vec<u8>, x: f32) {
    buf.extend_from_slice(&x.to_le_bytes());
}

fn write_string(buf: &mut Vec<u8>, s: &str) {
    write_u32(buf, s.len() as u32);
    buf.extend_from_slice(s.as_bytes());
}

fn write_stats(buf: &mut Vec<u8>, s: &MemoryStats) {
    write_f32(buf, s.activation);
    write_f32(buf, s.confidence);
    write_f32(buf, s.plasticity);
    write_f32(buf, s.salience);
    write_u32(buf, s.access_count);
    write_u32(buf, s.focus_success_count);
    write_u32(buf, s.support_count);
    write_u32(buf, s.support_diversity);
    write_f32(buf, s.prediction_error);
    write_u64(buf, s.last_seen.0);
    write_u8(buf, if s.last_accessed.is_some() { 1 } else { 0 });
    write_u64(buf, s.last_accessed.map_or(0, |t| t.0));
}

fn write_element(buf: &mut Vec<u8>, e: &Element) {
    write_u32(buf, e.id.0);
    write_u32(buf, e.names.len() as u32);
    for n in &e.names {
        write_string(buf, n);
    }
    debug_assert_eq!(e.embedding.len(), EMBEDDING_DIM);
    for v in &e.embedding {
        write_f32(buf, *v);
    }
    write_stats(buf, &e.stats);
    write_u64(buf, e.created_at.0);
}

fn write_relation(buf: &mut Vec<u8>, r: &Relation) {
    write_u32(buf, r.id.0);
    write_u32(buf, r.attributes.len() as u32);
    for a in &r.attributes {
        write_u32(buf, a.name.0);
        match a.value {
            Term::Element(e) => {
                write_u8(buf, 0);
                write_u32(buf, e.0);
            }
            Term::Relation(rid) => {
                write_u8(buf, 1);
                write_u32(buf, rid.0);
            }
        }
    }
    let status_byte: u8 = match r.status {
        RelationStatus::Asserted => 0,
        RelationStatus::Entailed => 1,
        RelationStatus::Defeasible => 2,
        RelationStatus::Superseded => 3,
        RelationStatus::Retracted => 4,
    };
    write_u8(buf, status_byte);
    write_stats(buf, &r.stats);
    write_i8(buf, r.priority);
    write_u64(buf, r.created_at.0);
}
