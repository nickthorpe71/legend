import { useCallback } from 'react';
import { useTraceStore } from '../stores/traceStore';

/** Provides step-through controls for the timeline. */
export function useTimeline() {
  const position = useTraceStore((s) => s.position);
  const total = useTraceStore((s) => s.events.length);
  const setPosition = useTraceStore((s) => s.setPosition);
  const setPlayback = useTraceStore((s) => s.setPlayback);
  const selectEvent = useTraceStore((s) => s.selectEvent);

  const stepForward = useCallback(() => {
    const { events, position: pos } = useTraceStore.getState();
    if (pos < events.length - 1) {
      const next = pos + 1;
      setPosition(next);
      selectEvent(events[next]);
    }
  }, [setPosition, selectEvent]);

  const stepBackward = useCallback(() => {
    const { events, position: pos } = useTraceStore.getState();
    if (pos > 0) {
      const prev = pos - 1;
      setPosition(prev);
      selectEvent(events[prev]);
    }
  }, [setPosition, selectEvent]);

  const goLive = useCallback(() => {
    const { events } = useTraceStore.getState();
    setPlayback('live');
    if (events.length > 0) {
      selectEvent(events[events.length - 1]);
    }
  }, [setPlayback, selectEvent]);

  return { stepForward, stepBackward, goLive, position, total };
}
