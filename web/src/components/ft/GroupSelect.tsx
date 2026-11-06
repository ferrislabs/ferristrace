import { useMemo } from 'react'
import { NativeSelect } from '@/components/ui/input'
import { useAttributeKeys } from '@/data/attributes'

const BUILT_GROUPS = [
  { value: 'service', label: 'Service' },
  { value: 'status', label: 'Status' },
  { value: 'kind', label: 'Kind' },
  { value: 'name', label: 'Span name' },
  { value: 'http_status_code', label: 'HTTP status code' },
]

export function GroupSelect({ group, onChange, rangeMs, none }: { group: string; onChange: (group: string) => void; rangeMs: number; none: boolean }) {
  const { keys } = useAttributeKeys(rangeMs, '', true)
  const attributes = useMemo(() => keys.slice(0, 40).map((key) => key.key), [keys])
  return (
    <label className="flex items-center gap-2 text-caption font-semibold text-muted-foreground">
      Group by
      <NativeSelect aria-label="Group by" value={group} onChange={(event) => onChange(event.target.value)} className="h-8 w-[220px]">
        {none ? <option value="">Nothing</option> : null}
        <optgroup label="Fields">
          {BUILT_GROUPS.map((one) => <option key={one.value} value={one.value}>{one.label}</option>)}
        </optgroup>
        <optgroup label="Attributes">
          {group.startsWith('attr:') && !attributes.includes(group.slice(5)) ? <option value={group}>{group.slice(5)}</option> : null}
          {attributes.map((key) => <option key={key} value={`attr:${key}`}>{key}</option>)}
        </optgroup>
      </NativeSelect>
    </label>
  )
}
