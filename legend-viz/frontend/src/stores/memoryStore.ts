import { create } from 'zustand';
import type { BrainSnapshot, WorkingMemoryEntry, ShortTermEntry, GraphMemory } from '../types/memory';

interface MemoryState {
  snapshot: BrainSnapshot | null;
  l1: WorkingMemoryEntry[];
  l2: ShortTermEntry[];
  l3: GraphMemory | null;
  loading: boolean;

  setSnapshot: (snapshot: BrainSnapshot) => void;
  setL1: (entries: WorkingMemoryEntry[]) => void;
  setL2: (entries: ShortTermEntry[]) => void;
  setL3: (graph: GraphMemory) => void;
  setLoading: (loading: boolean) => void;
  refreshAll: () => Promise<void>;
}

async function fetchJson<T>(url: string): Promise<T> {
  const res = await fetch(url);
  return res.json();
}

export const useMemoryStore = create<MemoryState>((set) => ({
  snapshot: null,
  l1: [],
  l2: [],
  l3: null,
  loading: false,

  setSnapshot: (snapshot) => set({ snapshot }),
  setL1: (l1) => set({ l1 }),
  setL2: (l2) => set({ l2 }),
  setL3: (l3) => set({ l3 }),
  setLoading: (loading) => set({ loading }),

  refreshAll: async () => {
    set({ loading: true });
    try {
      const [snapshot, l1, l2, l3] = await Promise.all([
        fetchJson<BrainSnapshot>('/api/state'),
        fetchJson<WorkingMemoryEntry[]>('/api/state/l1'),
        fetchJson<ShortTermEntry[]>('/api/state/l2'),
        fetchJson<GraphMemory>('/api/state/l3'),
      ]);
      set({ snapshot, l1, l2, l3, loading: false });
    } catch {
      set({ loading: false });
    }
  },
}));
