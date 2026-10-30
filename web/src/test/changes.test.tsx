import { describe, expect, it } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ChangeLogPage } from '../pages/ChangeLogPage';
import type { ChangeEntry } from '../types';
import { mockApi } from './api';
import { renderAt } from './render';

const entry = (seq: number, key: string): ChangeEntry => ({
  seq,
  at: '2026-10-30T12:00:00.000Z',
  actor: 'human:t@example.com',
  item: { id: `req:${key}`, key, title: `Item ${key}`, type: 'Requirement' },
  kind: 'edited',
  fields: ['text'],
  summary: `edited ${key}`,
  origin: 'ui',
  opIds: [],
});

describe('the change log', () => {
  it('clears one entry, and all after a confirmation', async () => {
    let log = [entry(3, 'FR-001'), entry(5, 'FR-002')];
    const calls = mockApi({
      changes: () => log,
      changes_done: (a) => {
        const seqs = a.seqs as number[];
        log = log.filter((e) => !seqs.includes(e.seq));
        return { removed: seqs.length };
      },
    });
    const user = userEvent.setup();
    renderAt('/changes', [{ path: '/changes', element: <ChangeLogPage /> }]);
    expect(await screen.findByText('edited FR-001')).toBeInTheDocument();
    await user.click(screen.getAllByRole('button', { name: 'Clear' })[0]);
    await waitFor(() => expect(screen.queryByText('edited FR-001')).toBeNull());
    expect(calls.find((c) => c.method === 'changes_done')?.args).toMatchObject({ seqs: [3] });
    await user.click(screen.getByRole('button', { name: 'Clear all' }));
    expect(await screen.findByText('Clear all 1 entries?')).toBeInTheDocument();
    await user.click(screen.getAllByRole('button', { name: 'Clear all' }).at(-1)!);
    await waitFor(() => expect(screen.getByText(/The change log is empty/)).toBeInTheDocument());
    expect(calls.filter((c) => c.method === 'changes_done').at(-1)?.args).toMatchObject({ seqs: [5] });
  });
});
