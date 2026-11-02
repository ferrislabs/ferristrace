import { ChevronRight, Search, Waypoints } from 'lucide-react'
import { useMemo, useState } from 'react'
import { Link } from '@/lib/router'
import { InputGroup } from '@/components/ui/input'
import { Kbd } from '@/components/ui/misc'
import { EmptyState, PageHeader, SeverityBadge } from '@/components/ft/bits'
import { useLogs } from '@/data/logs'
import type { Sev } from '@/lib/otel'
import { cn } from '@/lib/utils'

const LEVELS: Sev[] = ['trace', 'debug', 'info', 'warn', 'error', 'fatal']

export function Logs() {
  const [q, setQ] = useState('')
  const [min, setMin] = useState<Sev>('trace')
  const [open, setOpen] = useState<number | null>(2)
  const logs = useLogs()
  const rows = useMemo(() => logs.filter((l) => LEVELS.indexOf(l.sev) >= LEVELS.indexOf(min) && (l.body + l.svc).toLowerCase().includes(q.toLowerCase())), [logs, q, min])
  return (
    <>
      <PageHeader title="Logs" meta="Live tail paused · newest first"><span className="hidden items-center gap-1 text-caption text-muted-foreground lg:inline-flex"><Kbd>/</Kbd>search<Kbd>T</Kbd>open trace</span></PageHeader>
      <div className="flex flex-wrap items-center gap-2" role="search">
        <InputGroup className="w-[360px] max-w-full"><Search aria-hidden /><input aria-label="Search log bodies" placeholder='Search bodies, e.g. "timeout" service:payments' value={q} onChange={(e) => setQ(e.target.value)} /></InputGroup>
        <div className="flex flex-wrap gap-1" role="group" aria-label="Minimum severity">
          {LEVELS.map((l) => (
            <button key={l} type="button" aria-pressed={l === min} onClick={() => setMin(l)} className={cn('cursor-pointer rounded-sm opacity-50 focus-visible:outline-2 focus-visible:outline-ring', LEVELS.indexOf(l) >= LEVELS.indexOf(min) && 'opacity-100')}>
              <SeverityBadge level={l} />
            </button>
          ))}
        </div>
      </div>
      {rows.length === 0 ? <EmptyState title="No log records match">Lower the minimum severity or shorten the search.</EmptyState> : (
        <div className="-mx-4 border-t md:-mx-5" role="list" aria-label="Log records">
          {rows.map((l, i) => (
            <div key={l.time + i} role="listitem" className="border-b border-border-subtle">
              <button type="button" aria-expanded={open === i} onClick={() => setOpen(open === i ? null : i)} className={cn('grid w-full cursor-pointer grid-cols-[16px_96px_70px_120px_minmax(0,1fr)] items-center gap-3 px-5 py-1.5 text-left hover:bg-row-hover max-md:grid-cols-[16px_70px_minmax(0,1fr)]', open === i && 'bg-accent-subtle')}>
                <ChevronRight className={cn('size-3.5 text-muted-foreground transition-transform', open === i && 'rotate-90')} aria-hidden />
                <time className="font-mono text-caption text-muted-foreground max-md:hidden">{l.time}</time>
                <SeverityBadge level={l.sev} />
                <span className="truncate text-caption text-muted-foreground max-md:hidden">{l.svc}</span>
                <span className={cn('truncate font-mono text-[12.5px]', (l.sev === 'error' || l.sev === 'fatal') && 'text-destructive-text')}>{l.body}</span>
              </button>
              {open === i ? (
                <div className="flex flex-col gap-2 bg-sidebar px-5 py-3 pl-[52px]">
                  <pre className="m-0 whitespace-pre-wrap break-all font-mono text-[12.5px]">{l.body}</pre>
                  <dl className="m-0 grid max-w-[720px] grid-cols-[200px_1fr] gap-x-3 text-caption">
                    {l.attrs.map(([k, v]) => <div key={k} className="contents"><dt className="py-1 font-mono text-muted-foreground">{k}</dt><dd className="m-0 break-all py-1 font-mono">{v}</dd></div>)}
                  </dl>
                  {l.trace ? <Link to={`/traces/${l.trace}`} className="inline-flex w-fit items-center gap-1.5 text-control"><Waypoints className="size-3.5" aria-hidden />Open trace {l.trace.slice(0, 8)}…</Link> : <span className="text-caption text-muted-foreground">No trace attached to this record.</span>}
                </div>
              ) : null}
            </div>
          ))}
        </div>
      )}
    </>
  )
}
