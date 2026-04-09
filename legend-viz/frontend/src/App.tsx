import { useWebSocket } from './hooks/useWebSocket';
import { ControlPanel } from './components/ControlPanel';
import { BrainStats } from './components/BrainStats';
import { TimelineScrubber } from './components/TimelineScrubber';
import { StepDetail } from './components/StepDetail';
import { LayerPanels } from './components/LayerPanels';

function App() {
  useWebSocket();

  return (
    <div className="flex h-screen w-screen overflow-hidden">
      {/* Left sidebar — controls + stats */}
      <div className="w-[300px] flex-shrink-0 border-r border-[var(--border)] flex flex-col overflow-y-auto">
        <div className="p-3 border-b border-[var(--border)]">
          <h1 className="text-[var(--amber)] text-sm uppercase tracking-[3px] m-0">Legend Viz</h1>
          <span className="text-[10px] text-[var(--text)]">Brain Debugger</span>
        </div>
        <ControlPanel />
        <BrainStats />
      </div>

      {/* Main area — layers + trace detail + timeline */}
      <div className="flex-1 flex flex-col min-w-0">
        {/* Layer panels (top) */}
        <div className="flex-1 min-h-0 overflow-hidden">
          <LayerPanels />
        </div>

        {/* Trace detail (bottom) */}
        <div className="border-t border-[var(--border)]">
          <StepDetail />
        </div>

        {/* Timeline scrubber */}
        <div className="border-t border-[var(--border)]">
          <TimelineScrubber />
        </div>
      </div>
    </div>
  );
}

export default App;
