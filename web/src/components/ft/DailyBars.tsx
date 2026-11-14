import { useEffect, useRef, useState } from 'react'
import { Card } from '@/components/ui/card'
import { cn } from '@/lib/utils'
import type { Day } from '@/data/usage'

const DAY = 86_400_000
const DRAG_PIXELS = 4

const dayLabel = (at: number) => new Date(at).toLocaleDateString('en-US', { month: 'short', day: 'numeric', timeZone: 'UTC' })
const stamp = (at: number, step: number) =>
  step >= DAY
    ? new Date(at).toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric', timeZone: 'UTC' })
    : `${dayLabel(at)}, ${new Date(at).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit', timeZone: 'UTC' })}`
const edge = (at: number, step: number) => (step >= DAY ? dayLabel(at) : stamp(at, step))

export function DailyBars({ title, what, label = 'total', days, format, step = DAY, onSelect }: {
  title: string
  what: string
  label?: string
  days: Day[]
  format: (value: number) => string
  step?: number
  onSelect?: (from: number, to: number) => void
}) {
  const hi = Math.max(1, ...days.map((d) => d.bytes))
  const [hovered, setHovered] = useState<number | null>(null)
  const [drag, setDrag] = useState<{ from: number; to: number } | null>(null)
  const origin = useRef<number | null>(null)
  const moved = useRef(false)
  const dragging = drag !== null

  useEffect(() => {
    if (!dragging) return
    const finish = () => {
      setDrag((current) => {
        if (current && moved.current && onSelect) {
          const lo = Math.min(current.from, current.to)
          const up = Math.max(current.from, current.to)
          onSelect(days[lo].at, days[up].at + step)
        }
        return null
      })
      origin.current = null
      moved.current = false
    }
    const track = (event: MouseEvent) => {
      if (origin.current !== null && Math.abs(event.clientX - origin.current) > DRAG_PIXELS) moved.current = true
    }
    window.addEventListener('mouseup', finish)
    window.addEventListener('mousemove', track)
    return () => {
      window.removeEventListener('mouseup', finish)
      window.removeEventListener('mousemove', track)
    }
  }, [dragging, days, step, onSelect])

  const day = hovered === null ? null : days[hovered]
  const centre = hovered === null ? 0 : ((hovered + 0.5) / days.length) * 100
  const flip = centre > 55
  const band = drag && moved.current ? { lo: Math.min(drag.from, drag.to), up: Math.max(drag.from, drag.to) } : null

  return (
    <Card className="flex flex-col gap-2.5 p-3.5">
      <div className="flex items-baseline gap-2">
        <span className="flex-1 text-control font-semibold">{title}</span>
        <span className="text-caption text-muted-foreground">{onSelect ? 'drag to zoom · ' : ''}peak {format(hi)}</span>
      </div>
      <div className="relative select-none" onMouseLeave={() => setHovered(null)}>
        <div
          className={cn('flex h-[120px] items-end gap-[2px]', onSelect && 'cursor-crosshair')}
          role="img"
          aria-label={`${what} per ${step >= DAY ? 'day' : 'hour'}, ${days.length} bars, peak ${format(hi)}`}
        >
          {days.map((d, index) => (
            <div
              key={d.at}
              className="flex h-full flex-1 items-end"
              onMouseEnter={() => {
                setHovered(index)
                setDrag((current) => (current ? { ...current, to: index } : current))
              }}
              onMouseDown={(event) => {
                if (!onSelect || event.button !== 0) return
                event.preventDefault()
                origin.current = event.clientX
                moved.current = false
                setDrag({ from: index, to: index })
              }}
            >
              <div
                className={cn('w-full rounded-t-[4px] bg-series', d.bytes === 0 && 'bg-border', hovered !== null && hovered !== index && !dragging && 'opacity-60')}
                style={{ height: `${Math.max(d.bytes === 0 ? 1 : 2, (d.bytes / hi) * 100)}%` }}
              />
            </div>
          ))}
        </div>
        {band ? (
          <span
            className="pointer-events-none absolute inset-y-0 rounded-sm border-x border-series bg-series/15"
            style={{ left: `${(band.lo / days.length) * 100}%`, width: `${((band.up - band.lo + 1) / days.length) * 100}%` }}
            aria-hidden
          />
        ) : null}
        {day && !dragging ? (
          <>
            <span className="pointer-events-none absolute inset-y-0 border-l border-dashed border-foreground/50" style={{ left: `${centre}%` }} aria-hidden />
            <div
              role="status"
              className="pointer-events-none absolute top-2 z-10 min-w-[200px] rounded-lg border bg-popover px-3 py-2.5 text-popover-foreground shadow-lg"
              style={flip ? { right: `${100 - centre + 2}%` } : { left: `${centre + 2}%` }}
            >
              <div className="pb-2 text-control font-semibold">{stamp(day.at, step)}</div>
              <div className="flex items-center gap-2 border-t border-dashed pt-2 text-control">
                <span className="size-2.5 shrink-0 rounded-[3px] bg-series" aria-hidden />
                <span className="font-mono text-muted-foreground">{label}</span>
                <span className="mx-1 flex-1 border-b border-dotted border-series/60" aria-hidden />
                <span className="font-mono font-semibold">{format(day.bytes)}</span>
              </div>
            </div>
          </>
        ) : null}
      </div>
      <div className="flex justify-between font-mono text-caption text-faint-foreground">
        <span>{days.length > 0 ? edge(days[0].at, step) : ''}</span>
        <span>{days.length > 0 ? edge(days[days.length - 1].at, step) : ''}</span>
      </div>
    </Card>
  )
}
