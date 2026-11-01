// The web UI (MVP-043): the spec, the change log, diagnostics and logs, served by the daemon.

import { useEffect, useMemo, useRef, useState } from 'react';
import {
  ActionIcon,
  Alert,
  AppShell,
  Badge,
  Code,
  Group,
  Kbd,
  Modal,
  NavLink,
  Table,
  Text,
  Title,
  Tooltip,
  useMantineColorScheme,
} from '@mantine/core';
import { useHotkeys } from '@mantine/hooks';
import { Spotlight, type SpotlightActionData, spotlight } from '@mantine/spotlight';
import {
  IconActivity,
  IconBolt,
  IconFileText,
  IconHelp,
  IconHistory,
  IconListDetails,
  IconMoonStars,
  IconPlus,
  IconRefresh,
  IconSearch,
  IconSun,
} from '@tabler/icons-react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Navigate, Route, Routes, useLocation, useNavigate } from 'react-router-dom';
import { call } from './api';
import { connect, useLive } from './live';
import type { Health, Session, SpecItem, Status } from './types';
import { SPEC_TYPES } from './types';
import { SpecPage } from './pages/SpecPage';
import { ChangeLogPage } from './pages/ChangeLogPage';
import { DiagnosticsPage } from './pages/DiagnosticsPage';
import { LogsPage } from './pages/LogsPage';
import { BootScreen } from './components/BootScreen';
import { bootPhase, describeState } from './boot';

const STATE_COLORS: Record<string, string> = {
  ready: 'green',
  starting: 'gray',
  reconciling: 'blue',
  'git-operation': 'blue',
  degraded: 'orange',
};

export function App() {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const location = useLocation();
  const live = useLive();
  const [help, setHelp] = useState(false);
  const { colorScheme, toggleColorScheme } = useMantineColorScheme();

  useEffect(() => {
    connect(() => {
      void queryClient.invalidateQueries({ queryKey: ['spec'] });
      void queryClient.invalidateQueries({ queryKey: ['item'] });
      void queryClient.invalidateQueries({ queryKey: ['changes'] });
      void queryClient.invalidateQueries({ queryKey: ['status'] });
    });
  }, [queryClient]);

  const session = useQuery({ queryKey: ['session'], queryFn: () => call<Session>('session') });
  const health = useQuery({
    queryKey: ['health'],
    queryFn: () => call<Health>('health'),
    // Poll quickly until the daemon can serve the graph, then settle (FR-095).
    refetchInterval: (query) => (query.state.data?.ready ? 5000 : 300),
  });
  const ready = health.data?.ready === true;
  // Spec items are fetched only once the daemon is ready, or the person asks to look anyway.
  // A fetch during startup can fail or come back empty and then stay cached.
  const [proceeded, setProceeded] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  useEffect(() => {
    if (ready) return;
    const start = Date.now();
    const id = window.setInterval(() => setElapsed(Date.now() - start), 200);
    return () => window.clearInterval(id);
  }, [ready]);
  const serveGraph = ready || proceeded;
  const status = useQuery({
    queryKey: ['status'],
    queryFn: () => call<Status>('status', {}, { cached: true }),
    enabled: serveGraph,
  });
  const allItems = useQuery({
    queryKey: ['spec', 'all'],
    queryFn: () => call<SpecItem[]>('spec_list', {}),
    enabled: serveGraph,
  });
  const wasReady = useRef(false);
  useEffect(() => {
    if (!ready || wasReady.current) return;
    wasReady.current = true;
    void queryClient.invalidateQueries({ queryKey: ['spec'] });
    void queryClient.invalidateQueries({ queryKey: ['status'] });
  }, [ready, queryClient]);

  useHotkeys([
    ['shift+/', () => setHelp(true)],
    ['l', () => navigate('/changes')],
    ['g', () => void queryClient.invalidateQueries()],
  ]);

  const actions: SpotlightActionData[] = useMemo(() => {
    const go = (id: string, label: string, path: string, icon: React.ReactNode) => ({
      id,
      label,
      group: 'Go to',
      leftSection: icon,
      onClick: () => navigate(path),
    });
    const list: SpotlightActionData[] = [
      go('go-spec', 'Spec items', '/spec', <IconListDetails size={18} />),
      go('go-changes', 'Change log', '/changes', <IconHistory size={18} />),
      go('go-diagnostics', 'Diagnostics', '/diagnostics', <IconActivity size={18} />),
      go('go-logs', 'Logs', '/logs', <IconFileText size={18} />),
      ...SPEC_TYPES.map((t) => ({
        id: `add-${t}`,
        label: `Add a ${t.toLowerCase()}`,
        group: 'Actions',
        leftSection: <IconPlus size={18} />,
        onClick: () => navigate(`/spec?add=${t}`),
      })),
      {
        id: 'reconcile',
        label: 'Reconcile now',
        description: 'Bring the graph up to date with the files on disk',
        group: 'Actions',
        leftSection: <IconRefresh size={18} />,
        onClick: () => void call('reconcile', {}).then(() => queryClient.invalidateQueries()),
      },
      {
        id: 'theme',
        label: 'Switch light or dark',
        group: 'Actions',
        leftSection: <IconBolt size={18} />,
        onClick: () => toggleColorScheme(),
      },
    ];
    for (const item of allItems.data ?? []) {
      const ref = item.key ?? item.id;
      list.push({
        id: `item-${item.id}`,
        label: `${item.key ?? ''} ${item.title}`.trim(),
        description: `${item.type} · ${item.status}`,
        group: 'Spec items',
        keywords: [item.text.slice(0, 200)],
        onClick: () => navigate(`/spec/${encodeURIComponent(ref)}`),
      });
    }
    return list;
  }, [allItems.data, navigate, queryClient, toggleColorScheme]);

  const state = health.data?.state ?? '…';
  const pending = status.data?.pendingChanges ?? 0;
  const path = location.pathname;
  const phase = bootPhase(ready, elapsed, proceeded);
  // Logs and diagnostics read the daemon's own buffers and answer during startup.
  const escape = path.startsWith('/logs') || path.startsWith('/diagnostics');
  const showBoot = phase !== 'ready' && !escape;

  return (
    <AppShell header={{ height: 52 }} navbar={{ width: 200, breakpoint: 'sm' }} padding="md">
      <AppShell.Header>
        <Group h="100%" px="md" justify="space-between" wrap="nowrap">
          <Group gap="sm" wrap="nowrap">
            <Title order={4}>Resonance</Title>
            {session.data && (
              <Tooltip label={session.data.root}>
                <Text fw={500} truncate>
                  {session.data.repository}
                </Text>
              </Tooltip>
            )}
            {session.data?.branch && <Badge variant="light">{session.data.branch}</Badge>}
            {status.data && (
              <Tooltip label={`Participation: ${status.data.participation}`}>
                <Badge variant="outline" color="gray">
                  stage {status.data.stage}
                </Badge>
              </Tooltip>
            )}
          </Group>
          <Group gap="xs" wrap="nowrap">
            {pending > 0 && (
              <Tooltip label="Changes your agent has not handled yet">
                <Badge color="yellow" style={{ cursor: 'pointer' }} onClick={() => navigate('/changes')}>
                  {pending} pending
                </Badge>
              </Tooltip>
            )}
            <Tooltip label={health.data?.lastError ?? `daemon ${state}`}>
              <Badge color={STATE_COLORS[state] ?? 'gray'} variant="dot">
                {state}
              </Badge>
            </Tooltip>
            <Tooltip label="Search and actions (⌘K)">
              <ActionIcon variant="subtle" onClick={() => spotlight.open()} aria-label="Search and actions">
                <IconSearch size={18} />
              </ActionIcon>
            </Tooltip>
            <ActionIcon variant="subtle" onClick={() => toggleColorScheme()} aria-label="Switch light or dark">
              {colorScheme === 'dark' ? <IconSun size={18} /> : <IconMoonStars size={18} />}
            </ActionIcon>
            <ActionIcon variant="subtle" onClick={() => setHelp(true)} aria-label="Help">
              <IconHelp size={18} />
            </ActionIcon>
          </Group>
        </Group>
      </AppShell.Header>
      <AppShell.Navbar p="xs">
        <NavLink
          label="Spec"
          leftSection={<IconListDetails size={18} />}
          active={path === '/' || path.startsWith('/spec')}
          onClick={() => navigate('/spec')}
        />
        <NavLink
          label="Change log"
          leftSection={<IconHistory size={18} />}
          rightSection={pending > 0 ? <Badge size="xs" color="yellow">{pending}</Badge> : null}
          active={path.startsWith('/changes')}
          onClick={() => navigate('/changes')}
        />
        <NavLink
          label="Diagnostics"
          leftSection={<IconActivity size={18} />}
          active={path.startsWith('/diagnostics')}
          onClick={() => navigate('/diagnostics')}
        />
        <NavLink
          label="Logs"
          leftSection={<IconFileText size={18} />}
          active={path.startsWith('/logs')}
          onClick={() => navigate('/logs')}
        />
      </AppShell.Navbar>
      <AppShell.Main>
        <div style={{ display: 'flex', flexDirection: 'column', height: 'calc(100dvh - 84px)', minHeight: 0 }}>
          <ConnectionBanner />
          <div style={{ flex: 1, minHeight: 0, overflow: 'auto', display: 'flex', flexDirection: 'column' }}>
            {showBoot ? (
              <BootScreen
                phase={phase}
                elapsedMs={elapsed}
                state={health.data?.state}
                pendingEvents={health.data?.pendingEvents ?? 0}
                lagMs={health.data?.lagMs ?? 0}
                error={health.error ? (health.error as Error).message : (health.data?.lastError ?? null)}
                onLogs={() => navigate('/logs')}
                onDiagnostics={() => navigate('/diagnostics')}
                onProceed={() => {
                  setProceeded(true);
                  navigate('/spec');
                }}
              />
            ) : (
            <>
            {proceeded && !ready && (
              <Alert mb="sm" color="yellow" title="The daemon is not ready">
                The daemon is still {describeState(health.data?.state)}. Spec items refresh when it is ready.
              </Alert>
            )}
            <Routes>
              <Route path="/" element={<Navigate to="/spec" replace />} />
              <Route path="/spec" element={<SpecPage />} />
              <Route path="/spec/:ref" element={<SpecPage />} />
              <Route path="/changes" element={<ChangeLogPage />} />
              <Route path="/diagnostics" element={<DiagnosticsPage />} />
              <Route path="/logs" element={<LogsPage />} />
              <Route path="*" element={<Navigate to="/spec" replace />} />
            </Routes>
            </>
            )}
          </div>
        </div>
      </AppShell.Main>
      <Spotlight
        actions={actions}
        nothingFound="Nothing found"
        highlightQuery
        limit={30}
        shortcut={['mod + K']}
        searchProps={{ leftSection: <IconSearch size={18} />, placeholder: 'Search spec items and actions…' }}
      />
      <HelpModal opened={help} onClose={() => setHelp(false)} live={live.connection} />
    </AppShell>
  );
}

function ConnectionBanner() {
  const { connection } = useLive();
  if (connection === 'live' || connection === 'connecting') return null;
  const text = {
    reconnecting: 'Lost the connection to the daemon; reconnecting… If it stopped, run `resonance ui` to start it again.',
    stopped: 'The daemon stopped. Run `resonance ui` in the repository to start it and open this page again.',
    'signed-out': 'This page is not signed in to the running daemon. Run `resonance ui` in the repository to open it again.',
  }[connection];
  return (
    <Alert color={connection === 'reconnecting' ? 'yellow' : 'red'} mb="md" title="Not connected">
      {text}
    </Alert>
  );
}

const KEYS: [string, string][] = [
  ['/', 'Filter spec items by text'],
  ['j / k, ↓ / ↑', 'Next or previous item'],
  ['a', 'Add a spec item'],
  ['e', 'Edit the selected item'],
  ['r', 'Retire the selected item'],
  ['l', 'Open the change log'],
  ['d / shift+D', 'Clear the selected change-log entry / all of them'],
  ['g', 'Reload'],
  ['⌘K / ctrl+K', 'Search spec items and actions'],
  ['⌘S / ctrl+S', 'Save a form'],
  ['esc', 'Cancel'],
  ['?', 'This help'],
];

function HelpModal({ opened, onClose, live }: { opened: boolean; onClose: () => void; live: string }) {
  return (
    <Modal opened={opened} onClose={onClose} title="Keys" size="lg">
      <Table>
        <Table.Tbody>
          {KEYS.map(([k, what]) => (
            <Table.Tr key={k}>
              <Table.Td>
                <Kbd>{k}</Kbd>
              </Table.Td>
              <Table.Td>{what}</Table.Td>
            </Table.Tr>
          ))}
        </Table.Tbody>
      </Table>
      <Text size="sm" c="dimmed" mt="md">
        Your edits go to the graph as you, and land in the change log for your agent. Connection: <Code>{live}</Code>.
      </Text>
    </Modal>
  );
}
