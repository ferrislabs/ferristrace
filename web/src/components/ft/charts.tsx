import { Link } from '@/lib/router'
import { TIME_LABELS } from '@/data/series'

export type Marker = { at: number; label: string; to?: string }

const pts = (vals: number[], lo: number, hi: number, h: number) =>
  vals.map((v, i) => `${((i * 600) / Math.max(1, vals.length - 1)).toFixed(1)},${(h - ((Math.min(Math.max(v, lo), hi) - lo) / (hi - lo || 1)) * h).toFixed(1)}`).join(' ')

export function LineChart({ title, legend, values, second, threshold, lo = 0, hi, yLabels, aria, height = 140, markers = [], xLabels = TIME_LABELS }: {
  title: string; legend?: string; values: number[]; second?: number[]; threshold?: number; lo?: number; hi: number; yLabels: [string, string, string]; aria: string; height?: number; markers?: Marker[]; xLabels?: string[]
}) {
  const p = pts(values, lo, hi, height)
  const ty = threshold !== undefined ? height - ((threshold - lo) / (hi - lo)) * height : null
  return (
    <figure className="m-0 flex min-w-0 flex-col gap-2 rounded-lg border bg-card px-3 pb-2 pt-3">
      <figcaption className="flex flex-wrap items-baseline gap-2">
        <span className="flex-1 text-control font-semibold">{title}</span>
        {legend ? <span className="text-caption text-muted-foreground">{legend}</span> : null}
      </figcaption>
      <div className="grid grid-cols-[40px_minmax(0,1fr)] gap-1.5">
        <div className="flex flex-col justify-between text-right font-mono text-[10px] leading-3 text-faint-foreground" style={{ height }}>
          {yLabels.map((l, i) => <span key={i}>{l}</span>)}
        </div>
        <div className="relative">
        <svg viewBox={`0 0 600 ${height}`} preserveAspectRatio="none" width="100%" height={height} role="img" aria-label={aria} className="block overflow-visible">
          <line x1="0" y1="0.5" x2="600" y2="0.5" stroke="var(--chart-grid)" vectorEffect="non-scaling-stroke" />
          <line x1="0" y1={height / 2} x2="600" y2={height / 2} stroke="var(--chart-grid)" vectorEffect="non-scaling-stroke" />
          <line x1="0" y1={height - 0.5} x2="600" y2={height - 0.5} stroke="var(--border)" vectorEffect="non-scaling-stroke" />
          {ty !== null ? <line x1="0" y1={ty} x2="600" y2={ty} stroke="var(--warning-text)" strokeDasharray="4 4" vectorEffect="non-scaling-stroke" /> : null}
          <polygon points={`0,${height} ${p} 600,${height}`} fill="var(--chart-fill)" />
          {second ? <polyline points={pts(second, lo, hi, height)} fill="none" stroke="var(--muted-foreground)" strokeWidth="1.5" strokeDasharray="3 3" vectorEffect="non-scaling-stroke" /> : null}
          <polyline points={p} fill="none" stroke="var(--chart-line)" strokeWidth="1.75" strokeLinejoin="round" vectorEffect="non-scaling-stroke" />
        </svg>
        {markers.map((m, index) => {
          const left = `${(m.at / Math.max(1, values.length - 1)) * 100}%`
          const tag = <span className="absolute left-1 whitespace-nowrap rounded-sm border bg-background px-1 font-mono text-[10px] leading-4 text-foreground shadow-sm" style={{ top: `${(index % 3) * 16}px` }}>▲ {m.label}</span>
          return (
            <span key={`${m.label}-${m.at}`} className="absolute inset-y-0 border-l border-dashed border-foreground/60" style={{ left }}>
              {m.to ? <Link to={m.to} aria-label={`Release ${m.label}`} className="no-underline">{tag}</Link> : tag}
            </span>
          )
        })}
        </div>
      </div>
      <div className="flex justify-between pl-[46px] font-mono text-[10px] text-faint-foreground">{xLabels.map((t, at) => <span key={`${at}-${t}`}>{t}</span>)}</div>
    </figure>
  )
}

export function Sparkline({ values, warn = false }: { values: number[]; warn?: boolean }) {
  if (!values.length) return <svg width="100" height="20" aria-hidden><line x1="0" y1="19" x2="100" y2="19" stroke="var(--border)" /></svg>
  const p = values.map((v, i) => `${((i * 100) / Math.max(1, values.length - 1)).toFixed(1)},${(19 - Math.max(0, Math.min(1, v)) * 18).toFixed(1)}`).join(' ')
  return (
    <svg viewBox="0 0 100 20" preserveAspectRatio="none" width="100" height="20" aria-hidden className="block">
      <polyline points={p} fill="none" stroke={warn ? 'var(--warning-text)' : 'var(--chart-line)'} strokeWidth="1.5" strokeLinejoin="round" vectorEffect="non-scaling-stroke" />
    </svg>
  )
}

const STACK_PALETTE = ['#4f46e5', '#0891b2', '#d97706', '#059669', '#db2777', '#7c3aed', '#65a30d', '#dc2626']

export const stackColour = (index: number) => STACK_PALETTE[index % STACK_PALETTE.length]

export type StackLayer = { key: string; values: number[] }

export function StackedChart({ title, legend, layers, yLabels, aria, height = 160, markers = [], xLabels }: {
  title: string; legend?: string; layers: StackLayer[]; yLabels: (top: number) => [string, string, string]; aria: string; height?: number; markers?: Marker[]; xLabels: string[]
}) {
  const count = layers[0]?.values.length ?? 0
  const totals = Array.from({ length: count }, (_, at) => layers.reduce((sum, layer) => sum + layer.values[at], 0))
  const top = Math.max(...totals, 1e-9)
  const x = (at: number) => (at * 600) / Math.max(1, count - 1)
  const y = (value: number) => height - (value / top) * height
  const below = new Array<number>(count).fill(0)
  const bands = layers.map((layer, index) => {
    const lower = [...below]
    layer.values.forEach((value, at) => { below[at] += value })
    const upper = [...below]
    const outline = upper.map((value, at) => `${x(at).toFixed(1)},${y(value).toFixed(1)}`)
    const back = lower.map((value, at) => `${x(at).toFixed(1)},${y(value).toFixed(1)}`).reverse()
    return { key: layer.key, colour: stackColour(index), points: [...outline, ...back].join(' ') }
  })
  return (
    <figure className="m-0 flex min-w-0 flex-col gap-2 rounded-lg border bg-card px-3 pb-2 pt-3">
      <figcaption className="flex flex-wrap items-baseline gap-2">
        <span className="flex-1 text-control font-semibold">{title}</span>
        {legend ? <span className="text-caption text-muted-foreground">{legend}</span> : null}
      </figcaption>
      <div className="grid grid-cols-[48px_minmax(0,1fr)] gap-1.5">
        <div className="flex flex-col justify-between text-right font-mono text-[10px] leading-3 text-faint-foreground" style={{ height }}>
          {yLabels(top).map((l, i) => <span key={i}>{l}</span>)}
        </div>
        <div className="relative">
          <svg viewBox={`0 0 600 ${height}`} preserveAspectRatio="none" width="100%" height={height} role="img" aria-label={aria} className="block overflow-visible">
            <line x1="0" y1="0.5" x2="600" y2="0.5" stroke="var(--chart-grid)" vectorEffect="non-scaling-stroke" />
            <line x1="0" y1={height / 2} x2="600" y2={height / 2} stroke="var(--chart-grid)" vectorEffect="non-scaling-stroke" />
            <line x1="0" y1={height - 0.5} x2="600" y2={height - 0.5} stroke="var(--border)" vectorEffect="non-scaling-stroke" />
            {bands.map((band) => <polygon key={band.key} points={band.points} fill={band.colour} fillOpacity="0.55" stroke={band.colour} strokeWidth="1" vectorEffect="non-scaling-stroke"><title>{band.key || '(none)'}</title></polygon>)}
          </svg>
          {markers.map((m, index) => (
            <span key={`${m.label}-${m.at}`} className="absolute inset-y-0 border-l border-dashed border-foreground/60" style={{ left: `${(m.at / Math.max(1, count - 1)) * 100}%` }}>
              <span className="absolute left-1 whitespace-nowrap rounded-sm border bg-background px-1 font-mono text-[10px] leading-4 text-foreground shadow-sm" style={{ top: `${(index % 3) * 16}px` }}>▲ {m.label}</span>
            </span>
          ))}
        </div>
      </div>
      <div className="flex justify-between pl-[54px] font-mono text-[10px] text-faint-foreground">{xLabels.map((t, at) => <span key={`${at}-${t}`}>{t}</span>)}</div>
    </figure>
  )
}
