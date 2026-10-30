import { describe, expect, it } from 'vitest';
import { mergeBySeq } from '../live';
import { filterLogs, logLine } from '../pages/LogsPage';
import { filterEvents } from '../pages/DiagnosticsPage';
import type { DiagEvent, LogRecord } from '../types';

const rec = (seq: number, level: LogRecord['level'], component: string, message: string): LogRecord => ({
  seq,
  at: '2026-10-30T12:00:00.000Z',
  level,
  component,
  message,
});

describe('mergeBySeq', () => {
  it('appends, deduplicates, orders and bounds', () => {
    const a = [{ seq: 1 }, { seq: 2 }];
    expect(mergeBySeq(a, [{ seq: 3 }], 10).map((x) => x.seq)).toEqual([1, 2, 3]);
    expect(mergeBySeq(a, [{ seq: 2 }, { seq: 0 }], 10).map((x) => x.seq)).toEqual([0, 1, 2]);
    expect(mergeBySeq(a, [{ seq: 3 }, { seq: 4 }], 3).map((x) => x.seq)).toEqual([2, 3, 4]);
    expect(mergeBySeq(a, [], 1)).toBe(a);
  });
});

describe('log filters', () => {
  const logs = [rec(1, 'debug', 'ingest', 'scanned'), rec(2, 'warn', 'ui', 'refused a request'), rec(3, 'error', 'daemon', 'failed')];
  it('filters by level, component and text', () => {
    expect(filterLogs(logs, 'warn', [], '').map((r) => r.seq)).toEqual([2, 3]);
    expect(filterLogs(logs, 'trace', ['ingest'], '').map((r) => r.seq)).toEqual([1]);
    expect(filterLogs(logs, 'trace', [], 'REFUSED').map((r) => r.seq)).toEqual([2]);
  });
  it('renders a line like the log file', () => {
    expect(logLine({ ...rec(1, 'info', 'daemon', 'started'), fields: { pid: 7, root: '/r' } })).toBe(
      '2026-10-30T12:00:00.000Z INFO  daemon: started pid=7 root=/r',
    );
  });
});

describe('event filters', () => {
  const ev = (seq: number, kind: string, summary: string, fields = {}): DiagEvent => ({ seq, at: '', kind, summary, fields });
  it('filters by kind and by text in the summary or fields', () => {
    const events = [ev(1, 'request', 'status ok'), ev(2, 'reconcile', 'watcher: 1 file'), ev(3, 'git', 'caught up', { operation: 'checkout: main' })];
    expect(filterEvents(events, ['git', 'reconcile'], '').map((e) => e.seq)).toEqual([2, 3]);
    expect(filterEvents(events, [], 'checkout').map((e) => e.seq)).toEqual([3]);
  });
});
