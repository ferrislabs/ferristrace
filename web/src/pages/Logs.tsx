import { useNavigate as useRouterNavigate } from '@tanstack/react-router'
import { Braces, ChevronRight, Copy, Plus, Radio, Search, Waypoints, X } from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { Link, useSearchParam } from '@/lib/router'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { InputGroup, NativeSelect } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { Kbd } from '@/components/ui/misc'
import { EmptyState, PageHeader, SeverityBadge } from '@/components/ft/bits'
import { AttachToIncident } from '@/components/ft/AttachToIncident'
import {
  ANY,
  SEVERITIES,
  isPinned,
  msOf,
  useLogHistogram,
  useLogs,
  type Bucket,
  type LogQuery,
  type LogRecord,
} from '@/data/logs'
import { useServiceNames } from '@/data/services'
import { isPlaced, usePlacement, type Placement } from '@/data/placement'
import { PlacementChips, PlacementPicker, placedCount } from '@/components/ft/Placement'
import { Filters } from '@/components/ft/Filters'
import { ApiError, because } from '@/auth/client'
import { shortId, stamp } from '@/lib/format'
import { useSettled } from '@/lib/settle'
import type { Sev } from '@/lib/otel'
import { cn } from '@/lib/utils'
import { useWindow } from '@/lib/window'

const BAR: Record<Sev, string> = {
  trace: 'bg-sev-trace-bd',
  debug: 'bg-sev-debug-fg',
  info: 'bg-sev-info-fg',
  warn: 'bg-sev-warn-bd',
  error: 'bg-sev-error-fg',
  fatal: 'bg-sev-fatal-bg',
}

function decode(raw: string | undefined): LogQuery {
  if (!raw) return ANY
  const p = new URLSearchParams(raw)
  const number = (key: string) => {
    const value = Number(p.get(key))
    return p.get(key) && Number.isFinite(value) && value > 0 ? value : null
  }
  const from = number('from')
  const to = number('to')
  const pinned = from !== null && to !== null && from < to
  return {
    ...ANY,
    fromMs: pinned ? from : null,
    toMs: pinned ? to : null,
    services: p.getAll('service'),
    severities: p.getAll('severity').filter((s): s is Sev => (SEVERITIES as string[]).includes(s)),
    q: p.get('q') ?? '',
    attrs: p.getAll('attr'),
    trace: p.get('trace') ?? '',
  }
}

function encode(query: LogQuery): string | undefined {
  const p = new URLSearchParams()
  if (isPinned(query)) {
    p.set('from', String(query.fromMs))
    p.set('to', String(query.toMs))
  }
  for (const service of query.services) p.append('service', service)
  for (const severity of query.severities) p.append('severity', severity)
  if (query.q) p.set('q', query.q)
  for (const attr of query.attrs) p.append('attr', attr)
  if (query.trace) p.set('trace', query.trace)
  const out = p.toString()
  return out || undefined
}

const pad = (n: number, width = 2) => String(n).padStart(width, '0')

const idOf = (record: LogRecord) => `${record.time_ns}:${record.observed_ns}:${record.span_id ?? ''}:${record.body}`

function clock(ns: number, withDate: boolean): string {
  const ms = msOf(ns)
  const at = new Date(ms)
  const time = `${pad(at.getHours())}:${pad(at.getMinutes())}:${pad(at.getSeconds())}.${pad(ms % 1000, 3)}`
  return withDate ? `${pad(at.getMonth() + 1)}-${pad(at.getDate())} ${time}` : time
}

const total = (bucket: Bucket) => SEVERITIES.reduce((sum, level) => sum + bucket.counts[level], 0)

function Histogram({ buckets, windowMs, onPick }: { buckets: Bucket[]; windowMs: number; onPick: (from: number, to: number) => void }) {
  const peak = Math.max(1, ...buckets.map(total))
  const width = buckets.length > 1 ? msOf(buckets[1].start_ns - buckets[0].start_ns) : Math.max(1, Math.round(windowMs / 60))
  return (
    <div className="flex h-24 items-end gap-px rounded-md border bg-card px-2 pt-2" role="group" aria-label="Log volume by severity, select a bar to narrow the window">
      {buckets.map((bucket) => {
        const sum = total(bucket)
        const start = msOf(bucket.start_ns)
        const label = `${stamp(start)}, ${sum} record${sum === 1 ? '' : 's'}`
        return (
          <button
            key={bucket.start_ns}
            type="button"
            title={label}
            aria-label={label}
            disabled={sum === 0}
            onClick={() => onPick(start, start + width)}
            className="flex h-full min-w-0 flex-1 cursor-pointer flex-col justify-end rounded-sm hover:bg-row-hover focus-visible:outline-2 focus-visible:outline-ring disabled:cursor-default"
          >
            <span className="flex w-full flex-col-reverse" style={{ height: `${(sum / peak) * 100}%` }}>
              {SEVERITIES.map((level) => bucket.counts[level] > 0 ? <span key={level} className={BAR[level]} style={{ height: `${(bucket.counts[level] / sum) * 100}%` }} /> : null)}
            </span>
          </button>
        )
      })}
    </div>
  )
}

function Row({ record, index, open, focused, onToggle, dated, onPlace, onAttr }: { record: LogRecord; index: number; open: boolean; focused: boolean; onToggle: () => void; dated: boolean; onPlace: (part: Partial<Placement>) => void; onAttr: (pair: string) => void }) {
  const hot = record.severity === 'error' || record.severity === 'fatal'
  const attributes = Object.entries(record.attributes)
  const resource = Object.entries(record.resource)
  const copy = (whole: boolean) => {
    void navigator.clipboard?.writeText(whole ? JSON.stringify(record, null, 2) : record.body)
    toast.success(whole ? 'Record copied as JSON' : 'Body copied')
  }
  return (
    <div role="listitem" data-row={index} className="border-b border-border-subtle">
      <button type="button" aria-expanded={open} onClick={onToggle} className={cn('grid w-full cursor-pointer grid-cols-[16px_128px_70px_140px_minmax(0,1fr)] items-center gap-3 px-5 py-1.5 text-left hover:bg-row-hover focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring max-md:grid-cols-[16px_70px_minmax(0,1fr)]', open && 'bg-accent-subtle', focused && 'outline-2 -outline-offset-2 outline-ring')}>
        <ChevronRight className={cn('size-3.5 text-muted-foreground transition-transform', open && 'rotate-90')} aria-hidden />
        <time className="font-mono text-caption text-muted-foreground max-md:hidden" dateTime={new Date(msOf(record.time_ns)).toISOString()}>{clock(record.time_ns, dated)}</time>
        <SeverityBadge level={record.severity} />
        <span className="truncate text-caption text-muted-foreground max-md:hidden">{record.service}</span>
        <span className={cn('truncate font-mono text-[12.5px]', hot && 'text-destructive-text')}>{record.body}</span>
      </button>
      {open ? (
        <div className="flex flex-col gap-2 bg-sidebar px-5 py-3 pl-[52px]">
          <pre className="m-0 whitespace-pre-wrap break-all font-mono text-[12.5px]">{record.body}</pre>
          <div className="flex flex-wrap items-center gap-2">
            <Button variant="outline" size="sm" onClick={() => copy(false)}><Copy aria-hidden />Copy body</Button>
            <Button variant="outline" size="sm" onClick={() => copy(true)}><Braces aria-hidden />Copy JSON</Button>
          </div>
          <PlacementChips attributes={record.resource} onPick={onPlace} />
          <Pairs title="Attributes" pairs={attributes} onPick={onAttr} />
          <Pairs title="Resource" pairs={resource} />
          {record.trace_id ? (
            <Link to={`/traces/${record.trace_id}`} className="inline-flex w-fit items-center gap-1.5 rounded-sm text-control focus-visible:outline-2 focus-visible:outline-ring"><Waypoints className="size-3.5" aria-hidden />Open trace {shortId(record.trace_id)}</Link>
          ) : <span className="text-caption text-muted-foreground">No trace attached to this record.</span>}
        </div>
      ) : null}
    </div>
  )
}

function Pairs({ title, pairs, onPick }: { title: string; pairs: [string, string][]; onPick?: (pair: string) => void }) {
  if (pairs.length === 0) return null
  return (
    <section aria-label={title}>
      <h3 className="m-0 text-caption font-semibold text-muted-foreground">{title}</h3>
      <dl className="m-0 grid max-w-[720px] grid-cols-[200px_1fr] gap-x-3 text-caption">
        {pairs.map(([k, v]) => (
          <div key={k} className="contents">
            <dt className="py-1 font-mono text-muted-foreground">{k}</dt>
            <dd className="m-0 break-all py-1 font-mono">
              {onPick
                ? <button type="button" onClick={() => onPick(`${k}=${v}`)} title={`Keep only records where ${k} is ${v}`} className="cursor-pointer rounded-sm text-left underline decoration-dotted decoration-muted-foreground underline-offset-2 hover:decoration-foreground focus-visible:outline-2 focus-visible:outline-ring">{v}</button>
                : v}
            </dd>
          </div>
        ))}
      </dl>
    </section>
  )
}

function Chip({ children, onRemove, label }: { children: React.ReactNode; onRemove: () => void; label: string }) {
  return (
    <span className="inline-flex h-6 items-center gap-1 rounded-sm border bg-muted pl-2 pr-1 font-mono text-caption">
      {children}
      <button type="button" aria-label={label} onClick={onRemove} className="inline-flex size-4 cursor-pointer items-center justify-center rounded-sm hover:bg-border-subtle focus-visible:outline-2 focus-visible:outline-ring"><X className="size-3" aria-hidden /></button>
    </span>
  )
}

export function Logs() {
  const navigate = useRouterNavigate()
  const raw = useSearchParam('f')
  const span = useWindow()
  const query = useMemo(() => ({ ...decode(raw), rangeMs: span.ms }), [raw, span.ms])
  const [typed, setTyped] = useState(query.q)
  const [attr, setAttr] = useState('')
  const [live, setLive] = useState(false)
  const [open, setOpen] = useState<string | null>(null)
  const [cursor, setCursor] = useState(-1)
  const list = useRef<HTMLDivElement>(null)
  const settled = useSettled(typed)

  const { placement, place } = usePlacement()
  const apply = (part: Partial<LogQuery>, keepPlacement = true) => {
    const f = encode({ ...query, ...part })
    void navigate({ to: '/logs' as never, search: (keepPlacement ? (was: Record<string, unknown>) => ({ ...was, f }) : { f }) as never, replace: true })
    setOpen(null)
    setCursor(-1)
  }
  const placeHere = (part: Partial<Placement>) => { place(part); setOpen(null); setCursor(-1) }

  useEffect(() => {
    if (settled === typed && settled !== query.q) apply({ q: settled })
  }, [settled])

  useEffect(() => {
    setTyped(query.q)
  }, [query.q])

  const { records, loading, pending, failed, more, older, loadingOlder } = useLogs(query, live, placement)
  const histogram = useLogHistogram(query, live, placement)
  const { names } = useServiceNames(query.rangeMs)
  const ids = useMemo(() => {
    const seen = new Map<string, number>()
    return records.map((record) => {
      const base = idOf(record)
      const nth = seen.get(base) ?? 0
      seen.set(base, nth + 1)
      return nth === 0 ? base : `${base}#${nth}`
    })
  }, [records])
  const pickAttr = (pair: string) => { if (!query.attrs.includes(pair)) apply({ attrs: [...query.attrs, pair] }) }

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement
      if (el.tagName === 'INPUT' || el.tagName === 'SELECT' || el.tagName === 'TEXTAREA' || el.isContentEditable) return
      if (e.metaKey || e.ctrlKey || e.altKey || ids.length === 0) return
      if (e.key === 'j' || e.key === 'ArrowDown') { e.preventDefault(); setCursor((c) => Math.min(ids.length - 1, c + 1)); return }
      if (e.key === 'k' || e.key === 'ArrowUp') { e.preventDefault(); setCursor((c) => Math.max(0, c - 1)); return }
      if (e.key === 'Escape') { setOpen(null); return }
      if (e.key !== 'Enter' && e.key !== ' ') return
      if (el.closest('button, a')) return
      e.preventDefault()
      const at = Math.min(Math.max(cursor, 0), ids.length - 1)
      setCursor(at)
      setOpen((was) => (was === ids[at] ? null : ids[at]))
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [ids, cursor])

  useEffect(() => {
    if (cursor < 0) return
    list.current?.querySelector<HTMLElement>(`[data-row="${cursor}"]`)?.scrollIntoView({ block: 'nearest' })
  }, [cursor])

  const windowMs = isPinned(query) ? (query.toMs as number) - (query.fromMs as number) : query.rangeMs
  const dated = windowMs > 24 * 60 * 60 * 1000
  const sifting = Boolean(query.services.length || query.severities.length || query.q || query.attrs.length || query.trace || isPlaced(placement))
  const clear = () => { setTyped(''); apply({ services: [], severities: [], q: '', attrs: [], trace: '' }, false) }
  const toggle = <T,>(list: T[], one: T) => (list.includes(one) ? list.filter((x) => x !== one) : [...list, one])
  const addAttr = () => {
    const value = attr.trim()
    if (!value.includes('=') || value.startsWith('=') || query.attrs.includes(value)) return
    apply({ attrs: [...query.attrs, value] })
    setAttr('')
  }
  const range = isPinned(query) ? `${stamp(query.fromMs as number)} to ${stamp(query.toMs as number)}` : `last ${span.label}`
  const unconfigured = failed instanceof ApiError && failed.status === 503
  const pinnedNow = () => (isPinned(query) ? query : { ...query, fromMs: Date.now() - query.rangeMs, toMs: Date.now() })
  const attachment = {
    kind: 'logs' as const,
    reference: encode(pinnedNow()) ?? '',
    label: ['Logs', query.services.join(', '), query.severities.join(', '), query.q ? `"${query.q}"` : '', query.trace ? `trace ${query.trace.slice(0, 8)}` : ''].filter(Boolean).join(' · ').slice(0, 200),
  }
  const invalid = failed instanceof ApiError && failed.status === 422

  return (
    <>
      <PageHeader title="Logs" meta={`Newest first · ${range}${live ? ' · live' : ''}`}>
        <span className="hidden items-center gap-1 text-caption text-muted-foreground lg:inline-flex"><Kbd>J</Kbd><Kbd>K</Kbd>move<Kbd>↵</Kbd>expand</span>
        <AttachToIncident attachment={attachment} />
        <Switch checked={live} onCheckedChange={setLive} label={<span className="inline-flex items-center gap-1.5"><Radio className="size-3.5" aria-hidden />Live</span>} />
      </PageHeader>
      <Filters active={query.services.length + query.attrs.length + (query.q ? 1 : 0) + (query.trace ? 1 : 0) + placedCount(placement)}>
      <div className="flex flex-wrap items-center gap-2" role="search">
        {isPinned(query) ? <Button size="sm" onClick={() => apply({ fromMs: null, toMs: null })}><X aria-hidden />Back to the last {span.label}</Button> : null}
        <NativeSelect aria-label="Add a service" value="" onChange={(e) => e.target.value && apply({ services: toggle(query.services, e.target.value) })} className="w-[170px]">
          <option value="">Service: add…</option>
          {names.filter((n) => !query.services.includes(n)).map((n) => <option key={n} value={n}>{n}</option>)}
        </NativeSelect>
        <InputGroup className="w-[300px] max-w-full"><Search aria-hidden /><input aria-label="Search log bodies" placeholder='Search bodies, e.g. "timeout"' value={typed} onChange={(e) => setTyped(e.target.value)} /></InputGroup>
        <form className="flex items-center gap-1" onSubmit={(e) => { e.preventDefault(); addAttr() }}>
          <InputGroup className="w-[240px] max-w-full"><input aria-label="Attribute filter" placeholder="attribute key=value" className="font-mono" value={attr} onChange={(e) => setAttr(e.target.value)} /></InputGroup>
          <Button type="submit" variant="outline" size="icon" aria-label="Add attribute filter" disabled={!attr.includes('=')}><Plus aria-hidden /></Button>
        </form>
        <Button variant="ghost" size="sm" disabled={!sifting && !isPinned(query)} onClick={() => { setTyped(''); apply({ services: [], severities: [], q: '', attrs: [], trace: '', fromMs: null, toMs: null }, false) }}>Clear</Button>
      </div>
      <PlacementPicker placement={placement} onChange={placeHere} windowMs={windowMs} />
      </Filters>
      <div className="flex flex-wrap items-center gap-2">
        <div className="flex flex-wrap gap-1" role="group" aria-label="Severities">
          {SEVERITIES.map((level) => {
            const on = query.severities.includes(level)
            return (
              <button key={level} type="button" aria-pressed={on} onClick={() => apply({ severities: toggle(query.severities, level) })} className={cn('cursor-pointer rounded-sm focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-ring', query.severities.length > 0 && !on && 'opacity-45')}>
                <SeverityBadge level={level} />
              </button>
            )
          })}
        </div>
        {query.services.map((s) => <Chip key={s} label={`Remove service ${s}`} onRemove={() => apply({ services: query.services.filter((x) => x !== s) })}>service:{s}</Chip>)}
        {query.attrs.map((a) => <Chip key={a} label={`Remove attribute ${a}`} onRemove={() => apply({ attrs: query.attrs.filter((x) => x !== a) })}>{a}</Chip>)}
        {query.trace ? <Chip label="Remove trace filter" onRemove={() => apply({ trace: '' })}>trace:{shortId(query.trace)}</Chip> : null}
      </div>
      {!unconfigured && !invalid && !histogram.failed && histogram.buckets.length > 0 ? (
        <Histogram buckets={histogram.buckets} windowMs={windowMs} onPick={(from, to) => apply({ fromMs: from, toMs: to })} />
      ) : null}
      {unconfigured ? (
        <EmptyState title="Logs are not configured">This deployment has no span store, so there are no logs to read. Configure one to see log records here.</EmptyState>
      ) : invalid ? (
        <EmptyState title="That search is not valid" actions={<Button size="sm" onClick={clear}>Clear filters</Button>}>{because(failed, 'Check the attribute filters and the trace id.')}</EmptyState>
      ) : failed ? (
        <EmptyState title="The logs could not be read">{because(failed, 'The log store did not answer.')}</EmptyState>
      ) : records.length === 0 ? (
        <EmptyState title={pending || loading ? 'Reading the logs…' : sifting ? 'No log records match' : 'No logs in this window'} actions={sifting && !pending ? <Button size="sm" onClick={clear}>Clear filters</Button> : null}>
          {pending || loading ? 'Asking the log store.' : sifting ? 'Try fewer filters, a shorter search, or a wider window.' : 'Nothing arrived in the window you are looking at. Try a wider one.'}
        </EmptyState>
      ) : (
        <div className="-mx-4 border-t md:-mx-5">
          <div role="list" aria-label="Log records" ref={list}>
            {records.map((record, i) => {
              const id = ids[i]
              return <Row key={id} record={record} index={i} dated={dated} open={open === id} focused={i === cursor} onToggle={() => { setOpen(open === id ? null : id); setCursor(i) }} onPlace={placeHere} onAttr={pickAttr} />
            })}
          </div>
          <div className="flex flex-wrap items-center gap-3 px-5 py-3">
            <span className={cn('text-caption', loading ? 'text-sev-info-fg' : 'text-muted-foreground')}>{records.length} record{records.length === 1 ? '' : 's'} loaded{live ? ', refreshing every 5 s' : ''}.</span>
            {more ? <Button size="sm" onClick={older} disabled={loadingOlder}>{loadingOlder ? 'Loading…' : 'Load older'}</Button> : null}
          </div>
        </div>
      )}
    </>
  )
}
