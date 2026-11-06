import { useNavigate as useRouterNavigate } from '@tanstack/react-router'
import { Boxes, Braces, ChevronRight, Copy, ListFilter, Radio, Waypoints } from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { Link, useSearchParam } from '@/lib/router'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Switch } from '@/components/ui/switch'
import { Kbd } from '@/components/ui/misc'
import { EmptyState, PageHeader, SeverityBadge } from '@/components/ft/bits'
import { AttachToIncident } from '@/components/ft/AttachToIncident'
import { Facets } from '@/components/ft/Facets'
import { FilterBar } from '@/components/ft/FilterBar'
import { LogHistogram } from '@/components/ft/Histogram'
import { PlacementChips, PlacementPicker, placedCount } from '@/components/ft/Placement'
import { QueryBar } from '@/components/ft/QueryBar'
import { ViewsPanel, ViewsToggle, useViewsState } from '@/components/ft/Views'
import { legacyText, legacyZoom, msOf, useLogExplorer, type LogRecord } from '@/data/logs'
import { isPlaced, NOWHERE, usePlacement, type Placement } from '@/data/placement'
import type { Definition } from '@/data/views'
import { ApiError, because } from '@/auth/client'
import { useZoom } from '@/lib/bounds'
import { shortId } from '@/lib/format'
import { isChip, itemsText, normalise, parse, queryTextOf, type Node } from '@/lib/queryLang'
import { SignalProvider } from '@/lib/signal'
import type { Chip } from '@/lib/traceQuery'
import { cn } from '@/lib/utils'
import { useWindow } from '@/lib/window'

const pad = (n: number, width = 2) => String(n).padStart(width, '0')

const idOf = (record: LogRecord) => `${record.time_ns}:${record.observed_ns}:${record.span_id ?? ''}:${record.body}`

function clock(ns: number, withDate: boolean): string {
  const ms = msOf(ns)
  const at = new Date(ms)
  const time = `${pad(at.getHours())}:${pad(at.getMinutes())}:${pad(at.getSeconds())}.${pad(ms % 1000, 3)}`
  return withDate ? `${pad(at.getMonth() + 1)}-${pad(at.getDate())} ${time}` : time
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
      <button type="button" aria-expanded={open} onClick={onToggle} className={cn('grid w-full cursor-pointer grid-cols-[16px_128px_70px_140px_minmax(0,1fr)] items-center gap-3 px-3 py-1.5 text-left hover:bg-row-hover focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring max-md:grid-cols-[16px_70px_minmax(0,1fr)]', open && 'bg-accent-subtle', focused && 'outline-2 -outline-offset-2 outline-ring')}>
        <ChevronRight className={cn('size-3.5 text-muted-foreground transition-transform', open && 'rotate-90')} aria-hidden />
        <time className="font-mono text-caption text-muted-foreground max-md:hidden" dateTime={new Date(msOf(record.time_ns)).toISOString()}>{clock(record.time_ns, dated)}</time>
        <SeverityBadge level={record.severity} />
        <span className="truncate text-caption text-muted-foreground max-md:hidden">{record.service}</span>
        <span className={cn('truncate font-mono text-[12.5px]', hot && 'text-destructive-text')}>{record.body}</span>
      </button>
      {open ? (
        <div className="flex flex-col gap-2 bg-sidebar px-3 py-3 pl-[40px]">
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

export function Logs() {
  return (
    <SignalProvider value="logs">
      <LogsPage />
    </SignalProvider>
  )
}

const FIRST_PAGE = 100

function LogsPage() {
  const goTo = useRouterNavigate()
  const { ms: windowMs, label: windowLabel } = useWindow()
  const rawQuery = useSearchParam('q')
  const legacy = useSearchParam('f')
  const viewParam = useSearchParam('view')
  const zoom = useZoom()
  const text = useMemo(() => (rawQuery !== undefined ? queryTextOf(rawQuery) : legacyText(legacy)), [rawQuery, legacy])
  const parsed = useMemo(() => parse(text), [text])
  const items = useMemo<Node[]>(() => (parsed.ok ? parsed.items : []), [parsed])
  const chips = useMemo(() => items.filter(isChip), [items])
  const others = useMemo(() => items.filter((item) => !isChip(item)), [items])
  const { placement, place } = usePlacement()
  const [live, setLive] = useState(false)
  const [open, setOpen] = useState<string | null>(null)
  const [cursor, setCursor] = useState(-1)
  const [panel, setPanel] = useState<'facets' | 'views'>('facets')
  const [mobileOpen, setMobileOpen] = useState(false)
  const list = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (legacy === undefined) return
    const was = legacyZoom(legacy)
    void goTo({
      to: '.' as never,
      search: ((search: Record<string, unknown>) => ({
        ...search,
        f: undefined,
        q: search.q ?? (legacyText(legacy) || undefined),
        from: was ? was.from : search.from,
        to: was ? was.to : search.to,
      })) as never,
      replace: true,
    })
  }, [legacy, goTo])

  const viewKey = viewParam ?? (items.length === 0 ? 'recent' : undefined)
  const current = useMemo<Definition>(() => ({ q: normalise(text), o: 'started', t: 'traces', m: 'count', g: '' }), [text])
  const show = (next: Partial<{ q: string; view: string | undefined }>) => {
    const merged = { q: text, view: viewParam, ...next }
    void goTo({
      to: '.' as never,
      search: ((search: Record<string, unknown>) => ({ ...search, f: undefined, q: merged.q.trim() === '' ? undefined : merged.q, view: merged.view })) as never,
      replace: true,
    })
    setOpen(null)
    setCursor(-1)
  }
  const zoomTo = (from: number, to: number | undefined) => {
    void goTo({
      to: '.' as never,
      search: ((search: Record<string, unknown>) => ({ ...search, from: to === undefined ? undefined : Math.round(from), to: to === undefined ? undefined : Math.round(to) })) as never,
      replace: true,
    })
    setOpen(null)
    setCursor(-1)
  }
  const setItems = (next: Node[]) => show({ q: itemsText(next) })
  const setChips = (next: Chip[]) => setItems([...others, ...next])
  const select = (key: string | undefined, definition: Definition) => show({ q: definition.q, view: key })
  const viewsState = useViewsState(current, viewKey, select)
  const placeHere = (part: Partial<Placement>) => { place(part); setOpen(null); setCursor(-1) }
  const pickAttr = (pair: string) => {
    const at = pair.indexOf('=')
    if (at > 0) setItems([...items, { field: { attr: pair.slice(0, at) }, op: 'eq', value: pair.slice(at + 1) }])
  }

  const { records, loading, pending, failed, more, older, loadingOlder } = useLogExplorer(items, windowMs, FIRST_PAGE, live, placement)
  const ids = useMemo(() => {
    const seen = new Map<string, number>()
    return records.map((record) => {
      const base = idOf(record)
      const nth = seen.get(base) ?? 0
      seen.set(base, nth + 1)
      return nth === 0 ? base : `${base}#${nth}`
    })
  }, [records])

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

  const spanMs = zoom ? zoom.to - zoom.from : windowMs
  const dated = spanMs > 24 * 60 * 60 * 1000
  const sifting = items.length > 0 || isPlaced(placement)
  const clear = () => { show({ q: '', view: undefined }); place(NOWHERE) }
  const unconfigured = failed instanceof ApiError && failed.status === 503
  const invalid = failed instanceof ApiError && (failed.status === 422 || failed.status === 400)
  const range = zoom ? `${new Date(zoom.from).toLocaleString()} to ${new Date(zoom.to).toLocaleString()}` : `last ${windowLabel}`
  const pinnedNow = () => {
    const to = zoom ? zoom.to : Date.now()
    const from = zoom ? zoom.from : to - windowMs
    return { from, to }
  }
  const attachment = {
    kind: 'logs' as const,
    reference: new URLSearchParams({ query: text, from: String(pinnedNow().from), to: String(pinnedNow().to) }).toString(),
    label: `Logs · ${text || 'everything'}`.slice(0, 200),
  }

  return (
    <>
      <PageHeader title="Logs" meta={`Newest first · ${range}${live ? ' · live' : ''}`}>
        <span className="hidden items-center gap-1 text-caption text-muted-foreground lg:inline-flex"><Kbd>J</Kbd><Kbd>K</Kbd>move<Kbd>↵</Kbd>expand</span>
        <ViewsToggle state={viewsState} on={panel === 'views'} onToggle={() => { setPanel((was) => (was === 'views' ? 'facets' : 'views')); setMobileOpen(true) }} />
        <Button size="sm" variant="ghost" className="md:hidden" aria-expanded={mobileOpen} onClick={() => setMobileOpen((was) => !was)}>
          <ListFilter />Filters{items.length > 0 ? <span className="rounded-full bg-primary px-1.5 font-mono text-[11px] text-primary-foreground">{items.length}</span> : null}
        </Button>
        <AttachToIncident attachment={attachment} />
        <Switch checked={live} onCheckedChange={setLive} label={<span className="inline-flex items-center gap-1.5"><Radio className="size-3.5" aria-hidden />Live</span>} />
      </PageHeader>
      {viewsState.dialog}
      <div className="grid gap-4 md:grid-cols-[264px_minmax(0,1fr)]">
        <aside className={cn('min-w-0 md:sticky md:top-[100px] md:max-h-[calc(100vh-116px)] md:self-start md:overflow-y-auto md:pr-1', !mobileOpen && 'max-md:hidden')}>
          {panel === 'views' ? (
            <ViewsPanel state={viewsState} viewKey={viewKey} onSelect={select} onClose={() => setPanel('facets')} />
          ) : (
            <Facets chips={chips} query={items} onChange={setChips} rangeMs={windowMs} />
          )}
        </aside>
        <div className="flex min-w-0 flex-col gap-3">
          <QueryBar text={text} onCommit={(next) => show({ q: next })} rangeMs={windowMs} />
          {!parsed.ok ? <p role="alert" className="m-0 text-caption text-destructive-text">The query in this link is not valid, so nothing is filtered: {parsed.error}.</p> : null}
          <div className="flex flex-wrap items-center gap-2" role="search">
            <FilterBar items={items} onChange={setItems} rangeMs={windowMs} />
            <Popover>
              <PopoverTrigger asChild>
                <Button variant="outline" size="sm" aria-label="Kubernetes placement"><Boxes />Kubernetes{placedCount(placement) > 0 ? <span className="rounded-full bg-primary px-1.5 font-mono text-[11px] text-primary-foreground">{placedCount(placement)}</span> : null}</Button>
              </PopoverTrigger>
              <PopoverContent align="start" className="w-[min(560px,calc(100vw-32px))] p-3">
                <PlacementPicker placement={placement} onChange={placeHere} windowMs={windowMs} />
              </PopoverContent>
            </Popover>
            {sifting ? <Button variant="ghost" size="sm" onClick={clear}>Reset</Button> : null}
          </div>
          {zoom ? (
            <div className="flex items-center gap-2 text-caption text-muted-foreground">
              <span>Zoomed on a period of {Math.max(1, Math.round(spanMs / 1000))} s.</span>
              <Button size="sm" variant="ghost" onClick={() => zoomTo(0, undefined)}>Back to the last {windowLabel}</Button>
            </div>
          ) : null}
          {!unconfigured && !invalid ? <LogHistogram items={items} rangeMs={windowMs} onZoom={(from, to) => zoomTo(from, to)} placement={placement} live={live} /> : null}
          {unconfigured ? (
            <EmptyState title="Logs are not configured">This deployment has no span store, so there are no logs to read. Configure one to see log records here.</EmptyState>
          ) : invalid ? (
            <EmptyState title="That search is not valid" actions={<Button size="sm" onClick={clear}>Clear filters</Button>}>{because(failed, 'Check the query and the filters.')}</EmptyState>
          ) : failed ? (
            <EmptyState title="The logs could not be read">{because(failed, 'The log store did not answer.')}</EmptyState>
          ) : records.length === 0 ? (
            <EmptyState title={pending || loading ? 'Reading the logs…' : sifting ? 'No log records match' : 'No logs in this window'} actions={sifting && !pending ? <Button size="sm" onClick={clear}>Clear filters</Button> : null}>
              {pending || loading ? 'Asking the log store.' : sifting ? 'Try fewer filters, a shorter search, or a wider window.' : 'Nothing arrived in the window you are looking at. Try a wider one.'}
            </EmptyState>
          ) : (
            <div className="overflow-hidden rounded-lg border">
              <div role="list" aria-label="Log records" ref={list}>
                {records.map((record, i) => {
                  const id = ids[i]
                  return <Row key={id} record={record} index={i} dated={dated} open={open === id} focused={i === cursor} onToggle={() => { setOpen(open === id ? null : id); setCursor(i) }} onPlace={placeHere} onAttr={pickAttr} />
                })}
              </div>
              <div className="flex flex-wrap items-center gap-3 px-3 py-2.5">
                <span className={cn('text-caption', loading ? 'text-sev-info-fg' : 'text-muted-foreground')}>{records.length} record{records.length === 1 ? '' : 's'} loaded{live ? ', refreshing every 5 s' : ''}.</span>
                {more ? <Button size="sm" onClick={older} disabled={loadingOlder}>{loadingOlder ? 'Loading…' : 'Load older'}</Button> : null}
              </div>
            </div>
          )}
        </div>
      </div>
    </>
  )
}
