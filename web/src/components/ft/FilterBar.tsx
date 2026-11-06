import { Plus, X } from 'lucide-react'
import { useId, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Input, NativeSelect } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { useAttributeKeys, useAttributeValues } from '@/data/attributes'
import { useServiceNames } from '@/data/services'
import { useSettled } from '@/lib/settle'
import { useSignal } from '@/lib/signal'
import { cn } from '@/lib/utils'
import { isChip, nodeText, type Node } from '@/lib/queryLang'
import {
  builtInsFor, CHOICES, MAX_CHIPS, OPS, kindOf, labelOf, opsFor, parsedValue, show,
  type Chip, type FieldRef, type Op,
} from '@/lib/traceQuery'

const sentence = (chip: Chip) => {
  const op = OPS[chip.op]
  return op.bare ? `${labelOf(chip.field)} ${op.label}` : `${labelOf(chip.field)} ${op.label} ${show(chip.value)}`
}

export function FilterBar({ items, onChange, rangeMs }: { items: Node[]; onChange: (items: Node[]) => void; rangeMs: number }) {
  const [editing, setEditing] = useState<number | 'new' | null>(null)
  const replace = (at: number | 'new', chip: Chip) => {
    onChange(at === 'new' ? [...items, chip] : items.map((was, index) => (index === at ? chip : was)))
    setEditing(null)
  }
  const remove = (index: number) => onChange(items.filter((_, at) => at !== index))
  return (
    <div className="flex flex-wrap items-center gap-1.5" role="group" aria-label="Filters">
      {items.map((item, index) => {
        if (!isChip(item)) {
          const text = nodeText(item)
          return (
            <span key={`${text}-${index}`} className="inline-flex h-7 items-center rounded-md border bg-muted text-control">
              <span className="max-w-[360px] truncate px-2 font-mono text-[12px]" title={`${text}. Edit it in the query box.`}>{text}</span>
              <button type="button" aria-label={`Remove filter: ${text}`} onClick={() => remove(index)} className="inline-flex h-full w-6 cursor-pointer items-center justify-center rounded-r-md text-muted-foreground hover:bg-border-subtle hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring">
                <X className="size-3.5" aria-hidden />
              </button>
            </span>
          )
        }
        const chip = item
        return (
          <span key={`${sentence(chip)}-${index}`} className="inline-flex h-7 items-center rounded-md border bg-muted text-control">
            <Popover open={editing === index} onOpenChange={(open) => setEditing(open ? index : null)}>
              <PopoverTrigger asChild>
                <button type="button" className="h-full max-w-[320px] cursor-pointer truncate rounded-l-md px-2 text-left hover:bg-border-subtle focus-visible:outline-2 focus-visible:outline-ring" aria-label={`Edit filter: ${sentence(chip)}`}>
                  <span className="font-semibold">{labelOf(chip.field)}</span> <span className="text-muted-foreground">{OPS[chip.op].label}</span>{OPS[chip.op].bare ? null : <> <span>{show(chip.value)}</span></>}
                </button>
              </PopoverTrigger>
              <PopoverContent align="start" className="w-[340px] p-3">
                <ChipEditor start={chip} rangeMs={rangeMs} onApply={(next) => replace(index, next)} onCancel={() => setEditing(null)} />
              </PopoverContent>
            </Popover>
            <button type="button" aria-label={`Remove filter: ${sentence(chip)}`} onClick={() => remove(index)} className="inline-flex h-full w-6 cursor-pointer items-center justify-center rounded-r-md text-muted-foreground hover:bg-border-subtle hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring">
              <X className="size-3.5" aria-hidden />
            </button>
          </span>
        )
      })}
      {items.length < MAX_CHIPS ? (
        <Popover open={editing === 'new'} onOpenChange={(open) => setEditing(open ? 'new' : null)}>
          <PopoverTrigger asChild>
            <Button variant="outline" size="sm"><Plus />Add filter</Button>
          </PopoverTrigger>
          <PopoverContent align="start" className="w-[340px] p-3">
            <ChipEditor rangeMs={rangeMs} onApply={(next) => replace('new', next)} onCancel={() => setEditing(null)} />
          </PopoverContent>
        </Popover>
      ) : null}
    </div>
  )
}

function ChipEditor({ start, rangeMs, onApply, onCancel }: { start?: Chip; rangeMs: number; onApply: (chip: Chip) => void; onCancel: () => void }) {
  const signal = useSignal()
  const [field, setField] = useState<FieldRef | null>(start?.field ?? null)
  const [typed, setTyped] = useState(start ? labelOf(start.field) : '')
  const [op, setOp] = useState<Op>(start?.op ?? 'eq')
  const [value, setValue] = useState(start ? show(start.value) : '')
  const list = useId()
  const settledPrefix = useSettled(field ? '' : typed)
  const { keys, loading } = useAttributeKeys(rangeMs, settledPrefix, field === null)
  const [numeric, setNumeric] = useState(() => (Array.isArray(start?.value) ? start.value : [start?.value]).every((one) => typeof one === 'number'))

  const kind = field ? kindOf(field, numeric) : 'text'
  const ops = field ? opsFor(field, kind === 'number') : []
  const bare = OPS[op].bare === true
  const isList = OPS[op].list === true

  const pick = (next: FieldRef, isNumeric = false) => {
    setNumeric(isNumeric)
    setField(next)
    setTyped(labelOf(next))
    setOp('eq')
    setValue('')
  }
  const back = () => {
    setField(null)
    setTyped('')
  }

  const suggestAttribute = 'attr' in (field ?? { built: 'service' }) ? (field as { attr: string }).attr : undefined
  const { values } = useAttributeValues(suggestAttribute, rangeMs, '')
  const { names: services } = useServiceNames(rangeMs)
  const options =
    field && 'built' in field
      ? field.built === 'service' ? services : CHOICES[field.built] ?? []
      : values.map((seen) => seen.value)

  const parsed = bare ? undefined : parsedValue(value, kind, isList)
  const ready = field !== null && (bare || parsed !== null)
  const apply = () => {
    if (!field || !ready) return
    onApply(bare ? { field, op } : { field, op, value: parsed ?? undefined })
  }

  const wanted = typed.trim().toLowerCase()
  const matches = builtInsFor(signal).filter((built) => built.label.toLowerCase().includes(wanted) || built.name.includes(wanted))
  const attributes = keys.filter((key) => key.key.toLowerCase().includes(wanted)).slice(0, 12)
  const unknownKey = wanted !== '' && !keys.some((key) => key.key === typed.trim())

  return (
    <form className="flex flex-col gap-2.5" onSubmit={(event) => { event.preventDefault(); apply() }}>
      <label className="flex flex-col gap-1 text-caption font-semibold text-muted-foreground">
        Field
        <Input
          autoFocus={field === null}
          value={typed}
          placeholder="Service, http.method, rpc.method…"
          onChange={(event) => { setTyped(event.target.value); if (field) setField(null) }}
          onKeyDown={(event) => { if (event.key === 'Escape') onCancel() }}
          aria-label="Field"
        />
      </label>
      {field === null ? (
        <div role="listbox" aria-label="Fields" className="flex max-h-[240px] flex-col overflow-y-auto rounded-md border">
          {matches.map((built) => (
            <button key={built.name} type="button" role="option" aria-selected={false} onClick={() => pick({ built: built.name })} className="flex h-8 shrink-0 cursor-pointer items-center justify-between px-2 text-left text-control hover:bg-muted focus-visible:bg-muted focus-visible:outline-none">
              {built.label}<span className="font-mono text-[11px] text-faint-foreground">built-in</span>
            </button>
          ))}
          {attributes.map((key) => (
            <button key={key.key} type="button" role="option" aria-selected={false} onClick={() => pick({ attr: key.key }, key.numeric)} className="flex h-8 shrink-0 cursor-pointer items-center justify-between gap-2 px-2 text-left text-control hover:bg-muted focus-visible:bg-muted focus-visible:outline-none">
              <span className="truncate font-mono text-[12.5px]">{key.key}</span>
              <span className="font-mono text-[11px] text-faint-foreground">{key.numeric ? 'number' : 'text'} · {key.spans}</span>
            </button>
          ))}
          {unknownKey ? (
            <button type="button" role="option" aria-selected={false} onClick={() => pick({ attr: typed.trim() })} className="h-8 shrink-0 cursor-pointer px-2 text-left text-control text-accent-text hover:bg-muted focus-visible:bg-muted focus-visible:outline-none">
              Use attribute “{typed.trim()}”
            </button>
          ) : null}
          {loading && attributes.length === 0 && matches.length === 0 ? <span className="px-2 py-2 text-caption text-muted-foreground">Looking for attributes…</span> : null}
        </div>
      ) : (
        <>
          <label className="flex flex-col gap-1 text-caption font-semibold text-muted-foreground">
            Operator
            <NativeSelect aria-label="Operator" value={op} onChange={(event) => setOp(event.target.value as Op)}>
              {ops.map((one) => <option key={one} value={one}>{OPS[one].label}</option>)}
            </NativeSelect>
          </label>
          {bare ? null : (
            <label className="flex flex-col gap-1 text-caption font-semibold text-muted-foreground">
              Value{isList ? ' (separate with commas)' : ''}{'built' in field && field.built === 'duration' ? ' (ms)' : ''}
              <Input
                value={value}
                list={list}
                inputMode={kind === 'number' ? 'decimal' : undefined}
                autoFocus
                onChange={(event) => setValue(event.target.value)}
                onKeyDown={(event) => { if (event.key === 'Escape') onCancel() }}
                aria-label="Value"
              />
              <datalist id={list}>{options.slice(0, 20).map((option) => <option key={option} value={option} />)}</datalist>
            </label>
          )}
        </>
      )}
      <div className={cn('flex items-center gap-2', field === null && 'hidden')}>
        <Button type="button" variant="ghost" size="sm" onClick={back}>Change field</Button>
        <span className="flex-1" />
        <Button type="button" variant="ghost" size="sm" onClick={onCancel}>Cancel</Button>
        <Button type="submit" variant="primary" size="sm" disabled={!ready}>{start ? 'Update' : 'Add'}</Button>
      </div>
    </form>
  )
}

