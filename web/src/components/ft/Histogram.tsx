import { useId, useMemo, useRef, useState } from 'react'
import { useCacheSeries } from '@/data/caches'
import { useLogSeries } from '@/data/logs'
import type { Placement } from '@/data/placement'
import { useSpanSeries } from '@/data/spanSeries'
import type { Node } from '@/lib/queryLang'
import { cn } from '@/lib/utils'

const HEIGHT = 84

const compact = new Intl.NumberFormat('en', { notation: 'compact', maximumFractionDigits: 1 })

const whole = new Intl.NumberFormat('en')

const clock = (ms: number) => new Date(ms).toTimeString().slice(0, 8)

const day = (ms: number) => new Date(ms).toISOString().slice(5, 10)

const stamp = (ms: number) => `${day(ms)} ${clock(ms)}`

export type Bar = { at: number; count: number; errors: number }

export function Histogram({ items, rangeMs, onZoom }: { items: Node[]; rangeMs: number; onZoom: (from: number, to: number) => void }) {
  const total = useSpanSeries(rangeMs, 'count', '', items, true)
  const failed = useSpanSeries(rangeMs, 'errors', '', items, true)
  const bars = useMemo<Bar[]>(() => {
    const counts = total.lines[0]?.points ?? []
    const errors = failed.lines[0]?.points ?? []
    return counts.map(([ns, value], index) => ({ at: ns / 1_000_000, count: value ?? 0, errors: errors[index]?.[1] ?? 0 }))
  }, [total.lines, failed.lines])
  return <Bars bars={bars} loading={total.loading} onZoom={onZoom} noun="spans" title="Spans over time" />
}

export function LogHistogram({ items, rangeMs, onZoom, placement, live }: { items: Node[]; rangeMs: number; onZoom: (from: number, to: number) => void; placement: Placement; live: boolean }) {
  const found = useLogSeries(items, rangeMs, placement, live)
  return <Bars bars={found.bars} loading={found.loading} onZoom={onZoom} noun="records" title="Records over time" />
}

export function CacheHistogram({ rangeMs, onZoom, environment, placement }: { rangeMs: number; onZoom: (from: number, to: number) => void; environment: string; placement: Placement }) {
  const { series, pending } = useCacheSeries(rangeMs, environment, placement)
  const bars = useMemo<Bar[]>(
    () => (series?.points ?? []).map((point) => ({ at: point.atMs, count: point.calls, errors: point.misses })),
    [series],
  )
  return <Bars bars={bars} loading={pending} onZoom={onZoom} noun="operations" title="Cache operations over time" />
}

function Bars({ bars, loading, onZoom, noun, title }: { bars: Bar[]; loading: boolean; onZoom: (from: number, to: number) => void; noun: string; title: string }) {
  const id = useId()
  const box = useRef<HTMLDivElement>(null)
  const [drag, setDrag] = useState<{ from: number; to: number } | null>(null)
  const [hover, setHover] = useState<number | null>(null)
  const [cursor, setCursor] = useState<number | null>(null)
  const [anchor, setAnchor] = useState<number | null>(null)

  const step = bars.length > 1 ? bars[1].at - bars[0].at : 0
  const start = bars[0]?.at ?? 0
  const end = bars.length > 0 ? bars[bars.length - 1].at + step : 0
  const peak = Math.max(1, ...bars.map((bar) => bar.count))
  const spans = bars.reduce((sum, bar) => sum + bar.count, 0)
  const errors = bars.reduce((sum, bar) => sum + bar.errors, 0)

  const indexAt = (clientX: number): number => {
    const rect = box.current?.getBoundingClientRect()
    if (!rect || rect.width === 0 || bars.length === 0) return 0
    const fraction = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width))
    return Math.min(bars.length - 1, Math.floor(fraction * bars.length))
  }

  const apply = (a: number, b: number) => {
    if (bars.length === 0) return
    const low = Math.min(a, b)
    const high = Math.max(a, b)
    onZoom(bars[low].at, bars[high].at + step)
  }

  const selecting = drag ?? (cursor !== null && anchor !== null ? { from: anchor, to: cursor } : null)
  const low = selecting ? Math.min(selecting.from, selecting.to) : 0
  const high = selecting ? Math.max(selecting.from, selecting.to) : 0
  const pointed = hover ?? cursor
  const bar = pointed !== null ? bars[pointed] : undefined
  const left = pointed !== null && bars.length > 0 ? ((pointed + 0.5) / bars.length) * 100 : 0

  if (bars.length === 0 || spans === 0) {
    return (
      <div className="flex h-[84px] items-center justify-center rounded-lg border border-dashed text-caption text-muted-foreground" role="status">
        {loading ? `Counting the ${noun}…` : `No ${noun.replace(/s$/, '')} matches in this window.`}
      </div>
    )
  }

  return (
    <figure className="m-0 flex flex-col gap-2 rounded-lg border bg-card px-3 pb-2.5 pt-2.5">
      <div className="flex items-baseline gap-2 text-caption">
        <span className="font-semibold text-foreground">{title}</span>
        <span className="text-muted-foreground">{whole.format(spans)} in all{errors > 0 ? `, ${whole.format(errors)} failed` : ''}</span>
        <span className="ml-auto hidden text-faint-foreground sm:inline">Drag to zoom on a period</span>
      </div>
      <div className="grid grid-cols-[34px_minmax(0,1fr)] gap-2">
        <div className="flex flex-col justify-between text-right font-mono text-[10px] leading-3 text-faint-foreground" style={{ height: HEIGHT }} aria-hidden>
          <span>{compact.format(peak)}</span>
          <span>0</span>
        </div>
        <div className="relative">
          <div
            ref={box}
            role="application"
            aria-roledescription="histogram with a time selection"
            aria-label={`${title}, from ${stamp(start)} to ${stamp(end)}, ${whole.format(spans)} in all. Drag to zoom on a period, or use the arrow keys, Shift to select, Enter to zoom, Escape to cancel.`}
            tabIndex={0}
            className="relative flex cursor-crosshair touch-none select-none items-end gap-[2px] border-b border-border focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-ring"
            style={{ height: HEIGHT, backgroundImage: 'linear-gradient(to bottom, var(--chart-grid) 1px, transparent 1px)', backgroundSize: '100% 50%' }}
            onPointerDown={(event) => {
              event.currentTarget.setPointerCapture(event.pointerId)
              const at = indexAt(event.clientX)
              setDrag({ from: at, to: at })
              setCursor(null)
              setAnchor(null)
            }}
            onPointerMove={(event) => {
              const at = indexAt(event.clientX)
              setHover(at)
              if (drag) setDrag({ from: drag.from, to: at })
            }}
            onPointerUp={() => {
              if (drag) apply(drag.from, drag.to)
              setDrag(null)
            }}
            onPointerLeave={() => setHover(null)}
            onPointerCancel={() => setDrag(null)}
            onKeyDown={(event) => {
              const last = bars.length - 1
              if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
                event.preventDefault()
                const next = Math.min(last, Math.max(0, (cursor ?? (event.key === 'ArrowLeft' ? last : 0)) + (event.key === 'ArrowLeft' ? -1 : 1)))
                if (event.shiftKey) setAnchor((was) => was ?? cursor ?? next)
                else setAnchor(null)
                setCursor(next)
              } else if (event.key === 'Enter' && cursor !== null && anchor !== null) {
                event.preventDefault()
                apply(anchor, cursor)
                setAnchor(null)
                setCursor(null)
              } else if (event.key === 'Escape') {
                setAnchor(null)
                setCursor(null)
                setDrag(null)
              }
            }}
            onBlur={() => { setAnchor(null); setCursor(null) }}
          >
            {bars.map((one, index) => {
              const tall = one.count === 0 ? 0 : Math.max(3, (one.count / peak) * 100)
              const share = one.count === 0 ? 0 : Math.min(1, one.errors / one.count)
              const dim = selecting !== null && (index < low || index > high)
              return (
                <div key={one.at} className="relative flex h-full min-w-0 flex-1 items-end" aria-hidden>
                  <div
                    className={cn('relative w-full overflow-hidden rounded-t-[3px] bg-primary transition-opacity', index === pointed ? 'opacity-100' : dim ? 'opacity-30' : 'opacity-60')}
                    style={{ height: `${tall}%` }}
                  >
                    {share > 0 ? <div className="absolute inset-x-0 bottom-0 bg-destructive" style={{ height: `${Math.max(share * 100, 12)}%` }} /> : null}
                  </div>
                </div>
              )
            })}
            {selecting ? (
              <div
                aria-hidden
                className="pointer-events-none absolute inset-y-0 rounded-sm border border-primary bg-primary/10"
                style={{ left: `${(low / bars.length) * 100}%`, width: `${((high - low + 1) / bars.length) * 100}%` }}
              />
            ) : null}
          </div>
          {bar ? (
            <div
              id={`${id}-tip`}
              role="tooltip"
              className="pointer-events-none absolute bottom-full z-20 mb-2 flex w-max max-w-[260px] -translate-x-1/2 flex-col gap-0.5 rounded-md border bg-popover px-2.5 py-1.5 text-caption shadow-md"
              style={{ left: `clamp(70px, ${left}%, calc(100% - 70px))` }}
            >
              <span className="font-mono text-[11px] text-muted-foreground">{clock(bar.at)} to {clock(bar.at + step)}</span>
              <span className="flex items-center gap-1.5"><span className="size-2 rounded-sm bg-primary" aria-hidden /><span className="font-semibold">{whole.format(bar.count)}</span> {noun}</span>
              {bar.errors > 0 ? (
                <span className="flex items-center gap-1.5 text-destructive-text"><span className="size-2 rounded-sm bg-destructive" aria-hidden /><span className="font-semibold">{whole.format(bar.errors)}</span> failed ({((bar.errors / Math.max(1, bar.count)) * 100).toFixed(1)}%)</span>
              ) : null}
            </div>
          ) : null}
        </div>
      </div>
      <div className="grid grid-cols-[34px_minmax(0,1fr)] gap-2 font-mono text-[10px] text-faint-foreground" aria-hidden>
        <span />
        <span className="flex justify-between">
          <span>{stamp(start)}</span>
          <span className="max-sm:hidden">{clock((start + end) / 2)}</span>
          <span>{stamp(end)}</span>
        </span>
      </div>
    </figure>
  )
}
