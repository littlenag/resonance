// Facets in the style of log explorers: a column of checkboxes per facet, with counts. None
// checked shows everything; "only" checks just that value.

import { useState } from 'react';
import { Anchor, Box, Checkbox, Collapse, Group, Stack, Text, UnstyledButton } from '@mantine/core';
import { IconChevronDown, IconChevronRight } from '@tabler/icons-react';
import { FACETS, type Selection } from '../specFilter';

export function Facets({
  counts,
  selection,
  onChange,
}: {
  counts: Record<string, { value: string; count: number }[]>;
  selection: Selection;
  onChange: (next: Selection) => void;
}) {
  const any = Object.values(selection).some((v) => v.length > 0);
  return (
    <Stack gap="sm">
      <Group justify="space-between" h={20}>
        <Text size="xs" fw={700} c="dimmed" tt="uppercase">
          Filters
        </Text>
        {any && (
          <Anchor size="xs" component="button" onClick={() => onChange({})}>
            Clear all
          </Anchor>
        )}
      </Group>
      {FACETS.map((f) => (
        <FacetGroup
          key={f.id}
          label={f.label}
          values={counts[f.id] ?? []}
          checked={selection[f.id] ?? []}
          show={f.show}
          onChange={(v) => onChange({ ...selection, [f.id]: v })}
        />
      ))}
    </Stack>
  );
}

function FacetGroup({
  label,
  values,
  checked,
  show,
  onChange,
}: {
  label: string;
  values: { value: string; count: number }[];
  checked: string[];
  show?: (v: string) => string;
  onChange: (checked: string[]) => void;
}) {
  const [open, setOpen] = useState(true);
  const [hover, setHover] = useState<string | null>(null);
  return (
    <div role="group" aria-label={label}>
      <Group justify="space-between" gap={4}>
        <UnstyledButton onClick={() => setOpen((o) => !o)} aria-expanded={open}>
          <Group gap={4}>
            {open ? <IconChevronDown size={14} /> : <IconChevronRight size={14} />}
            <Text size="sm" fw={600}>
              {label}
            </Text>
          </Group>
        </UnstyledButton>
        {checked.length > 0 && (
          <Anchor size="xs" component="button" onClick={() => onChange([])}>
            clear
          </Anchor>
        )}
      </Group>
      <Collapse expanded={open}>
        <Stack gap={4} mt={6} pl={18}>
          {values.map(({ value, count }) => {
            const isChecked = checked.includes(value);
            return (
              <Group
                key={value}
                justify="space-between"
                wrap="nowrap"
                gap={4}
                onMouseEnter={() => setHover(value)}
                onMouseLeave={() => setHover((h) => (h === value ? null : h))}
              >
                <Checkbox
                  size="xs"
                  label={show ? show(value) : value}
                  checked={isChecked}
                  onChange={(e) =>
                    onChange(e.currentTarget.checked ? [...checked, value] : checked.filter((v) => v !== value))
                  }
                  styles={{ label: { opacity: count === 0 && !isChecked ? 0.5 : 1 } }}
                />
                <Box style={{ flexShrink: 0 }}>
                  {hover === value && !(checked.length === 1 && isChecked) ? (
                    <Anchor size="xs" component="button" onClick={() => onChange([value])} aria-label={`only ${value}`}>
                      only
                    </Anchor>
                  ) : (
                    <Text size="xs" c="dimmed">
                      {count}
                    </Text>
                  )}
                </Box>
              </Group>
            );
          })}
        </Stack>
      </Collapse>
    </div>
  );
}
