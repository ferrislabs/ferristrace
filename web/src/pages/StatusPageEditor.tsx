import { ArrowDown, ArrowUp, ExternalLink, Eye, EyeOff, Plus, RadioTower, Trash2 } from 'lucide-react'
import { useEffect, useState } from 'react'
import { toast } from 'sonner'
import { because } from '@/auth/client'
import { roleRank } from '@/auth/api'
import { useOrganisations } from '@/auth/session'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardHeader, CardTitle } from '@/components/ui/card'
import { Dialog, DialogContent, DialogFooter } from '@/components/ui/dialog'
import { Field, Input, NativeSelect, Textarea } from '@/components/ui/input'
import { Segmented } from '@/components/ui/segmented'
import { Switch } from '@/components/ui/switch'
import { EmptyState } from '@/components/ft/bits'
import { StatusPageView } from '@/components/ft/StatusPageView'
import { useAlerts } from '@/data/alerts'
import { useProjects } from '@/data/projects'
import { useServiceNames } from '@/data/services'
import {
  ACCENTS, COMPONENT_STATUSES, MODES, STATUS_WORDS, slugProblem, useAddComponent, useChangeComponent, useChangeStatusPage, useCreateStatusPage,
  useDeleteStatusPage, useOrderComponents, useRemoveComponent, useStatusPage, useStatusPages, useStatusPreview,
  type Accent, type ComponentLink, type ComponentStatus, type Mode, type StatusSheet,
} from '@/data/status-page'
import { Link } from '@/lib/router'
import { cn } from '@/lib/utils'

const ACCENT_HEX: Record<Accent, string> = { indigo: '#4f46e5', teal: '#0f766e', orange: '#c2410c', rose: '#be123c', slate: '#334155' }
const DAY_MS = 86_400_000

function useIsAdmin() {
  const { active } = useOrganisations()
  return active ? roleRank[active.role] <= roleRank.admin : false
}

export function StatusPageEditor() {
  const { pages, failed } = useStatusPages()
  const admin = useIsAdmin()
  const [chosen, setChosen] = useState<string | null>(null)
  const [creating, setCreating] = useState(false)
  const selected = chosen && pages?.some((page) => page.id === chosen) ? chosen : (pages?.[0]?.id ?? null)
  return (
    <>
      <div className="flex flex-wrap items-center gap-3">
        <h1 className="text-title">Status pages</h1>
        <span className="flex-1" />
        {pages && pages.length > 1 ? (
          <NativeSelect aria-label="Status page" value={selected ?? ''} onChange={(event) => setChosen(event.target.value)} className="w-[220px]">
            {pages.map((page) => <option key={page.id} value={page.id}>{page.name} · /status/{page.slug}</option>)}
          </NativeSelect>
        ) : null}
        {admin ? <Button variant="primary" size="sm" onClick={() => setCreating(true)}><Plus />New status page</Button> : null}
      </div>
      {failed ? <span className="text-control text-destructive-text">{because(failed, 'The status pages could not be loaded.')}</span> : null}
      {pages && pages.length === 0 ? (
        <EmptyState icon={<RadioTower />} title="No status page yet">
          A status page shows your users the state of chosen components and the incidents you mark public, at an address that needs no account.
        </EmptyState>
      ) : null}
      {selected ? <Editing key={selected} id={selected} admin={admin} /> : null}
      <CreateDialog open={creating} onOpenChange={setCreating} onCreated={(id) => setChosen(id)} />
    </>
  )
}

function Editing({ id, admin }: { id: string; admin: boolean }) {
  const { sheet, failed } = useStatusPage(id)
  if (failed) return <span className="text-control text-destructive-text">{because(failed, 'This status page could not be loaded.')}</span>
  if (!sheet) return null
  return (
    <div className="flex flex-wrap items-start gap-6">
      <div className="flex min-w-0 flex-[1_1_480px] flex-col gap-4">
        <Settings sheet={sheet} admin={admin} />
        <Components sheet={sheet} admin={admin} />
      </div>
      <Preview sheet={sheet} />
    </div>
  )
}

function Settings({ sheet, admin }: { sheet: StatusSheet; admin: boolean }) {
  const change = useChangeStatusPage()
  const remove = useDeleteStatusPage()
  const [name, setName] = useState(sheet.name)
  const [slug, setSlug] = useState(sheet.slug)
  const [intro, setIntro] = useState(sheet.intro ?? '')
  useEffect(() => {
    setName(sheet.name)
    setSlug(sheet.slug)
    setIntro(sheet.intro ?? '')
  }, [sheet.name, sheet.slug, sheet.intro])
  const send = (patch: Parameters<typeof change.mutate>[0]['change'], done?: string) =>
    change.mutate({ page: sheet.id, change: patch }, {
      onSuccess: () => done && toast.success(done),
      onError: (error) => toast.error(because(error, 'The status page was not changed')),
    })
  const problem = slugProblem(slug)
  const edited = name !== sheet.name || slug !== sheet.slug || intro !== (sheet.intro ?? '')
  return (
    <Card className="flex flex-col gap-4 p-4">
      <div className="flex flex-wrap items-center gap-2">
        <h2 className="text-sub">Page</h2>
        {sheet.public ? <Badge variant="success">Public</Badge> : <Badge variant="outline">Private</Badge>}
        <span className="text-caption text-muted-foreground">{sheet.subscribers} confirmed subscriber{sheet.subscribers === 1 ? '' : 's'}</span>
        <span className="flex-1" />
        {sheet.public ? <Link to={`/status/${sheet.slug}`} target="_blank" className="inline-flex items-center gap-1 font-mono text-control">/status/{sheet.slug}<ExternalLink className="size-3" aria-hidden /></Link> : null}
      </div>
      <form className="flex flex-col gap-4" onSubmit={(event) => {
        event.preventDefault()
        send({ name: name.trim(), slug, intro: intro.trim() ? intro : null }, 'Status page saved')
      }}>
        <Field label="Name" htmlFor="sp-name"><Input id="sp-name" value={name} disabled={!admin} onChange={(event) => setName(event.target.value)} /></Field>
        <Field label="Address" htmlFor="sp-slug" hint={problem ?? `Served at /status/${slug}`}><Input id="sp-slug" mono value={slug} disabled={!admin} onChange={(event) => setSlug(event.target.value)} /></Field>
        <Field label="Introduction" htmlFor="sp-intro" hint="Optional. Shown above the components."><Textarea id="sp-intro" rows={2} value={intro} disabled={!admin} onChange={(event) => setIntro(event.target.value)} /></Field>
        {admin ? <Button type="submit" size="sm" variant="primary" className="self-start" disabled={!edited || Boolean(problem) || !name.trim() || change.isPending}>Save</Button> : null}
      </form>
      <fieldset className="m-0 flex flex-col gap-1 border-0 p-0" disabled={!admin}>
        <Switch checked={sheet.public} onCheckedChange={(on) => send({ public: on }, on ? 'The status page is public' : 'The status page is private')} label="Public: anyone with the address sees it, without signing in" />
        <Switch checked={sheet.hide_from_search} onCheckedChange={(on) => send({ hide_from_search: on })} label="Ask search engines not to index it" />
      </fieldset>
      <Field label="Accent colour">
        <div role="radiogroup" aria-label="Accent colour" className="flex items-center gap-2.5">
          {ACCENTS.map((accent) => (
            <button key={accent} type="button" role="radio" aria-checked={sheet.theme.accent === accent} aria-label={accent} disabled={!admin} onClick={() => send({ theme: { ...sheet.theme, accent } })} className={cn('size-7 cursor-pointer rounded-full border-2 border-background ring-1 ring-border', sheet.theme.accent === accent && 'ring-2 ring-ring')} style={{ background: ACCENT_HEX[accent] }} />
          ))}
        </div>
      </Field>
      <Field label="Theme"><div><Segmented<Mode> label="Theme" value={sheet.theme.mode} onChange={(mode) => admin && send({ theme: { ...sheet.theme, mode } })} options={MODES.map((mode) => ({ value: mode, label: mode === 'auto' ? 'Follow the visitor' : mode === 'light' ? 'Light' : 'Dark' }))} /></div></Field>
      {admin ? (
        <Button variant="ghost" size="sm" className="self-start text-destructive-text" disabled={remove.isPending} onClick={() => {
          if (!window.confirm(`Delete the status page ${sheet.name}? Its subscribers are forgotten.`)) return
          remove.mutate(sheet.id, { onSuccess: () => toast.success('Status page deleted'), onError: (error) => toast.error(because(error, 'The status page was not deleted')) })
        }}><Trash2 />Delete this status page</Button>
      ) : null}
    </Card>
  )
}

function linkWords(link: ComponentLink, alerts: Map<string, string>, projects: Map<string, string>) {
  if (link.kind === 'manual') return 'Set by hand'
  if (link.kind === 'alert') return `Alert · ${alerts.get(link.alert) ?? 'a deleted rule'}`
  return `Service · ${link.service}${link.project ? ` in ${projects.get(link.project) ?? 'a deleted project'}` : ''} · error rate ≥ 5 % degraded, ≥ 25 % down`
}

function Components({ sheet, admin }: { sheet: StatusSheet; admin: boolean }) {
  const { alerts } = useAlerts()
  const projects = useProjects()
  const alertNames = new Map((alerts ?? []).map((rule) => [rule.id, rule.name]))
  const projectNames = new Map(projects.map((project) => [project.id, project.name]))
  const order = useOrderComponents()
  const change = useChangeComponent()
  const remove = useRemoveComponent()
  const [adding, setAdding] = useState(false)
  const failed = (what: string) => (error: unknown) => toast.error(because(error, what))
  const move = (index: number, by: number) => {
    const ids = sheet.components.map((component) => component.id)
    const [moved] = ids.splice(index, 1)
    ids.splice(index + by, 0, moved)
    order.mutate({ page: sheet.id, ids }, { onError: failed('The order was not saved') })
  }
  return (
    <Card className="overflow-hidden">
      <CardHeader><CardTitle>Components</CardTitle><span className="flex-1" />{admin ? <Button size="sm" onClick={() => setAdding(true)}><Plus />Add component</Button> : null}</CardHeader>
      {sheet.components.length === 0 ? <div className="px-4 pb-4 text-control text-muted-foreground">No component yet. A component follows an alert, a service's error rate, or a status you set.</div> : null}
      {sheet.components.map((component, index) => (
        <div key={component.id} className={cn('flex items-center gap-2 border-t border-border-subtle px-4 py-2.5', !component.public && 'opacity-60')}>
          <div className="flex min-w-0 flex-1 flex-col gap-0.5">
            <span className="inline-flex items-center gap-1.5 text-control font-semibold">{component.name}{component.public ? null : <Badge variant="outline" size="xs">Hidden</Badge>}</span>
            <span className="truncate font-mono text-[11.5px] text-muted-foreground">{linkWords(component.link, alertNames, projectNames)}</span>
          </div>
          {component.link.kind === 'manual' && admin ? (
            <NativeSelect aria-label={`Status of ${component.name}`} className="w-[130px]" value={component.manual_status ?? 'operational'} onChange={(event) => change.mutate({ page: sheet.id, id: component.id, change: { manual_status: event.target.value as ComponentStatus } }, { onError: failed('The status was not saved') })}>
              {COMPONENT_STATUSES.map((status) => <option key={status} value={status}>{STATUS_WORDS[status]}</option>)}
            </NativeSelect>
          ) : null}
          {admin ? (
            <>
              <Button variant="ghost" size="icon-sm" aria-label={`Move ${component.name} up`} disabled={index === 0 || order.isPending} onClick={() => move(index, -1)}><ArrowUp /></Button>
              <Button variant="ghost" size="icon-sm" aria-label={`Move ${component.name} down`} disabled={index === sheet.components.length - 1 || order.isPending} onClick={() => move(index, 1)}><ArrowDown /></Button>
              <Button variant="ghost" size="icon-sm" aria-label={`${component.public ? 'Hide' : 'Show'} ${component.name}`} onClick={() => change.mutate({ page: sheet.id, id: component.id, change: { public: !component.public } }, { onError: failed('The component was not changed') })}>{component.public ? <Eye /> : <EyeOff />}</Button>
              <Button variant="ghost" size="icon-sm" aria-label={`Remove ${component.name}`} onClick={() => remove.mutate({ page: sheet.id, id: component.id }, { onError: failed('The component was not removed') })}><Trash2 /></Button>
            </>
          ) : null}
        </div>
      ))}
      <AddComponentDialog open={adding} onOpenChange={setAdding} page={sheet.id} />
    </Card>
  )
}

function AddComponentDialog({ open, onOpenChange, page }: { open: boolean; onOpenChange: (open: boolean) => void; page: string }) {
  const add = useAddComponent()
  const { alerts } = useAlerts()
  const projects = useProjects()
  const { names } = useServiceNames(DAY_MS)
  const [name, setName] = useState('')
  const [kind, setKind] = useState<ComponentLink['kind']>('service')
  const [service, setService] = useState('')
  const [project, setProject] = useState('')
  const [alert, setAlert] = useState('')
  const link: ComponentLink | null =
    kind === 'manual' ? { kind } : kind === 'alert' ? (alert ? { kind, alert } : null) : service.trim() ? { kind, service: service.trim(), project: project || null } : null
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent size="md" title="Add component" description="What your users see, and what decides its status.">
        <form className="flex flex-col gap-4" onSubmit={(event) => {
          event.preventDefault()
          if (!link) return
          add.mutate({ page, component: { name: name.trim(), link } }, {
            onSuccess: () => {
              setName('')
              setService('')
              setAlert('')
              onOpenChange(false)
            },
            onError: (error) => toast.error(because(error, 'The component was not added')),
          })
        }}>
          <Field label="Name" htmlFor="sc-name"><Input id="sc-name" value={name} onChange={(event) => setName(event.target.value)} placeholder="Checkout" /></Field>
          <Field label="Status from"><div><Segmented<ComponentLink['kind']> label="Status from" value={kind} onChange={setKind} options={[{ value: 'service', label: 'A service' }, { value: 'alert', label: 'An alert' }, { value: 'manual', label: 'Set by hand' }]} /></div></Field>
          {kind === 'service' ? (
            <>
              <Field label="Service" htmlFor="sc-service" hint="Degraded at 5 % errors over the last five minutes, down at 25 %.">
                <Input id="sc-service" mono list="sc-services" value={service} onChange={(event) => setService(event.target.value)} placeholder="checkout-api" />
              </Field>
              <datalist id="sc-services">{names.map((one) => <option key={one} value={one} />)}</datalist>
              <Field label="Project" htmlFor="sc-project">
                <NativeSelect id="sc-project" value={project} onChange={(event) => setProject(event.target.value)}>
                  <option value="">Every project</option>
                  {projects.map((one) => <option key={one.id} value={one.id}>{one.name}</option>)}
                </NativeSelect>
              </Field>
            </>
          ) : null}
          {kind === 'alert' ? (
            <Field label="Alert" htmlFor="sc-alert" hint="Down while a critical rule fires, degraded while a warning or info rule fires.">
              <NativeSelect id="sc-alert" value={alert} onChange={(event) => setAlert(event.target.value)}>
                <option value="">Choose an alert rule</option>
                {(alerts ?? []).map((rule) => <option key={rule.id} value={rule.id}>{rule.name}</option>)}
              </NativeSelect>
            </Field>
          ) : null}
          <DialogFooter><Button type="button" onClick={() => onOpenChange(false)}>Cancel</Button><Button type="submit" variant="primary" disabled={!name.trim() || !link || add.isPending}>Add</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

function CreateDialog({ open, onOpenChange, onCreated }: { open: boolean; onOpenChange: (open: boolean) => void; onCreated: (id: string) => void }) {
  const create = useCreateStatusPage()
  const [name, setName] = useState('')
  const [slug, setSlug] = useState('')
  const problem = slug ? slugProblem(slug) : null
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent title="New status page" description="It stays private until you make it public.">
        <form className="flex flex-col gap-4" onSubmit={(event) => {
          event.preventDefault()
          create.mutate({ name: name.trim(), slug }, {
            onSuccess: (sheet) => {
              onCreated(sheet.id)
              setName('')
              setSlug('')
              onOpenChange(false)
            },
            onError: (error) => toast.error(because(error, 'The status page was not created')),
          })
        }}>
          <Field label="Name" htmlFor="new-sp-name"><Input id="new-sp-name" value={name} onChange={(event) => {
            setName(event.target.value)
            if (!slug || slug === slugOf(name)) setSlug(slugOf(event.target.value))
          }} placeholder="Acme" /></Field>
          <Field label="Address" htmlFor="new-sp-slug" hint={problem ?? (slug ? `Served at /status/${slug}` : 'Lowercase letters, digits and hyphens.')}><Input id="new-sp-slug" mono value={slug} onChange={(event) => setSlug(event.target.value)} /></Field>
          <DialogFooter><Button type="button" onClick={() => onOpenChange(false)}>Cancel</Button><Button type="submit" variant="primary" disabled={!name.trim() || !slug || Boolean(problem) || create.isPending}>Create</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

const slugOf = (name: string) => name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 48)

function Preview({ sheet }: { sheet: StatusSheet }) {
  const { preview, failed } = useStatusPreview(sheet.id)
  const [dark, setDark] = useState(sheet.theme.mode === 'dark')
  return (
    <div className="flex w-[560px] max-w-full shrink-0 flex-col gap-2">
      <div className="flex items-center gap-2"><span className="flex-1 text-control font-semibold">Preview · what visitors see now</span><Segmented size="sm" label="Preview theme" value={dark ? 'dark' : 'light'} onChange={(v) => setDark(v === 'dark')} options={[{ value: 'light', label: 'Light' }, { value: 'dark', label: 'Dark' }]} /></div>
      <div className="overflow-hidden rounded-lg border shadow-sm">
        <div className="flex h-8 items-center gap-2 border-b bg-sidebar px-2.5"><span className="flex gap-1">{[0, 1, 2].map((i) => <span key={i} className="size-2 rounded-full bg-border" />)}</span><span className="flex h-5 flex-1 items-center rounded bg-muted px-2 font-mono text-[11px] text-muted-foreground">/status/{sheet.slug}</span></div>
        <div className={cn('h-[640px] overflow-y-auto', dark ? 'dark' : 'light')}>
          {preview ? <div className="origin-top-left scale-[0.62]" style={{ width: '161%' }}><StatusPageView view={preview} compact /></div> : <div className="p-4 text-control text-muted-foreground">{failed ? because(failed, 'The preview could not be loaded.') : 'Loading the preview…'}</div>}
        </div>
      </div>
    </div>
  )
}
