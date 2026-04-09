import { motion, AnimatePresence } from 'framer-motion';
import { useMemoryStore } from '../stores/memoryStore';

export function L1Panel() {
  const l1 = useMemoryStore((s) => s.l1);

  return (
    <div className="p-3 overflow-y-auto max-h-full">
      <div className="section-header text-[var(--cyan)]">L1 — Working Memory</div>
      <div className="flex flex-col gap-2">
        <AnimatePresence>
          {l1.map((entry) => (
            <motion.div
              key={entry.id}
              initial={{ opacity: 0, y: -8 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: 8 }}
              className="panel panel-cyan p-2"
            >
              <div className="flex justify-between text-[10px] mb-1">
                <span className="text-[var(--cyan)]">#{entry.id}</span>
                <span>sal: {entry.salience.toFixed(2)}</span>
                {entry.promoted && <span className="text-[var(--green)]">PROMOTED</span>}
              </div>
              <div className="text-[12px] text-[var(--text-bright)] break-words">
                {entry.text.slice(0, 120)}
              </div>
            </motion.div>
          ))}
        </AnimatePresence>
        {l1.length === 0 && <div className="text-[var(--text)] text-[11px]">Empty</div>}
      </div>
    </div>
  );
}
