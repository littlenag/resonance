// Shown on the spec and change log until the daemon is ready to serve them (FR-047,
// FR-095). Logs and diagnostics stay reachable: they read the daemon's own buffers,
// which answer while startup reconcile is still running.

import { Button, Group, Loader, Progress, Stack, Text, Title } from '@mantine/core';
import { BOOT_WAIT_MS, describeState, type BootPhase } from '../boot';

export function BootScreen({
  phase,
  elapsedMs,
  state,
  pendingEvents,
  lagMs,
  error,
  onLogs,
  onDiagnostics,
  onProceed,
}: {
  phase: Exclude<BootPhase, 'ready'>;
  elapsedMs: number;
  state?: string;
  pendingEvents: number;
  lagMs: number;
  error: string | null;
  onLogs: () => void;
  onDiagnostics: () => void;
  onProceed: () => void;
}) {
  const waiting = phase === 'waiting';
  const seconds = Math.max(0, Math.floor(elapsedMs / 1000));
  const where = describeState(state);
  return (
    <Stack
      align="center"
      justify="center"
      gap="md"
      h="100%"
      maw={560}
      mx="auto"
      role="status"
      aria-live="polite"
      aria-label="Starting Resonance"
    >
      <Loader size="lg" />
      <Title order={2}>{waiting ? 'Starting Resonance' : 'Still starting'}</Title>
      <Text ta="center">
        {waiting
          ? `The page is up. The daemon is still ${where}, so spec items are not loaded yet.`
          : `Startup can take longer than 10 seconds on a large repository. The daemon has been ${where} for ${seconds}s.`}
      </Text>
      {waiting ? (
        <Progress
          value={Math.min(100, (elapsedMs / BOOT_WAIT_MS) * 100)}
          w="100%"
          aria-label="Time waited"
        />
      ) : (
        <Progress value={100} animated w="100%" aria-label="Still waiting for the daemon" />
      )}
      <Text size="sm" c="dimmed">
        {waiting ? `${seconds}s of ${BOOT_WAIT_MS / 1000}s` : 'Waiting for the daemon to become ready.'}
        {pendingEvents > 0 &&
          ` ${pendingEvents} file event${pendingEvents === 1 ? '' : 's'} still waiting${
            lagMs >= 1000 ? ` (oldest ${Math.round(lagMs / 1000)}s)` : ''
          }.`}
      </Text>
      {error && (
        <Text size="sm" c="red" ta="center">
          {error}
        </Text>
      )}
      <Group>
        <Button variant="default" onClick={onLogs}>
          Logs
        </Button>
        <Button variant="default" onClick={onDiagnostics}>
          Diagnostics
        </Button>
        {!waiting && <Button onClick={onProceed}>Open the spec anyway</Button>}
      </Group>
    </Stack>
  );
}
