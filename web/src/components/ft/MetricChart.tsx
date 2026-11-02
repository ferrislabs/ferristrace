import { useMemo, useState } from 'react'
import { Link } from '@/lib/router'
import { shortId } from '@/lib/format'
import { cn } from '@/lib/utils'
import type { Exemplar, MetricLine } from '@/data/metrics'

const PALETTE = ['#4f46e5', '#0891b2', '#d97706', '#059669', '#db2777', '#7c3aed', '#65a30d', '#dc2626', '#0284c7', '#a16207']
const HEIGHT = 220
const WIDTH = 600

export const labelOf = (labels: Record<string, string>) => {
  const pairs = Object.entries(labels)
  return pairs.length === 0 ? 'all' : pairs.map(([k, v]) => `${k}=${v}`).join(', ')
}

export const formatValue = (value: number, unit: string) =>
  `${value.toLocaleString('en-US', { maximumFractionDigits: 4 })}${unit ? ` ${unit}` : ''}`

const msOf = (ns: number) => ns / 1_000_000

function clock(ns: number) {
  const at = new Date(msOf(ns))
  return at.toISOString().slice(0, 19).replace('T', ' ')
}

function segments(points: [number, number | null][], x: (t: number) => number, y: (v: number) => number): string[] {
  const out: string[] = []
  let run: string[] = []
  for (const [t, v] of points) {
    if (v === null) {
      if (run.length) out.push(run.join(' '))
      run = []
    } else {
      run.push(`${x(t).toFixed(1)},${y(v).toFixed(1)}`)
    }
  }
  if (run.length) out.push(run.join(' '))
  return out
}

export type DeployMark = { atNs: number; label: string; to: string }

export function MetricChart({ series, exemplars, unit, aria, threshold, deploys = [] }: { series: MetricLine[]; exemplars: Exemplar[]; unit: string; aria: string; threshold?: number; deploys?: DeployMark[] }) {
  const [pinned, setPinned] = useState<string[]>([])
  const [hover, setHover] = useState<number | null>(null)
  const keys = series.map((s) => labelOf(s.labels))
  const visible = series.map((_, i) => pinned.length === 0 || pinned.includes(keys[i]))

  const frame = useMemo(() => {
    const times = series.flatMap((s) => s.points.map((p) => p[0]))
    const values = series.flatMap((s, i) => (visible[i] ? s.points.flatMap((p) => (p[1] === null ? [] : [p[1]])) : []))
    const t0 = times.length ? Math.min(...times) : 0
    const t1 = times.length ? Math.max(...times) : 1
    const marked = threshold === undefined ? [] : [threshold]
    const lo = Math.min(0, ...values, ...marked)
    const hi = Math.max(...values, ...marked, lo + 1e-9)
    return { t0, t1, lo, hi, sorted: [...new Set(times)].sort((a, b) => a - b) }
  }, [series, pinned, threshold])

  const x = (t: number) => ((t - frame.t0) / (frame.t1 - frame.t0 || 1)) * WIDTH
  const y = (v: number) => HEIGHT - ((v - frame.lo) / (frame.hi - frame.lo || 1)) * HEIGHT
  const toggle = (key: string) => setPinned(pinned.includes(key) ? pinned.filter((k) => k !== key) : [...pinned, key])
  const at = hover === null ? null : frame.sorted[hover]
  const shown = exemplars.filter((e) => e.time_ns >= frame.t0 && e.time_ns <= frame.t1)
  const deployed = deploys.filter((d) => d.atNs >= frame.t0 && d.atNs <= frame.t1)

  const onMove = (event: React.MouseEvent<HTMLDivElement>) => {
    const box = event.currentTarget.getBoundingClientRect()
    if (box.width === 0 || frame.sorted.length === 0) return
    const t = frame.t0 + ((event.clientX - box.left) / box.width) * (frame.t1 - frame.t0)
    let best = 0
    for (let i = 1; i < frame.sorted.length; i++) {
      if (Math.abs(frame.sorted[i] - t) < Math.abs(frame.sorted[best] - t)) best = i
    }
    setHover(best)
  }

  return (
    <figure className="m-0 flex min-w-0 flex-col gap-3 rounded-lg border bg-card px-3 pb-3 pt-3">
      <div className="grid grid-cols-[56px_minmax(0,1fr)] gap-1.5">
        <div className="flex flex-col justify-between text-right font-mono text-[10px] leading-3 text-faint-foreground" style={{ height: HEIGHT }}>
          <span>{frame.hi.toLocaleString('en-US', { maximumFractionDigits: 2 })}</span>
          <span>{((frame.hi + frame.lo) / 2).toLocaleString('en-US', { maximumFractionDigits: 2 })}</span>
          <span>{frame.lo.toLocaleString('en-US', { maximumFractionDigits: 2 })}</span>
        </div>
        <div className="relative" onMouseMove={onMove} onMouseLeave={() => setHover(null)}>
          <svg viewBox={`0 0 ${WIDTH} ${HEIGHT}`} preserveAspectRatio="none" width="100%" height={HEIGHT} role="img" aria-label={aria} className="block overflow-visible">
            <line x1="0" y1="0.5" x2={WIDTH} y2="0.5" stroke="var(--chart-grid)" vectorEffect="non-scaling-stroke" />
            <line x1="0" y1={HEIGHT / 2} x2={WIDTH} y2={HEIGHT / 2} stroke="var(--chart-grid)" vectorEffect="non-scaling-stroke" />
            <line x1="0" y1={HEIGHT - 0.5} x2={WIDTH} y2={HEIGHT - 0.5} stroke="var(--border)" vectorEffect="non-scaling-stroke" />
            {series.map((s, i) => visible[i]
              ? segments(s.points, x, y).map((points, n) => (
                  points.includes(' ')
                    ? <polyline key={`${keys[i]}-${n}`} points={points} fill="none" stroke={PALETTE[i % PALETTE.length]} strokeWidth="1.75" strokeLinejoin="round" vectorEffect="non-scaling-stroke" />
                    : <circle key={`${keys[i]}-${n}`} cx={points.split(',')[0]} cy={points.split(',')[1]} r="1.5" fill={PALETTE[i % PALETTE.length]} />
                ))
              : null)}
            {threshold !== undefined ? <line x1="0" x2={WIDTH} y1={y(threshold)} y2={y(threshold)} stroke="var(--destructive-text)" strokeDasharray="6 4" strokeWidth="1.5" vectorEffect="non-scaling-stroke"><title>{`Threshold ${formatValue(threshold, unit)}`}</title></line> : null}
            {at !== null ? <line x1={x(at)} x2={x(at)} y1="0" y2={HEIGHT} stroke="var(--muted-foreground)" strokeDasharray="3 3" vectorEffect="non-scaling-stroke" /> : null}
          </svg>
          {deployed.map((d) => (
            <span key={`${d.label}-${d.atNs}`} className="absolute inset-y-0 border-l border-dashed border-foreground/60" style={{ left: `${(x(d.atNs) / WIDTH) * 100}%` }}>
              <Link to={d.to} aria-label={`Release ${d.label}`} className="no-underline"><span className="absolute -top-0.5 left-1 whitespace-nowrap rounded-sm border bg-background px-1 font-mono text-[10px] leading-4 text-foreground shadow-sm">▲ {d.label}</span></Link>
            </span>
          ))}
          {shown.map((e) => (
            <Link
              key={`${e.trace_id}-${e.span_id}-${e.time_ns}`}
              to={`/traces/${e.trace_id}`}
              aria-label={`Exemplar ${formatValue(e.value, unit)}, open trace ${shortId(e.trace_id)}`}
              title={`${formatValue(e.value, unit)} · trace ${shortId(e.trace_id)}`}
              className="absolute size-2.5 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-background bg-foreground focus-visible:outline-2 focus-visible:outline-ring"
              style={{ left: `${(x(e.time_ns) / WIDTH) * 100}%`, top: `${(Math.min(Math.max(y(e.value), 0), HEIGHT) / HEIGHT) * 100}%` }}
            />
          ))}
          {at !== null ? (
            <div role="tooltip" className="pointer-events-none absolute top-0 z-10 flex max-w-[320px] flex-col gap-0.5 rounded-md border bg-background px-2 py-1.5 text-caption shadow-md" style={{ left: `${(x(at) / WIDTH) * 100}%`, transform: x(at) > WIDTH / 2 ? 'translateX(calc(-100% - 8px))' : 'translateX(8px)' }}>
              <span className="font-mono text-muted-foreground">{clock(at)}</span>
              {series.map((s, i) => {
                const point = visible[i] ? s.points.find((p) => p[0] === at) : undefined
                if (!point) return null
                return (
                  <span key={keys[i]} className="flex items-center gap-1.5">
                    <span className="size-2 shrink-0 rounded-full" style={{ background: PALETTE[i % PALETTE.length] }} aria-hidden />
                    <span className="truncate font-mono">{keys[i]}</span>
                    <span className="ml-auto pl-2 font-mono font-semibold">{point[1] === null ? 'no data' : formatValue(point[1], unit)}</span>
                  </span>
                )
              })}
            </div>
          ) : null}
        </div>
      </div>
      <div className="flex justify-between pl-[62px] font-mono text-[10px] text-faint-foreground">
        <span>{clock(frame.t0)}</span>
        <span>{clock(frame.t1)}</span>
      </div>
      <ul className="m-0 flex list-none flex-wrap gap-1 p-0" aria-label="Legend, select a series to pin it">
        {series.map((_, i) => {
          const on = pinned.includes(keys[i])
          return (
            <li key={keys[i]}>
              <button type="button" aria-pressed={on} onClick={() => toggle(keys[i])} className={cn('inline-flex max-w-[360px] cursor-pointer items-center gap-1.5 rounded-sm border px-1.5 py-0.5 font-mono text-caption hover:bg-muted focus-visible:outline-2 focus-visible:outline-ring', on && 'bg-muted', !visible[i] && 'opacity-45')}>
                <span className="size-2 shrink-0 rounded-full" style={{ background: PALETTE[i % PALETTE.length] }} aria-hidden />
                <span className="truncate">{keys[i]}</span>
              </button>
            </li>
          )
        })}
        {pinned.length > 0 ? <li><button type="button" onClick={() => setPinned([])} className="cursor-pointer rounded-sm px-1.5 py-0.5 text-caption text-muted-foreground hover:bg-muted">Show all</button></li> : null}
      </ul>
    </figure>
  )
}
