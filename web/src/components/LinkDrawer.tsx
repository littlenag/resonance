// What a link points to (FR-292): for code units, tests and document regions, the whole file
// with the linked lines highlighted; for spec items, the item, with a way to open it.

import { useEffect, useRef } from 'react';
import { Alert, Badge, Button, Drawer, Group, Loader, Stack, Text } from '@mantine/core';
import { IconAlertTriangle, IconArrowRight } from '@tabler/icons-react';
import { useQuery } from '@tanstack/react-query';
import { useNavigate } from 'react-router-dom';
import { call } from '../api';
import type { LinkView, ShowView } from '../types';
import { SPEC_TYPES } from '../types';

export interface FileText {
  path: string;
  size: number;
  lines: number;
  text: string;
}

export function LinkDrawer({ link, onClose }: { link: LinkView | null; onClose: () => void }) {
  const other = link?.other;
  return (
    <Drawer
      opened={!!link}
      onClose={onClose}
      position="bottom"
      size="60%"
      title={
        other && (
          <Group gap="xs">
            <Text fw={600} className="mono">
              {other.label}
            </Text>
            {other.type && <Badge variant="outline">{other.type}</Badge>}
            {link && (
              <Badge variant="light" color={link.status === 'stale' ? 'orange' : 'gray'}>
                {link.relation}: {link.status}
              </Badge>
            )}
          </Group>
        )
      }
    >
      {other?.id && <Target id={other.id} type={other.type} onClose={onClose} />}
    </Drawer>
  );
}

function Target({ id, type, onClose }: { id: string; type?: string; onClose: () => void }) {
  const navigate = useNavigate();
  const shown = useQuery({ queryKey: ['item', id, 'show'], queryFn: () => call<ShowView>('show', { ref: id }) });
  const isSpec = (SPEC_TYPES as string[]).includes(type ?? '');
  const loc = shown.data?.location;
  const path = loc?.path ?? (type === 'File' ? (shown.data?.node.label ?? null) : null);
  const file = useQuery({
    queryKey: ['file', path],
    queryFn: () => call<FileText>('file_text', { path }),
    enabled: !!path,
  });

  if (shown.isLoading) return <Loader size="sm" />;
  if (shown.error) return <Text c="red">{(shown.error as Error).message}</Text>;
  if (isSpec) {
    const ref = String(shown.data?.properties.key ?? id);
    return (
      <Stack gap="sm">
        <Text>{String(shown.data?.properties.text ?? shown.data?.properties.title ?? shown.data?.node.label ?? '')}</Text>
        <Group>
          <Button
            size="xs"
            rightSection={<IconArrowRight size={14} />}
            onClick={() => {
              onClose();
              navigate(`/spec/${encodeURIComponent(ref)}`);
            }}
          >
            Open {ref}
          </Button>
        </Group>
      </Stack>
    );
  }
  if (!path) return <Text c="dimmed">This node has no place in a file to show.</Text>;
  const orphaned = loc?.state && loc.state !== 'attached';
  return (
    <Stack gap="xs">
      <Group gap="xs">
        <Text size="sm" className="mono">
          {path}
          {loc?.startLine ? `:${loc.startLine}${loc.endLine && loc.endLine !== loc.startLine ? `–${loc.endLine}` : ''}` : ''}
        </Text>
        {loc && loc.confidence !== undefined && loc.confidence < 1 && (
          <Badge size="xs" variant="light" color="yellow">
            confidence {Math.round(loc.confidence * 100)}%
          </Badge>
        )}
      </Group>
      {orphaned && (
        <Alert color="orange" icon={<IconAlertTriangle size={16} />} p="xs">
          This region is {loc?.state}: Resonance could not find it in the file any more. The highlighted lines are where it
          was last seen.
        </Alert>
      )}
      {file.isLoading && <Loader size="sm" />}
      {file.error && <Text c="red">{(file.error as Error).message}</Text>}
      {file.data && <CodeView text={file.data.text} start={loc?.startLine} end={loc?.endLine} />}
    </Stack>
  );
}

/** A file with line numbers, the lines `start`–`end` (1-based, inclusive) highlighted and in view. */
export function CodeView({ text, start, end }: { text: string; start?: number; end?: number }) {
  const first = useRef<HTMLDivElement>(null);
  const lines = text.split('\n');
  if (lines.length > 1 && lines[lines.length - 1] === '') lines.pop();
  const last = end ?? start;
  useEffect(() => {
    first.current?.scrollIntoView({ block: 'center' });
  }, [text, start]);
  const width = String(lines.length).length;
  return (
    <div className="code-view" data-testid="code-view">
      {lines.map((line, i) => {
        const n = i + 1;
        const hit = start !== undefined && last !== undefined && n >= start && n <= last;
        return (
          <div
            key={n}
            ref={n === start ? first : undefined}
            className={hit ? 'code-line code-hit' : 'code-line'}
            data-line={n}
            data-highlighted={hit || undefined}
          >
            <span className="code-gutter">{String(n).padStart(width, ' ')}</span>
            <span className="code-text">{line || ' '}</span>
          </div>
        );
      })}
    </div>
  );
}
