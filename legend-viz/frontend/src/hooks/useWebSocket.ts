import { useEffect, useRef } from 'react';
import type { TraceEvent } from '../types/trace';
import { useTraceStore } from '../stores/traceStore';

export function useWebSocket() {
  const wsRef = useRef<WebSocket | null>(null);
  const reconnectTimer = useRef<ReturnType<typeof setTimeout>>(undefined);

  useEffect(() => {
    let disposed = false;

    function connect() {
      if (disposed) return;
      // Close any existing connection first
      if (wsRef.current) {
        wsRef.current.onclose = null; // prevent reconnect from stale close
        wsRef.current.close();
        wsRef.current = null;
      }

      const wsUrl = window.location.port === '5173'
        ? 'ws://127.0.0.1:3333/ws'
        : `${window.location.protocol === 'https:' ? 'wss:' : 'ws:'}://${window.location.host}/ws`;
      const ws = new WebSocket(wsUrl);

      ws.onmessage = (e) => {
        try {
          const event: TraceEvent = JSON.parse(e.data);
          if (event.step) useTraceStore.getState().addEvent(event);
        } catch { /* ignore non-trace messages */ }
      };

      ws.onclose = () => {
        if (!disposed) {
          reconnectTimer.current = setTimeout(connect, 2000);
        }
      };

      ws.onerror = () => ws.close();
      wsRef.current = ws;
    }

    connect();
    return () => {
      disposed = true;
      clearTimeout(reconnectTimer.current);
      if (wsRef.current) {
        wsRef.current.onclose = null; // prevent reconnect on cleanup close
        wsRef.current.close();
        wsRef.current = null;
      }
    };
  }, []);
}
