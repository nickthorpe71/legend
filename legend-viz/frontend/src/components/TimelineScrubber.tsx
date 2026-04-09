import { useTraceStore } from '../stores/traceStore';
import { useTimeline } from '../hooks/useTimeline';
import { getStepColor } from '../types/trace';

export function TimelineScrubber() {
  const events = useTraceStore((s) => s.events);
  const position = useTraceStore((s) => s.position);
  const playback = useTraceStore((s) => s.playback);
  const { stepForward, stepBackward, goLive } = useTimeline();
  const setPosition = useTraceStore((s) => s.setPosition);
  const selectEvent = useTraceStore((s) => s.selectEvent);

  return (
    <div className="panel panel-amber p-2 flex flex-col gap-1">
      <div className="flex items-center gap-2">
        <div className="flex gap-1">
          <button className="px-2 py-1 text-[10px]" onClick={stepBackward}>&lt;</button>
          <button className="px-2 py-1 text-[10px]" onClick={goLive}>
            {playback === 'live' ? 'LIVE' : 'GO LIVE'}
          </button>
          <button className="px-2 py-1 text-[10px]" onClick={stepForward}>&gt;</button>
        </div>
        <span className="text-[10px] text-[var(--text)] uppercase tracking-widest ml-2">
          {events.length > 0 ? `${position + 1} / ${events.length}` : 'No events'}
        </span>
        {playback === 'live' && (
          <span className="text-[10px] text-[var(--green)] ml-auto animate-pulse">● LIVE</span>
        )}
      </div>

      {/* Timeline bar */}
      <div className="relative h-6 bg-[var(--bg)] border border-[var(--border)] overflow-hidden">
        {events.map((event, i) => (
          <div
            key={i}
            className="absolute top-0 bottom-0 cursor-pointer hover:opacity-100 opacity-70"
            style={{
              left: `${(i / Math.max(events.length - 1, 1)) * 100}%`,
              width: '2px',
              background: getStepColor(event.step),
            }}
            onClick={() => {
              setPosition(i);
              selectEvent(event);
            }}
          />
        ))}
        {/* Playhead */}
        {events.length > 0 && (
          <div
            className="absolute top-0 bottom-0 w-[2px] bg-[var(--amber)] z-10"
            style={{
              left: `${(position / Math.max(events.length - 1, 1)) * 100}%`,
              boxShadow: '0 0 6px var(--amber)',
            }}
          />
        )}
      </div>

      {/* Scrub slider */}
      {events.length > 1 && (
        <input
          type="range"
          min={0}
          max={events.length - 1}
          value={position}
          onChange={(e) => {
            const idx = parseInt(e.target.value);
            setPosition(idx);
            selectEvent(events[idx]);
          }}
          className="w-full h-1 appearance-none bg-[var(--border)] cursor-pointer [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:w-2 [&::-webkit-slider-thumb]:h-4 [&::-webkit-slider-thumb]:bg-[var(--amber)]"
        />
      )}
    </div>
  );
}
