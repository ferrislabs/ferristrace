import { useId, useMemo, useRef, useState } from 'react'
import { useSpanSeries } from '@/data/spanSeries'
import type { Node } from '@/lib/queryLang'
import { cn } from '@/lib/utils'

const HEIGHT = 56
const MIN_BARS_SELECTED = 1

const clock = (ms: number) => new Date(ms).toTimeString().slice(0, 8)

const longClock = (ms: number) => `${new Date(ms).toISOString().slice(5, 10)} ${clock(ms)}`

export function Histogram({ items, rangeMs, onZoom }: { items: Node[]; rangeMs: number; onZoom: (from: number, to: number) => void }) {
  const total = useSpanSeries(rangeMs, 'count', '', items, true)
  const failed = useSpanSeries(rangeMs, 'errors', '', items, true)
  const id = useId()
  const box = useRef<HTMLDivElement>(null)
  const [drag, setDrag] = useState<{ from: number; to: number } | null>(null)
  const [cursor, setCursor] = useState<number | null>(null)
  const [anchor, setAnchor] = useState<number | null>(null)

  const bars = useMemo(() => {
    const counts = total.lines[0]?.points ?? []
    const errors = failed.lines[0]?.points ?? []
    return counts.map(([ns, value], index) => ({ at: ns / 1_000_000, count: value ?? 0, errors: errors[index]?.[1] ?? 0 }))
  }, [total.lines, failed.lines])

  const step = bars.length > 1 ? bars[1].at - bars[0].at : 0
  const start = bars[0]?.at ?? 0
  const end = bars.length > 0 ? bars[bars.length - 1].at + step : 0
  const peak = Math.max(1, ...bars.map((bar) => bar.count))

  const indexAt = (clientX: number): number => {
    const rect = box.current?.getBoundingClientRect()
    if (!rect || rect.width === 0 || bars.length === 0) return 0
    const fraction = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width))
    return Math.min(bars.length - 1, Math.floor(fraction * bars.length))
  }

  const apply = (a: number, b: number) => {
    const low = Math.min(a, b)
    const high = Math.max(a, b)
    if (bars.length === 0 || high - low + 1 < MIN_BARS_SELECTED) return
    onZoom(bars[low].at, bars[high].at + step)
  }

  const selected = drag ?? (cursor !== null && anchor !== null ? { from: anchor, to: cursor } : null)
  const low = selected ? Math.min(selected.from, selected.to) : 0
  const high = selected ? Math.max(selected.from, selected.to) : 0
  const width = 100 / Math.max(1, bars.length)
  const description = bars.length === 0 ? 'No spans in this window' : `Spans from ${longClock(start)} to ${longClock(end)}, ${bars.reduce((sum, bar) => sum + bar.count, 0).toLocaleString('en-US')} in all`

  if (bars.length === 0 || peak <= 1 && bars.every((bar) => bar.count === 0)) {
    return (
      <div className="flex h-[56px] items-center justify-center rounded-lg border border-dashed text-caption text-muted-foreground" role="status">
        {total.loading ? 'Counting the spans…' : 'No span matches in this window.'}
      </div>
    )
  }

  return (
    <figure className="m-0 flex flex-col gap-1">
      <div
        ref={box}
        role="application"
        aria-roledescription="histogram with a time selection"
        aria-label={`${description}. Drag to zoom on a period, or use the arrow keys, Shift to select, Enter to zoom, Escape to cancel.`}
        aria-describedby={`${id}-hint`}
        tabIndex={0}
        className="relative cursor-crosshair touch-none select-none rounded-lg border bg-card focus-visible:outline-2 focus-visible:outline-ring"
        style={{ height: HEIGHT }}
        onPointerDown={(event) => {
          event.currentTarget.setPointerCapture(event.pointerId)
          const at = indexAt(event.clientX)
          setDrag({ from: at, to: at })
          setCursor(null)
          setAnchor(null)
        }}
        onPointerMove={(event) => {
          if (drag) setDrag({ from: drag.from, to: indexAt(event.clientX) })
        }}
        onPointerUp={() => {
          if (drag) apply(drag.from, drag.to)
          setDrag(null)
        }}
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
        <svg viewBox={`0 0 100 ${HEIGHT}`} preserveAspectRatio="none" width="100%" height={HEIGHT} aria-hidden className="block">
          {bars.map((bar, index) => {
            const tall = (bar.count / peak) * (HEIGHT - 4)
            const red = bar.count === 0 ? 0 : (Math.min(bar.errors, bar.count) / bar.count) * tall
            return (
              <g key={bar.at}>
                <rect x={index * width + 0.08} width={Math.max(0.1, width - 0.16)} y={HEIGHT - tall} height={tall} fill="var(--primary)" opacity={selected && (index < low || index > high) ? 0.35 : 0.75} />
                {red > 0 ? <rect x={index * width + 0.08} width={Math.max(0.1, width - 0.16)} y={HEIGHT - red} height={red} fill="var(--destructive)" /> : null}
              </g>
            )
          })}
          {cursor !== null && anchor === null ? <rect x={cursor * width} width={width} y={0} height={HEIGHT} fill="none" stroke="var(--ring)" strokeWidth="0.4" vectorEffect="non-scaling-stroke" /> : null}
          {selected ? <rect x={low * width} width={(high - low + 1) * width} y={0} height={HEIGHT} fill="var(--primary)" opacity={0.12} stroke="var(--primary)" strokeWidth="1" vectorEffect="non-scaling-stroke" /> : null}
        </svg>
      </div>
      <figcaption id={`${id}-hint`} className={cn('flex justify-between font-mono text-[10px] text-faint-foreground')}>
        <span>{longClock(start)}</span>
        <span>{selected ? `${longClock(bars[low].at)} to ${longClock(bars[high].at + step)}` : 'Drag to zoom. Red is the share of failed spans.'}</span>
        <span>{longClock(end)}</span>
      </figcaption>
    </figure>
  )
}
