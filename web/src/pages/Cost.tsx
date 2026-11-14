import { useMemo, useState } from 'react'
import { DailyBars } from '@/components/ft/DailyBars'
import { Meter } from '@/components/ft/Meter'
import { Card } from '@/components/ui/card'
import { Segmented } from '@/components/ui/segmented'
import { Table, Td, Th, Tr } from '@/components/ui/misc'
import { SIGNALS, bytes, count, dailyOf, projected, startOfMonth, startOfNextMonth, useMetered, type Signal } from '@/data/usage'
import { Link } from '@/lib/router'
import { H, Sub } from './settings/parts'

const DAY = 86_400_000

const month = (at: number) => new Date(at).toLocaleDateString('en-US', { month: 'long', year: 'numeric', timeZone: 'UTC' })
const dayLabel = (at: number) => new Date(at).toLocaleDateString('en-US', { month: 'short', day: 'numeric', timeZone: 'UTC' })

const TOTALS: Record<Signal, 'spans_bytes' | 'logs_bytes' | 'metrics_points'> = {
  spans: 'spans_bytes',
  logs: 'logs_bytes',
  metrics: 'metrics_points',
}

export function Cost() {
  const [period, setPeriod] = useState('this')
  const now = Date.now()

  const [from, to] = useMemo(() => {
    const start = startOfMonth(now)
    return period === 'this' ? [start, startOfNextMonth(now)] : [startOfMonth(start - DAY), start]
  }, [period, now])

  const { metered, nothing } = useMetered(from, to)
  const until = Math.min(to, now + DAY)
  const series = useMemo(
    () => SIGNALS.map((signal) => ({ signal, days: metered ? dailyOf(metered.hours, signal.read, from, until) : [] })),
    [metered, from, until],
  )
  const split = metered ? metered.hours.length - metered.by_signal.unsplit_hours : 0
  const show = (unit: 'bytes' | 'points', value: number) => (unit === 'bytes' ? bytes(value) : count(value))
  const rows = useMemo(() => series[0].days.map((_, at) => ({ at: series[0].days[at].at, cells: series.map((one) => one.days[at].bytes) })), [series])

  return (
    <div className="flex flex-col gap-4 p-4">
      <H title="Cost meter" sub={`${month(from)} · what was ingested, by signal, for the whole organisation.`}>
        <Segmented
          label="Period"
          value={period}
          onChange={setPeriod}
          options={[{ value: 'this', label: 'This month' }, { value: 'last', label: 'Last month' }]}
        />
      </H>
      {metered ? (
        split === 0 ? (
          <Card className="px-4 py-3 text-control text-muted-foreground">
            No finished hour of this month carries a split yet. Hours are rolled up once they end, so the first figures appear within the hour after a payload arrives.
          </Card>
        ) : (
          <>
            <div className="grid grid-cols-[repeat(auto-fit,minmax(240px,1fr))] gap-3">
              {SIGNALS.map((signal) => {
                const total = metered.by_signal[TOTALS[signal.key]]
                const ahead = projected(total, now)
                const ofAll = signal.unit === 'bytes' && metered.ingested_bytes > 0 ? (total / metered.ingested_bytes) * 100 : null
                return (
                  <Meter
                    key={signal.key}
                    label={signal.label}
                    used={show(signal.unit, total)}
                    of={signal.unit === 'bytes' ? 'decompressed' : 'datapoints'}
                    share={ofAll}
                    note={
                      period === 'this'
                        ? `About ${show(signal.unit, ahead)} at month end, at this rate.`
                        : ofAll === null ? 'Every finished hour of that month.' : `${ofAll.toFixed(0)} % of what that month ingested.`
                    }
                  />
                )
              })}
            </div>
            {metered.by_signal.unsplit_hours > 0 ? (
              <p className="m-0 text-caption text-muted-foreground">
                {metered.by_signal.unsplit_hours} {metered.by_signal.unsplit_hours === 1 ? 'hour was' : 'hours were'} rolled up before signals were counted apart. They are in the <Link to="/settings/usage">usage total</Link> and left out of these figures.
              </p>
            ) : null}
            <div className="grid grid-cols-[repeat(auto-fit,minmax(320px,1fr))] gap-3">
              {series.map(({ signal, days }) => (
                <DailyBars
                  key={signal.key}
                  title={`${signal.label} per day`}
                  what={signal.label}
                  label={signal.unit === 'bytes' ? 'size' : 'datapoints'}
                  days={days}
                  format={(value) => show(signal.unit, value)}
                />
              ))}
            </div>
            <Sub>By day</Sub>
            <Card className="max-h-[320px] overflow-auto">
              <Table aria-label="Ingested per day and signal">
                <thead>
                  <tr>
                    <Th>Day</Th>
                    {SIGNALS.map((signal) => <Th key={signal.key} numeric className="w-[160px]">{signal.label}</Th>)}
                  </tr>
                </thead>
                <tbody>
                  {[...rows].reverse().map((row) => (
                    <Tr key={row.at}>
                      <Td className="font-mono">{dayLabel(row.at)}</Td>
                      {row.cells.map((cell, index) => <Td key={SIGNALS[index].key} numeric className="font-mono">{show(SIGNALS[index].unit, cell)}</Td>)}
                    </Tr>
                  ))}
                </tbody>
              </Table>
            </Card>
          </>
        )
      ) : (
        <Card className="px-4 py-3 text-control text-muted-foreground">{nothing}</Card>
      )}
      <p className="m-0 text-caption text-muted-foreground">
        Logs and spans are counted in decompressed payload bytes, metrics in datapoints, when a payload is accepted. Hours from before that can be rebuilt from what the span store holds with <code>ferristrace backfill-usage</code>; those figures are measured on stored rows and sit a few percent under the payload bytes. A payload refused at the ceiling counts in none of them. The total the <Link to="/settings/billing">plan</Link> is measured against stays on <Link to="/settings/usage">Usage</Link>.
      </p>
    </div>
  )
}
