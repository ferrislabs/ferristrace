import { CircleX, Copy, ScrollText, X } from 'lucide-react'
import { useState } from 'react'
import { useNavigate as useRouterNavigate } from '@tanstack/react-router'
import { Link, useParams } from '@/lib/router'
import { PlacementChips } from '@/components/ft/Placement'
import { placementSearch, type Placement } from '@/data/placement'
import { AttachToIncident } from '@/components/ft/AttachToIncident'
import { toast } from 'sonner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { EmptyState, ServiceDot, SeverityBadge, svcBg } from '@/components/ft/bits'
import { Switch } from '@/components/ui/switch'
import { Sheet, SheetContent } from '@/components/ui/sheet'
import { useTrace, type Span } from '@/data/traces'
import { formatCost, formatTokens, modelTimeMs, useTraceCalls, type LlmCall } from '@/data/llm'
import { msOf, traceLogsFilter, useTraceLogs, type LogRecord } from '@/data/logs'
import { because } from '@/auth/client'
import { dur, num, stamp } from '@/lib/format'
import { cn } from '@/lib/utils'
import { useMedia } from '@/lib/media'
import { NotFound } from './NotFound'

export function TraceDetail() {
  const { id } = useParams()
  const { row: t, spans, pending, failed } = useTrace(id)
  const route = useRouterNavigate()
  const filterTraces = (part: Partial<Placement>) => void route({ to: '/traces' as never, search: placementSearch(part) as never })
  const [picked, setPicked] = useState<string | null>(null)
  const [panel, setPanel] = useState(true)
  const [sheet, setSheet] = useState(false)
  const wide = useMedia('(min-width: 1024px)')
  const [onlySpan, setOnlySpan] = useState(false)
  const logs = useTraceLogs(t?.id, t?.startedAtMs ?? 0, t?.durationMs ?? 0)
  const llm = useTraceCalls(t?.id, t?.startedAtMs ?? 0, t?.durationMs ?? 0)
  if (pending) return null
  if (failed) return <EmptyState title="The trace could not be read">{because(failed, 'The span store did not answer')}</EmptyState>
  if (!t) return <NotFound />
  const s = spans.find((x) => x.id === picked) ?? spans.find((x) => x.err) ?? spans[0]
  const services = [...new Set(spans.map((x) => x.svc))]
  const logsFilter = { f: traceLogsFilter(t.id, t.startedAtMs, t.durationMs) }
  const callOf = new Map(llm.calls.map((call) => [call.spanId, call]))
  const modelMs = modelTimeMs(llm.calls)
  const modelCost = llm.calls.reduce((sum, call) => sum + (call.cost ?? 0), 0)
  const unpriced = llm.calls.filter((call) => call.cost === null).length
  const spanLogs = logs.records.filter((record) => record.span_id === s?.id)
  const shownLogs = onlySpan ? spanLogs : logs.records
  return (
    <>
      <nav aria-label="Breadcrumb" className="text-control"><Link to="/traces" className="underline underline-offset-2">Traces</Link><span className="text-faint-foreground"> / </span><span className="font-mono">{t.id.slice(0, 8)}…</span></nav>
      <div className="flex flex-wrap items-center gap-3">
        <h1 className="text-title">{t.root}</h1>
        {t.errors ? <Badge variant="destructive"><CircleX />{t.errors} errors</Badge> : <Badge variant="success">OK</Badge>}
        <span className="flex-1" />
        <Button size="sm" onClick={() => { void navigator.clipboard?.writeText(t.id); toast.success('Trace id copied') }}><Copy />Copy id</Button>
        <Button asChild size="sm"><Link to="/logs" search={logsFilter}><ScrollText />Logs for this trace</Link></Button>
        <AttachToIncident attachment={{ kind: 'trace', reference: t.id, label: t.root.slice(0, 200) }} />
      </div>
      <div className="flex flex-wrap gap-x-4 gap-y-1 text-control text-muted-foreground">
        <span>Duration <span className="font-mono font-semibold text-foreground">{t.dur}</span></span><span>Spans <span className="font-mono text-foreground">{t.spans}</span></span><span>Services <span className="font-mono text-foreground">{services.length}</span></span><span>Started <span className="font-mono text-foreground">{stamp(t.startedAtMs)}</span></span><span className="font-mono max-md:hidden">{t.id}</span>
        {llm.calls.length ? (
          <span data-testid="model-time">Model time <span className="font-mono font-semibold text-foreground">{dur(modelMs)}</span>{t.durationMs > 0 ? <span className="font-mono"> ({Math.min(100, Math.round((modelMs / t.durationMs) * 100))} % of the request, {dur(Math.max(t.durationMs - modelMs, 0))} elsewhere)</span> : null} · <span className="font-mono text-foreground">{num(llm.calls.length)}</span> LLM {llm.calls.length === 1 ? 'call' : 'calls'} · cost <span className="font-mono text-foreground">{formatCost(modelCost)}</span>{unpriced ? <span> ({unpriced} unpriced)</span> : null}</span>
        ) : null}
      </div>
      <div className="flex flex-wrap gap-3 text-caption text-muted-foreground">{services.map((x) => <span key={x} className="inline-flex items-center gap-1.5"><ServiceDot name={x} />{x}</span>)}</div>
      <div className="flex overflow-hidden rounded-lg border lg:min-h-[320px]">
        <div className="min-w-0 flex-1 overflow-x-auto">
          <div className="grid grid-cols-[minmax(0,1fr)_auto] border-b bg-background text-caption font-semibold text-muted-foreground md:min-w-[720px] md:grid-cols-[minmax(240px,340px)_minmax(0,1fr)_84px]">
            <span className="px-3 py-2">Span</span>
            <span className="grid grid-cols-4 border-l border-border-subtle px-0 py-2 font-mono font-normal max-md:hidden">{[0, 1, 2, 3].map((q) => <span key={q} className="pl-1.5">{q === 0 ? 0 : dur((t.durationMs * q) / 4)}</span>)}</span>
            <span className="px-3 py-2 text-right">Duration</span>
          </div>
          <div role="tree" aria-label="Spans" className="md:min-w-[720px]">
            {spans.map((x) => (
              <button
                key={x.id}
                type="button"
                role="treeitem"
                aria-level={x.depth + 1}
                aria-selected={x.id === s?.id}
                onClick={() => { setPicked(x.id); if (wide) setPanel(true); else setSheet(true) }}
                className={cn('grid w-full cursor-pointer grid-cols-[minmax(0,1fr)_auto] items-center border-b border-border-subtle text-left text-control hover:bg-row-hover max-md:min-h-11 max-md:gap-y-1 max-md:py-2 md:h-[30px] md:grid-cols-[minmax(240px,340px)_minmax(0,1fr)_84px] focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring', x.id === s?.id && 'bg-accent-subtle')}
              >
                <span className={cn('flex min-w-0 items-center gap-1.5 pr-2', x.err && 'text-destructive-text')} style={{ paddingLeft: 12 + x.depth * 16 }}>
                  <ServiceDot name={x.svc} />{x.err ? <CircleX className="size-3 shrink-0" aria-label="Error" /> : null}<span className="truncate">{x.name}</span>{callOf.get(x.id) ? <LlmMark call={callOf.get(x.id) as LlmCall} /> : null}
                </span>
                <span className="relative border-border-subtle bg-[linear-gradient(to_right,var(--border-subtle)_1px,transparent_1px)] bg-[length:25%_100%] max-md:order-last max-md:col-span-2 max-md:mx-3 max-md:h-3 md:h-[30px] md:border-l">
                  <span className={cn('absolute h-3 min-w-[2px] rounded-[2px] md:top-[9px]', svcBg(x.svc), x.err && 'ring-1 ring-destructive ring-offset-2 ring-offset-background')} style={{ left: `${x.offset}%`, width: `${x.width}%` }} />
                </span>
                <span className="pr-3 text-right font-mono">{x.dur}</span>
              </button>
            ))}
          </div>
        </div>
        {panel && wide ? (
          <aside className="flex w-[400px] shrink-0 flex-col border-l bg-card" aria-label="Span details">
            <div className="flex items-center gap-2 border-b px-4 py-3">
              <ServiceDot name={s.svc} /><span className="flex-1 truncate font-semibold">{s.name}</span>
              <Button variant="ghost" size="icon-sm" aria-label="Close details" onClick={() => setPanel(false)}><X /></Button>
            </div>
            <div className="flex flex-col gap-3 overflow-y-auto p-4">
              <SpanDetails span={s} call={callOf.get(s.id)} logs={spanLogs} filter={logsFilter} onPick={filterTraces} onOnlySpan={() => setOnlySpan(true)} />
            </div>
          </aside>
        ) : null}
      </div>
      {wide ? null : (
        <Sheet open={sheet} onOpenChange={setSheet}>
          <SheetContent title={s.name}>
            <div className="flex flex-col gap-3 px-2 pb-2">
              <SpanDetails span={s} call={callOf.get(s.id)} logs={spanLogs} filter={logsFilter} onPick={filterTraces} onOnlySpan={() => { setOnlySpan(true); setSheet(false) }} />
            </div>
          </SheetContent>
        </Sheet>
      )}
      <section aria-label="Logs of this trace" className="flex flex-col overflow-hidden rounded-lg border">
        <div className="flex flex-wrap items-center gap-3 border-b px-4 py-2.5">
          <h2 className="m-0 text-control font-semibold">Logs of this trace</h2>
          <span className="text-caption text-muted-foreground">{logs.pending ? 'reading…' : `${logs.records.length}${logs.more ? '+' : ''} records, trace ±5 min`}</span>
          <span className="flex-1" />
          {s ? <Switch checked={onlySpan} onCheckedChange={setOnlySpan} label={<>Only span <span className="font-mono">{s.name}</span></>} /> : null}
          <Link to="/logs" search={logsFilter} className="text-control">Open in Logs</Link>
        </div>
        {logs.failed ? <p className="m-0 px-4 py-3 text-control text-muted-foreground">{because(logs.failed, 'The logs could not be read.')}</p> : null}
        {!logs.failed && !logs.pending && shownLogs.length === 0 ? <p className="m-0 px-4 py-3 text-control text-muted-foreground">{onlySpan ? 'No log record carries the selected span id.' : 'No log record carries this trace id.'}</p> : null}
        {shownLogs.map((record, at) => <LogLine key={`${record.time_ns}-${at}`} record={record} filter={logsFilter} wide />)}
      </section>
    </>
  )
}

function SpanDetails({ span: s, call, logs, filter, onPick, onOnlySpan }: { span: Span; call: LlmCall | undefined; logs: LogRecord[]; filter: { f: string }; onPick: (part: Partial<Placement>) => void; onOnlySpan: () => void }) {
  return (
    <>
      <div className="flex flex-wrap gap-2"><Badge variant="outline" mono>{s.kind}</Badge>{s.err ? <Badge variant="destructive">ERROR</Badge> : <Badge variant="success">OK</Badge>}<Badge variant="outline" mono>{s.dur}</Badge></div>
      {s.events.map((e, at) =>
        e.exception ? (
          <div key={`exc-${at}`} className="rounded-md border border-destructive-border bg-destructive-subtle px-3 py-2 text-caption text-destructive-text">
            <span className="font-mono font-semibold">{e.exception.type}{e.exception.message ? `: ${e.exception.message}` : null}</span>
            {e.exception.stack ? <pre className="m-0 mt-1.5 overflow-x-auto whitespace-pre-wrap break-words font-mono text-[11px]">{e.exception.stack}</pre> : null}
          </div>
        ) : null,
      )}
      {s.err && !s.events.some((e) => e.exception) ? (
        <span className="text-caption text-muted-foreground">This span failed and recorded no exception.</span>
      ) : null}
      {call ? <LlmPanel call={call} /> : null}
      <PlacementChips attributes={Object.fromEntries(s.attrs)} onPick={onPick} />
      <h2 className="m-0 text-caption font-semibold uppercase tracking-[.05em] text-muted-foreground">Attributes</h2>
      <dl className="m-0 grid grid-cols-[minmax(120px,42%)_1fr] gap-x-3 text-caption">
        {s.attrs.map(([k, v]) => (
          <div key={k} className="contents">
            <dt className="truncate border-b border-border-subtle py-1.5 font-mono text-muted-foreground">{k}</dt>
            <dd className="m-0 break-all border-b border-border-subtle py-1.5 font-mono">{v}</dd>
          </div>
        ))}
      </dl>
      {s.events.length ? (
        <>
          <h2 className="m-0 text-caption font-semibold uppercase tracking-[.05em] text-muted-foreground">Events</h2>
          <ol className="m-0 flex list-none flex-col gap-2 p-0">
            {s.events.map((e, at) => (
              <li key={`ev-${at}`} className="flex flex-col gap-1 border-b border-border-subtle pb-2 last:border-b-0 last:pb-0">
                <span className="flex items-baseline gap-2 text-caption">
                  <span className={cn('font-mono font-semibold', e.exception && 'text-destructive-text')}>{e.name}</span>
                  <span className="flex-1" />
                  <span className="font-mono text-muted-foreground">+{e.at}</span>
                </span>
                {e.attrs.length ? (
                  <dl className="m-0 grid grid-cols-[minmax(120px,42%)_1fr] gap-x-3 text-[11px]">
                    {e.attrs.map(([k, v]) => (
                      <div key={k} className="contents">
                        <dt className="truncate py-0.5 font-mono text-muted-foreground">{k}</dt>
                        <dd className="m-0 break-all py-0.5 font-mono">{v}</dd>
                      </div>
                    ))}
                  </dl>
                ) : null}
              </li>
            ))}
          </ol>
        </>
      ) : null}
      <h2 className="m-0 text-caption font-semibold uppercase tracking-[.05em] text-muted-foreground">Logs</h2>
      {logs.length ? (
        <div className="flex flex-col gap-1.5">
          {logs.slice(0, 5).map((record, at) => <LogLine key={`${record.time_ns}-${at}`} record={record} filter={filter} />)}
          <Button size="sm" variant="ghost" className="self-start" onClick={onOnlySpan}>Show the {logs.length} logs of this span below</Button>
        </div>
      ) : <span className="text-caption text-muted-foreground">No log record carries this span id.</span>}
      <span className="font-mono text-caption text-faint-foreground">span_id {s.id}</span>
    </>
  )
}

function LlmMark({ call }: { call: LlmCall }) {
  return <span className="shrink-0 rounded-sm border bg-muted px-1.5 font-mono text-[11px] text-muted-foreground" title="LLM call">{call.model} · {formatTokens(call.inputTokens + call.outputTokens)} tok · {formatCost(call.cost)}</span>
}

function LlmPanel({ call }: { call: LlmCall }) {
  const rows: [string, string][] = [
    ['Model', call.model || '—'],
    ['Provider', call.provider || '—'],
    ['Operation', call.operation || '—'],
    ['Input tokens', num(call.inputTokens)],
    ['Cached input', num(call.cachedInputTokens)],
    ['Output tokens', num(call.outputTokens)],
    ['Cost', formatCost(call.cost)],
    ['Feature', call.feature || '—'],
    ['Customer', call.customer || '—'],
    ['Prompt version', call.promptVersion || '—'],
  ]
  return (
    <>
      <h2 className="m-0 text-caption font-semibold uppercase tracking-[.05em] text-muted-foreground">LLM call</h2>
      <dl className="m-0 grid grid-cols-[minmax(120px,42%)_1fr] gap-x-3 text-caption">
        {rows.map(([k, v]) => (
          <div key={k} className="contents">
            <dt className="truncate border-b border-border-subtle py-1.5 text-muted-foreground">{k}</dt>
            <dd className="m-0 break-all border-b border-border-subtle py-1.5 font-mono">{v}</dd>
          </div>
        ))}
      </dl>
    </>
  )
}

const pad = (n: number, width = 2) => String(n).padStart(width, '0')

function clock(ns: number): string {
  const ms = msOf(ns)
  const at = new Date(ms)
  return `${pad(at.getHours())}:${pad(at.getMinutes())}:${pad(at.getSeconds())}.${pad(ms % 1000, 3)}`
}

function LogLine({ record, filter, wide = false }: { record: LogRecord; filter: { f: string }; wide?: boolean }) {
  return (
    <Link to="/logs" search={filter} className={cn('grid items-center gap-2.5 text-foreground no-underline hover:bg-row-hover', wide ? 'grid-cols-[96px_62px_140px_minmax(0,1fr)] border-b border-border-subtle px-4 py-1.5' : 'grid-cols-[62px_minmax(0,1fr)] rounded-sm py-0.5')}>
      {wide ? <time className="font-mono text-caption text-muted-foreground">{clock(record.time_ns)}</time> : null}
      <SeverityBadge level={record.severity} />
      {wide ? <span className="truncate text-caption text-muted-foreground">{record.service}</span> : null}
      <span className="truncate font-mono text-caption">{record.body}</span>
    </Link>
  )
}
