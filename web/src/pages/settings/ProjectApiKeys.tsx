import { AlertTriangle, Copy, KeyRound } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { because } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Dialog, DialogContent, DialogFooter } from '@/components/ui/dialog'
import { Field, Input } from '@/components/ui/input'
import { Alert, Table, Td, Th, Tr } from '@/components/ui/misc'
import { KEY_SCOPES, keyNameProblem, useIssueKey, useKeys, useRevokeKey, type ApiKey, type IssuedKey, type KeyScope } from '@/data/keys'
import { useCatalogue } from '@/data/projects'
import { stamp } from '@/lib/format'
import { Link } from '@/lib/router'
import { useScope } from '@/lib/scope'
import { H } from './parts'

const SCOPE_HINT: Record<KeyScope, string> = {
  ingest: 'Send OTLP traces to /v1/traces.',
  read: 'Read services, traces and issues through /api.',
}

export function ProjectApiKeys() {
  const { active } = useOrganisations()
  const { projects, nothing: noProjects } = useCatalogue()
  const { keys, nothing } = useKeys()
  const { project: chosen } = useScope()
  const issue = useIssueKey()
  const revoke = useRevokeKey()
  const [asking, setAsking] = useState(false)
  const [name, setName] = useState('')
  const [scopes, setScopes] = useState<KeyScope[]>(['ingest'])
  const [issued, setIssued] = useState<IssuedKey | null>(null)
  const [revoking, setRevoking] = useState<ApiKey | null>(null)

  const catalogue = projects ?? []
  const project = catalogue.find((one) => one.id === chosen) ?? catalogue[0]
  const mayChange = active?.role === 'owner' || active?.role === 'admin'

  if (!project) {
    return (
      <>
        <H title="Api keys" sub="Keys that send or read this project's telemetry." />
        <Card className="px-4 py-3 text-control text-muted-foreground">
          {projects ? <>This organisation holds no project yet. <Link to="/settings/projects">Make one</Link> and come back.</> : noProjects}
        </Card>
      </>
    )
  }

  const held = (keys ?? []).filter((one) => one.project === project.id || one.project === null)
  const problem = name.length === 0 ? null : keyNameProblem(name)
  const close = () => {
    setAsking(false)
    setIssued(null)
    setName('')
    setScopes(['ingest'])
  }
  const toggle = (scope: KeyScope) => setScopes((now) => (now.includes(scope) ? now.filter((one) => one !== scope) : KEY_SCOPES.filter((one) => one === scope || now.includes(one))))

  return (
    <>
      <H title="Api keys" sub={`Keys for ${project.name}, and keys bound to no project that reach every one. Each key is shown once, then only its prefix.`}>
        {mayChange ? <Button variant="primary" size="sm" onClick={() => setAsking(true)}><KeyRound />Create key</Button> : null}
      </H>
      {mayChange ? null : <span className="text-control text-muted-foreground">Only an owner or an administrator can create or revoke a key.</span>}
      <Card className="overflow-x-auto">
        {keys ? (
          <Table>
            <thead><tr><Th>Name</Th><Th className="w-[200px]">Key</Th><Th className="w-[140px]">Scopes</Th><Th className="w-[170px]">Created</Th><Th className="w-[170px]">Last used</Th>{mayChange ? <Th className="w-[100px]"><span className="sr-only">Actions</span></Th> : null}</tr></thead>
            <tbody>
              {held.length === 0 ? <Tr><Td colSpan={mayChange ? 6 : 5} className="text-muted-foreground">No key authenticates for this project yet.</Td></Tr> : null}
              {held.map((k) => (
                <Tr key={k.id}>
                  <Td><span className="flex flex-wrap items-center gap-2"><span className={k.name ? 'font-semibold' : 'text-muted-foreground'}>{k.name || 'unnamed'}</span>{k.project === null ? <Badge variant="accent">All projects</Badge> : null}</span></Td>
                  <Td className="font-mono">{k.prefix}<span className="text-faint-foreground">••••••</span></Td>
                  <Td><span className="flex gap-1">{k.scopes.map((s) => <Badge key={s} variant="outline">{s}</Badge>)}</span></Td>
                  <Td className="font-mono">{stamp(k.created_at_ms)}</Td>
                  <Td className={k.last_used_at_ms === null ? 'font-mono text-muted-foreground' : 'font-mono'}>{k.last_used_at_ms === null ? 'never' : stamp(k.last_used_at_ms)}</Td>
                  {mayChange ? <Td numeric><Button variant="ghost" size="sm" className="text-destructive-text" onClick={() => setRevoking(k)}>Revoke</Button></Td> : null}
                </Tr>
              ))}
            </tbody>
          </Table>
        ) : <div className="px-4 py-3 text-control text-muted-foreground">{nothing}</div>}
      </Card>
      <Dialog open={asking} onOpenChange={(o) => (o ? setAsking(true) : close())}>
        <DialogContent size={issued ? 'md' : 'sm'} title={issued ? `Key for ${project.name}` : 'Create key'} description={issued ? issued.key.name : 'The key is shown once after this step.'}>
          {issued ? (
            <>
              <Alert variant="warning" icon={<AlertTriangle />}>Copy this key now. It is shown once and cannot be retrieved later.</Alert>
              <div className="flex gap-2"><Input readOnly mono value={issued.secret} className="bg-muted text-[12px]" aria-label="New api key" /><Button onClick={() => { void navigator.clipboard?.writeText(issued.secret); toast.success('Key copied') }}><Copy />Copy</Button></div>
              {issued.key.scopes.includes('ingest') ? <pre className="m-0 overflow-x-auto rounded-md border bg-muted px-3 py-2.5 font-mono text-[12px]">export OTEL_EXPORTER_OTLP_HEADERS="authorization=Bearer {issued.key.prefix}…"</pre> : null}
              <DialogFooter><Button variant="primary" onClick={close}>Done</Button></DialogFooter>
            </>
          ) : (
            <form
              className="flex flex-col gap-4"
              onSubmit={(e) => {
                e.preventDefault()
                issue.mutate(
                  { name, scopes, project: project.id },
                  { onSuccess: setIssued, onError: (error) => toast.error(because(error, 'The key was not created')) },
                )
              }}
            >
              <Field label="Name" htmlFor="ak-n" hint="Where the key will live, so you know what breaks when you revoke it."><Input id="ak-n" value={name} aria-invalid={problem ? true : undefined} onChange={(e) => setName(e.target.value)} autoFocus /></Field>
              {problem ? <span className="text-caption text-destructive-text">{problem}</span> : null}
              <fieldset className="flex flex-col gap-2">
                <legend className="mb-1 text-control font-semibold">Scopes</legend>
                {KEY_SCOPES.map((scope) => (
                  <label key={scope} className="flex items-center gap-2 text-control">
                    <input type="checkbox" className="size-4 accent-[var(--primary)]" checked={scopes.includes(scope)} onChange={() => toggle(scope)} />
                    <span className="font-mono">{scope}</span><span className="text-muted-foreground">{SCOPE_HINT[scope]}</span>
                  </label>
                ))}
              </fieldset>
              <DialogFooter><Button type="button" onClick={close}>Cancel</Button><Button variant="primary" type="submit" disabled={issue.isPending || scopes.length === 0 || keyNameProblem(name) !== null}>Create key</Button></DialogFooter>
            </form>
          )}
        </DialogContent>
      </Dialog>
      <Dialog open={revoking !== null} onOpenChange={(o) => !o && setRevoking(null)}>
        <DialogContent role="alertdialog" title={`Revoke ${revoking?.name || revoking?.prefix || 'this key'}?`} description="Anything sending or reading with this key is refused from now on. This cannot be undone.">
          <DialogFooter>
            <Button onClick={() => setRevoking(null)}>Cancel</Button>
            <Button
              variant="destructive"
              disabled={revoke.isPending}
              onClick={() => {
                if (!revoking) return
                revoke.mutate(revoking.id, {
                  onSuccess: () => { setRevoking(null); toast.success('Key revoked') },
                  onError: (error) => toast.error(because(error, 'The key was not revoked')),
                })
              }}
            >Revoke key</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}
