//! Quantity & Arithmetic kernel.
//!
//! The `UnitIndex` — a surface-form → canonical-unit lookup used to
//! parse, compare, and convert quantities. The seeded `UNIT_*` / `DIM_*`
//! Elements (in `seed_pack.yaml`) are the *graph registry* of which units
//! exist; the *conversion factors* live here in [`UNIT_TABLE`], keyed by
//! the unit's real surface form. Keeping factors in code (never in
//! Element `names`) is what keeps `by_name` clean — see
//! `types::Hypergraph::by_name`. A test below asserts the two agree.

use std::collections::HashMap;

/// The physical (or notional) dimension a unit measures. Two quantities
/// are only comparable within a dimension — and, for currency, within a
/// single code (no FX in v0), which is enforced via [`UnitEntry::base_unit`]
/// rather than this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dimension {
    Currency,
    Mass,
    Length,
    Duration,
    Data,
}

/// One unit's canonical description. `to_base` converts a magnitude in
/// this unit to `base_unit` (`magnitude_in_base = magnitude * to_base`);
/// two units are comparable iff their `base_unit` strings match — which
/// holds across a dimension (all mass → `"kg"`) but deliberately *fails*
/// across currency codes (`"usd"` ≠ `"eur"`), so the kernel refuses
/// cross-currency comparison.
#[derive(Debug, Clone, Copy)]
pub struct UnitEntry {
    pub dimension: Dimension,
    pub base_unit: &'static str,
    pub to_base: f64,
}

/// Surface form (lowercased) → canonical unit. Built by
/// [`build_unit_index`] and stored `#[serde(skip)]` on the `Hypergraph`;
/// rebuilt on every load like the other derived indices.
pub type UnitIndex = HashMap<String, UnitEntry>;

/// The authoritative unit registry: `(surface forms, dimension,
/// base_unit, factor-to-base)`. Surface forms MUST match the `names` of
/// the corresponding `UNIT_*` Element seeded in `seed_pack.yaml` — drift
/// is caught by `every_indexed_unit_is_a_seeded_graph_citizen` below.
/// Covers the five multiplicative-factor dimensions (currency, mass,
/// length, duration, data); temperature (affine — needs an offset, not a
/// factor), area, and power are not handled.
const UNIT_TABLE: &[(&[&str], Dimension, &str, f64)] = &[
    // ── currency — each code is its own base; no FX, so cross-currency
    //    comparison is refused by the differing base_unit.
    (&["usd", "$", "dollars", "dollar"], Dimension::Currency, "usd", 1.0),
    (&["eur", "€", "euros", "euro"], Dimension::Currency, "eur", 1.0),
    (&["gbp", "£", "pounds sterling"], Dimension::Currency, "gbp", 1.0),
    // ── mass — base kilogram
    (&["kg", "kilogram", "kilograms"], Dimension::Mass, "kg", 1.0),
    (&["gram", "grams"], Dimension::Mass, "kg", 0.001),
    (&["mg", "milligram", "milligrams"], Dimension::Mass, "kg", 1e-6),
    (&["lb", "lbs", "pound", "pounds"], Dimension::Mass, "kg", 0.453_592_37),
    (&["oz", "ounce", "ounces"], Dimension::Mass, "kg", 0.028_349_523_125),
    // ── length — base metre
    (&["meter", "meters", "metre"], Dimension::Length, "m", 1.0),
    (&["cm", "centimeter", "centimeters"], Dimension::Length, "m", 0.01),
    (&["mm", "millimeter", "millimeters"], Dimension::Length, "m", 0.001),
    (&["km", "kilometer", "kilometers"], Dimension::Length, "m", 1000.0),
    (&["inch", "inches"], Dimension::Length, "m", 0.0254),
    (&["ft", "feet", "foot"], Dimension::Length, "m", 0.3048),
    (&["mi", "mile", "miles"], Dimension::Length, "m", 1609.344),
    // ── duration — base second
    (&["ns", "nanosecond", "nanoseconds"], Dimension::Duration, "s", 1e-9),
    (&["µs", "microsecond", "microseconds"], Dimension::Duration, "s", 1e-6),
    (&["ms", "millisecond", "milliseconds"], Dimension::Duration, "s", 1e-3),
    (&["sec", "second", "seconds"], Dimension::Duration, "s", 1.0),
    (&["min", "minute", "minutes"], Dimension::Duration, "s", 60.0),
    (&["hr", "hour", "hours"], Dimension::Duration, "s", 3600.0),
    (&["day", "days"], Dimension::Duration, "s", 86_400.0),
    (&["week", "weeks"], Dimension::Duration, "s", 604_800.0),
    // ── data — base byte (binary 1024 ladder)
    (&["byte", "bytes"], Dimension::Data, "byte", 1.0),
    (&["kb", "kilobyte", "kilobytes"], Dimension::Data, "byte", 1024.0),
    (&["mb", "megabyte", "megabytes"], Dimension::Data, "byte", 1_048_576.0),
    (&["gb", "gigabyte", "gigabytes"], Dimension::Data, "byte", 1_073_741_824.0),
    (&["tb", "terabyte", "terabytes"], Dimension::Data, "byte", 1_099_511_627_776.0),
    (&["pb", "petabyte", "petabytes"], Dimension::Data, "byte", 1_125_899_906_842_624.0),
];

/// Build the surface-form → [`UnitEntry`] index from [`UNIT_TABLE`].
/// Keys are lowercased so lookups are case-insensitive. Called by
/// `seed::rebuild_indices` on every load; pure (reads no graph state),
/// so the result is identical across graphs.
pub fn build_unit_index() -> UnitIndex {
    let mut index = UnitIndex::with_capacity(96);
    for &(surfaces, dimension, base_unit, to_base) in UNIT_TABLE {
        let entry = UnitEntry { dimension, base_unit, to_base };
        for &surface in surfaces {
            index.insert(surface.to_lowercase(), entry);
        }
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seed::load_seed_graph;
    use std::collections::HashSet;

    #[test]
    fn unit_table_covers_29_units_with_no_duplicate_surface_forms() {
        assert_eq!(UNIT_TABLE.len(), 29, "expected 29 seeded units");
        let mut seen = HashSet::new();
        for &(surfaces, ..) in UNIT_TABLE {
            for &s in surfaces {
                assert!(seen.insert(s.to_lowercase()), "duplicate surface form: {s}");
            }
        }
    }

    #[test]
    fn index_lookup_is_case_insensitive() {
        let idx = build_unit_index();
        assert_eq!(idx.get("kg").unwrap().dimension, Dimension::Mass);
        assert_eq!(idx.get("KG").map(|e| e.dimension), None, "keys are lowercased");
        // The kernel lowercases tokens before lookup; the index itself is
        // lowercased, so the round-trip succeeds.
        assert!(idx.contains_key(&"GB".to_lowercase()));
    }

    #[test]
    fn conversions_to_base_are_correct() {
        let idx = build_unit_index();
        assert_eq!(idx["kg"].to_base, 1.0);
        assert!((idx["gram"].to_base - 0.001).abs() < 1e-12);
        assert_eq!(idx["km"].to_base, 1000.0);
        assert_eq!(idx["min"].to_base, 60.0);
        assert_eq!(idx["kb"].to_base, 1024.0);
        assert_eq!(idx["gb"].to_base, 1024.0 * 1024.0 * 1024.0);
        // cross-form magnitude: 2 km and 2000 m reach the same base value.
        assert_eq!(2.0 * idx["km"].to_base, 2000.0 * idx["meter"].to_base);
        // 8 lb ≈ 3.6287 kg.
        assert!((8.0 * idx["lb"].to_base - 3.628_739).abs() < 1e-4);
    }

    #[test]
    fn currencies_are_mutually_incomparable_but_aliases_agree() {
        let idx = build_unit_index();
        assert_ne!(idx["usd"].base_unit, idx["eur"].base_unit, "no FX: USD ≠ EUR");
        assert_eq!(idx["$"].base_unit, idx["dollars"].base_unit, "aliases share a base");
        assert_eq!(idx["$"].base_unit, "usd");
    }

    #[test]
    fn every_indexed_unit_is_a_seeded_graph_citizen() {
        // The factor table (code) and the unit Elements (seed pack) must
        // not drift: every surface form the kernel can parse must resolve
        // to a seeded Element via `by_name`. The reverse direction (29
        // seeded units) is pinned by the gen_seed_graph element budget.
        let hypergraph = load_seed_graph();
        let idx = build_unit_index();
        for surface in idx.keys() {
            assert!(
                hypergraph.by_name.contains_key(surface),
                "unit '{surface}' is in UNIT_TABLE but not seeded in seed_pack.yaml",
            );
        }
    }
}
