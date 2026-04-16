import { useEffect } from 'react';
import { useMemoryStore } from '../stores/memoryStore';

export function BrainStats() {
  const { snapshot, refreshAll } = useMemoryStore();

  useEffect(() => { refreshAll(); }, []);

  if (!snapshot) return <div className="p-3 text-[var(--text)]">Loading...</div>;

  const stats = [
    { label: 'Clock', value: snapshot.clock, color: 'var(--amber)' },
    { label: 'L1 (WM)', value: snapshot.l1_count, color: 'var(--cyan)' },
    { label: 'L2 (Episodic)', value: snapshot.l2_count, color: 'var(--green)' },
    { label: 'L3 Nodes', value: snapshot.l3_node_count, color: 'var(--green)' },
    { label: 'L3 Edges', value: snapshot.l3_edge_count, color: 'var(--green)' },
    { label: 'Ticks/Consol', value: snapshot.ticks_since_consolidation, color: 'var(--orange)' },
    { label: 'Consol Pressure', value: snapshot.consolidation_pressure.toFixed(2), color: 'var(--rose)' },
    { label: 'Next ID', value: snapshot.next_id, color: 'var(--text)' },
    { label: 'Keywords', value: snapshot.keyword_cache_size, color: 'var(--text)' },
    { label: 'Embed Dim', value: snapshot.embedding_dim, color: 'var(--text)' },
    { label: 'WM Capacity', value: snapshot.immediate_capacity, color: 'var(--text)' },
  ];

  return (
    <div className="p-3 border-t border-[var(--border)]">
      <div className="section-header text-[var(--amber)]">Brain Stats</div>
      <div className="flex flex-col gap-1">
        {stats.map((s) => (
          <div key={s.label} className="flex justify-between items-center">
            <span className="text-[10px] uppercase tracking-widest">{s.label}</span>
            <span className="text-sm font-medium" style={{ color: s.color }}>{s.value}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
