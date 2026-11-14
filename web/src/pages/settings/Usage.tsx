import { useMemo, useState } from 'react'
import { DailyBars } from '@/components/ft/DailyBars'
import { Meter } from '@/components/ft/Meter'
import { Card } from '@/components/ui/card'
import { Segmented } from '@/components/ui/segmented'
import { Table, Td, Th, Tr } from '@/components/ui/misc'
import { byDay, bytes, projected, startOfMonth, startOfNextMonth, useMetered, type Day } from '@/data/usage'
import { Link } from '@/lib/router'
import { H, Sub } from './parts'

const DAY = 86_400_000

const month = (at: number) => new Date(at).toLocaleDateString('en-US', { month: 'long', year: 'numeric', timeZone: 'UTC' })
const dayLabel = (at: number) => new Date(at).toLocaleDateString('en-US', { month: 'short', day: 'numeric', timeZone: 'UTC' })

export function Usage() {
  const [period, setPeriod] = useState('this')
  const now = Date.now()

  const [from, to] = useMemo(() => {
    const start = startOfMonth(now)
    return period === 'this' ? [start, startOfNextMonth(now)] : [startOfMonth(start - DAY), start]
  }, [period, now])

  const { metered, nothing } = useMetered(from, to)
  const days = useMemo(() => (metered ? byDay(metered.hours, from, Math.min(to, now + DAY)) : []), [metered, from, to, now])

  const rolled = days.reduce((total, d) => total + d.bytes, 0)
  const used = period === 'this' ? (metered?.ingested_bytes ?? 0) : rolled
  const included = metered?.included_bytes ?? null
  const busiest = days.reduce<Day | null>((peak, d) => (peak === null || d.bytes > peak.bytes ? d : peak), null)
  const average = days.length > 0 ? Math.round(rolled / days.length) : 0
  const ceiling = (count: number) => (included && included > 0 ? (count / included) * 100 : null)
  const ofIncluded = included ? `of ${bytes(included)} included` : undefined
  const noCeiling = 'This plan sets no ceiling.'
  const ahead = projected(used, now)

  return (
    <>
      <H title="Usage" sub={`${month(from)} · decompressed payload bytes, per organisation.`}>
        <Segmented
          label="Period"
          value={period}
          onChange={setPeriod}
          options={[{ value: 'this', label: 'This month' }, { value: 'last', label: 'Last month' }]}
        />
      </H>
      {metered ? (
        <>
          <div className="grid grid-cols-[repeat(auto-fit,minmax(240px,1fr))] gap-3">
            <Meter
              label={period === 'this' ? 'Ingested this month' : 'Ingested'}
              used={bytes(used)}
              of={ofIncluded}
              share={ceiling(used)}
              note={included ? `${(ceiling(used) ?? 0).toFixed(0)} % of what the plan allows` : noCeiling}
            />
            {period === 'this' ? (
              <Meter
                label="Projected at month end"
                used={bytes(ahead)}
                of={ofIncluded}
                share={ceiling(ahead)}
                note={
                  included && (ceiling(ahead) ?? 0) > 100
                    ? 'Past the ceiling before the month is out, at this rate.'
                    : 'If the rest of the month looks like the part that has happened.'
                }
              />
            ) : (
              <Meter
                label="Rolled up"
                used={bytes(rolled)}
                of={ofIncluded}
                share={ceiling(rolled)}
                note="Every finished hour of that month, added up."
              />
            )}
            <Meter
              label="Busiest day"
              used={bytes(busiest?.bytes ?? 0)}
              of={rolled > 0 ? `of ${bytes(rolled)} rolled up` : undefined}
              share={rolled > 0 ? ((busiest?.bytes ?? 0) / rolled) * 100 : null}
              note={busiest && busiest.bytes > 0 ? dayLabel(busiest.at) : 'Nothing has arrived yet.'}
            />
            <Meter
              label="Average per day"
              used={bytes(average)}
              of={busiest && busiest.bytes > 0 ? `of ${bytes(busiest.bytes)} at peak` : undefined}
              share={busiest && busiest.bytes > 0 ? (average / busiest.bytes) * 100 : null}
              note={days.length > 0 ? `Across the ${days.length} days read.` : 'No day has been rolled up yet.'}
            />
          </div>
          <DailyBars title="Ingested per day" what="Bytes ingested" days={days} format={bytes} />
          <Sub>By day</Sub>
          <Card className="max-h-[320px] overflow-auto">
            <Table aria-label="Ingested per day">
              <thead><tr><Th>Day</Th><Th numeric className="w-[160px]">Ingested</Th><Th numeric className="w-[120px]">Share</Th></tr></thead>
              <tbody>{[...days].reverse().map((d) => (
                <Tr key={d.at}>
                  <Td className="font-mono">{dayLabel(d.at)}</Td>
                  <Td numeric className="font-mono">{bytes(d.bytes)}</Td>
                  <Td numeric className="text-muted-foreground">{rolled > 0 ? `${((d.bytes / rolled) * 100).toFixed(1)} %` : '—'}</Td>
                </Tr>
              ))}</tbody>
            </Table>
            {days.length === 0 ? <p className="m-0 px-4 py-3 text-control text-muted-foreground">No finished hour of this month has been rolled up yet.</p> : null}
          </Card>
        </>
      ) : (
        <Card className="px-4 py-3 text-control text-muted-foreground">{nothing}</Card>
      )}
      <p className="m-0 text-caption text-muted-foreground">
        What is metered is the decompressed size of each payload accepted, counted per organisation — not per project, because the counter is keyed by the organisation. The <Link to="/cost">Cost meter</Link> splits it by signal, and nothing measures what the span store holds. The ceiling is the <Link to="/settings/billing">plan</Link> the control plane carries, and it is the same number a payload is refused against.
      </p>
    </>
  )
}
