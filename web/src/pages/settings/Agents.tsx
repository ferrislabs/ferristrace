import { Bot } from 'lucide-react'
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
import { AGENT_SCOPES, agentNameProblem, clientIdProblem, useAgents, useRegisterAgent, useRevokeAgent, type Agent, type AgentScope } from '@/data/agents'
import { useProjects } from '@/data/projects'
import { stamp } from '@/lib/format'
import { H } from './parts'

export const SCOPE_HINT: Record<AgentScope, string> = {
  'telemetry:read': 'Read services, traces, logs, metrics, issues and incidents.',
  'incidents:comment': 'Comment on an incident.',
  'incidents:resolve': 'Mark an incident resolved.',
  'oncall:acknowledge': 'Acknowledge a page.',
}

export function Agents() {
  const { active } = useOrganisations()
  const mayChange = active?.role === 'owner' || active?.role === 'admin'
  const { roll, nothing } = useAgents(mayChange)
  const projects = useProjects()
  const register = useRegisterAgent()
  const revoke = useRevokeAgent()
  const [asking, setAsking] = useState(false)
  const [name, setName] = useState('')
  const [clientId, setClientId] = useState('')
  const [scopes, setScopes] = useState<AgentScope[]>(['telemetry:read'])
  const [project, setProject] = useState('')
  const [revoking, setRevoking] = useState<Agent | null>(null)

  if (!mayChange) {
    return (
      <>
        <H title="Agents" sub="AI agents that sign in as themselves and act within the scopes an administrator ticked." />
        <Card className="px-4 py-3 text-control text-muted-foreground">Only an owner or an administrator can see or change the agents.</Card>
      </>
    )
  }

  const problem = name.length === 0 ? null : agentNameProblem(name)
  const clientProblem = clientId.length === 0 ? null : clientIdProblem(clientId)
  const close = () => {
    setAsking(false)
    setName('')
    setClientId('')
    setScopes(['telemetry:read'])
    setProject('')
  }
  const toggle = (scope: AgentScope) => setScopes((now) => (now.includes(scope) ? now.filter((one) => one !== scope) : AGENT_SCOPES.filter((one) => one === scope || now.includes(one))))
  const projectName = (id: string | null) => (id === null ? 'All projects' : (projects.find((one) => one.id === id)?.name ?? id))
  const enabled = roll?.enabled ?? false

  return (
    <>
      <H title="Agents" sub="AI agents that sign in as themselves through FerrisKey. Each read and write is audited with the token that allowed it.">
        {enabled ? <Button variant="primary" size="sm" onClick={() => setAsking(true)}><Bot />Register agent</Button> : null}
      </H>
      {roll && !enabled ? (
        <Alert variant="warning">Agents are off on this server: {roll.reason ?? 'no agent issuer is configured'}. Fix the configuration and restart, then register the FerrisKey client here.</Alert>
      ) : null}
      <Card className="overflow-x-auto">
        {roll ? (
          <Table>
            <thead><tr><Th>Name</Th><Th className="w-[160px]">Client id</Th><Th className="w-[200px]">Scopes</Th><Th className="w-[120px]">Project</Th><Th className="w-[170px]">Last used</Th><Th className="w-[100px]">State</Th><Th className="w-[100px]"><span className="sr-only">Actions</span></Th></tr></thead>
            <tbody>
              {roll.agents.length === 0 ? <Tr><Td colSpan={7} className="text-muted-foreground">No agent is registered yet.</Td></Tr> : null}
              {roll.agents.map((agent) => (
                <Tr key={agent.id} className={agent.revoked_at_ms === null ? undefined : 'text-muted-foreground'}>
                  <Td><span className="font-semibold">{agent.name}</span></Td>
                  <Td className="font-mono">{agent.client_id}</Td>
                  <Td><span className="flex flex-wrap gap-1">{agent.scopes.map((scope) => <Badge key={scope} variant="outline">{scope}</Badge>)}</span></Td>
                  <Td>{projectName(agent.project)}</Td>
                  <Td className={agent.last_used_at_ms === null ? 'font-mono text-muted-foreground' : 'font-mono'}>{agent.last_used_at_ms === null ? 'never' : stamp(agent.last_used_at_ms)}</Td>
                  <Td>{agent.revoked_at_ms === null ? <Badge variant="accent">Active</Badge> : <Badge variant="outline" title={stamp(agent.revoked_at_ms)}>Revoked</Badge>}</Td>
                  <Td numeric>{agent.revoked_at_ms === null ? <Button variant="ghost" size="sm" className="text-destructive-text" onClick={() => setRevoking(agent)}>Revoke</Button> : null}</Td>
                </Tr>
              ))}
            </tbody>
          </Table>
        ) : <div className="px-4 py-3 text-control text-muted-foreground">{nothing}</div>}
      </Card>
      <Dialog open={asking} onOpenChange={(o) => (o ? setAsking(true) : close())}>
        <DialogContent size="sm" title="Register agent" description="Create the client in FerrisKey first (client credentials), then paste its client id here.">
          <form
            className="flex flex-col gap-4"
            onSubmit={(e) => {
              e.preventDefault()
              register.mutate(
                { name, client_id: clientId, scopes, project: project || null },
                { onSuccess: () => { close(); toast.success('Agent registered') }, onError: (error) => toast.error(because(error, 'The agent was not registered')) },
              )
            }}
          >
            <Field label="Name" htmlFor="ag-n" hint="What this agent is, so the audit log reads well."><Input id="ag-n" value={name} aria-invalid={problem ? true : undefined} onChange={(e) => setName(e.target.value)} autoFocus /></Field>
            {problem ? <span className="text-caption text-destructive-text">{problem}</span> : null}
            <Field label="Client id" htmlFor="ag-c" hint="The client id of the FerrisKey client the agent signs in with."><Input id="ag-c" mono value={clientId} aria-invalid={clientProblem ? true : undefined} onChange={(e) => setClientId(e.target.value)} /></Field>
            {clientProblem ? <span className="text-caption text-destructive-text">{clientProblem}</span> : null}
            <fieldset className="flex flex-col gap-2">
              <legend className="mb-1 text-control font-semibold">Scopes</legend>
              {AGENT_SCOPES.map((scope) => (
                <label key={scope} className="flex items-center gap-2 text-control">
                  <input type="checkbox" className="size-4 accent-[var(--primary)]" checked={scopes.includes(scope)} onChange={() => toggle(scope)} />
                  <span className="font-mono">{scope}</span><span className="text-muted-foreground">{SCOPE_HINT[scope]}</span>
                </label>
              ))}
            </fieldset>
            <Field label="Project" htmlFor="ag-p" hint="An agent bound to a project reads only that project.">
              <NativeSelect id="ag-p" value={project} onChange={(e) => setProject(e.target.value)}>
                <option value="">All projects</option>
                {projects.map((one) => <option key={one.id} value={one.id}>{one.name}</option>)}
              </NativeSelect>
            </Field>
            <DialogFooter><Button type="button" onClick={close}>Cancel</Button><Button variant="primary" type="submit" disabled={register.isPending || scopes.length === 0 || agentNameProblem(name) !== null || clientIdProblem(clientId) !== null}>Register agent</Button></DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
      <Dialog open={revoking !== null} onOpenChange={(o) => !o && setRevoking(null)}>
        <DialogContent role="alertdialog" title={`Revoke ${revoking?.name ?? 'this agent'}?`} description="The agent is refused on its next request, whatever its token's expiry. Register it again to let it back in.">
          <DialogFooter>
            <Button onClick={() => setRevoking(null)}>Cancel</Button>
            <Button
              variant="destructive"
              disabled={revoke.isPending}
              onClick={() => {
                if (!revoking) return
                revoke.mutate(revoking.id, {
                  onSuccess: () => { setRevoking(null); toast.success('Agent revoked') },
                  onError: (error) => toast.error(because(error, 'The agent was not revoked')),
                })
              }}
            >Revoke agent</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}
