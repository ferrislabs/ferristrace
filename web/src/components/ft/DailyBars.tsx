import { useState } from 'react'
import { Card } from '@/components/ui/card'
import { cn } from '@/lib/utils'
import type { Day } from '@/data/usage'

const dayLabel = (at: number) => new Date(at).toLocaleDateString('en-US', { month: 'short', day: 'numeric', timeZone: 'UTC' })

export function DailyBars({ title, what, days, format }: { title: string; what: string; days: Day[]; format: (value: number) => string }) {
  const hi = Math.max(1, ...days.map((d) => d.bytes))
  const [hovered, setHovered] = useState<Day | null>(null)
  return (
    <Card className="flex flex-col gap-2.5 p-3.5">
      <div className="flex items-baseline gap-2">
        <span className="flex-1 text-control font-semibold">{title}</span>
        {hovered ? (
          <span className="text-control"><span className="font-semibold">{dayLabel(hovered.at)}</span> <span className="font-mono">{format(hovered.bytes)}</span></span>
        ) : (
          <span className="text-caption text-muted-foreground">peak {format(hi)}</span>
        )}
      </div>
      <div
        className="flex h-[120px] items-end gap-[2px]"
        role="img"
        aria-label={`${what} per day, ${days.length} days, peak ${format(hi)}`}
        onMouseLeave={() => setHovered(null)}
      >
        {days.map((d) => (
          <div
            key={d.at}
            className="flex h-full flex-1 items-end"
            title={`${dayLabel(d.at)} · ${format(d.bytes)}`}
            onMouseEnter={() => setHovered(d)}
          >
            <div
              className={cn('w-full rounded-t-[4px] bg-series', d.bytes === 0 && 'bg-border')}
              style={{ height: `${Math.max(d.bytes === 0 ? 1 : 2, (d.bytes / hi) * 100)}%` }}
            />
          </div>
        ))}
      </div>
      <div className="flex justify-between font-mono text-caption text-faint-foreground">
        <span>{days.length > 0 ? dayLabel(days[0].at) : ''}</span>
        <span>{days.length > 0 ? dayLabel(days[days.length - 1].at) : ''}</span>
      </div>
    </Card>
  )
}
