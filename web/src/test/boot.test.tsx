import { describe, expect, it, vi } from 'vitest';
import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App } from '../App';
import { BOOT_WAIT_MS, bootPhase, describeState } from '../boot';
import type { Health } from '../types';
import { mockApi } from './api';
import { renderAt } from './render';
import { resetLive } from '../live';

const health = (over: Partial<Health> = {}): Health => ({
  state: 'reconciling',
  ready: false,
  pid: 1,
  root: '/tmp/repo',
  endpoint: 'sock',
  version: '0',
  startedAt: 't',
  ingestWatermark: 0,
  eventsReceived: 0,
  pendingEvents: 0,
  lagMs: 0,
  lastIngest: null,
  lastError: null,
  connections: 1,
  subscribers: 0,
  requestsServed: 0,
  ...over,
});

const session = {
  actor: 'a',
  root: '/tmp/repo',
  repository: 'repo',
  branch: 'main',
  ui: null,
  bundled: true,
};

function renderApp(path = '/spec') {
  return renderAt(path, [{ path: '/*', element: <App /> }]);
}

describe('boot phase', () => {
  it('waits 10 seconds, then says startup is taking longer, and leaves once the daemon is ready', () => {
    expect(bootPhase(false, 0, false)).toBe('waiting');
    expect(bootPhase(false, BOOT_WAIT_MS - 1, false)).toBe('waiting');
    expect(bootPhase(false, BOOT_WAIT_MS, false)).toBe('delayed');
    expect(bootPhase(true, 0, false)).toBe('ready');
    expect(bootPhase(false, 1_000, true)).toBe('ready');
    expect(describeState('reconciling')).toBe('reconciling the repository');
    expect(describeState(undefined)).toBe('not answering yet');
  });
});

describe('the startup screen', () => {
  it('holds the spec page until the daemon is ready, and opens logs without fetching spec items', async () => {
    resetLive();
    const calls = mockApi({
      health: () => health(),
      session: () => session,
    });
    const user = userEvent.setup();
    renderApp();
    const boot = await screen.findByRole('status', { name: 'Starting Resonance' });
    await waitFor(() => expect(boot).toHaveTextContent('reconciling the repository'));
    expect(within(boot).getByRole('button', { name: 'Logs' })).toBeInTheDocument();
    expect(within(boot).getByRole('button', { name: 'Diagnostics' })).toBeInTheDocument();
    expect(screen.queryByText(/No spec items yet/)).toBeNull();
    await waitFor(() => expect(calls.some((c) => c.method === 'health')).toBe(true));
    expect(calls.some((c) => c.method === 'spec_list')).toBe(false);

    await user.click(within(boot).getByRole('button', { name: 'Logs' }));
    expect(await screen.findByRole('heading', { name: 'Logs' })).toBeInTheDocument();
    expect(calls.some((c) => c.method === 'spec_list')).toBe(false);
  });

  it('shows the spec once the daemon is ready', async () => {
    resetLive();
    mockApi({
      health: () => health({ ready: true, state: 'ready' }),
      session: () => session,
      status: () => ({ pendingChanges: 0, stage: '1', participation: 'offline' }),
      spec_list: () => [],
    });
    renderApp();
    expect(await screen.findByText(/No spec items yet/)).toBeInTheDocument();
    expect(screen.queryByRole('status', { name: 'Starting Resonance' })).toBeNull();
  });

  it('after 10 seconds explains that startup can take longer', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      resetLive();
      mockApi({
        health: () => health({ state: 'reconciling', pendingEvents: 4, lagMs: 2000 }),
        session: () => session,
      });
      renderApp();
      expect(await screen.findByText(/so spec items are not loaded yet/)).toBeInTheDocument();
      await vi.advanceTimersByTimeAsync(BOOT_WAIT_MS + 200);
      expect(screen.getByRole('status', { name: 'Starting Resonance' })).toHaveTextContent(
        'can take longer than 10 seconds',
      );
      expect(screen.getByRole('button', { name: 'Open the spec anyway' })).toBeInTheDocument();
      expect(screen.getByText(/4 file events still waiting/)).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });
});
