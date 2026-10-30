// The live connection to the daemon (FR-290): one event stream for change notifications,
// diagnostics events and log records. Events and records are kept here, bounded, so that the
// diagnostics and logs views show everything since the daemon started (MVP-044).

import { useSyncExternalStore } from 'react';
import { call, setClientId } from './api';
import type { DiagEvent, Diagnostics, LogRecord, Logs } from './types';

export type Connection = 'connecting' | 'live' | 'reconnecting' | 'stopped' | 'signed-out';

export interface LiveState {
  connection: Connection;
  events: DiagEvent[];
  logs: LogRecord[];
  /** Events the daemon dropped before this page saw them. */
  droppedEvents: number;
  droppedLogs: number;
  daemon: { pid: number; startedAt: string } | null;
}

const MAX_EVENTS = 10000;
const MAX_LOGS = 5000;

let state: LiveState = {
  connection: 'connecting',
  events: [],
  logs: [],
  droppedEvents: 0,
  droppedLogs: 0,
  daemon: null,
};
const listeners = new Set<() => void>();
let pending = false;

function emit() {
  if (pending) return;
  pending = true;
  setTimeout(() => {
    pending = false;
    listeners.forEach((l) => l());
  }, 50);
}

function set(next: Partial<LiveState>) {
  state = { ...state, ...next };
  emit();
}

function subscribe(l: () => void) {
  listeners.add(l);
  return () => listeners.delete(l);
}

export function useLive(): LiveState {
  return useSyncExternalStore(subscribe, () => state);
}

export function getLive(): LiveState {
  return state;
}

/** Merge `incoming` into `current` by sequence number, keeping at most `max`. */
export function mergeBySeq<T extends { seq: number }>(current: T[], incoming: T[], max: number): T[] {
  if (incoming.length === 0) return current;
  const last = current.length ? current[current.length - 1].seq : 0;
  let merged: T[];
  if (incoming.every((x) => x.seq > last)) {
    merged = current.concat(incoming);
  } else {
    const bySeq = new Map<number, T>();
    for (const x of current) bySeq.set(x.seq, x);
    for (const x of incoming) bySeq.set(x.seq, x);
    merged = [...bySeq.values()].sort((a, b) => a.seq - b.seq);
  }
  return merged.length > max ? merged.slice(merged.length - max) : merged;
}

async function seed() {
  try {
    const [d, l] = await Promise.all([
      call<Diagnostics>('diagnostics', {}),
      call<Logs>('logs', {}),
    ]);
    set({
      events: mergeBySeq(state.events, d.events, MAX_EVENTS),
      logs: mergeBySeq(state.logs, l.records, MAX_LOGS),
      droppedEvents: d.dropped,
      droppedLogs: l.dropped,
    });
  } catch {
    // The stream is up; the next reconnect seeds again.
  }
}

let source: EventSource | null = null;
let retry: ReturnType<typeof setTimeout> | null = null;

/** Open the stream. `onChanged` runs for every change notification, to refresh views. */
export function connect(onChanged: () => void) {
  if (source) return;
  const es = new EventSource('/api/events', { withCredentials: true });
  source = es;
  es.addEventListener('hello', (e) => {
    const hello = JSON.parse((e as MessageEvent).data) as {
      client: number;
      pid: number;
      startedAt: string;
    };
    setClientId(hello.client);
    const another = state.daemon && (state.daemon.pid !== hello.pid || state.daemon.startedAt !== hello.startedAt);
    if (another) {
      // A new daemon: sequence numbers started again.
      state = { ...state, events: [], logs: [], droppedEvents: 0, droppedLogs: 0 };
    }
    set({ connection: 'live', daemon: { pid: hello.pid, startedAt: hello.startedAt } });
    void seed();
    if (another) onChanged();
  });
  es.addEventListener('event', (e) => {
    const ev = JSON.parse((e as MessageEvent).data) as DiagEvent;
    set({ events: mergeBySeq(state.events, [ev], MAX_EVENTS) });
  });
  es.addEventListener('log', (e) => {
    const r = JSON.parse((e as MessageEvent).data) as LogRecord;
    set({ logs: mergeBySeq(state.logs, [r], MAX_LOGS) });
  });
  es.addEventListener('changed', () => onChanged());
  es.addEventListener('reset', () => {
    state = { ...state, events: [], logs: [] };
    void seed();
    onChanged();
  });
  es.addEventListener('bye', () => set({ connection: 'stopped' }));
  es.onerror = () => {
    if (es.readyState === EventSource.CLOSED) {
      // The server refused the stream (not signed in), or it went away for good.
      source = null;
      void call('health').then(
        () => scheduleReconnect(onChanged),
        (err: { unauthorized?: boolean }) => {
          if (err?.unauthorized) set({ connection: 'signed-out' });
          else {
            if (state.connection !== 'stopped') set({ connection: 'reconnecting' });
            scheduleReconnect(onChanged);
          }
        },
      );
    } else if (state.connection === 'live') {
      set({ connection: 'reconnecting' });
    }
  };
}

function scheduleReconnect(onChanged: () => void) {
  if (retry) return;
  retry = setTimeout(() => {
    retry = null;
    connect(onChanged);
  }, 3000);
}

/** For tests: forget everything. */
export function resetLive() {
  source?.close();
  source = null;
  state = { connection: 'connecting', events: [], logs: [], droppedEvents: 0, droppedLogs: 0, daemon: null };
}
