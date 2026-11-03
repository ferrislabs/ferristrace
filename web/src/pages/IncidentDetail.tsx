import { BellRing, LineChart, MessageSquare, Paperclip, Pencil, Rocket, ScrollText, Waypoints } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { because } from '@/auth/client'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Input, NativeSelect, Textarea } from '@/components/ui/input'
import { Alert } from '@/components/ui/misc'
import { Switch } from '@/components/ui/switch'
import { EmptyState } from '@/components/ft/bits'
import { AlertContext, StateBadge } from './alerts/bits'
import { SeverityPill, StatusPill } from './Incidents'
import {
  SEVERITIES, STATUSES, attachmentLink, personOf, useChangeIncident, useCommentIncident, useIncident,
  type Attachment, type AttachmentKind, type Dossier, type IncidentChange, type IncidentSeverity, type IncidentStatus, type TimelineEntry,
} from '@/data/incidents'
import { useMembers } from '@/data/members'
import { releasePath } from '@/data/releases'
import { stamp } from '@/lib/format'
import { Link, useParams } from '@/lib/router'
import { labelOf } from '@/components/ft/MetricChart'
import { PostmortemSection } from './PostmortemSection'

const KIND_ICON: Record<AttachmentKind, typeof Waypoints> = { trace: Waypoints, logs: ScrollText, metric: LineChart }

const KIND_WORD: Record<AttachmentKind, string> = { trace: 'Trace', logs: 'Logs', metric: 'Metric' }

export function IncidentDetail() {
  const { id } = useParams()
  const { incident, failed } = useIncident(id)

  return (
    <>
      <nav aria-label="Breadcrumb" className="text-control"><Link to="/incidents">Incidents</Link><span className="text-faint-foreground"> / </span><span>{incident?.title ?? '…'}</span></nav>
      {failed ? (
        <EmptyState title="This incident could not be read">{because(failed, 'The server did not answer.')}</EmptyState>
      ) : !incident ? (
        <Card className="px-4 py-3 text-control text-muted-foreground">Reading the incident…</Card>
      ) : (
        <Opened incident={incident} />
      )}
    </>
  )
}

function Opened({ incident }: { incident: Dossier }) {
  const change = useChangeIncident()
  const { members } = useMembers()
  const [renaming, setRenaming] = useState<string | null>(null)
  const send = (asked: IncidentChange, done?: () => void) =>
    change.mutate({ id: incident.id, change: asked }, { onSuccess: () => done?.(), onError: (error) => toast.error(because(error, 'The incident was not changed')) })

  return (
    <>
      <div className="flex flex-wrap items-center gap-3">
        {renaming === null ? (
          <>
            <h1 className="text-title">{incident.title}</h1>
            <Button variant="ghost" size="icon-sm" aria-label="Rename the incident" onClick={() => setRenaming(incident.title)}><Pencil /></Button>
          </>
        ) : (
          <form className="flex flex-1 flex-wrap items-center gap-2" onSubmit={(e) => { e.preventDefault(); send({ title: renaming }, () => setRenaming(null)) }}>
            <Input aria-label="Title" value={renaming} onChange={(e) => setRenaming(e.target.value)} className="min-w-[260px] flex-1" autoFocus />
            <Button type="submit" size="sm" variant="primary" disabled={change.isPending || !renaming.trim()}>Save</Button>
            <Button type="button" size="sm" onClick={() => setRenaming(null)}>Cancel</Button>
          </form>
        )}
        <SeverityPill severity={incident.severity} />
        <StatusPill status={incident.status} />
        {incident.public ? <Badge variant="outline">Public</Badge> : null}
      </div>
      <div className="flex flex-wrap gap-x-4 gap-y-1 text-control text-muted-foreground">
        <span>Opened <span className="font-mono text-foreground">{stamp(incident.opened_ms)}</span></span>
        {incident.resolved_ms !== null ? <span>Resolved <span className="font-mono text-foreground">{stamp(incident.resolved_ms)}</span></span> : null}
        <span>Owner <span className="text-foreground">{personOf(incident.owner)}</span></span>
        {incident.alert ? <span className="inline-flex items-center gap-1"><BellRing className="size-3.5" aria-hidden />From the alert <Link to="/alerts" search={{ rule: incident.alert.id }} className="font-semibold">{incident.alert.name}</Link></span> : null}
      </div>
      <Card className="flex flex-wrap items-end gap-3 px-4 py-3" aria-label="Incident controls">
        <label className="flex flex-col gap-1 text-caption font-semibold text-muted-foreground">
          Status
          <NativeSelect value={incident.status} disabled={change.isPending} onChange={(e) => send({ status: e.target.value as IncidentStatus })} className="w-[160px]">
            {STATUSES.map((one) => <option key={one} value={one}>{one}</option>)}
          </NativeSelect>
        </label>
        <label className="flex flex-col gap-1 text-caption font-semibold text-muted-foreground">
          Severity
          <NativeSelect value={incident.severity} disabled={change.isPending} onChange={(e) => send({ severity: e.target.value as IncidentSeverity })} className="w-[110px]">
            {SEVERITIES.map((one) => <option key={one} value={one}>{one.toUpperCase()}</option>)}
          </NativeSelect>
        </label>
        <label className="flex flex-col gap-1 text-caption font-semibold text-muted-foreground">
          Owner
          <NativeSelect value={incident.owner?.id ?? ''} disabled={change.isPending || !members} onChange={(e) => send({ owner: e.target.value || null })} className="w-[220px]">
            <option value="">Nobody</option>
            {incident.owner && !members?.some((one) => one.account === incident.owner?.id) ? <option value={incident.owner.id}>{personOf(incident.owner)}</option> : null}
            {(members ?? []).map((one) => <option key={one.account} value={one.account}>{one.name ?? one.email}</option>)}
          </NativeSelect>
        </label>
        <Switch checked={incident.public} disabled={change.isPending} onCheckedChange={(shared) => send({ public: shared })} label="Public" />
        {incident.status !== 'resolved' ? (
          <Button size="sm" className="ml-auto" disabled={change.isPending} onClick={() => send({ status: 'resolved' }, () => toast.success('Incident resolved'))}>Resolve</Button>
        ) : null}
      </Card>
      <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_300px]">
        <section aria-label="Timeline" className="flex min-w-0 flex-col gap-3">
          <h2 className="text-control font-semibold">Timeline</h2>
          <ol className="m-0 flex list-none flex-col gap-0 border-l pl-0">
            {incident.timeline.map((entry, index) => <Entry key={`${entry.at_ms}-${index}`} entry={entry} />)}
          </ol>
          <CommentBox id={incident.id} shown={incident.public} />
        </section>
        <section aria-label="Attachments" className="flex flex-col gap-2">
          <h2 className="text-control font-semibold">Attachments</h2>
          {incident.attachments.length === 0 ? (
            <span className="text-control text-muted-foreground">Nothing attached yet. On a trace, a logs search or a metric chart, use “Attach to incident”.</span>
          ) : (
            <ul className="m-0 flex list-none flex-col gap-1.5 p-0">
              {incident.attachments.map((one) => <li key={one.id}><AttachmentLink attachment={one} /></li>)}
            </ul>
          )}
        </section>
      </div>
      <PostmortemSection incident={incident} />
    </>
  )
}

function AttachmentLink({ attachment }: { attachment: Attachment }) {
  const Icon = KIND_ICON[attachment.kind]
  const link = attachmentLink(attachment)
  return (
    <Link to={link.to} search={link.search} className="inline-flex items-center gap-1.5 text-control">
      <Icon className="size-3.5 shrink-0" aria-hidden />
      <span className="font-semibold">{attachment.label}</span>
      <span className="text-caption text-muted-foreground">{KIND_WORD[attachment.kind]}</span>
    </Link>
  )
}

function CommentBox({ id, shown }: { id: string; shown: boolean }) {
  const comment = useCommentIncident()
  const [body, setBody] = useState('')
  const [outward, setOutward] = useState(false)
  const announced = outward && shown
  return (
    <form className="flex flex-col gap-2" onSubmit={(e) => {
      e.preventDefault()
      comment.mutate({ id, body, public: announced }, {
        onSuccess: () => {
          setBody('')
          setOutward(false)
          if (announced) toast.success('Posted on the status page', { description: 'Confirmed subscribers are emailed.' })
        },
        onError: (error) => toast.error(because(error, 'The comment was not posted')),
      })
    }}>
      <Textarea aria-label="Comment" placeholder={announced ? 'What your users notice and what you are doing. No trace ids or hostnames.' : 'What was seen, tried or decided'} value={body} onChange={(e) => setBody(e.target.value)} rows={3} />
      <div className="flex flex-wrap items-center gap-2">
        <Switch checked={announced} disabled={!shown} onCheckedChange={setOutward} label={shown ? 'Post publicly on the status page' : 'Post publicly on the status page (make the incident public first)'} />
        <span className="flex-1" />
        <Button type="submit" size="sm" variant="primary" disabled={comment.isPending || !body.trim()}><MessageSquare />{announced ? 'Post publicly' : 'Comment'}</Button>
      </div>
    </form>
  )
}

function Entry({ entry }: { entry: TimelineEntry }) {
  return (
    <li className="relative flex flex-col gap-1 py-2 pl-4 before:absolute before:-left-[5px] before:top-3.5 before:size-[9px] before:rounded-full before:border before:bg-card">
      <span className="flex flex-wrap items-center gap-1.5 text-caption text-muted-foreground">
        <span className="font-mono">{stamp(entry.at_ms)}</span>
        <span>·</span>
        {entry.author.kind === 'agent' && entry.author.on_behalf_of ? (
          <span>agent <span className="font-semibold text-foreground">{entry.author.label}</span> for <span className="font-semibold text-foreground">{entry.author.on_behalf_of}</span></span>
        ) : (
          <>
            <span className={entry.author.kind === 'system' ? 'font-semibold' : 'font-semibold text-foreground'}>{entry.author.label}</span>
            {entry.author.kind === 'agent' ? <Badge variant="outline" size="xs">agent</Badge> : null}
          </>
        )}
      </span>
      <div className="text-control"><Told entry={entry} /></div>
    </li>
  )
}

function Told({ entry }: { entry: TimelineEntry }) {
  switch (entry.kind) {
    case 'opened':
      return <span className="inline-flex flex-wrap items-center gap-1.5">Declared the incident at <SeverityPill severity={entry.detail.severity} />{entry.detail.alert ? <> from the alert <span className="font-semibold">{entry.detail.alert.name ?? 'a deleted rule'}</span></> : null}</span>
    case 'status':
      return <span className="inline-flex flex-wrap items-center gap-1.5">Moved from <StatusPill status={entry.detail.from} /> to <StatusPill status={entry.detail.to} /></span>
    case 'severity':
      return <span className="inline-flex flex-wrap items-center gap-1.5">Changed the severity from <SeverityPill severity={entry.detail.from} /> to <SeverityPill severity={entry.detail.to} /></span>
    case 'owner':
      return <span>{entry.detail.to ? <>Handed the incident to <span className="font-semibold">{personOf(entry.detail.to)}</span></> : <>Left the incident without an owner</>}</span>
    case 'title':
      return <span>Renamed the incident to <span className="font-semibold">{entry.detail.to}</span></span>
    case 'public':
      return <span>{entry.detail.to ? 'Made the incident public' : 'Made the incident private'}</span>
    case 'comment':
      return (
        <div className="flex flex-col gap-1">
          {entry.detail.public ? <Badge variant="accent" size="xs" className="self-start">On the status page</Badge> : null}
          <Alert className="whitespace-pre-wrap">{entry.detail.body}</Alert>
        </div>
      )
    case 'alert':
      return (
        <span className="inline-flex flex-wrap items-center gap-1.5">
          <BellRing className="size-3.5" aria-hidden />The alert <span className="font-semibold">{entry.detail.alert.name ?? 'a deleted rule'}</span> went from {entry.detail.from.replace('_', ' ')} to <StateBadge status={entry.detail.to} />
          {entry.detail.value !== null ? <span className="font-mono text-caption text-muted-foreground">value {entry.detail.value.toLocaleString('en-US', { maximumFractionDigits: 6 })}</span> : null}
          {Object.keys(entry.detail.labels).length ? <span className="font-mono text-caption text-muted-foreground">{labelOf(entry.detail.labels)}</span> : null}
          {entry.detail.context ? <span className="basis-full"><AlertContext context={entry.detail.context} /></span> : null}
        </span>
      )
    case 'attachment':
      return <span className="inline-flex flex-wrap items-center gap-1.5"><Paperclip className="size-3.5" aria-hidden />Attached <AttachmentLink attachment={entry.detail} /></span>
    case 'paged':
      return (
        <span className="inline-flex flex-wrap items-center gap-1.5">
          <BellRing className="size-3.5" aria-hidden />
          {entry.detail.target
            ? <>{entry.detail.step === 0 ? 'Paged' : 'Escalated the page to'} <span className="font-semibold">{personOf(entry.detail.target)}</span></>
            : <>Escalated the page to every administrator</>}
          {entry.detail.step > 0 ? <span className="font-mono text-caption text-muted-foreground">step {entry.detail.step}</span> : null}
        </span>
      )
    case 'acknowledged':
      return <span>Acknowledged the page, which stops its escalation</span>
    case 'deploy':
      return (
        <span className="inline-flex flex-wrap items-center gap-1.5">
          <Rocket className="size-3.5" aria-hidden />Deployed <span className="font-semibold">{entry.detail.service}</span>
          <Link to={releasePath(entry.detail.service, entry.detail.version)} className="font-mono">{entry.detail.version}</Link>
          {entry.detail.environment ? <span className="text-caption text-muted-foreground">to {entry.detail.environment}</span> : null}
        </span>
      )
  }
}
