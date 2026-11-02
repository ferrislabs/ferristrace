import { Plus } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { because } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Dialog, DialogContent, DialogFooter } from '@/components/ui/dialog'
import { Field, Input, NativeSelect } from '@/components/ui/input'
import { Table, Td, Th, Tr } from '@/components/ui/misc'
import { useCatalogue, useAssignProject, useCreateProject, projectNameProblem } from '@/data/projects'
import { useTeams } from '@/data/teams'
import { stamp } from '@/lib/format'
import { Link } from '@/lib/router'
import { useScope } from '@/lib/scope'
import { H } from './parts'

export function Projects() {
  const { active } = useOrganisations()
  const { projects, nothing } = useCatalogue()
  const { teams } = useTeams()
  const assign = useAssignProject()
  const { chooseProject } = useScope()
  const [adding, setAdding] = useState(false)
  const mayArrange = active?.role === 'owner' || active?.role === 'admin'
  const catalogue = projects ?? []

  const hand = (project: string, team: string) =>
    assign.mutate(
      { project, team: team === '' ? null : team },
      { onError: (error) => toast.error(because(error, 'The project did not change hands')) },
    )

  return (
    <>
      <H title="Projects" sub="A project is a named source of telemetry with its own api keys.">
        {mayArrange ? <Button size="sm" onClick={() => setAdding(true)}><Plus />New project</Button> : null}
      </H>
      <Card className="overflow-x-auto">
        <Table aria-label="Projects">
          <thead><tr><Th>Name</Th><Th className="w-[180px]">Owner team</Th><Th numeric className="w-[110px]">Api keys</Th><Th className="w-[190px]">Created</Th><Th className="w-[110px]"><span className="sr-only">Actions</span></Th></tr></thead>
          <tbody>{catalogue.map((p) => (
            <Tr key={p.id}>
              <Td className="h-11"><Link to="/settings/project/general" className="font-mono font-semibold text-foreground" onClick={() => chooseProject(p.id)}>{p.name}</Link></Td>
              <Td>{mayArrange ? (
                <NativeSelect aria-label={`Owner team for ${p.name}`} className="h-7 w-[150px]" value={p.team?.id ?? ''} disabled={assign.isPending} onChange={(e) => hand(p.id, e.target.value)}>
                  <option value="">No team</option>
                  {(teams ?? []).map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
                </NativeSelect>
              ) : (
                <span className="text-control">{p.team?.name ?? <span className="text-muted-foreground">No team</span>}</span>
              )}</Td>
              <Td numeric>{p.keys}</Td>
              <Td className="font-mono text-control">{stamp(p.created_at_ms)}</Td>
              <Td numeric><Button variant="ghost" size="sm" asChild><Link to="/settings/project/general" onClick={() => chooseProject(p.id)}>Settings</Link></Button></Td>
            </Tr>
          ))}</tbody>
        </Table>
        {catalogue.length === 0 ? <p className="m-0 px-4 py-3 text-control text-muted-foreground">{projects ? 'No project yet. Telemetry arrives under one, so a deployment needs at least one.' : nothing}</p> : null}
      </Card>
      <p className="m-0 text-caption text-muted-foreground">Renaming a project keeps its keys and its signals: nothing anywhere holds the name. Draw a key for one with <span className="font-mono">ferristrace issue-key</span>.</p>
      <NewProject open={adding} onOpenChange={setAdding} />
    </>
  )
}

function NewProject({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const create = useCreateProject()
  const [name, setName] = useState('')
  const problem = name.length === 0 ? null : projectNameProblem(name)

  const close = (o: boolean) => {
    onOpenChange(o)
    if (!o) setName('')
  }

  return (
    <Dialog open={open} onOpenChange={close}>
      <DialogContent title="New project" description="Draw its first ingest key afterwards, from the command line.">
        <form
          className="flex flex-col gap-4"
          onSubmit={(e) => {
            e.preventDefault()
            create.mutate(name, {
              onSuccess: () => { close(false); toast.success(`Project ${name.trim().toLowerCase()} created`) },
              onError: (error) => toast.error(because(error, 'The project was not created')),
            })
          }}
        >
          <Field label="Name" htmlFor="np-n" hint="Lowercase letters, digits and dashes. It is typed beside a key and read out of a url."><Input id="np-n" mono value={name} aria-invalid={problem ? true : undefined} onChange={(e) => setName(e.target.value)} autoFocus /></Field>
          {problem ? <span className="text-caption text-destructive-text">{problem}</span> : null}
          <DialogFooter><Button type="button" onClick={() => close(false)}>Cancel</Button><Button variant="primary" type="submit" disabled={name.trim().length === 0 || Boolean(problem) || create.isPending}>Create project</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
