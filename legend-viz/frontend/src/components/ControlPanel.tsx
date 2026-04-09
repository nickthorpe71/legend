import { useState } from 'react';
import { useMemoryStore } from '../stores/memoryStore';
import { useTraceStore } from '../stores/traceStore';

export function ControlPanel() {
  const [tickText, setTickText] = useState('');
  const [queryText, setQueryText] = useState('');
  const refreshAll = useMemoryStore((s) => s.refreshAll);
  const clearTrace = useTraceStore((s) => s.clear);

  const send = async (url: string, body: object) => {
    await fetch(url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    await refreshAll();
  };

  const handleTick = async () => {
    if (!tickText.trim()) return;
    await send('/api/tick', { text: tickText });
    setTickText('');
  };

  const handleQuery = async () => {
    if (!queryText.trim()) return;
    await send('/api/query', { query: queryText });
    setQueryText('');
  };

  return (
    <div className="flex flex-col gap-3 p-3">
      <div className="section-header text-[var(--amber)]">Controls</div>

      <div className="flex flex-col gap-1">
        <label className="text-[10px] uppercase tracking-widest text-[var(--text)]">Tick</label>
        <textarea
          className="w-full h-32 resize-none"
          placeholder="Enter a thought..."
          value={tickText}
          onChange={(e) => setTickText(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter' && e.metaKey) handleTick(); }}
        />
        <button onClick={handleTick}>Tick</button>
      </div>

      <div className="flex flex-col gap-1">
        <label className="text-[10px] uppercase tracking-widest text-[var(--text)]">Query</label>
        <input
          type="text"
          placeholder="Search memory..."
          value={queryText}
          onChange={(e) => setQueryText(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter') handleQuery(); }}
        />
        <button onClick={handleQuery}>Query</button>
      </div>

      <div className="flex gap-2">
        <button className="flex-1" onClick={() => send('/api/consolidate', {})}>Consolidate</button>
        <button className="flex-1" onClick={() => { send('/api/reset', {}); clearTrace(); }}>Reset</button>
      </div>
    </div>
  );
}
