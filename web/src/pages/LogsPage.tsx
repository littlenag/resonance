// The daemon's log since it started (FR-298): live, filtered by level, component and text,
// following or paused, with copy and export, and the session's level (FR-295).

import { useMemo, useState } from 'react';
import { Badge, Button, Group, MultiSelect, SegmentedControl, Select, Stack, Switch, Text, TextInput, Title } from '@mantine/core';
import { notifications } from '@mantine/notifications';
import { IconCopy, IconDownload, IconSearch } from '@tabler/icons-react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { call, downloadJson } from '../api';
import { useLive } from '../live';
import type { LogRecord, Logs } from '../types';
import { StreamList } from '../components/StreamList';
import { LEVELS } from './DiagnosticsPage';

const LEVEL_COLORS: Record<string, string> = {
  error: 'red',
  warn: 'orange',
  info: 'blue',
  debug: 'gray',
  trace: 'gray',
};

const RANK: Record<string, number> = { error: 0, warn: 1, info: 2, debug: 3, trace: 4 };

/** Records at `level` or more severe, of `components` (all when empty), containing `text`. */
export function filterLogs(records: LogRecord[], level: string, components: string[], text: string): LogRecord[] {
  const max = RANK[level] ?? 4;
  const t = text.trim().toLowerCase();
  return records.filter(
    (r) =>
      (RANK[r.level] ?? 4) <= max &&
      (components.length === 0 || components.includes(r.component)) &&
      (!t || r.message.toLowerCase().includes(t) || JSON.stringify(r.fields ?? {}).toLowerCase().includes(t)),
  );
}

/** A record as one line of text, as in the log file. */
export function logLine(r: LogRecord): string {
  const fields = Object.entries(r.fields ?? {})
    .map(([k, v]) => ` ${k}=${typeof v === 'string' ? v : JSON.stringify(v)}`)
    .join('');
  return `${r.at} ${r.level.toUpperCase().padEnd(5)} ${r.component}: ${r.message}${fields}`;
}

export function LogsPage() {
  const queryClient = useQueryClient();
  const live = useLive();
  const info = useQuery({ queryKey: ['logs', 'info'], queryFn: () => call<Logs>('logs', { since: Number.MAX_SAFE_INTEGER }) });
  const [level, setLevel] = useState('trace');
  const [components, setComponents] = useState<string[]>([]);
  const [text, setText] = useState('');
  const [follow, setFollow] = useState(true);
  const known = useMemo(() => [...new Set(live.logs.map((r) => r.component))].sort(), [live.logs]);
  const shown = useMemo(() => filterLogs(live.logs, level, components, text), [live.logs, level, components, text]);

  const setSessionLevel = async (l: string | null) => {
    if (!l) return;
    try {
      await call('log_level', { level: l });
      notifications.show({ color: 'green', message: `The daemon now logs at ${l} until it stops.` });
      await queryClient.invalidateQueries({ queryKey: ['logs'] });
      await queryClient.invalidateQueries({ queryKey: ['diagnostics'] });
    } catch (e) {
      notifications.show({ color: 'red', title: 'Level not changed', message: (e as Error).message });
    }
  };

  return (
    <Stack gap="sm">
      <Group justify="space-between">
        <div>
          <Title order={3}>Logs</Title>
          <Text size="sm" c="dimmed">
            Since the daemon started. Also written to <span className="mono">{info.data?.file ?? '…'}</span> (log.file).
          </Text>
        </div>
        <Group gap="xs">
          <Text size="sm">Daemon level</Text>
          <Select
            size="xs"
            w={110}
            data={LEVELS}
            value={LEVELS.includes(info.data?.level ?? '') ? info.data?.level : null}
            placeholder={info.data?.level}
            allowDeselect={false}
            onChange={(l) => void setSessionLevel(l)}
            aria-label="Daemon log level"
          />
        </Group>
      </Group>
      <Group gap="sm">
        <SegmentedControl size="xs" data={LEVELS} value={level} onChange={setLevel} aria-label="Show at least" />
        <MultiSelect
          size="xs"
          placeholder={components.length ? undefined : 'All components'}
          data={known}
          value={components}
          onChange={setComponents}
          clearable
          w={260}
          aria-label="Components"
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
        <Button
          size="xs"
          variant="light"
          leftSection={<IconCopy size={14} />}
          onClick={() =>
            void navigator.clipboard
              .writeText(shown.map(logLine).join('\n'))
              .then(() => notifications.show({ message: `Copied ${shown.length} line(s).` }))
          }
        >
          Copy
        </Button>
        <Button
          size="xs"
          variant="light"
          leftSection={<IconDownload size={14} />}
          onClick={() => downloadJson(`resonance-log-${new Date().toISOString().replace(/[:.]/g, '-')}.json`, shown)}
        >
          Export
        </Button>
        <Text size="xs" c="dimmed">
          {shown.length} of {live.logs.length}
          {live.droppedLogs > 0 && ` · ${live.droppedLogs} older record(s) dropped (log.bufferLines)`}
        </Text>
      </Group>
      <StreamList
        items={shown}
        follow={follow}
        height="calc(100vh - 210px)"
        render={(r) => (
          <Group gap="xs" wrap="nowrap" align="flex-start">
            <Text className="mono" c="dimmed" style={{ whiteSpace: 'nowrap' }}>
              {r.at.slice(11, 23)}
            </Text>
            <Badge size="xs" color={LEVEL_COLORS[r.level] ?? 'gray'} variant="light" w={52} style={{ flexShrink: 0 }}>
              {r.level}
            </Badge>
            <Text className="mono" c="dimmed" w={80} style={{ flexShrink: 0 }}>
              {r.component}
            </Text>
            <Text className="mono" style={{ wordBreak: 'break-word' }}>
              {r.message}
              {Object.entries(r.fields ?? {}).map(([k, v]) => (
                <Text span c="dimmed" key={k} className="mono">
                  {' '}
                  {k}={typeof v === 'string' ? v : JSON.stringify(v)}
                </Text>
              ))}
            </Text>
          </Group>
        )}
      />
    </Stack>
  );
}
