import { BellRing, Plus, TriangleAlert } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { because } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { NativeSelect } from '@/components/ui/input'
import { Alert, Table, Td, Th, Tr } from '@/components/ui/misc'
import { Switch } from '@/components/ui/switch'
import { EmptyState, PageHeader } from '@/components/ft/bits'
import { STATUSES, conditionOf, isForbidden, isNotConfigured, scopeOf, useAlerts, useToggleRule, type AlertStatus, type Rule } from '@/data/alerts'
import { useProjects } from '@/data/projects'
import { stamp } from '@/lib/format'
import { cn } from '@/lib/utils'
import { StateBadge } from './alerts/bits'
import { RuleDetail } from './alerts/RuleDetail'
import { RuleEditor } from './alerts/RuleEditor'

type Open = { view: 'detail'; id: string } | { view: 'edit'; id: string | null } | null

export function Alerts() {
  const { active } = useOrganisations()
  const { alerts, failed, nothing } = useAlerts()
  const projects = useProjects()
  const toggle = useToggleRule()
  const [state, setState] = useState<AlertStatus | ''>('')
  const [open, setOpen] = useState<Open>(() => {
    const asked = new URLSearchParams(window.location.search).get('rule')
    return asked ? { view: 'detail', id: asked } : null
  })
  const mayChange = active?.role === 'owner' || active?.role === 'admin'
  const projectName = (id: string | null) => (id === null ? 'All projects' : projects.find((one) => one.id === id)?.name ?? id)
  const shown = (alerts ?? []).filter((rule) => state === '' || rule.state.status === state)
  const selected = open ? alerts?.find((rule) => rule.id === open.id) ?? null : null
  const firing = (alerts ?? []).filter((rule) => rule.enabled && rule.state.status === 'firing').length

  return (
    <>
      <PageHeader title="Alerts" meta={alerts ? `${alerts.length} rules · ${firing} firing` : 'Rules on traces, logs and metrics'}>
        <NativeSelect aria-label="Filter by state" value={state} onChange={(e) => setState(e.target.value as AlertStatus | '')} className="w-[150px]">
          <option value="">State: any</option>
          {STATUSES.map((one) => <option key={one} value={one}>{one.replace('_', ' ')}</option>)}
        </NativeSelect>
        {mayChange ? <Button variant="primary" size="sm" onClick={() => setOpen({ view: 'edit', id: null })}><Plus />New alert</Button> : null}
      </PageHeader>
      {alerts && !mayChange ? <span className="text-control text-muted-foreground">Read only. Only an owner or an administrator can change a rule.</span> : null}

      {isNotConfigured(failed) ? (
        <Alert variant="warning" icon={<TriangleAlert />} title="Alerting is not configured">This server does not run the alert evaluator. Ask the operator to enable it.</Alert>
      ) : isForbidden(failed) ? (
        <Alert variant="warning" icon={<TriangleAlert />} title="You may not read alert rules">Your role in this organisation does not allow it.</Alert>
      ) : !alerts ? (
        <Card className="px-4 py-3 text-control text-muted-foreground">{nothing}</Card>
      ) : alerts.length === 0 ? (
        <EmptyState icon={<BellRing />} title="No alert rules yet" actions={mayChange ? <Button variant="primary" size="sm" onClick={() => setOpen({ view: 'edit', id: null })}><Plus />New alert</Button> : undefined}>
          A rule watches a trace, log or metric query and notifies a channel when it crosses a threshold.
        </EmptyState>
      ) : (
        <Card className="overflow-x-auto">
          <Table aria-label="Alert rules">
            <thead><tr><Th>Rule</Th><Th className="w-[90px]">Signal</Th><Th className="w-[100px]">Severity</Th><Th className="w-[210px]">State</Th><Th className="w-[140px]">Project</Th>{mayChange ? <Th numeric className="w-[70px]">On</Th> : null}</tr></thead>
            <tbody>
              {shown.length === 0 ? <Tr><Td colSpan={mayChange ? 6 : 5} className="text-muted-foreground">No rule is {state.replace('_', ' ')}.</Td></Tr> : null}
              {shown.map((rule) => <RuleRow key={rule.id} rule={rule} project={projectName(rule.project)} mayChange={mayChange} busy={toggle.isPending} onOpen={() => setOpen({ view: 'detail', id: rule.id })} onToggle={(enabled) => toggle.mutate({ id: rule.id, enabled }, { onError: (error) => toast.error(because(error, 'The rule was not changed')) })} />)}
            </tbody>
          </Table>
        </Card>
      )}

      <RuleDetail rule={open?.view === 'detail' ? selected : null} project={selected ? projectName(selected.project) : ''} mayChange={mayChange} onEdit={() => selected && setOpen({ view: 'edit', id: selected.id })} onClose={() => setOpen(null)} />
      <RuleEditor rule={open?.view === 'edit' ? selected : null} open={open?.view === 'edit'} onClose={() => setOpen(null)} />
    </>
  )
}

function RuleRow({ rule, project, mayChange, busy, onOpen, onToggle }: { rule: Rule; project: string; mayChange: boolean; busy: boolean; onOpen: () => void; onToggle: (enabled: boolean) => void }) {
  const { state } = rule
  return (
    <Tr className={cn(!rule.enabled && 'opacity-60')}>
      <Td className="whitespace-normal py-2">
        <div className="flex flex-col gap-0.5">
          <button type="button" onClick={onOpen} className="w-fit cursor-pointer border-0 bg-transparent p-0 text-left text-control font-semibold text-foreground hover:underline focus-visible:outline-2 focus-visible:outline-ring">{rule.name}</button>
          <span className="text-caption text-muted-foreground"><span className="font-mono">{conditionOf(rule)}</span> · {scopeOf(rule.signal)}</span>
        </div>
      </Td>
      <Td><Badge variant="outline">{rule.signal.kind}</Badge></Td>
      <Td><span className={cn('text-control font-semibold', rule.severity === 'critical' ? 'text-destructive-text' : rule.severity === 'warning' ? 'text-warning-text' : 'text-muted-foreground')}>{rule.severity}</span></Td>
      <Td><span className="flex flex-wrap items-center gap-1.5"><StateBadge status={state.status} /><span className="font-mono text-caption text-muted-foreground">since {stamp(state.since_ms)}</span></span></Td>
      <Td className="text-control">{project}</Td>
      {mayChange ? <Td numeric><Switch checked={rule.enabled} disabled={busy} onCheckedChange={onToggle} aria-label={`Enable ${rule.name}`} /></Td> : null}
    </Tr>
  )
}
