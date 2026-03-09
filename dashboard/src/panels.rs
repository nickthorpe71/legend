use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::data::LegendData;
use crate::graph::SelectedNode;

// ---------------------------------------------------------------------------
// egui overlay panels — event log, stats, short-term, sessions, inspection
// ---------------------------------------------------------------------------

pub fn ui_panels(mut ctx: EguiContexts, data: Res<LegendData>, mut selected: ResMut<SelectedNode>) {
    let c = ctx.ctx_mut();

    // ---- left panel: live event log ----
    egui::SidePanel::left("events")
        .default_width(360.0)
        .resizable(true)
        .show(c, |ui| {
            ui.heading("📡 Event Log");
            ui.separator();

            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    // Show newest at bottom
                    let start = data.events.len().saturating_sub(80);
                    for ev in &data.events[start..] {
                        ui.horizontal(|ui| {
                            let badge_color = cmd_color(&ev.command);
                            let badge =
                                egui::RichText::new(&ev.command).color(badge_color).strong();
                            ui.label(badge);
                            ui.label(egui::RichText::new(truncate(&ev.detail, 65)).weak());
                        });
                    }
                });
        });

    // ---- right panel: stats + short-term + sessions ----
    egui::SidePanel::right("info")
        .default_width(370.0)
        .resizable(true)
        .show(c, |ui| {
            // ---- stats ----
            ui.heading("📊 Memory Stats");
            ui.separator();
            stat_row(ui, "Clock", &data.clock.to_string());
            stat_row(ui, "Immediate", &data.immediate.len().to_string());
            stat_row(ui, "Short-term", &data.short_term.len().to_string());
            stat_row(ui, "Graph nodes", &data.graph_nodes.len().to_string());
            stat_row(ui, "Graph edges", &data.graph_edges.len().to_string());
            stat_row(ui, "Events seen", &data.events.len().to_string());

            ui.add_space(12.0);

            // ---- short-term entries ----
            ui.heading("📝 Short-term Entries");
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("st_scroll")
                .max_height(280.0)
                .show(ui, |ui| {
                    for entry in &data.short_term {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                if entry.labile {
                                    ui.label(
                                        egui::RichText::new("⚡ LABILE")
                                            .color(egui::Color32::from_rgb(255, 200, 50))
                                            .size(10.0)
                                            .strong(),
                                    );
                                }
                                if entry.reconsolidation_count > 0 {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "🔄×{}",
                                            entry.reconsolidation_count
                                        ))
                                        .color(egui::Color32::from_rgb(130, 200, 255))
                                        .size(10.0),
                                    );
                                }
                            });
                            ui.label(
                                egui::RichText::new(truncate(&entry.text, 120))
                                    .size(15.0)
                                    .color(if entry.labile {
                                        egui::Color32::from_rgb(255, 230, 150)
                                    } else {
                                        egui::Color32::LIGHT_GRAY
                                    }),
                            );
                            ui.add(
                                egui::ProgressBar::new(entry.salience)
                                    .text(format!("sal {:.2}  use {}", entry.salience, entry.usage))
                                    .fill(salience_bar_color(entry.salience)),
                            );
                            if !entry.refs.is_empty() {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "refs: {}",
                                        entry
                                            .refs
                                            .iter()
                                            .take(3)
                                            .map(|r| format!(
                                                "{}#L{}-{}",
                                                r.path, r.start_line, r.end_line
                                            ))
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    ))
                                    .size(11.0)
                                    .color(egui::Color32::from_rgb(170, 190, 220)),
                                );
                            }
                        });
                    }
                });

            ui.add_space(12.0);

            // ---- session log ----
            ui.heading("📜 Session Log");
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("sess_scroll")
                .max_height(200.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for sess in data.session_log.iter().rev().take(20) {
                        ui.label(
                            egui::RichText::new(format!(
                                "[t={}] {}",
                                sess.timestamp,
                                truncate(&sess.text, 120)
                            ))
                            .size(15.0),
                        );
                    }
                });
        });

    // ---- bottom panel: node inspection (only when a node is selected) ----
    if let Some(sel_id) = selected.id {
        let node_info = data.graph_nodes.iter().find(|n| n.id == sel_id);

        egui::TopBottomPanel::bottom("inspection")
            .default_height(280.0)
            .resizable(true)
            .show(c, |ui| {
                if let Some(node) = node_info {
                    // ---- Header bar ----
                    ui.horizontal(|ui| {
                        let kind_emoji = match node.kind.as_str() {
                            "Function" | "Method" => "⚙️",
                            "Struct" | "Class" | "Type" => "🧱",
                            "Module" | "Import" => "📦",
                            "Symbol" => "🔣",
                            "Summary" => "📋",
                            "Term" => "💬",
                            _ => "●",
                        };
                        ui.heading(format!("{} {}", kind_emoji, node.label));
                        let kind_color = kind_badge_color(&node.kind);
                        ui.label(
                            egui::RichText::new(format!(" [{}]", node.kind))
                                .color(kind_color)
                                .strong()
                                .size(14.0),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("✕ Close").clicked() {
                                selected.id = None;
                            }
                        });
                    });
                    ui.separator();

                    // ---- Precompute derived data ----
                    // Neighbors + edge data
                    let mut neighbors: Vec<(u64, f32, String, &str)> = Vec::new();
                    let mut total_edge_weight: f32 = 0.0;
                    let mut in_degree: usize = 0;
                    let mut out_degree: usize = 0;
                    for edge in &data.graph_edges {
                        if edge.from == sel_id {
                            neighbors.push((edge.to, edge.weight, edge.kind.clone(), "→"));
                            total_edge_weight += edge.weight;
                            out_degree += 1;
                        }
                        if edge.to == sel_id {
                            neighbors.push((edge.from, edge.weight, edge.kind.clone(), "←"));
                            total_edge_weight += edge.weight;
                            in_degree += 1;
                        }
                    }
                    // Sort neighbors by edge weight descending
                    neighbors
                        .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

                    // Find short-term entries that mention this node's label
                    let label_lower = node.label.to_lowercase();
                    let related_memories: Vec<&crate::data::DumpShortTerm> = data
                        .short_term
                        .iter()
                        .filter(|e| e.text.to_lowercase().contains(&label_lower))
                        .collect();

                    // Find session log entries that mention this node's label
                    let related_sessions: Vec<&crate::data::DumpSession> = data
                        .session_log
                        .iter()
                        .filter(|s| s.text.to_lowercase().contains(&label_lower))
                        .collect();

                    // Age
                    let age = data.clock.saturating_sub(node.last_seen);

                    // Strongest neighbor
                    let strongest_neighbor = neighbors.first().and_then(|(nid, w, _, _)| {
                        data.graph_nodes
                            .iter()
                            .find(|n| n.id == *nid)
                            .map(|n| (n.label.clone(), *w))
                    });

                    // ---- Three-column layout ----
                    ui.columns(3, |cols| {
                        // === Column 1: Node Properties ===
                        cols[0].vertical(|ui| {
                            ui.label(egui::RichText::new("📊 Properties").strong().size(14.0));
                            ui.add_space(4.0);

                            detail_row(ui, "ID", &node.id.to_string());
                            detail_row(ui, "Weight", &format!("{:.3}", node.weight));

                            // Salience with visual bar
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Salience:").strong().size(13.0));
                                ui.add(
                                    egui::ProgressBar::new(node.salience)
                                        .text(format!("{:.3}", node.salience))
                                        .fill(salience_bar_color(node.salience))
                                        .desired_width(100.0),
                                );
                            });

                            detail_row(ui, "Last Seen", &format!("t={}", node.last_seen));
                            detail_row(
                                ui,
                                "Age",
                                &if age == 0 {
                                    "current".to_string()
                                } else {
                                    format!("{} ticks ago", age)
                                },
                            );

                            ui.add_space(8.0);
                            ui.label(egui::RichText::new("🔗 Connectivity").strong().size(14.0));
                            ui.add_space(4.0);
                            detail_row(
                                ui,
                                "Degree",
                                &format!(
                                    "{} ({} in / {} out)",
                                    in_degree + out_degree,
                                    in_degree,
                                    out_degree
                                ),
                            );
                            detail_row(
                                ui,
                                "Avg Edge W",
                                &if neighbors.is_empty() {
                                    "—".to_string()
                                } else {
                                    format!("{:.3}", total_edge_weight / neighbors.len() as f32)
                                },
                            );
                            if let Some((lbl, w)) = &strongest_neighbor {
                                detail_row(ui, "Strongest", &format!("{} ({:.2})", lbl, w));
                            }
                            detail_row(
                                ui,
                                "Mentions",
                                &format!(
                                    "{} memories, {} sessions",
                                    related_memories.len(),
                                    related_sessions.len()
                                ),
                            );
                        });

                        // === Column 2: Related Memories ===
                        cols[1].vertical(|ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "🧠 Related Memories ({})",
                                    related_memories.len()
                                ))
                                .strong()
                                .size(14.0),
                            );
                            ui.add_space(4.0);

                            egui::ScrollArea::vertical()
                                .id_salt("related_mem_scroll")
                                .max_height(200.0)
                                .show(ui, |ui| {
                                    if related_memories.is_empty() {
                                        ui.label(
                                            egui::RichText::new(
                                                "No short-term memories reference this node",
                                            )
                                            .weak()
                                            .italics(),
                                        );
                                    }
                                    for entry in &related_memories {
                                        ui.group(|ui| {
                                            ui.label(
                                                egui::RichText::new(truncate(&entry.text, 100))
                                                    .size(12.0)
                                                    .color(egui::Color32::LIGHT_GRAY),
                                            );
                                            ui.horizontal(|ui| {
                                                ui.add(
                                                    egui::ProgressBar::new(entry.salience)
                                                        .text(format!("sal {:.2}", entry.salience))
                                                        .fill(salience_bar_color(entry.salience))
                                                        .desired_width(80.0),
                                                );
                                                ui.label(
                                                    egui::RichText::new(format!(
                                                        "use {} · t={}",
                                                        entry.usage, entry.last_access
                                                    ))
                                                    .weak()
                                                    .size(11.0),
                                                );
                                            });
                                        });
                                    }

                                    // Session mentions below
                                    if !related_sessions.is_empty() {
                                        ui.add_space(8.0);
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "📜 Session Mentions ({})",
                                                related_sessions.len()
                                            ))
                                            .strong()
                                            .size(13.0),
                                        );
                                        ui.add_space(2.0);
                                        for sess in related_sessions.iter().rev().take(10) {
                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "[t={}] {}",
                                                    sess.timestamp,
                                                    truncate(&sess.text, 90)
                                                ))
                                                .size(11.0)
                                                .color(egui::Color32::from_rgb(180, 180, 200)),
                                            );
                                        }
                                    }
                                });
                        });

                        // === Column 3: Connections (neighbors) ===
                        cols[2].vertical(|ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "🔗 Connections ({})",
                                    neighbors.len()
                                ))
                                .strong()
                                .size(14.0),
                            );
                            ui.add_space(4.0);

                            egui::ScrollArea::vertical()
                                .id_salt("neighbors_scroll")
                                .max_height(220.0)
                                .show(ui, |ui| {
                                    if neighbors.is_empty() {
                                        ui.label(
                                            egui::RichText::new("No connections").weak().italics(),
                                        );
                                    }
                                    for (neighbor_id, edge_weight, edge_kind, direction) in
                                        &neighbors
                                    {
                                        let neighbor_node =
                                            data.graph_nodes.iter().find(|n| n.id == *neighbor_id);
                                        let neighbor_label = neighbor_node
                                            .map(|n| n.label.as_str())
                                            .unwrap_or("???");
                                        let neighbor_kind =
                                            neighbor_node.map(|n| n.kind.as_str()).unwrap_or("");
                                        let neighbor_weight =
                                            neighbor_node.map(|n| n.weight).unwrap_or(0.0);

                                        ui.horizontal(|ui| {
                                            // Direction + button
                                            let btn_text =
                                                format!("{} {}", direction, neighbor_label);
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        egui::RichText::new(&btn_text)
                                                            .size(13.0)
                                                            .color(egui::Color32::from_rgb(
                                                                130, 200, 255,
                                                            )),
                                                    )
                                                    .frame(false),
                                                )
                                                .clicked()
                                            {
                                                selected.id = Some(*neighbor_id);
                                            }
                                            // Kind badge
                                            ui.label(
                                                egui::RichText::new(format!("[{}]", neighbor_kind))
                                                    .color(kind_badge_color(neighbor_kind))
                                                    .size(11.0),
                                            );
                                            // Edge info
                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "e={:.2} ({}) · nw={:.2}",
                                                    edge_weight, edge_kind, neighbor_weight
                                                ))
                                                .weak()
                                                .size(10.0),
                                            );
                                        });
                                    }
                                });
                        });
                    });
                } else {
                    ui.label("Selected node not found in current data.");
                    if ui.button("✕ Close").clicked() {
                        selected.id = None;
                    }
                }
            });
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn stat_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(value).monospace());
        });
    });
}

fn detail_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{}:", label))
                .strong()
                .size(13.0),
        );
        ui.label(egui::RichText::new(value).monospace().size(13.0));
    });
}

fn cmd_color(cmd: &str) -> egui::Color32 {
    match cmd {
        "tick" => egui::Color32::from_rgb(90, 200, 90),
        "query" => egui::Color32::from_rgb(90, 140, 255),
        "start" => egui::Color32::from_rgb(200, 100, 255),
        "reinforce" => egui::Color32::from_rgb(255, 200, 50),
        "consolidate" => egui::Color32::from_rgb(255, 130, 80),
        "reset" => egui::Color32::from_rgb(255, 80, 80),
        _ => egui::Color32::GRAY,
    }
}

fn kind_badge_color(kind: &str) -> egui::Color32 {
    match kind {
        "Function" | "Method" => egui::Color32::from_rgb(80, 180, 220),
        "Struct" | "Class" | "Type" => egui::Color32::from_rgb(80, 200, 120),
        "Module" | "Import" => egui::Color32::from_rgb(200, 200, 80),
        "Symbol" => egui::Color32::from_rgb(220, 160, 60),
        "Summary" => egui::Color32::from_rgb(180, 100, 220),
        "Term" => egui::Color32::from_rgb(100, 160, 230),
        _ => egui::Color32::from_rgb(160, 160, 160),
    }
}

fn salience_bar_color(s: f32) -> egui::Color32 {
    let r = (s * 220.0) as u8;
    let g = ((1.0 - s) * 180.0) as u8;
    egui::Color32::from_rgb(r, g, 120)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}
