import { useState } from 'react'
import { Card } from '@/components/ui/card'
import { cn } from '@/lib/utils'
import type { Day } from '@/data/usage'

const dayLabel = (at: number) => new Date(at).toLocaleDateString('en-US', { month: 'short', day: 'numeric', timeZone: 'UTC' })
const fullLabel = (at: number) => new Date(at).toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric', timeZone: 'UTC' })

export function DailyBars({ title, what, label = 'total', days, format }: { title: string; what: string; label?: string; days: Day[]; format: (value: number) => string }) {
  const hi = Math.max(1, ...days.map((d) => d.bytes))
  const [hovered, setHovered] = useState<number | null>(null)
  const day = hovered === null ? null : days[hovered]
  const centre = hovered === null ? 0 : ((hovered + 0.5) / days.length) * 100
  const flip = centre > 55
  return (
    <Card className="flex flex-col gap-2.5 p-3.5">
      <div className="flex items-baseline gap-2">
        <span className="flex-1 text-control font-semibold">{title}</span>
        <span className="text-caption text-muted-foreground">peak {format(hi)}</span>
      </div>
      <div className="relative" onMouseLeave={() => setHovered(null)}>
        <div
          className="flex h-[120px] items-end gap-[2px]"
          role="img"
          aria-label={`${what} per day, ${days.length} days, peak ${format(hi)}`}
        >
          {days.map((d, index) => (
            <div key={d.at} className="flex h-full flex-1 items-end" onMouseEnter={() => setHovered(index)}>
              <div
                className={cn('w-full rounded-t-[4px] bg-series', d.bytes === 0 && 'bg-border', hovered !== null && hovered !== index && 'opacity-60')}
                style={{ height: `${Math.max(d.bytes === 0 ? 1 : 2, (d.bytes / hi) * 100)}%` }}
              />
            </div>
          ))}
        </div>
        {day ? (
          <>
            <span className="pointer-events-none absolute inset-y-0 border-l border-dashed border-foreground/50" style={{ left: `${centre}%` }} aria-hidden />
            <div
              role="status"
              className="pointer-events-none absolute top-2 z-10 min-w-[200px] rounded-lg border bg-popover px-3 py-2.5 text-popover-foreground shadow-lg"
              style={flip ? { right: `${100 - centre + 2}%` } : { left: `${centre + 2}%` }}
            >
              <div className="pb-2 text-control font-semibold">{fullLabel(day.at)}</div>
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
        <span>{days.length > 0 ? dayLabel(days[0].at) : ''}</span>
        <span>{days.length > 0 ? dayLabel(days[days.length - 1].at) : ''}</span>
      </div>
    </Card>
  )
}
