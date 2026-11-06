import { ChevronDown } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { Input } from '@/components/ui/input'
import { useAttributeKeys } from '@/data/attributes'
import { facetKey, useFacets } from '@/data/facets'
import { dur } from '@/lib/format'
import { cn } from '@/lib/utils'
import type { Node } from '@/lib/queryLang'
import { durationBounds, selectedValues, withDuration, withValue, type Chip, type FieldRef } from '@/lib/traceQuery'

type Def = { id: string; field: FieldRef; label: string; numeric?: boolean }

const BASE: Def[] = [
  { id: facetKey({ built: 'service' }), field: { built: 'service' }, label: 'Service' },
  { id: facetKey({ built: 'status' }), field: { built: 'status' }, label: 'Status' },
  { id: facetKey({ built: 'kind' }), field: { built: 'kind' }, label: 'Kind' },
  { id: facetKey({ built: 'name' }), field: { built: 'name' }, label: 'Span name' },
  { id: facetKey({ built: 'http_status_code' }), field: { built: 'http_status_code' }, label: 'HTTP status code', numeric: true },
]

const KNOWN = [
  'deployment.environment', 'deployment.environment.name', 'http.method', 'http.request.method', 'http.route', 'http.host',
  'server.address', 'http.url', 'url.full', 'rpc.service', 'rpc.method', 'db.system', 'db.operation', 'messaging.system',
  'peer.service', 'k8s.namespace.name', 'k8s.deployment.name', 'k8s.pod.name', 'k8s.node.name',
]

const MAX_ATTRIBUTE_FACETS = 8

const DURATION_ID = 'duration'

const OPEN_KEY = 'ft.facets.open'

const DEFAULT_OPEN = [DURATION_ID, BASE[0].id, BASE[1].id]

function loadOpen(): Set<string> {
  try {
    const saved = window.localStorage.getItem(OPEN_KEY)
    const parsed: unknown = saved ? JSON.parse(saved) : null
    if (Array.isArray(parsed)) return new Set(parsed.filter((item): item is string => typeof item === 'string'))
  } catch {
    return new Set(DEFAULT_OPEN)
  }
  return new Set(DEFAULT_OPEN)
}

const compact = new Intl.NumberFormat('en', { notation: 'compact', maximumFractionDigits: 1 })

export function Facets({ chips, query, onChange, rangeMs }: { chips: Chip[]; query: Node[]; onChange: (chips: Chip[]) => void; rangeMs: number }) {
  const { keys } = useAttributeKeys(rangeMs, '', true)
  const [open, setOpen] = useState(loadOpen)
  useEffect(() => {
    try {
      window.localStorage.setItem(OPEN_KEY, JSON.stringify([...open]))
    } catch {
      return
    }
  }, [open])

  const defs = useMemo<Def[]>(() => {
    const seen = new Map(keys.map((key) => [key.key, key]))
    const attributes = KNOWN.filter((known) => seen.has(known))
      .sort((a, b) => (seen.get(b)?.spans ?? 0) - (seen.get(a)?.spans ?? 0))
      .slice(0, MAX_ATTRIBUTE_FACETS)
      .map<Def>((attr) => ({ id: facetKey({ attr }), field: { attr }, label: attr, numeric: seen.get(attr)?.numeric }))
    return [...BASE, ...attributes]
  }, [keys])

  const wanted = defs.filter((def) => open.has(def.id)).map((def) => def.field)
  const { facets, spread, loading } = useFacets(query, rangeMs, wanted, open.has(DURATION_ID))
  const toggle = (id: string) => setOpen((was) => { const next = new Set(was); if (!next.delete(id)) next.add(id); return next })

  return (
    <div className="flex flex-col" role="group" aria-label="Facets">
      <Section id={DURATION_ID} label="Duration" open={open.has(DURATION_ID)} onToggle={toggle} chosen={Object.values(durationBounds(chips)).filter((one) => one !== undefined).length}>
        <DurationRange chips={chips} onChange={onChange} seen={spread} />
      </Section>
      {defs.map((def) => {
        const chosen = selectedValues(chips, def.field)
        const shown = facets.get(def.id)
        const values = shown ? [...shown, ...chosen.filter((one) => !shown.some((seen) => seen.value === one)).map((value) => ({ value, spans: -1 }))] : []
        return (
          <Section key={def.id} id={def.id} label={def.label} open={open.has(def.id)} onToggle={toggle} chosen={chosen.length}>
            {shown === undefined ? (
              <p className="m-0 px-1 py-2 text-caption text-muted-foreground">{loading ? 'Counting…' : 'Nothing to show.'}</p>
            ) : values.length === 0 ? (
              <p className="m-0 px-1 py-2 text-caption text-muted-foreground">No values found</p>
            ) : (
              <ul className="m-0 flex list-none flex-col p-0">
                {chosen.length > 0 ? (
                  <li className="px-1 pb-1">
                    <button type="button" onClick={() => onChange(chosen.reduce((next, value) => withValue(next, def.field, value, false, def.numeric === true), chips))} className="cursor-pointer bg-transparent p-0 text-caption text-accent-text hover:underline focus-visible:outline-2 focus-visible:outline-ring">Clear</button>
                  </li>
                ) : null}
                {values.map((one) => {
                  const on = chosen.includes(one.value)
                  return (
                    <li key={one.value}>
                      <label className={cn('flex h-7 cursor-pointer items-center gap-2 rounded-sm px-1 text-control hover:bg-muted', on && 'font-semibold')}>
                        <input type="checkbox" checked={on} onChange={(event) => onChange(withValue(chips, def.field, one.value, event.target.checked, def.numeric === true))} className="size-3.5 shrink-0 cursor-pointer accent-[var(--color-primary)]" />
                        <span className="min-w-0 flex-1 truncate" title={one.value}>{one.value}</span>
                        <span className="font-mono text-[11px] text-faint-foreground tabular">{one.spans < 0 ? '—' : compact.format(one.spans)}</span>
                      </label>
                    </li>
                  )
                })}
              </ul>
            )}
          </Section>
        )
      })}
    </div>
  )
}

function Section({ id, label, open, chosen, onToggle, children }: { id: string; label: string; open: boolean; chosen: number; onToggle: (id: string) => void; children: React.ReactNode }) {
  return (
    <section className="border-b border-border-subtle py-1">
      <button type="button" aria-expanded={open} onClick={() => onToggle(id)} className="flex h-8 w-full cursor-pointer items-center gap-1.5 rounded-sm bg-transparent px-1 text-left text-control font-semibold hover:bg-muted focus-visible:outline-2 focus-visible:outline-ring">
        <ChevronDown className={cn('size-3.5 text-muted-foreground transition-transform', !open && '-rotate-90')} aria-hidden />
        <span className="flex-1 truncate">{label}</span>
        {chosen > 0 ? <span className="rounded-full bg-primary px-1.5 font-mono text-[11px] font-semibold leading-[18px] text-primary-foreground">{chosen}</span> : null}
      </button>
      {open ? <div className="px-1 pb-1.5 pt-0.5">{children}</div> : null}
    </section>
  )
}

function DurationRange({ chips, onChange, seen }: { chips: Chip[]; onChange: (chips: Chip[]) => void; seen: { minUs: number; maxUs: number } | null }) {
  const bounds = durationBounds(chips)
  const [min, setMin] = useState(bounds.min?.toString() ?? '')
  const [max, setMax] = useState(bounds.max?.toString() ?? '')
  useEffect(() => { setMin(bounds.min?.toString() ?? '') }, [bounds.min])
  useEffect(() => { setMax(bounds.max?.toString() ?? '') }, [bounds.max])
  const parse = (text: string): number | undefined => {
    const number = Number(text)
    return text.trim() !== '' && Number.isFinite(number) && number >= 0 ? number : undefined
  }
  const apply = () => {
    const low = parse(min)
    const high = parse(max)
    if (low === bounds.min && high === bounds.max) return
    onChange(withDuration(chips, low, high))
  }
  const field = (label: string, value: string, set: (value: string) => void) => (
    <label className="flex items-center gap-2 text-caption font-semibold text-muted-foreground">
      <span className="w-7">{label}</span>
      <Input value={value} inputMode="decimal" aria-label={`${label} duration in milliseconds`} placeholder={label === 'Min' ? '0' : 'no limit'} onChange={(event) => set(event.target.value)} onBlur={apply} onKeyDown={(event) => { if (event.key === 'Enter') apply() }} className="h-7 flex-1" />
      <span className="font-mono text-[11px] font-normal">ms</span>
    </label>
  )
  return (
    <div className="flex flex-col gap-1.5">
      {field('Min', min, setMin)}
      {field('Max', max, setMax)}
      {seen ? <p className="m-0 text-caption text-muted-foreground">Seen: {dur(seen.minUs / 1000)} to {dur(seen.maxUs / 1000)}</p> : null}
    </div>
  )
}
