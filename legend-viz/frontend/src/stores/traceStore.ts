import { create } from 'zustand';
import type { TraceEvent } from '../types/trace';

export type PlaybackState = 'live' | 'paused' | 'scrubbing';

interface TraceState {
  events: TraceEvent[];
  /** Current timeline position (index into events array) */
  position: number;
  playback: PlaybackState;
  speed: number;
  selectedEvent: TraceEvent | null;
  /** Which trace_id group is currently being viewed */
  activeTraceId: number | null;

  addEvent: (event: TraceEvent) => void;
  addEvents: (events: TraceEvent[]) => void;
  setPosition: (pos: number) => void;
  setPlayback: (state: PlaybackState) => void;
  setSpeed: (speed: number) => void;
  selectEvent: (event: TraceEvent | null) => void;
  clear: () => void;
}

export const useTraceStore = create<TraceState>((set) => ({
  events: [],
  position: 0,
  playback: 'live',
  speed: 1,
  selectedEvent: null,
  activeTraceId: null,

  addEvent: (event) =>
    set((s) => {
      const events = [...s.events, event];
      const isLive = s.playback === 'live';
      return {
        events,
        position: isLive ? events.length - 1 : s.position,
        activeTraceId: event.trace_id,
      };
    }),

  addEvents: (newEvents) =>
    set((s) => {
      const events = [...s.events, ...newEvents];
      const isLive = s.playback === 'live';
      return {
        events,
        position: isLive ? events.length - 1 : s.position,
        activeTraceId: newEvents.length > 0 ? newEvents[newEvents.length - 1].trace_id : s.activeTraceId,
      };
    }),

  setPosition: (position) => set({ position, playback: 'scrubbing' }),
  setPlayback: (playback) => set({ playback }),
  setSpeed: (speed) => set({ speed }),
  selectEvent: (selectedEvent) => set({ selectedEvent }),
  clear: () => set({ events: [], position: 0, playback: 'live', selectedEvent: null, activeTraceId: null }),
}));
