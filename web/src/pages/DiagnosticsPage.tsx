// Diagnostics (FR-293–FR-295, MVP-044): the daemon's internals since it started. Identity,
// health, the store, counters, clients, configuration with sources, the activity timeline,
// and the daemon's operational actions.

import { useMemo, useState } from 'react';
import {
  Alert,
  Badge,
  Button,
  Card,
  Group,
  MultiSelect,
  Select,
  SimpleGrid,
  Stack,
  Switch,
  Table,
  Tabs,
  Text,
  TextInput,
  Title,
} from '@mantine/core';
import { modals } from '@mantine/modals';
import { notifications } from '@mantine/notifications';
import { IconDownload, IconPlayerStop, IconRefresh, IconRotate, IconSearch, IconTools } from '@tabler/icons-react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { call, downloadJson } from '../api';
import { getLive, useLive } from '../live';
import type { DiagEvent, Diagnostics, Status } from '../types';
import { StreamList } from '../components/StreamList';

export const EVENT_KINDS = [
  'request',
  'client',
  'reconcile',
  'git',
  'changeset',
  'changelog',
  'divergence',
  'delta',
  'registry',
  'state',
  'action',
  'ui',
];

const KIND_COLORS: Record<string, string> = {
  request: 'gray',
  client: 'cyan',
  reconcile: 'blue',
  git: 'violet',
  changeset: 'green',
  changelog: 'yellow',
  divergence: 'orange',
  delta: 'grape',
  registry: 'pink',
  state: 'indigo',
  action: 'red',
  ui: 'teal',
};

export const LEVELS = ['error', 'warn', 'info', 'debug', 'trace'];

/** Events matching `kinds` (all when empty) and `text`, in the summary or fields. */
export function filterEvents(events: DiagEvent[], kinds: string[], text: string): DiagEvent[] {
  const t = text.trim().toLowerCase();
  return events.filter(
    (e) =>
      (kinds.length === 0 || kinds.includes(e.kind)) &&
      (!t || e.summary.toLowerCase().includes(t) || JSON.stringify(e.fields).toLowerCase().includes(t)),
  );
}

function uptime(s: number) {
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return h ? `${h}h ${m}m` : m ? `${m}m ${s % 60}s` : `${s}s`;
}

function Row({ k, v }: { k: string; v: React.ReactNode }) {
  return (
    <Table.Tr>
      <Table.Td c="dimmed" w={150}>
        {k}
      </Table.Td>
      <Table.Td style={{ wordBreak: 'break-all' }}>{v}</Table.Td>
    </Table.Tr>
  );
}

export function DiagnosticsPage() {
  const queryClient = useQueryClient();
  const live = useLive();
  // The snapshot without events: the stream carries those.
  const diag = useQuery({
    queryKey: ['diagnostics'],
    queryFn: () => call<Diagnostics>('diagnostics', { kinds: ['-'] }),
    refetchInterval: 3000,
  });
  const status = useQuery({
    queryKey: ['status'],
    queryFn: () => call<Status>('status', {}, { cached: true }),
    refetchInterval: 5000,
  });
  const [kinds, setKinds] = useState<string[]>([]);
  const [text, setText] = useState('');
  const [follow, setFollow] = useState(true);
  const events = useMemo(() => filterEvents(live.events, kinds, text), [live.events, kinds, text]);

  const d = diag.data;
  const s = status.data;

  const action = (label: string, method: string, args: Record<string, unknown>, confirm?: string) => {
    const run = async () => {
      try {
        await call(method, args);
        notifications.show({ color: 'green', message: `${label}: done.` });
        await queryClient.invalidateQueries();
      } catch (e) {
        notifications.show({ color: 'red', title: label, message: (e as Error).message });
      }
    };
    if (!confirm) return void run();
    modals.openConfirmModal({
      title: `${label}?`,
      children: <Text size="sm">{confirm}</Text>,
      labels: { confirm: label, cancel: 'Cancel' },
      confirmProps: { color: method === 'daemon_stop' ? 'red' : undefined },
      onConfirm: () => void run(),
    });
  };

  const exportAll = () => {
    const live = getLive();
    downloadJson(`resonance-diagnostics-${new Date().toISOString().replace(/[:.]/g, '-')}.json`, {
      exportedAt: new Date().toISOString(),
      identity: d?.identity,
      health: d?.health,
      config: d?.config,
      counters: d?.counters,
      clients: d?.clients,
      log: d?.log,
      events: live.events,
      droppedEvents: live.droppedEvents,
      logs: live.logs,
      droppedLogs: live.droppedLogs,
    });
  };

  return (
    <Stack gap="md">
      <Group justify="space-between">
        <div>
          <Title order={3}>Diagnostics</Title>
          <Text size="sm" c="dimmed">
            What this daemon has done since it started. Kept in memory only; nothing here outlives the daemon.
          </Text>
        </div>
        <Button variant="light" leftSection={<IconDownload size={16} />} onClick={exportAll}>
          Export JSON
        </Button>
      </Group>
      {diag.error && <Alert color="red">{(diag.error as Error).message}</Alert>}

      <SimpleGrid cols={{ base: 1, md: 3 }}>
        <Card withBorder>
          <Text fw={600} mb="xs">
            Daemon
          </Text>
          <Table fz="xs" verticalSpacing={2}>
            <Table.Tbody>
              <Row k="Process" v={d?.identity.pid} />
              <Row k="Version" v={d?.identity.version} />
              <Row k="Up for" v={d ? uptime(d.identity.uptimeSeconds) : ''} />
              <Row k="Started" v={d?.identity.startedAt} />
              <Row k="Repository" v={<span className="mono">{d?.identity.root}</span>} />
              <Row k="Socket" v={<span className="mono">{d?.identity.endpoint}</span>} />
              <Row k="Web UI" v={<span className="mono">{d?.identity.ui}</span>} />
            </Table.Tbody>
          </Table>
        </Card>
        <Card withBorder>
          <Text fw={600} mb="xs">
            Health
          </Text>
          <Table fz="xs" verticalSpacing={2}>
            <Table.Tbody>
              <Row
                k="State"
                v={
                  <Badge size="sm" color={d?.health.ready ? 'green' : 'orange'}>
                    {d?.health.state}
                  </Badge>
                }
              />
              <Row k="Ingest lag" v={d ? `${d.health.lagMs} ms` : ''} />
              <Row k="Pending events" v={d?.health.pendingEvents} />
              <Row k="Watermark" v={d ? `${d.health.ingestWatermark} / ${d.health.eventsReceived}` : ''} />
              <Row k="Last ingest" v={d?.health.lastIngest ?? '—'} />
              <Row k="Requests" v={d?.health.requestsServed} />
              <Row k="Last error" v={d?.health.lastError ? <Text c="red" size="xs">{d.health.lastError}</Text> : '—'} />
            </Table.Tbody>
          </Table>
        </Card>
        <Card withBorder>
          <Text fw={600} mb="xs">
            Store
          </Text>
          <Table fz="xs" verticalSpacing={2}>
            <Table.Tbody>
              <Row k="Stage" v={s ? `${s.stage} (${s.participation})` : ''} />
              <Row k="Files" v={s?.counts.files} />
              <Row k="Requirements" v={s ? `${s.counts.requirements} (${s.counts.requirementsRetired} retired)` : ''} />
              <Row k="Decisions" v={s?.counts.decisions} />
              <Row k="Code units / tests" v={s ? `${s.counts.codeUnits} / ${s.counts.tests}` : ''} />
              <Row
                k="Links"
                v={s ? Object.entries(s.counts.links).map(([k, n]) => `${k} ${n}`).join(', ') || '0' : ''}
              />
              <Row k="Open delta" v={s ? `${s.history.openOperations} operation(s)` : ''} />
              <Row k="Sealed deltas" v={s?.history.sealed} />
              <Row k="Pending changes" v={s?.pendingChanges} />
              <Row k="Repair list" v={s?.repair.length} />
              <Row k="Divergences" v={s?.divergences.length} />
            </Table.Tbody>
          </Table>
        </Card>
      </SimpleGrid>

      <Card withBorder>
        <Group gap="xs">
          <Text fw={600} mr="sm">
            Actions
          </Text>
          <Button size="xs" variant="light" leftSection={<IconRefresh size={14} />} onClick={() => action('Reconcile', 'reconcile', {})}>
            Reconcile now
          </Button>
          <Button
            size="xs"
            variant="light"
            leftSection={<IconRefresh size={14} />}
            onClick={() =>
              action('Full reconcile', 'reconcile', { full: true }, 'Every file is read again, ignoring sizes and times. Other clients wait until it finishes.')
            }
          >
            Full reconcile
          </Button>
          <Button
            size="xs"
            variant="light"
            leftSection={<IconTools size={14} />}
            onClick={() =>
              action('Rebuild derived data', 'rebuild_derived', {}, 'Derived data (containment, the file cache) is dropped and rebuilt. Other clients wait until it finishes.')
            }
          >
            Rebuild derived data
          </Button>
          <Button
            size="xs"
            variant="light"
            color="orange"
            leftSection={<IconRotate size={14} />}
            onClick={() =>
              action('Restart the daemon', 'daemon_restart', {}, 'Connected agents and terminals are disconnected and reconnect to the new daemon. This page reconnects on its own.')
            }
          >
            Restart
          </Button>
          <Button
            size="xs"
            variant="light"
            color="red"
            leftSection={<IconPlayerStop size={14} />}
            onClick={() =>
              action('Stop the daemon', 'daemon_stop', {}, 'Connected agents and terminals are disconnected. This page stops working until you run `resonance ui` again; the next agent request starts a daemon too.')
            }
          >
            Stop
          </Button>
          <Group gap={4} ml="auto">
            <Text size="sm">Log level for this session</Text>
            <Select
              size="xs"
              w={110}
              data={LEVELS}
              value={LEVELS.includes(d?.log.level ?? '') ? d?.log.level : null}
              placeholder={d?.log.level}
              allowDeselect={false}
              onChange={(level) => level && action('Change the log level', 'log_level', { level })}
              aria-label="Log level"
            />
          </Group>
        </Group>
      </Card>

      <Tabs defaultValue="activity" keepMounted={false}>
        <Tabs.List>
          <Tabs.Tab value="activity">Activity ({live.events.length})</Tabs.Tab>
          <Tabs.Tab value="counters">Counters</Tabs.Tab>
          <Tabs.Tab value="clients">Clients ({d?.clients.length ?? 0})</Tabs.Tab>
          <Tabs.Tab value="config">Configuration</Tabs.Tab>
        </Tabs.List>
        <Tabs.Panel value="activity" pt="sm">
          <Group mb="xs" gap="sm">
            <MultiSelect
              placeholder={kinds.length ? undefined : 'All kinds'}
              data={EVENT_KINDS}
              value={kinds}
              onChange={setKinds}
              clearable
              w={360}
              size="xs"
              aria-label="Kinds"
            />
            <TextInput
              size="xs"
              placeholder="Filter"
              leftSection={<IconSearch size={14} />}
              value={text}
              onChange={(e) => setText(e.currentTarget.value)}
              w={220}
            />
            <Switch size="sm" label="Follow" checked={follow} onChange={(e) => setFollow(e.currentTarget.checked)} />
            <Text size="xs" c="dimmed">
              {events.length} shown
              {live.droppedEvents > 0 && ` · ${live.droppedEvents} older event(s) dropped (diagnostics.bufferEvents)`}
            </Text>
          </Group>
          <StreamList
            items={events}
            follow={follow}
            height="calc(100vh - 520px)"
            render={(e) => (
              <Group gap="xs" wrap="nowrap" align="flex-start">
                <Text className="mono" c="dimmed" style={{ whiteSpace: 'nowrap' }}>
                  {e.at.slice(11, 23)}
                </Text>
                <Badge size="xs" variant="light" color={KIND_COLORS[e.kind] ?? 'gray'} w={84} style={{ flexShrink: 0 }}>
                  {e.kind}
                </Badge>
                <Text size="xs" title={JSON.stringify(e.fields)}>
                  {e.summary}
                </Text>
              </Group>
            )}
          />
        </Tabs.Panel>
        <Tabs.Panel value="counters" pt="sm">
          <Table fz="sm" striped>
            <Table.Thead>
              <Table.Tr>
                <Table.Th>Method</Table.Th>
                <Table.Th ta="right">Count</Table.Th>
                <Table.Th ta="right">Errors</Table.Th>
                <Table.Th ta="right">Median</Table.Th>
                <Table.Th ta="right">95th pct.</Table.Th>
                <Table.Th ta="right">Max</Table.Th>
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {(d?.counters.methods ?? []).map((m) => (
                <Table.Tr key={m.method}>
                  <Table.Td className="mono">{m.method}</Table.Td>
                  <Table.Td ta="right">{m.count}</Table.Td>
                  <Table.Td ta="right" c={m.errors ? 'red' : undefined}>
                    {m.errors}
                  </Table.Td>
                  <Table.Td ta="right">{m.medianMs} ms</Table.Td>
                  <Table.Td ta="right">{m.p95Ms} ms</Table.Td>
                  <Table.Td ta="right">{m.maxMs} ms</Table.Td>
                </Table.Tr>
              ))}
            </Table.Tbody>
          </Table>
          <Group gap="xs" mt="sm">
            {Object.entries(d?.counters.events ?? {}).map(([k, n]) => (
              <Badge key={k} variant="outline" color={KIND_COLORS[k] ?? 'gray'}>
                {k}: {n}
              </Badge>
            ))}
          </Group>
        </Tabs.Panel>
        <Tabs.Panel value="clients" pt="sm">
          <Table fz="sm">
            <Table.Thead>
              <Table.Tr>
                <Table.Th>#</Table.Th>
                <Table.Th>Kind</Table.Th>
                <Table.Th>Actor</Table.Th>
                <Table.Th>Connected</Table.Th>
                <Table.Th ta="right">Requests</Table.Th>
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {(d?.clients ?? []).map((c) => (
                <Table.Tr key={c.id}>
                  <Table.Td>{c.id}</Table.Td>
                  <Table.Td>
                    <Badge size="sm" variant="light">
                      {c.kind}
                    </Badge>
                  </Table.Td>
                  <Table.Td>{c.actor ?? '—'}</Table.Td>
                  <Table.Td>{c.connectedAt}</Table.Td>
                  <Table.Td ta="right">{c.requests}</Table.Td>
                </Table.Tr>
              ))}
            </Table.Tbody>
          </Table>
          {d && d.clients.length === 0 && <Text c="dimmed">No clients connected.</Text>}
        </Tabs.Panel>
        <Tabs.Panel value="config" pt="sm">
          <Table fz="sm" striped>
            <Table.Thead>
              <Table.Tr>
                <Table.Th>Setting</Table.Th>
                <Table.Th>Value</Table.Th>
                <Table.Th>From</Table.Th>
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {(d?.config ?? []).map((c) => (
                <Table.Tr key={c.key}>
                  <Table.Td className="mono">{c.key}</Table.Td>
                  <Table.Td className="mono">{JSON.stringify(c.value)}</Table.Td>
                  <Table.Td>
                    <Badge size="sm" variant="light" color={c.source === 'default' ? 'gray' : c.source === 'file' ? 'blue' : 'orange'}>
                      {c.source}
                    </Badge>
                  </Table.Td>
                </Table.Tr>
              ))}
            </Table.Tbody>
          </Table>
        </Tabs.Panel>
      </Tabs>
    </Stack>
  );
}
