use crate::memory::{GraphEdge, GraphNode, MemoryState};
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use serde::Deserialize;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::io::{self, BufRead};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Rich Event Data Types (parsed from .legend/events.jsonl)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type")]
pub enum EventData {
    Tick(TickEventData),
    Query(QueryEventData),
    Reinforce(ReinforceEventData),
    Consolidate(ConsolidateEventData),
    Start(StartEventData),
}

#[derive(Clone, Debug, Deserialize)]
pub struct TickEventData {
    pub entry_id: Option<u64>,
    pub matches: Vec<MatchedEntry>,
    pub graph_nodes: Vec<GraphHit>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct QueryEventData {
    pub matches: Vec<MatchedEntry>,
    pub graph_nodes: Vec<GraphHit>,
    pub primed_count: usize,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MatchedEntry {
    pub id: u64,
    pub similarity: f32,
    pub text_preview: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GraphHit {
    #[allow(dead_code)]
    pub id: u64,
    pub label: String,
    pub kind: String,
    pub weight: f32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ReinforceEventData {
    pub signal: f32,
    pub entries: Vec<ReinforceEntryData>,
    pub graph_nodes_affected: usize,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ReinforceEntryData {
    pub id: u64,
    pub before: f32,
    pub after: f32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ConsolidateEventData {
    pub groups_merged: usize,
    pub summaries: Vec<ConsolidatedGroup>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ConsolidatedGroup {
    pub node_id: u64,
    pub label: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct StartEventData {
    pub clock: u64,
    pub short_term_count: usize,
    pub long_term_nodes: usize,
    pub session_log_entries: usize,
}

#[derive(Clone, Debug)]
pub struct EventEntry {
    pub timestamp: u64,
    pub command: String,
    pub detail: String,
    pub data: Option<EventData>,
}

fn load_events() -> Vec<EventEntry> {
    let events_path = std::path::Path::new(".legend/events.jsonl");
    let Ok(file) = std::fs::File::open(events_path) else {
        return Vec::new();
    };
    let reader = io::BufReader::new(file);

    reader
        .lines()
        .filter_map(|line| {
            let line = line.ok()?;
            let json: serde_json::Value = serde_json::from_str(&line).ok()?;
            let event_data: Option<EventData> = json
                .get("data")
                .and_then(|d| serde_json::from_value(d.clone()).ok());
            Some(EventEntry {
                timestamp: json["ts"].as_u64().unwrap_or(0),
                command: json["cmd"].as_str().unwrap_or("").to_string(),
                detail: json["detail"].as_str().unwrap_or("").to_string(),
                data: event_data,
            })
        })
        .collect()
}

/// Active view in the main panel
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    ShortTerm,
    Graph,
    Events,
}

impl View {
    fn next(self) -> Self {
        match self {
            View::ShortTerm => View::Graph,
            View::Graph => View::Events,
            View::Events => View::ShortTerm,
        }
    }

    fn prev(self) -> Self {
        match self {
            View::ShortTerm => View::Events,
            View::Graph => View::ShortTerm,
            View::Events => View::Graph,
        }
    }

    fn label(self) -> &'static str {
        match self {
            View::ShortTerm => "Short-Term Memory",
            View::Graph => "Knowledge Graph",
            View::Events => "Command Events",
        }
    }
}

/// Input mode for the TUI
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Search,
    Query,
    Detail, // Viewing item details
}

/// TUI Application state
pub struct App {
    pub memory: MemoryState,
    pub events: Vec<EventEntry>, // Loaded from .legend/events.jsonl
    pub view: View,
    pub input_mode: InputMode,
    pub input_buffer: String,
    pub list_state: ListState,
    pub graph_stack: Vec<u64>,
    pub should_quit: bool,
    pub last_query_result: Option<String>,
    pub status_message: Option<String>,
    pub detail_content: Option<String>, // Rendered detail text
}

#[derive(Debug, Clone)]
struct GraphListItem {
    id: u64,
    label: String,
    kind: String,
    weight: f32,
    edge_type: Option<String>,
}

impl App {
    pub fn new(memory: MemoryState) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        let events = load_events();
        Self {
            memory,
            events,
            view: View::ShortTerm,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            list_state,
            graph_stack: Vec::new(),
            should_quit: false,
            last_query_result: None,
            status_message: None,
            detail_content: None,
        }
    }

    pub fn reload_memory(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.memory = crate::memory::load_or_default()?;
        self.events = load_events();
        Ok(())
    }

    fn graph_items(&self) -> Vec<GraphListItem> {
        if let Some(&root_id) = self.graph_stack.last() {
            let mut dedup: HashMap<u64, GraphListItem> = HashMap::new();

            for edge in &self.memory.brain.long_term.edges {
                let neighbor_id = if edge.from == root_id {
                    edge.to
                } else if edge.to == root_id {
                    edge.from
                } else {
                    continue;
                };

                if let Some(node) = self.memory.brain.long_term.nodes.get(&neighbor_id) {
                    let weight = node.weight + edge.weight;
                    let item = GraphListItem {
                        id: node.id,
                        label: node.label.clone(),
                        kind: node.kind.clone(),
                        weight,
                        edge_type: Some(edge.kind.clone()),
                    };

                    dedup
                        .entry(node.id)
                        .and_modify(|existing| {
                            if weight > existing.weight {
                                *existing = item.clone();
                            }
                        })
                        .or_insert(item);
                }
            }

            let mut items: Vec<GraphListItem> = dedup.into_values().collect();
            items.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap_or(Ordering::Equal));
            items
        } else {
            let mut nodes: Vec<_> = self.memory.brain.long_term.nodes.values().collect();
            nodes.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap_or(Ordering::Equal));
            nodes
                .into_iter()
                .map(|node| GraphListItem {
                    id: node.id,
                    label: node.label.clone(),
                    kind: node.kind.clone(),
                    weight: node.weight,
                    edge_type: None,
                })
                .collect()
        }
    }

    fn graph_selected_id(&self) -> Option<u64> {
        let items = self.graph_items();
        let idx = self.list_state.selected().unwrap_or(0);
        items.get(idx).map(|i| i.id)
    }

    fn graph_focus_label(&self, id: u64) -> String {
        self.memory
            .brain
            .long_term
            .nodes
            .get(&id)
            .map(|n| n.label.clone())
            .unwrap_or_else(|| format!("{}", id))
    }

    fn drill_graph(&mut self) {
        if let Some(id) = self.graph_selected_id() {
            self.graph_stack.push(id);
            self.list_state.select(Some(0));
            let label = self.graph_focus_label(id);
            self.status_message = Some(format!("Graph: neighbors of {}", label));
        }
    }

    fn graph_back(&mut self) {
        if self.graph_stack.pop().is_some() {
            self.list_state.select(Some(0));
            if let Some(&id) = self.graph_stack.last() {
                let label = self.graph_focus_label(id);
                self.status_message = Some(format!("Graph: neighbors of {}", label));
            } else {
                self.status_message = Some("Graph: top nodes".to_string());
            }
        }
    }

    fn item_count(&self) -> usize {
        match self.view {
            View::ShortTerm => self.memory.brain.short_term.len(),
            View::Graph => self.graph_items().len(),
            View::Events => self.events.len(),
        }
    }

    fn select_next(&mut self) {
        let count = self.item_count();
        if count == 0 {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => (i + 1) % count,
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn select_prev(&mut self) {
        let count = self.item_count();
        if count == 0 {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    count - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn select_first(&mut self) {
        if self.item_count() > 0 {
            self.list_state.select(Some(0));
        }
    }

    fn select_last(&mut self) {
        let count = self.item_count();
        if count > 0 {
            self.list_state.select(Some(count - 1));
        }
    }

    fn switch_view(&mut self, view: View) {
        self.view = view;
        self.list_state.select(Some(0));
        self.last_query_result = None;
    }

    fn execute_query(&mut self) {
        if self.input_buffer.is_empty() {
            return;
        }
        let query = self.input_buffer.clone();
        let ctx = crate::memory::retrieve_context(&mut self.memory.brain, &query);
        let _ = crate::memory::save(&self.memory);

        let mut result = format!("Query: \"{}\"\n\n", query);
        result.push_str("Short-term matches:\n");
        for snippet in &ctx.short_term {
            result.push_str(&format!(
                "  [{:.2}] {}\n",
                snippet.similarity,
                truncate(&snippet.text, 60)
            ));
            if !snippet.refs.is_empty() {
                result.push_str("    refs:\n");
                for reference in snippet.refs.iter().take(3) {
                    if reference.kind.is_empty() || reference.kind == "source" {
                        result.push_str(&format!(
                            "      {}#L{}-{}\n",
                            reference.path, reference.start_line, reference.end_line
                        ));
                    } else {
                        result
                            .push_str(&format!("      {}: {}\n", reference.kind, reference.value));
                    }
                }
            }
        }
        result.push_str("\nGraph nodes:\n");
        for node in &ctx.long_term {
            result.push_str(&format!(
                "  [{}] {} (w={:.2})\n",
                node.kind,
                truncate(&node.label, 40),
                node.weight
            ));
        }
        self.last_query_result = Some(result);
        self.input_buffer.clear();
        self.input_mode = InputMode::Normal;
    }

    fn filter_view(&mut self) {
        let filter = self.input_buffer.to_lowercase();
        self.status_message = Some(format!("Filter: {}", filter));
        self.input_buffer.clear();
        self.input_mode = InputMode::Normal;
    }

    fn show_detail(&mut self) {
        let idx = self.list_state.selected().unwrap_or(0);

        let content = match self.view {
            View::ShortTerm => self.format_short_term_detail(idx),
            View::Graph => self
                .graph_selected_id()
                .and_then(|id| self.format_graph_detail(id)),
            View::Events => self.format_event_detail(idx),
        };

        if let Some(content) = content {
            self.detail_content = Some(content);
            self.input_mode = InputMode::Detail;
        }
    }

    fn selected_text(&self) -> Option<String> {
        let idx = self.list_state.selected().unwrap_or(0);
        match self.view {
            View::ShortTerm => self
                .memory
                .brain
                .short_term
                .get(idx)
                .map(|e| e.text.clone()),
            View::Graph => self.graph_selected_id().and_then(|id| {
                self.memory
                    .brain
                    .long_term
                    .nodes
                    .get(&id)
                    .map(|n| n.label.clone())
            }),
            View::Events => {
                let events: Vec<_> = self.events.iter().rev().collect();
                events
                    .get(idx)
                    .map(|e| format!("[{}] {}: {}", e.timestamp, e.command, e.detail))
            }
        }
    }

    fn format_short_term_detail(&self, idx: usize) -> Option<String> {
        let entry = self.memory.brain.short_term.get(idx)?;
        let age = self.memory.brain.clock.saturating_sub(entry.last_access);

        let labile_status = if entry.labile_until >= self.memory.brain.clock {
            format!(
                "Yes (expires in {} ticks)",
                entry.labile_until - self.memory.brain.clock
            )
        } else {
            "No".to_string()
        };

        Some(format!(
            "SHORT-TERM ENTRY #{}\n\n\
             ID: {}\n\
             Salience: {:.3}\n\
             Usage: {} hits\n\
             Age: {} ticks since last access\n\
             Reconsolidations: {}\n\
             Labile: {}\n\n\
             --- TEXT ---\n{}\n\n\
             --- SUMMARY ---\n{}",
            idx,
            entry.id,
            entry.salience,
            entry.usage,
            age,
            entry.reconsolidation_count,
            labile_status,
            entry.text,
            if entry.summary.is_empty() {
                "(none)"
            } else {
                &entry.summary
            }
        ))
    }

    fn format_graph_detail(&self, node_id: u64) -> Option<String> {
        let node = self.memory.brain.long_term.nodes.get(&node_id)?;
        let age = self.memory.brain.clock.saturating_sub(node.last_seen);

        // Find edges involving this node
        let mut incoming: Vec<(&GraphEdge, &GraphNode)> = vec![];
        let mut outgoing: Vec<(&GraphEdge, &GraphNode)> = vec![];

        for edge in &self.memory.brain.long_term.edges {
            if edge.to == node.id {
                if let Some(from_node) = self.memory.brain.long_term.nodes.get(&edge.from) {
                    incoming.push((edge, from_node));
                }
            } else if edge.from == node.id {
                if let Some(to_node) = self.memory.brain.long_term.nodes.get(&edge.to) {
                    outgoing.push((edge, to_node));
                }
            }
        }

        // Sort by edge weight
        incoming.sort_by(|a, b| {
            b.0.weight
                .partial_cmp(&a.0.weight)
                .unwrap_or(Ordering::Equal)
        });
        outgoing.sort_by(|a, b| {
            b.0.weight
                .partial_cmp(&a.0.weight)
                .unwrap_or(Ordering::Equal)
        });

        let mut result = format!(
            "GRAPH NODE\n\n\
             ID: {}\n\
             Kind: {}\n\
             Weight: {:.3}\n\
             Salience: {:.3}\n\
             Age: {} ticks since last seen\n\n\
             --- LABEL ---\n{}\n\n\
             --- CONNECTIONS ({} in, {} out) ---\n",
            node.id,
            node.kind,
            node.weight,
            node.salience,
            age,
            node.label,
            incoming.len(),
            outgoing.len()
        );

        if !incoming.is_empty() {
            result.push_str("\nIncoming:\n");
            for (edge, from) in incoming.iter().take(10) {
                result.push_str(&format!(
                    "  <- [{}] {} (w={:.2})\n",
                    edge.kind,
                    truncate(&from.label, 30),
                    edge.weight
                ));
            }
        }

        if !outgoing.is_empty() {
            result.push_str("\nOutgoing:\n");
            for (edge, to) in outgoing.iter().take(10) {
                result.push_str(&format!(
                    "  -> [{}] {} (w={:.2})\n",
                    edge.kind,
                    truncate(&to.label, 30),
                    edge.weight
                ));
            }
        }

        Some(result)
    }

    fn format_event_detail(&self, idx: usize) -> Option<String> {
        // Events are displayed in reverse order
        let events: Vec<_> = self.events.iter().rev().collect();
        let event = events.get(idx)?;

        let mut output = format!(
            "COMMAND EVENT: {}\n\n\
             Timestamp: {}\n\
             Detail: {}\n",
            event.command.to_uppercase(),
            event.timestamp,
            event.detail
        );

        // Add rich data details based on event type
        match &event.data {
            Some(EventData::Tick(t)) => {
                output.push_str(&format!(
                    "\n--- TICK DETAILS ---\n\
                     Entry ID: {}\n",
                    t.entry_id
                        .map(|id| id.to_string())
                        .unwrap_or_else(|| "N/A".to_string())
                ));

                if !t.matches.is_empty() {
                    output.push_str("\nMatched Entries:\n");
                    for m in &t.matches {
                        output.push_str(&format!(
                            "  #{} [{:.0}%] {}\n",
                            m.id,
                            m.similarity * 100.0,
                            m.text_preview
                        ));
                    }
                }

                if !t.graph_nodes.is_empty() {
                    output.push_str("\nGraph Nodes Surfaced:\n");
                    for n in &t.graph_nodes {
                        output.push_str(&format!(
                            "  {} [{}] weight={:.2}\n",
                            n.label, n.kind, n.weight
                        ));
                    }
                }
            }
            Some(EventData::Query(q)) => {
                output.push_str(&format!(
                    "\n--- QUERY RESULTS ---\n\
                     Total Matches: {}\n\
                     Primed Nodes: {}\n",
                    q.matches.len(),
                    q.primed_count
                ));

                if !q.matches.is_empty() {
                    output.push_str("\nMatched Entries:\n");
                    for m in &q.matches {
                        output.push_str(&format!(
                            "  #{} [{:.0}%] {}\n",
                            m.id,
                            m.similarity * 100.0,
                            m.text_preview
                        ));
                    }
                }

                if !q.graph_nodes.is_empty() {
                    output.push_str("\nGraph Nodes Retrieved:\n");
                    for n in &q.graph_nodes {
                        output.push_str(&format!(
                            "  {} [{}] weight={:.2}\n",
                            n.label, n.kind, n.weight
                        ));
                    }
                }
            }
            Some(EventData::Reinforce(r)) => {
                output.push_str(&format!(
                    "\n--- REINFORCEMENT ---\n\
                     Signal: {:.2}\n\
                     Graph Nodes Affected: {}\n",
                    r.signal, r.graph_nodes_affected
                ));

                if !r.entries.is_empty() {
                    output.push_str("\nSalience Changes:\n");
                    for e in &r.entries {
                        let delta = e.after - e.before;
                        let arrow = if delta > 0.0 { "↑" } else { "↓" };
                        output.push_str(&format!(
                            "  #{}: {:.3} → {:.3} ({}{:.3})\n",
                            e.id,
                            e.before,
                            e.after,
                            arrow,
                            delta.abs()
                        ));
                    }
                }
            }
            Some(EventData::Consolidate(c)) => {
                output.push_str(&format!(
                    "\n--- CONSOLIDATION ---\n\
                     Groups Merged: {}\n",
                    c.groups_merged
                ));

                if !c.summaries.is_empty() {
                    output.push_str("\nCreated Nodes:\n");
                    for g in &c.summaries {
                        output.push_str(&format!("  #{}: {}\n", g.node_id, g.label));
                    }
                }
            }
            Some(EventData::Start(s)) => {
                output.push_str(&format!(
                    "\n--- SESSION START ---\n\
                     Clock: {}\n\
                     Short-term Entries: {}\n\
                     Long-term Nodes: {}\n\
                     Session Log Entries: {}\n",
                    s.clock, s.short_term_count, s.long_term_nodes, s.session_log_entries
                ));
            }
            None => {
                output.push_str("\n(No rich data available for this event)\n");
            }
        }

        Some(output)
    }
}

/// Copy text to the system clipboard via OSC 52 escape sequence.
/// Works in most modern terminals (kitty, alacritty, iTerm2, xterm, etc.)
/// and over SSH without any external dependencies.
fn copy_to_clipboard(text: &str) {
    use std::io::Write;
    let encoded = base64_encode(text.as_bytes());
    // Write to /dev/tty to bypass the alternate screen buffer
    if let Ok(mut tty) = std::fs::OpenOptions::new().write(true).open("/dev/tty") {
        let _ = write!(tty, "\x1b]52;c;{}\x07", encoded);
    }
}

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i] as u32;
        let b1 = if i + 1 < data.len() {
            data[i + 1] as u32
        } else {
            0
        };
        let b2 = if i + 2 < data.len() {
            data[i + 2] as u32
        } else {
            0
        };
        out.push(CHARS[((b0 >> 2) & 0x3F) as usize] as char);
        out.push(CHARS[((b0 << 4 | b1 >> 4) & 0x3F) as usize] as char);
        out.push(if i + 1 < data.len() {
            CHARS[((b1 << 2 | b2 >> 6) & 0x3F) as usize] as char
        } else {
            '='
        });
        out.push(if i + 2 < data.len() {
            CHARS[(b2 & 0x3F) as usize] as char
        } else {
            '='
        });
        i += 3;
    }
    out
}

/// Run the TUI application
pub fn run_tui() -> Result<(), Box<dyn std::error::Error>> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    // Note: we intentionally do NOT enable mouse capture so the terminal
    // can handle native text selection for copy-paste.
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Load memory and create app
    let memory = crate::memory::load_or_default()?;
    let mut app = App::new(memory);

    // Auto-refresh interval
    let refresh_interval = Duration::from_secs(2);
    let mut last_refresh = Instant::now();

    // Main loop
    let result = loop {
        // Draw UI
        terminal.draw(|f| ui(f, &mut app))?;

        // Auto-refresh memory state
        if last_refresh.elapsed() >= refresh_interval {
            if let Err(e) = app.reload_memory() {
                app.status_message = Some(format!("Reload error: {}", e));
            }
            last_refresh = Instant::now();
        }

        // Handle input with timeout for refresh
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match app.input_mode {
                    InputMode::Normal => match key.code {
                        KeyCode::Char('q') => {
                            app.should_quit = true;
                        }
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app.should_quit = true;
                        }
                        KeyCode::Char('j') | KeyCode::Down => app.select_next(),
                        KeyCode::Char('k') | KeyCode::Up => app.select_prev(),
                        KeyCode::Char('g') => app.select_first(),
                        KeyCode::Char('G') => app.select_last(),
                        KeyCode::Tab => app.switch_view(app.view.next()),
                        KeyCode::BackTab => app.switch_view(app.view.prev()),
                        KeyCode::Char('1') => app.switch_view(View::ShortTerm),
                        KeyCode::Char('2') => app.switch_view(View::Graph),
                        KeyCode::Char('3') => app.switch_view(View::Events),
                        KeyCode::Char('/') => {
                            app.input_mode = InputMode::Search;
                            app.input_buffer.clear();
                        }
                        KeyCode::Char(':') => {
                            app.input_mode = InputMode::Query;
                            app.input_buffer.clear();
                        }
                        KeyCode::Char('r') => {
                            if let Err(e) = app.reload_memory() {
                                app.status_message = Some(format!("Reload error: {}", e));
                            } else {
                                app.status_message = Some("Reloaded".to_string());
                            }
                            last_refresh = Instant::now();
                        }
                        KeyCode::Char('b') | KeyCode::Backspace => {
                            if app.view == View::Graph {
                                app.graph_back();
                            }
                        }
                        KeyCode::Char('d') => {
                            if app.view == View::Graph {
                                app.show_detail();
                            }
                        }
                        KeyCode::Enter => {
                            if app.view == View::Graph {
                                app.drill_graph();
                            } else {
                                app.show_detail();
                            }
                        }
                        KeyCode::Char('y') => {
                            if let Some(text) = app.selected_text() {
                                copy_to_clipboard(&text);
                                app.status_message = Some("Copied to clipboard".to_string());
                            }
                        }
                        _ => {}
                    },
                    InputMode::Search => match key.code {
                        KeyCode::Enter => app.filter_view(),
                        KeyCode::Esc => {
                            app.input_mode = InputMode::Normal;
                            app.input_buffer.clear();
                        }
                        KeyCode::Backspace => {
                            app.input_buffer.pop();
                        }
                        KeyCode::Char(c) => {
                            app.input_buffer.push(c);
                        }
                        _ => {}
                    },
                    InputMode::Query => match key.code {
                        KeyCode::Enter => app.execute_query(),
                        KeyCode::Esc => {
                            app.input_mode = InputMode::Normal;
                            app.input_buffer.clear();
                        }
                        KeyCode::Backspace => {
                            app.input_buffer.pop();
                        }
                        KeyCode::Char(c) => {
                            app.input_buffer.push(c);
                        }
                        _ => {}
                    },
                    InputMode::Detail => match key.code {
                        KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                            app.input_mode = InputMode::Normal;
                            app.detail_content = None;
                        }
                        KeyCode::Char('y') => {
                            if let Some(ref content) = app.detail_content.clone() {
                                copy_to_clipboard(content);
                                app.status_message = Some("Copied to clipboard".to_string());
                            }
                        }
                        _ => {}
                    },
                }
            }
        }

        if app.should_quit {
            break Ok(());
        }
    };

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn ui(f: &mut Frame, app: &mut App) {
    // Main layout: sidebar (20 chars) | main panel
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(24), Constraint::Min(40)])
        .split(f.area());

    render_sidebar(f, app, chunks[0]);
    render_main_panel(f, app, chunks[1]);
}

fn render_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Legend ")
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Stats section
    let stats = vec![
        Line::from(vec![
            Span::styled("Clock: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", app.memory.brain.clock),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("ST: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", app.memory.brain.short_term.len()),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw(" entries"),
        ]),
        Line::from(vec![
            Span::styled("Nodes: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", app.memory.brain.long_term.nodes.len()),
                Style::default().fg(Color::Green),
            ),
        ]),
        Line::from(vec![
            Span::styled("Edges: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", app.memory.brain.long_term.edges.len()),
                Style::default().fg(Color::Green),
            ),
        ]),
        Line::from(""),
        // Labile count
        Line::from(vec![
            Span::styled("Labile: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!(
                    "{}",
                    app.memory
                        .brain
                        .short_term
                        .iter()
                        .filter(|e| e.labile_until >= app.memory.brain.clock)
                        .count()
                ),
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "--- Keys ---",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("j/k", Style::default().fg(Color::Cyan)),
            Span::raw(" up/down"),
        ]),
        Line::from(vec![
            Span::styled("g/G", Style::default().fg(Color::Cyan)),
            Span::raw(" top/bottom"),
        ]),
        Line::from(vec![
            Span::styled("Tab", Style::default().fg(Color::Cyan)),
            Span::raw(" next view"),
        ]),
        Line::from(vec![
            Span::styled("1-3", Style::default().fg(Color::Cyan)),
            Span::raw(" switch view"),
        ]),
        Line::from(vec![
            Span::styled("/", Style::default().fg(Color::Cyan)),
            Span::raw(" search"),
        ]),
        Line::from(vec![
            Span::styled(":", Style::default().fg(Color::Cyan)),
            Span::raw(" query"),
        ]),
        Line::from(vec![
            Span::styled("r", Style::default().fg(Color::Cyan)),
            Span::raw(" reload"),
        ]),
        Line::from(vec![
            Span::styled("Enter", Style::default().fg(Color::Cyan)),
            Span::raw(" drill (Graph)"),
        ]),
        Line::from(vec![
            Span::styled("d", Style::default().fg(Color::Cyan)),
            Span::raw(" details"),
        ]),
        Line::from(vec![
            Span::styled("y", Style::default().fg(Color::Cyan)),
            Span::raw(" copy"),
        ]),
        Line::from(vec![
            Span::styled("b", Style::default().fg(Color::Cyan)),
            Span::raw(" back (Graph)"),
        ]),
        Line::from(vec![
            Span::styled("q", Style::default().fg(Color::Cyan)),
            Span::raw(" quit"),
        ]),
    ];

    let paragraph = Paragraph::new(stats);
    f.render_widget(paragraph, inner);
}

fn render_main_panel(f: &mut Frame, app: &mut App, area: Rect) {
    // Split: tabs + content + input/status
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // tabs
            Constraint::Min(10),   // content
            Constraint::Length(3), // input/status
        ])
        .split(area);

    render_tabs(f, app, chunks[0]);

    // Check for detail mode first
    if app.input_mode == InputMode::Detail {
        if let Some(ref content) = app.detail_content {
            render_detail(f, content, chunks[1]);
        }
    } else if let Some(ref result) = app.last_query_result {
        // If we have a query result, show it
        render_query_result(f, result, chunks[1]);
    } else {
        // Otherwise show the list view
        match app.view {
            View::ShortTerm => render_short_term(f, app, chunks[1]),
            View::Graph => render_graph(f, app, chunks[1]),
            View::Events => render_events(f, app, chunks[1]),
        }
    }

    render_input_bar(f, app, chunks[2]);
}

fn render_tabs(f: &mut Frame, app: &App, area: Rect) {
    let tabs: Vec<Span> = [View::ShortTerm, View::Graph, View::Events]
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let num = format!("[{}] ", i + 1);
            let label = v.label();
            if v == app.view {
                Span::styled(
                    format!("{}{}", num, label),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(
                    format!("{}{}", num, label),
                    Style::default().fg(Color::DarkGray),
                )
            }
        })
        .collect();

    let tabs_line = Line::from(
        tabs.into_iter()
            .flat_map(|s| vec![s, Span::raw("  ")])
            .collect::<Vec<_>>(),
    );

    let block = Block::default().borders(Borders::ALL);
    let paragraph = Paragraph::new(tabs_line).block(block);
    f.render_widget(paragraph, area);
}

fn render_short_term(f: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .memory
        .brain
        .short_term
        .iter()
        .map(|entry| {
            let labile_marker = if entry.labile_until >= app.memory.brain.clock {
                Span::styled(" [L]", Style::default().fg(Color::Magenta))
            } else {
                Span::raw("")
            };

            let recon_marker = if entry.reconsolidation_count > 0 {
                Span::styled(
                    format!(" R{}", entry.reconsolidation_count),
                    Style::default().fg(Color::Yellow),
                )
            } else {
                Span::raw("")
            };

            let text = truncate(&entry.text, 60);
            let line = Line::from(vec![
                Span::styled(
                    format!("[{:.2}] ", entry.salience),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw(text),
                labile_marker,
                recon_marker,
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Short-Term Memory "),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    f.render_stateful_widget(list, area, &mut app.list_state);
}

fn render_graph(f: &mut Frame, app: &mut App, area: Rect) {
    let graph_items = app.graph_items();
    let title = if let Some(&root_id) = app.graph_stack.last() {
        let label = app
            .memory
            .brain
            .long_term
            .nodes
            .get(&root_id)
            .map(|n| truncate(&n.label, 30))
            .unwrap_or_else(|| format!("{}", root_id));
        format!(" Neighbors of {} ", label)
    } else {
        " Knowledge Graph (by weight) ".to_string()
    };

    let items: Vec<ListItem> = graph_items
        .iter()
        .map(|item| {
            let edge_marker = if let Some(ref edge) = item.edge_type {
                Span::styled(format!(" ({})", edge), Style::default().fg(Color::DarkGray))
            } else {
                Span::raw("")
            };

            let line = Line::from(vec![
                Span::styled(
                    format!("[{}] ", item.kind),
                    Style::default().fg(Color::Blue),
                ),
                Span::raw(truncate(&item.label, 40)),
                edge_marker,
                Span::styled(
                    format!(" w={:.2}", item.weight),
                    Style::default().fg(Color::DarkGray),
                ),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    f.render_stateful_widget(list, area, &mut app.list_state);
}

fn render_events(f: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .events
        .iter()
        .rev()
        .map(|event| {
            // Color-code by command type
            let cmd_color = match event.command.as_str() {
                "tick" => Color::Green,
                "query" => Color::Blue,
                "start" => Color::Magenta,
                "reinforce" => Color::Yellow,
                "consolidate" | "auto_consolidate" => Color::Rgb(255, 130, 80),
                "reset" => Color::Red,
                "task_set" | "task_clear" => Color::Cyan,
                _ => Color::Gray,
            };

            // Build summary based on rich data if available
            let summary = match &event.data {
                Some(EventData::Tick(t)) => {
                    let id_str = t.entry_id.map(|id| format!("#{}", id)).unwrap_or_default();
                    let match_count = t.matches.len();
                    let graph_count = t.graph_nodes.len();
                    format!(
                        "{} {} matches, {} graph nodes",
                        id_str, match_count, graph_count
                    )
                }
                Some(EventData::Query(q)) => {
                    let top_sim = q
                        .matches
                        .first()
                        .map(|m| format!("{:.0}%", m.similarity * 100.0))
                        .unwrap_or_else(|| "0%".to_string());
                    format!(
                        "{} matches (top: {}), {} primed",
                        q.matches.len(),
                        top_sim,
                        q.primed_count
                    )
                }
                Some(EventData::Reinforce(r)) => {
                    format!(
                        "sig={:.1} {} entries, {} graph",
                        r.signal,
                        r.entries.len(),
                        r.graph_nodes_affected
                    )
                }
                Some(EventData::Consolidate(c)) => {
                    format!("{} groups merged", c.groups_merged)
                }
                Some(EventData::Start(s)) => {
                    format!(
                        "clock={} st={} lt={}",
                        s.clock, s.short_term_count, s.long_term_nodes
                    )
                }
                None => truncate(&event.detail, 40),
            };

            let line = Line::from(vec![
                Span::styled(
                    format!("[{}] ", event.timestamp),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("[{}] ", event.command),
                    Style::default().fg(cmd_color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(summary),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Command Events (newest first) - Press Enter for details "),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    f.render_stateful_widget(list, area, &mut app.list_state);
}

fn render_query_result(f: &mut Frame, result: &str, area: Rect) {
    let paragraph = Paragraph::new(result)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Query Result (press any key to dismiss) "),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(paragraph, area);
}

fn render_detail(f: &mut Frame, content: &str, area: Rect) {
    let paragraph = Paragraph::new(content)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Detail View (Esc to close) "),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

fn render_input_bar(f: &mut Frame, app: &App, area: Rect) {
    let (prompt, style) = match app.input_mode {
        InputMode::Normal => {
            let msg = app.status_message.as_deref().unwrap_or(
                "Press ':' to query, '/' to search, Enter to drill (Graph) or view details",
            );
            (msg.to_string(), Style::default().fg(Color::DarkGray))
        }
        InputMode::Search => (
            format!("/{}", app.input_buffer),
            Style::default().fg(Color::Yellow),
        ),
        InputMode::Query => (
            format!(":{}", app.input_buffer),
            Style::default().fg(Color::Cyan),
        ),
        InputMode::Detail => (
            "Press Esc/Enter to close  y to copy".to_string(),
            Style::default().fg(Color::Green),
        ),
    };

    let paragraph = Paragraph::new(prompt)
        .style(style)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(paragraph, area);
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}
