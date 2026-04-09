import { useMemoryStore } from '../stores/memoryStore';

export function L2Panel() {
  const l2 = useMemoryStore((s) => s.l2);

  return (
    <div className="p-3 overflow-y-auto max-h-full">
      <div className="section-header text-[var(--green)]">L2 — Episodic Memory</div>
      <table className="w-full text-[11px]">
        <thead>
          <tr className="text-left text-[var(--text)] uppercase tracking-wider">
            <th className="pb-1">ID</th>
            <th className="pb-1">Salience</th>
            <th className="pb-1">Usage</th>
            <th className="pb-1">Stability</th>
            <th className="pb-1">Valence</th>
            <th className="pb-1">Text</th>
          </tr>
        </thead>
        <tbody>
          {l2.map((entry) => (
            <tr key={entry.id} className="border-t border-[var(--border)] hover:bg-[rgba(48,224,96,0.05)]">
              <td className="py-1 text-[var(--green)]">{entry.id}</td>
              <td className="py-1">{entry.salience.toFixed(2)}</td>
              <td className="py-1">{entry.usage}</td>
              <td className="py-1">{entry.stability.toFixed(1)}</td>
              <td className="py-1" style={{ color: entry.emotional_valence > 0 ? 'var(--green)' : entry.emotional_valence < 0 ? 'var(--rose)' : 'var(--text)' }}>
                {entry.emotional_valence.toFixed(2)}
              </td>
              <td className="py-1 text-[var(--text-bright)] max-w-[200px] truncate">{entry.text}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {l2.length === 0 && <div className="text-[var(--text)] text-[11px] mt-2">Empty</div>}
    </div>
  );
}
