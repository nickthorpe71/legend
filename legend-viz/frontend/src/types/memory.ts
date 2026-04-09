export interface BrainSnapshot {
  clock: number;
  l1_count: number;
  l2_count: number;
  l3_node_count: number;
  l3_edge_count: number;
  ticks_since_consolidation: number;
  recent_valence_sum: number;
  next_id: number;
  keyword_cache_size: number;
  embedding_dim: number;
  immediate_capacity: number;
}

export interface WorkingMemoryEntry {
  id: number;
  text: string;
  embedding: number[];
  salience: number;
  emotional_valence: number;
  promoted: boolean;
  rehearsal_count: number;
}

export interface ShortTermEntry {
  id: number;
  text: string;
  embedding: number[];
  salience: number;
  usage: number;
  last_access: number;
  created_at: number;
  stability: number;
  last_retrieval_interval: number;
  labile_until: number;
  consolidated: boolean;
  emotional_valence: number;
  refs: MemoryRef[];
  summary: string | null;
}

export interface MemoryRef {
  id: string;
  kind: string;
}

export interface GraphMemory {
  nodes: Record<string, GraphNode>;
  edges: GraphEdge[];
  index: Record<string, number>;
}

export interface GraphNode {
  id: number;
  label: string;
  kind: string;
  weight: number;
  last_seen: number;
  salience: number;
  source_texts: string[];
  embedding: number[];
  full_text: string | null;
}

export interface GraphEdge {
  source: number;
  target: number;
  kind: string;
  weight: number;
  last_seen: number;
}
