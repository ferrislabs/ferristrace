import { Plus, Siren, TriangleAlert } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { because } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Dialog, DialogContent, DialogFooter } from '@/components/ui/dialog'
import { Field, Input, NativeSelect } from '@/components/ui/input'
import { Alert, Table, Td, Th, Tr } from '@/components/ui/misc'
import { Segmented } from '@/components/ui/segmented'
import { Switch } from '@/components/ui/switch'
import { EmptyState, PageHeader } from '@/components/ft/bits'
import { isForbidden, isNotConfigured } from '@/data/alerts'
import {
  SEVERITIES, STATUSES, personOf, severityVariant, statusVariant, useDeclareIncident, useIncidents,
  type Incident, type IncidentScope, type IncidentSeverity, type IncidentStatus,
} from '@/data/incidents'
import { useProjects } from '@/data/projects'
import { stamp } from '@/lib/format'
import { Link, useNavigate } from '@/lib/router'
import { cn } from '@/lib/utils'

export const SeverityPill = ({ severity }: { severity: IncidentSeverity }) => <Badge variant={severityVariant(severity)} mono>{severity.toUpperCase()}</Badge>

export const StatusPill = ({ status }: { status: IncidentStatus }) => <Badge variant={statusVariant(status)}>{status}</Badge>

export function Incidents() {
  const { active } = useOrganisations()
  const [scope, setScope] = useState<IncidentScope>('open')
  const [severity, setSeverity] = useState<IncidentSeverity | ''>('')
  const [statuses, setStatuses] = useState<IncidentStatus[]>([])
  const [declaring, setDeclaring] = useState(false)
  const { incidents, failed, nothing } = useIncidents(scope)
  const projects = useProjects()
  const mayDeclare = Boolean(active)
  const projectName = (id: string | null) => (id === null ? 'All projects' : projects.find((one) => one.id === id)?.name ?? id)
  const shown = (incidents ?? []).filter((one) => (severity === '' || one.severity === severity) && (statuses.length === 0 || statuses.includes(one.status)))
  const toggle = (status: IncidentStatus) => setStatuses((held) => (held.includes(status) ? held.filter((one) => one !== status) : [...held, status]))
  const chips = scope === 'resolved' ? [] : STATUSES.filter((one) => scope === 'all' || one !== 'resolved')

  return (
    <>
      <PageHeader title="Incidents" meta={incidents ? `${incidents.length} ${scope === 'all' ? '' : `${scope} `}incident${incidents.length === 1 ? '' : 's'}` : 'Disruptions, from the first page to resolution'}>
        <Segmented<IncidentScope> label="Incidents shown" value={scope} onChange={(value) => { setScope(value); setStatuses([]) }} options={[{ value: 'open', label: 'Open' }, { value: 'resolved', label: 'Resolved' }, { value: 'all', label: 'All' }]} />
        <NativeSelect aria-label="Filter by severity" value={severity} onChange={(e) => setSeverity(e.target.value as IncidentSeverity | '')} className="w-[140px]">
          <option value="">Severity: any</option>
          {SEVERITIES.map((one) => <option key={one} value={one}>{one.toUpperCase()}</option>)}
        </NativeSelect>
        {mayDeclare ? <Button variant="primary" size="sm" onClick={() => setDeclaring(true)}><Plus />Declare incident</Button> : null}
      </PageHeader>
      {chips.length ? (
        <div className="flex flex-wrap items-center gap-1.5" role="group" aria-label="Filter by status">
          {chips.map((one) => (
            <button key={one} type="button" aria-pressed={statuses.includes(one)} onClick={() => toggle(one)} className={cn('cursor-pointer rounded-full border px-2.5 py-0.5 text-caption font-semibold text-muted-foreground hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring', statuses.includes(one) && 'border-accent-text bg-accent-subtle text-accent-text')}>{one}</button>
          ))}
        </div>
      ) : null}

      {isNotConfigured(failed) ? (
        <Alert variant="warning" icon={<TriangleAlert />} title="Incidents are not available">This server has no control plane to keep incidents. Ask the operator to configure one.</Alert>
      ) : isForbidden(failed) ? (
        <Alert variant="warning" icon={<TriangleAlert />} title="You may not read incidents">Your role in this organisation does not allow it.</Alert>
      ) : !incidents ? (
        <Card className="px-4 py-3 text-control text-muted-foreground">{nothing}</Card>
      ) : incidents.length === 0 ? (
        <EmptyState icon={<Siren />} title={scope === 'open' ? 'No open incident' : 'No incident yet'} actions={mayDeclare ? <Button variant="primary" size="sm" onClick={() => setDeclaring(true)}><Plus />Declare incident</Button> : undefined}>
          An incident tracks a disruption: who owns it, how bad it is, what was tried, and the traces, logs and metrics that explain it.
        </EmptyState>
      ) : (
        <Card className="overflow-x-auto">
          <Table aria-label="Incidents">
            <thead><tr><Th>Incident</Th><Th className="w-[80px]">Severity</Th><Th className="w-[130px]">Status</Th><Th className="w-[180px]">Owner</Th><Th className="w-[140px]">Project</Th><Th className="w-[170px]">Opened</Th></tr></thead>
            <tbody>
              {shown.length === 0 ? <Tr><Td colSpan={6} className="text-muted-foreground">No incident matches these filters.</Td></Tr> : null}
              {shown.map((incident) => <IncidentRow key={incident.id} incident={incident} project={projectName(incident.project)} />)}
            </tbody>
          </Table>
        </Card>
      )}

      <DeclareIncident open={declaring} onClose={() => setDeclaring(false)} />
    </>
  )
}

function IncidentRow({ incident, project }: { incident: Incident; project: string }) {
  return (
    <Tr>
      <Td className="whitespace-normal py-2">
        <div className="flex flex-col gap-0.5">
          <Link to={`/incidents/${incident.id}`} className="w-fit text-control font-semibold text-foreground hover:underline">{incident.title}</Link>
          <span className="text-caption text-muted-foreground">
            {incident.alert ? <>From the alert <span className="font-semibold">{incident.alert.name}</span></> : 'Declared by hand'}
            {incident.public ? ' · public' : ''}
          </span>
        </div>
      </Td>
      <Td><SeverityPill severity={incident.severity} /></Td>
      <Td><StatusPill status={incident.status} /></Td>
      <Td className="text-control">{personOf(incident.owner)}</Td>
      <Td className="text-control">{project}</Td>
      <Td className="font-mono text-caption">{stamp(incident.opened_ms)}</Td>
    </Tr>
  )
}

export function DeclareIncident({ open, onClose, alert }: { open: boolean; onClose: () => void; alert?: { id: string; name: string } }) {
  const declare = useDeclareIncident()
  const navigate = useNavigate()
  const [title, setTitle] = useState('')
  const [severity, setSeverity] = useState<IncidentSeverity | ''>('')
  const [shared, setShared] = useState(false)
  const close = () => { setTitle(''); setSeverity(''); setShared(false); onClose() }
  const submit = () => {
    const asked = {
      ...(title.trim() ? { title: title.trim() } : {}),
      ...(severity ? { severity } : {}),
      ...(alert ? { alert: alert.id } : {}),
      public: shared,
    }
    declare.mutate(asked, {
      onSuccess: (incident) => { toast.success('Incident declared', { description: incident.title }); close(); navigate(`/incidents/${incident.id}`) },
      onError: (error) => toast.error(because(error, 'The incident was not declared')),
    })
  }
  return (
    <Dialog open={open} onOpenChange={(o) => !o && close()}>
      <DialogContent title={alert ? 'Declare incident from this alert' : 'Declare incident'} description={alert ? `The incident follows ${alert.name}: its later state changes land in the timeline.` : 'Everyone in the organisation can work on it.'}>
        <form className="flex flex-col gap-3" onSubmit={(e) => { e.preventDefault(); submit() }}>
          <Field label="Title" htmlFor="incident-title" hint={alert ? 'Leave empty to use the alert name.' : undefined}>
            <Input id="incident-title" value={title} placeholder={alert?.name ?? 'Some card payments fail at checkout'} onChange={(e) => setTitle(e.target.value)} autoFocus />
          </Field>
          <Field label="Severity" htmlFor="incident-severity" hint={alert ? 'Leave as is to follow the alert: critical is SEV2, warning SEV3, info SEV4.' : 'SEV3 unless you choose.'}>
            <NativeSelect id="incident-severity" value={severity} onChange={(e) => setSeverity(e.target.value as IncidentSeverity | '')}>
              <option value="">{alert ? 'From the alert' : 'SEV3'}</option>
              {SEVERITIES.map((one) => <option key={one} value={one}>{one.toUpperCase()}</option>)}
            </NativeSelect>
          </Field>
          <Switch checked={shared} onCheckedChange={setShared} label="Public, once status pages show incidents" />
          <DialogFooter>
            <Button type="button" onClick={close}>Cancel</Button>
            <Button type="submit" variant="primary" disabled={declare.isPending || (!alert && !title.trim())}>Declare</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
