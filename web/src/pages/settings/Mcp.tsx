import { Copy, Plug } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { because } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Field, NativeSelect } from '@/components/ui/input'
import { Alert } from '@/components/ui/misc'
import { AGENT_SCOPES, type AgentScope } from '@/data/agents'
import { useMcp, useTurnMcpOff, useTurnMcpOn, type McpState } from '@/data/mcp'
import { useProjects } from '@/data/projects'
import { stamp } from '@/lib/format'
import { H } from './parts'
import { SCOPE_HINT } from './Agents'

const REQUIRED: AgentScope = 'telemetry:read'

function Settings({ state }: { state: McpState }) {
  const projects = useProjects()
  const on = useTurnMcpOn()
  const off = useTurnMcpOff()
  const held = state.agent?.scopes ?? [REQUIRED]
  const [scopes, setScopes] = useState<AgentScope[] | null>(null)
  const [project, setProject] = useState<string | null>(null)
  const chosen = scopes ?? held
  const chosenProject = project ?? state.agent?.project ?? ''
  const changed = state.agent === null || chosen.join() !== held.join() || chosenProject !== (state.agent.project ?? '')
  const toggle = (scope: AgentScope) =>
    setScopes(AGENT_SCOPES.filter((one) => (one === scope ? !chosen.includes(one) : chosen.includes(one))))

  const apply = () =>
    on.mutate(
      { scopes: chosen, project: chosenProject || null },
      {
        onSuccess: () => {
          setScopes(null)
          setProject(null)
          toast.success(state.enabled ? 'MCP updated' : 'MCP is on')
        },
        onError: (error) => toast.error(because(error, 'MCP was not turned on')),
      },
    )

  const turnOff = () =>
    off.mutate(undefined, {
      onSuccess: () => {
        setScopes(null)
        setProject(null)
        toast.success('MCP is off')
      },
      onError: (error) => toast.error(because(error, 'MCP was not turned off')),
    })

  return (
    <>
      <Card className="flex flex-col gap-3.5 p-4">
        <span className="flex flex-wrap items-center gap-2">
          <span className="text-heading">MCP for this organisation</span>
          {state.enabled ? <Badge variant="accent">On</Badge> : <Badge variant="outline">Off</Badge>}
        </span>
        <span className="text-caption text-muted-foreground">
          Turning MCP on lets the gateway ({state.gateway_client_id ? <span className="font-mono">{state.gateway_client_id}</span> : 'the configured client'}) call FerrisTrace for this organisation, with the scopes ticked here. FerrisKey&apos;s policies decide, call by call, what each person may do through it; FerrisTrace still enforces these scopes and the project.
        </span>
        <fieldset className="flex flex-col gap-2">
          <legend className="mb-1 text-control font-semibold">Scopes</legend>
          {AGENT_SCOPES.map((scope) => (
            <label key={scope} className="flex items-center gap-2 text-control">
              <input type="checkbox" className="size-4 accent-[var(--primary)]" checked={chosen.includes(scope)} disabled={scope === REQUIRED} onChange={() => toggle(scope)} />
              <span className="font-mono">{scope}</span><span className="text-muted-foreground">{SCOPE_HINT[scope]}{scope === REQUIRED ? ' The gateway needs it to look facts up.' : ''}</span>
            </label>
          ))}
        </fieldset>
        <Field label="Project" htmlFor="mcp-p" hint="Bound to a project, MCP reads only that project.">
          <NativeSelect id="mcp-p" value={chosenProject} onChange={(e) => setProject(e.target.value)}>
            <option value="">All projects</option>
            {projects.map((one) => <option key={one.id} value={one.id}>{one.name}</option>)}
          </NativeSelect>
        </Field>
        <span className="flex flex-wrap items-center gap-2">
          <Button variant="primary" size="sm" disabled={!changed || !chosen.includes(REQUIRED) || on.isPending} onClick={apply}><Plug />{state.enabled ? 'Save scopes' : 'Turn MCP on'}</Button>
          {state.enabled ? <Button size="sm" variant="destructive" disabled={off.isPending} onClick={turnOff}>Turn MCP off</Button> : null}
        </span>
        {state.agent ? <span className="text-caption text-muted-foreground">Registered {stamp(state.agent.created_at_ms)}{state.agent.last_used_at_ms === null ? ', not used yet.' : `, last used ${stamp(state.agent.last_used_at_ms)}.`}</span> : null}
      </Card>
      {state.enabled && state.url ? (
        <Card className="flex flex-col gap-2 p-4">
          <span className="text-heading">Give this URL to your MCP client</span>
          <span className="inline-flex items-center gap-2 font-mono text-control">
            <span className="break-all">{state.url}</span>
            <Button variant="ghost" size="icon-sm" aria-label="Copy the MCP URL" onClick={() => { void navigator.clipboard?.writeText(state.url ?? ''); toast.success('Copied') }}><Copy /></Button>
          </span>
          <span className="text-caption text-muted-foreground">Claude Code, Claude Desktop and any MCP host sign in through FerrisKey; the gateway refuses anyone whose policies say no.</span>
        </Card>
      ) : null}
    </>
  )
}

export function Mcp() {
  const { active } = useOrganisations()
  const mayChange = active?.role === 'owner' || active?.role === 'admin'
  const { state, nothing } = useMcp(mayChange)

  if (!mayChange) {
    return (
      <>
        <H title="MCP" sub="Let an MCP client read FerrisTrace and act on incidents, within the rules FerrisKey enforces." />
        <Card className="px-4 py-3 text-control text-muted-foreground">Only an owner or an administrator can see or change MCP.</Card>
      </>
    )
  }

  return (
    <>
      <H title="MCP" sub="Let an MCP client read FerrisTrace and act on incidents, within the rules FerrisKey enforces." />
      {state && !state.agents_enabled ? (
        <Alert variant="warning">Agents are off on this server: {state.agents_off_reason ?? 'no agent issuer is configured'}. MCP needs them.</Alert>
      ) : null}
      {state && !state.configured ? (
        <Card className="px-4 py-3 text-control text-muted-foreground">
          MCP is not configured on this server. Set <span className="font-mono">FERRISTRACE_MCP__GATEWAY_CLIENT_ID</span> to the client id of the gateway and <span className="font-mono">FERRISTRACE_MCP__PUBLIC_URL</span> to the address MCP clients reach it at, then restart.
        </Card>
      ) : null}
      {state?.configured ? <Settings state={state} /> : null}
      {state ? null : <Card className="px-4 py-3 text-control text-muted-foreground">{nothing}</Card>}
    </>
  )
}
