// Spec items (FR-291): facets and a sortable list beside the selected item's detail; add,
// edit and retire. Every item has its own address, /spec/<key>, and the filters and sort
// are in the query string, so that a view can be bookmarked.

import { useMemo, useRef, useState, type ReactNode } from 'react';
import { Badge, Box, Button, Flex, Group, Menu, Stack, Table, Text, TextInput, UnstyledButton } from '@mantine/core';
import { useHotkeys } from '@mantine/hooks';
import { modals } from '@mantine/modals';
import { notifications } from '@mantine/notifications';
import { IconChevronDown, IconChevronUp, IconPlus, IconSearch, IconSelector } from '@tabler/icons-react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useLocation, useNavigate, useParams, useSearchParams } from 'react-router-dom';
import { call } from '../api';
import type { SpecItem, SpecType } from '../types';
import { SPEC_TYPES } from '../types';
import { ItemDetail } from '../components/ItemDetail';
import { ItemForm, type FormMode } from '../components/ItemForm';
import { Facets } from '../components/Facets';
import { SpecTypesGuide } from '../components/SpecTypesGuide';
import {
  applyFilters,
  facetCounts,
  readView,
  sortItems,
  writeView,
  type Selection,
  type SortDir,
  type SortKey,
} from '../specFilter';

export function refOf(item: SpecItem) {
  return item.key ?? item.id;
}

export function SpecPage() {
  const { ref } = useParams();
  const navigate = useNavigate();
  const location = useLocation();
  const [search, setSearch] = useSearchParams();
  const queryClient = useQueryClient();
  const [form, setForm] = useState<FormMode | null>(null);
  const filterRef = useRef<HTMLInputElement>(null);
  const view = readView(search);

  // Every spec item, in one query that the event stream keeps current (FR-290); filtering
  // and sorting happen here, so that no filter can show a stale list.
  const list = useQuery({
    queryKey: ['spec', 'all'],
    queryFn: () => call<SpecItem[]>('spec_list', {}),
    refetchOnWindowFocus: true,
  });
  const all = useMemo(() => list.data ?? [], [list.data]);
  const items = useMemo(
    () => sortItems(applyFilters(all, view.text, view.selection), view.sort, view.dir),
    [all, view.text, view.selection, view.sort, view.dir],
  );
  const counts = useMemo(() => facetCounts(all, view.text, view.selection), [all, view.text, view.selection]);

  const setView = (next: Partial<typeof view>) =>
    setSearch(writeView(search, { ...view, ...next }), { replace: true });
  const setSort = (key: SortKey) => {
    const dir: SortDir = view.sort === key && view.dir === 'asc' ? 'desc' : 'asc';
    setView({ sort: key, dir });
  };

  const decoded = ref ? decodeURIComponent(ref) : undefined;
  const index = decoded ? items.findIndex((i) => i.key === decoded || i.id === decoded) : -1;
  const current = index >= 0 ? items[index] : !decoded ? items[0] : all.find((i) => i.key === decoded || i.id === decoded);
  const selectedRef = decoded ?? (items[0] ? refOf(items[0]) : undefined);

  // `/spec?add=Decision` (from the command palette) opens the add form.
  const addParam = search.get('add');
  const fromUrl: FormMode | null =
    addParam && (SPEC_TYPES as string[]).includes(addParam) ? { mode: 'add', type: addParam as SpecType } : null;
  const openForm = form ?? fromUrl;
  const closeForm = () => {
    setForm(null);
    if (addParam) {
      search.delete('add');
      setSearch(search, { replace: true });
    }
  };

  const go = (r: string) => navigate(`/spec/${encodeURIComponent(r)}${location.search}`);
  const select = (item: SpecItem | undefined) => {
    if (item) go(refOf(item));
  };
  const move = (delta: number) => {
    if (items.length === 0) return;
    if (index < 0) return select(items[0]);
    select(items[Math.min(items.length - 1, Math.max(0, index + delta))]);
  };

  const retire = (item: SpecItem) => {
    if (item.status === 'retired') {
      notifications.show({ message: 'Already retired.' });
      return;
    }
    const label = item.key ?? item.title;
    modals.openConfirmModal({
      title: `Retire ${label}?`,
      children: (
        <Text size="sm">
          Retiring never deletes: {label} stays in the graph as retired, and its {item.links} link(s) are kept, so
          that they can be reviewed. Your agent sees the change in the change log.
        </Text>
      ),
      labels: { confirm: 'Retire', cancel: 'Cancel' },
      confirmProps: { color: 'red' },
      onConfirm: async () => {
        try {
          await call('spec_retire', { ref: item.id });
          notifications.show({ color: 'green', message: `Retired ${label} (its ${item.links} link(s) were kept).` });
          await queryClient.invalidateQueries();
        } catch (e) {
          notifications.show({ color: 'red', title: 'Not retired', message: (e as Error).message });
        }
      },
    });
  };

  const onlyType = view.selection.type?.length === 1 ? (view.selection.type[0] as SpecType) : 'Requirement';
  useHotkeys([
    ['/', (e) => {
      e.preventDefault();
      filterRef.current?.focus();
    }],
    ['j', () => move(1)],
    ['ArrowDown', () => move(1)],
    ['n', () => move(1)],
    ['k', () => move(-1)],
    ['ArrowUp', () => move(-1)],
    ['p', () => move(-1)],
    ['a', () => setForm({ mode: 'add', type: onlyType })],
    ['e', () => current && setForm({ mode: 'edit', item: current })],
    ['r', () => current && retire(current)],
  ]);

  const filtered = view.text !== '' || Object.values(view.selection).some((v) => v.length > 0);
  const th = (key: SortKey, label: ReactNode, w?: number) => (
    <Table.Th w={w} aria-sort={view.sort === key ? (view.dir === 'asc' ? 'ascending' : 'descending') : 'none'}>
      <SortHeader active={view.sort === key} dir={view.dir} onClick={() => setSort(key)} label={typeof label === 'string' ? label : key}>
        {label}
      </SortHeader>
    </Table.Th>
  );

  return (
    <Stack gap="sm" style={{ flex: 1, minHeight: 0, overflow: 'hidden' }}>
      <Group gap="sm" wrap="wrap">
        <TextInput
          ref={filterRef}
          placeholder="Search keys and text (/)"
          leftSection={<IconSearch size={16} />}
          value={view.text}
          onChange={(e) => setView({ text: e.currentTarget.value })}
          onKeyDown={(e) => {
            if (e.key === 'Escape') e.currentTarget.blur();
          }}
          w={300}
          aria-label="Search"
        />
        <Menu position="bottom-start">
          <Menu.Target>
            <Button leftSection={<IconPlus size={16} />} rightSection={<IconChevronDown size={14} />}>
              Add
            </Button>
          </Menu.Target>
          <Menu.Dropdown>
            {SPEC_TYPES.map((t) => (
              <Menu.Item key={t} onClick={() => setForm({ mode: 'add', type: t })}>
                {t}
              </Menu.Item>
            ))}
          </Menu.Dropdown>
        </Menu>
        <Text size="sm" c="dimmed">
          {list.isLoading ? 'Loading…' : filtered ? `${items.length} of ${all.length} item(s)` : `${all.length} item(s)`}
        </Text>
      </Group>
      {list.error && (
        <Text c="red">
          {(list.error as Error).message}
        </Text>
      )}
      <SpecTypesGuide />
      <Flex gap="md" align="stretch" wrap="nowrap" style={{ flex: '1 1 0', minHeight: 0, overflow: 'hidden' }}>
        <Box w={210} style={{ flex: '0 0 210px', minHeight: 0, overflow: 'auto' }}>
          <Facets counts={counts} selection={view.selection} onChange={(s: Selection) => setView({ selection: s })} />
        </Box>
        <Box style={{ flex: '1.2 1 0', minWidth: 0, minHeight: 0, overflow: 'auto' }}>
          <Table highlightOnHover stickyHeader verticalSpacing={4} fz="sm">
              <Table.Thead>
                <Table.Tr>
                  {th('key', 'Key')}
                  {th('type', 'Type')}
                  {th('status', 'Status')}
                  {th('title', 'Title')}
                  {th('links', 'Links')}
                </Table.Tr>
              </Table.Thead>
              <Table.Tbody>
                {items.map((item) => {
                  const isSelected = current?.id === item.id;
                  return (
                    <Table.Tr
                      key={item.id}
                      className={`row-clickable ${isSelected ? 'row-selected' : ''} ${item.pendingChange ? 'row-pending' : ''}`}
                      onClick={() => select(item)}
                      aria-selected={isSelected}
                      data-pending={item.pendingChange ? true : undefined}
                      title={
                        item.pendingChange
                          ? `Pending change #${item.pendingChange.seq} (${item.pendingChange.kind}): not handled by your agent yet`
                          : undefined
                      }
                      data-testid={`item-${refOf(item)}`}
                    >
                      <Table.Td className="mono" style={{ whiteSpace: 'nowrap' }}>
                        {item.key ?? '—'}
                      </Table.Td>
                      <Table.Td>
                        <Text size="xs">{item.type}</Text>
                      </Table.Td>
                      <Table.Td>
                        <StatusBadge status={item.status} />
                      </Table.Td>
                      <Table.Td>
                        <Text size="sm" lineClamp={2}>
                          {item.title}
                        </Text>
                      </Table.Td>
                      <Table.Td style={{ whiteSpace: 'nowrap' }}>{item.links}</Table.Td>
                    </Table.Tr>
                  );
                })}
              </Table.Tbody>
          </Table>
          {!list.isLoading && !list.error && items.length === 0 && (
            <Text c="dimmed" p="md">
              {filtered
                ? 'No spec items match these filters.'
                : 'No spec items yet: add one, or ask your agent to bring in your spec documents.'}
            </Text>
          )}
        </Box>
        <Box style={{ flex: '1 1 0', minWidth: 0, minHeight: 0, overflow: 'auto' }}>
          {selectedRef ? (
            <ItemDetail
              // The address's reference when there is one, so that the detail is fetched once.
              itemRef={decoded ?? current?.id ?? selectedRef}
              listItem={current}
              onEdit={(item) => setForm({ mode: 'edit', item })}
              onRetire={retire}
            />
          ) : (
            <Text c="dimmed">Select an item to see its details.</Text>
          )}
        </Box>
      </Flex>
      {openForm && (
        <ItemForm
          mode={openForm}
          onClose={closeForm}
          onSaved={(saved) => {
            closeForm();
            void queryClient.invalidateQueries().then(() => {
              if (saved) go(saved);
            });
          }}
        />
      )}
    </Stack>
  );
}

function SortHeader({
  active,
  dir,
  onClick,
  label,
  children,
}: {
  active: boolean;
  dir: SortDir;
  onClick: () => void;
  label: string;
  children: ReactNode;
}) {
  const Icon = active ? (dir === 'asc' ? IconChevronUp : IconChevronDown) : IconSelector;
  return (
    <UnstyledButton
      onClick={onClick}
      className="sort-header"
      aria-label={`Sort by ${label}`}
    >
      {children}
      <Icon size={14} opacity={active ? 1 : 0.4} />
    </UnstyledButton>
  );
}

const STATUS_COLORS: Record<string, string> = {
  active: 'green',
  accepted: 'green',
  draft: 'gray',
  proposed: 'blue',
  superseded: 'violet',
  deprecated: 'orange',
  retired: 'red',
};

export function StatusBadge({ status }: { status: string }) {
  return (
    <Badge size="sm" variant="light" color={STATUS_COLORS[status] ?? 'gray'} style={{ overflow: 'visible' }} miw="fit-content">
      {status}
    </Badge>
  );
}
