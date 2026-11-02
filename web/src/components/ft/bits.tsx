import { CircleCheck, CircleMinus, CircleX, TriangleAlert, Wrench } from 'lucide-react'
import { Link } from '@/lib/router'
import { svcIndex, type Sev } from '@/lib/otel'
import { cn } from '@/lib/utils'

const SVC_BG = ['', 'bg-svc-1', 'bg-svc-2', 'bg-svc-3', 'bg-svc-4', 'bg-svc-5', 'bg-svc-6', 'bg-svc-7', 'bg-svc-8']
export const svcBg = (name: string) => SVC_BG[svcIndex(name)]

export function ServiceDot({ name, className }: { name: string; className?: string }) {
  return <span aria-hidden className={cn('inline-block size-2 shrink-0 rounded-[2px]', svcBg(name), className)} />
}

const SEV: Record<Sev, string> = {
  trace: 'text-sev-trace-fg bg-sev-trace-bg border-sev-trace-bd',
  debug: 'text-sev-debug-fg bg-sev-debug-bg border-sev-debug-bd',
  info: 'text-sev-info-fg bg-sev-info-bg border-sev-info-bd',
  warn: 'text-sev-warn-fg bg-sev-warn-bg border-sev-warn-bd',
  error: 'text-sev-error-fg bg-sev-error-bg border-sev-error-bd',
  fatal: 'text-sev-fatal-fg bg-sev-fatal-bg border-sev-fatal-bd',
}
export function SeverityBadge({ level }: { level: Sev }) {
  return <span className={cn('inline-flex h-5 min-w-[62px] items-center justify-center rounded-sm border px-1.5 text-[11px] font-semibold uppercase tracking-[.02em]', SEV[level])}>{level}</span>
}

export type Status = 'operational' | 'degraded' | 'outage' | 'nodata' | 'maintenance'
const ST = {
  operational: { cls: 'text-success-text', Icon: CircleCheck, label: 'Operational' },
  degraded: { cls: 'text-warning-text', Icon: TriangleAlert, label: 'Degraded' },
  outage: { cls: 'text-destructive-text', Icon: CircleX, label: 'Outage' },
  nodata: { cls: 'text-muted-foreground', Icon: CircleMinus, label: 'No data' },
  maintenance: { cls: 'text-muted-foreground', Icon: Wrench, label: 'Maintenance' },
}
export function StatusIndicator({ status, label, className }: { status: Status; label?: string; className?: string }) {
  const s = ST[status]
  return (
    <span className={cn('inline-flex items-center gap-1 text-control font-semibold', s.cls, className)}>
      <s.Icon className="size-3.5" aria-hidden />
      {label ?? s.label}
    </span>
  )
}

export function UptimeBars({ days, bad, fresh, live, incident, name }: { days: number; bad: Record<number, 'deg' | 'out'>; fresh?: number; live?: boolean; incident: boolean; name: string }) {
  const bars: { cls: string; tip: string }[] = []
  let loss = 0
  for (let i = days - 1; i >= 0; i--) {
    let s: 'ok' | 'deg' | 'out' | 'none' = bad[i] || 'ok'
    if (i === 0 && s === 'deg' && !(live && incident)) s = 'ok'
    if (fresh && i > fresh) s = 'none'
    loss += { ok: 0, deg: 0.0005, out: 0.012, none: 0 }[s]
    bars.push({ cls: { ok: 'bg-status-ok', deg: 'bg-status-deg', out: 'bg-status-out', none: 'bg-status-none' }[s], tip: `${i === 0 ? 'Today' : `${i} days ago`}: ${{ ok: 'No downtime', deg: 'Degraded performance', out: 'Partial outage', none: 'No data' }[s]}` })
  }
  const incidents = bars.filter((b) => b.cls === 'bg-status-deg' || b.cls === 'bg-status-out').length
  return (
    <div className="flex flex-col gap-2">
      <div className="flex h-7 items-stretch gap-[2px]" role="img" aria-label={`${name}, ${days} days: ${incidents} days with incidents`}>
        {bars.map((b, i) => <span key={i} className={cn('min-w-px flex-1 rounded-[1px]', b.cls)} title={b.tip} />)}
      </div>
      <div className="flex justify-between text-caption text-muted-foreground"><span>{days} days ago</span><span className="font-mono">{(100 - (loss * 100) / days).toFixed(2)} % uptime</span><span>Today</span></div>
    </div>
  )
}

export function Kpi({ label, value, sub, tone, to }: { label: string; value: string; sub?: string; tone?: 'warn' | 'ok' | 'err'; to?: string }) {
  const t = tone === 'warn' ? 'text-warning-text' : tone === 'ok' ? 'text-success-text' : tone === 'err' ? 'text-destructive-text' : ''
  const body = (
    <>
      <span className="text-caption text-muted-foreground">{label}</span>
      <span className={cn('font-mono text-[22px] font-semibold leading-[30px]', t)}>{value}</span>
      {sub ? <span className={cn('text-caption', tone === 'warn' ? 'text-warning-text' : 'text-muted-foreground')}>{sub}</span> : null}
    </>
  )
  const cls = 'flex flex-col gap-0.5 rounded-lg border bg-card px-3.5 py-3'
  return to ? <Link to={to} className={cn(cls, 'text-inherit no-underline hover:bg-row-hover')}>{body}</Link> : <div className={cls}>{body}</div>
}

export function PageHeader({ title, meta, children }: { title: React.ReactNode; meta?: React.ReactNode; children?: React.ReactNode }) {
  return (
    <div className="flex flex-wrap items-center gap-3">
      <h1 className="text-title">{title}</h1>
      {meta ? <span className="text-control text-muted-foreground">{meta}</span> : null}
      <span className="flex-1" />
      {children}
    </div>
  )
}

export function EmptyState({ icon, title, children, code, actions }: { icon?: React.ReactNode; title: string; children: React.ReactNode; code?: string; actions?: React.ReactNode }) {
  return (
    <div className="flex max-w-[680px] flex-col gap-3 rounded-lg border bg-card p-6">
      {icon ? <span className="text-muted-foreground [&_svg]:size-5">{icon}</span> : null}
      <h2 className="text-heading">{title}</h2>
      <p className="m-0 text-muted-foreground">{children}</p>
      {code ? <pre className="m-0 overflow-x-auto whitespace-pre rounded-md border bg-muted px-3 py-2.5 font-mono text-[12px] leading-5">{code}</pre> : null}
      {actions ? <div className="flex gap-2">{actions}</div> : null}
    </div>
  )
}

const MAP: Record<string, string> = {
  mono: 'font-mono', strong: 'font-semibold', sm: 'text-control', small: 'text-caption', muted: 'text-muted-foreground',
  'warn-text': 'text-warning-text', 'ok-text': 'text-success-text', 'err-text': 'text-destructive-text', 'accent-text': 'text-accent-text',
  num: 'text-right tabular', truncate: 'max-w-[420px] truncate',
}
export const mapCls = (s: string) => s.split(/\s+/).filter(Boolean).map((c) => (c.startsWith('cw-') ? `w-[${c.slice(3)}px]` : MAP[c] ?? '')).join(' ')
export const barBg = (c?: string) => (c === 'bar-w' ? 'bg-status-deg' : c === 'bar-o' ? 'bg-status-ok' : c === 'bar-x' ? 'bg-status-out' : 'bg-chart-line')
