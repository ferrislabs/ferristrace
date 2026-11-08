import { Gauge, TriangleAlert } from 'lucide-react'
import { useState } from 'react'
import { Card, CardHeader, CardTitle } from '@/components/ui/card'
import { Alert, Table, Td, Th, Tr } from '@/components/ui/misc'
import { Segmented } from '@/components/ui/segmented'
import { EmptyState, Kpi, PageHeader } from '@/components/ft/bits'
import { isForbidden, isNotConfigured } from '@/data/alerts'
import { useIncidentInsights, type InsightGroup, type Timing } from '@/data/incidents'
import { SeverityPill } from './Incidents'

const DAYS = [7, 30, 90] as const

export function span(ms: number | null): string {
  if (ms === null) return '–'
  const minutes = Math.round(ms / 60_000)
  if (minutes < 1) return '< 1 m'
  if (minutes < 60) return `${minutes} m`
  const hours = Math.floor(minutes / 60)
  if (hours < 48) return `${hours} h ${minutes % 60} m`
  return `${Math.round(hours / 24)} d`
}

const times = (timing: Timing) => (timing.samples === 0 ? '–' : `${span(timing.median_ms)} median · ${span(timing.mean_ms)} mean`)

function Row({ label, group }: { label: React.ReactNode; group: InsightGroup }) {
  return (
    <Tr>
      <Td className="font-semibold">{label}</Td>
      <Td numeric>{group.incidents}</Td>
      <Td numeric>{group.open}</Td>
      <Td className="font-mono text-caption">{times(group.acknowledge)}</Td>
      <Td className="font-mono text-caption">{times(group.resolve)}</Td>
    </Tr>
  )
}

function Header() {
  return <thead><tr><Th>{' '}</Th><Th className="w-[90px]">Incidents</Th><Th className="w-[70px]">Open</Th><Th>Time to acknowledge</Th><Th>Time to resolve</Th></tr></thead>
}

export function IncidentInsights() {
  const [days, setDays] = useState<(typeof DAYS)[number]>(30)
  const { insights, failed } = useIncidentInsights(days)
  const peak = Math.max(1, ...(insights?.per_day.map((day) => day.incidents) ?? [1]))

  return (
    <>
      <PageHeader title="Incident insights" meta="From the incident timelines, no extra tracking">
        <Segmented<string> label="Window" value={String(days)} onChange={(value) => setDays(Number(value) as (typeof DAYS)[number])} options={DAYS.map((one) => ({ value: String(one), label: `${one} days` }))} />
      </PageHeader>

      {isNotConfigured(failed) ? (
        <Alert variant="warning" icon={<TriangleAlert />} title="Incidents are not available">This server has no control plane to keep incidents.</Alert>
      ) : isForbidden(failed) ? (
        <Alert variant="warning" icon={<TriangleAlert />} title="You may not read incidents">Your role in this organisation does not allow it.</Alert>
      ) : !insights ? (
        <Card className="px-4 py-3 text-control text-muted-foreground">Reading the incidents…</Card>
      ) : insights.overall.incidents === 0 ? (
        <EmptyState icon={<Gauge />} title="No incident in this window">Incidents opened in the last {days} days are counted here, with how long they took to be acknowledged and resolved.</EmptyState>
      ) : (
        <>
          <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
            <Kpi label="Incidents" value={String(insights.overall.incidents)} sub={`${insights.overall.open} still open`} tone={insights.overall.open > 0 ? 'warn' : undefined} />
            <Kpi label="Time to acknowledge" value={span(insights.overall.acknowledge.median_ms)} sub={`median of ${insights.overall.acknowledge.samples} · first status change, owner or acknowledged page`} />
            <Kpi label="Time to resolve" value={span(insights.overall.resolve.median_ms)} sub={`median of ${insights.overall.resolve.samples} resolved`} />
            <Kpi label="Mean time to resolve" value={span(insights.overall.resolve.mean_ms)} sub="resolved incidents only" />
          </div>

          <Card>
            <CardHeader><CardTitle className="flex-1">Incidents per day</CardTitle><span className="text-caption text-muted-foreground">opened, UTC</span></CardHeader>
            <div className="flex h-24 items-end gap-0.5 px-4 pb-3 pt-2" role="img" aria-label="Incidents opened per day">
              {insights.per_day.map((day) => (
                <div key={day.day_ms} title={`${new Date(day.day_ms).toISOString().slice(0, 10)}: ${day.incidents}`} className="flex-1 rounded-sm bg-chart-line" style={{ height: `${Math.max(day.incidents === 0 ? 2 : 8, (day.incidents / peak) * 100)}%`, opacity: day.incidents === 0 ? 0.25 : 1 }} />
              ))}
            </div>
          </Card>

          <Card className="overflow-x-auto">
            <CardHeader><CardTitle>By severity</CardTitle></CardHeader>
            <Table aria-label="Incidents by severity">
              <Header />
              <tbody>{insights.by_severity.map((group) => <Row key={group.severity} label={<SeverityPill severity={group.severity} />} group={group} />)}</tbody>
            </Table>
          </Card>

          <Card className="overflow-x-auto">
            <CardHeader><CardTitle className="flex-1">By service</CardTitle><span className="text-caption text-muted-foreground">the services of the alert that opened the incident</span></CardHeader>
            <Table aria-label="Incidents by service">
              <Header />
              <tbody>{insights.by_service.map((group) => <Row key={group.service ?? 'none'} label={group.service ?? <span className="font-normal text-muted-foreground">Declared by hand</span>} group={group} />)}</tbody>
            </Table>
          </Card>
        </>
      )}
    </>
  )
}
