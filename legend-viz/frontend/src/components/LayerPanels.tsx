import { useState } from 'react';
import { L1Panel } from './L1Panel';
import { L2Panel } from './L2Panel';
import { L3Panel } from './L3Panel';

type LayerTab = 'l1' | 'l2' | 'l3';

export function LayerPanels() {
  const [tab, setTab] = useState<LayerTab>('l1');

  const tabs: { label: string; value: LayerTab; color: string }[] = [
    { label: 'L1', value: 'l1', color: 'var(--cyan)' },
    { label: 'L2', value: 'l2', color: 'var(--green)' },
    { label: 'L3', value: 'l3', color: 'var(--green)' },
  ];

  return (
    <div className="flex flex-col h-full">
      <div className="flex border-b border-[var(--border)]">
        {tabs.map((t) => (
          <button
            key={t.value}
            className={`flex-1 px-2 py-1 text-[10px] border-0 border-b-2 ${
              tab === t.value
                ? `border-b-[${t.color}]`
                : 'border-b-transparent'
            }`}
            style={{ color: tab === t.value ? t.color : 'var(--text)' }}
            onClick={() => setTab(t.value)}
          >
            {t.label}
          </button>
        ))}
      </div>
      <div className="flex-1 overflow-hidden">
        {tab === 'l1' && <L1Panel />}
        {tab === 'l2' && <L2Panel />}
        {tab === 'l3' && <L3Panel />}
      </div>
    </div>
  );
}
