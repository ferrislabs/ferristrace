import { useState } from 'react'
import { toast } from 'sonner'
import { because } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Field, Input, NativeSelect } from '@/components/ui/input'
import {
  projectNameProblem,
  useAssignProject,
  useCatalogue,
  useForgetProject,
  useRenameProject,
} from '@/data/projects'
import { useTeams } from '@/data/teams'
import { Link } from '@/lib/router'
import { useScope } from '@/lib/scope'
import { ConfirmDelete, Danger, H } from './parts'

export function ProjectGeneral() {
  const { active } = useOrganisations()
  const { projects, nothing } = useCatalogue()
  const { teams } = useTeams()
  const { project: chosen, chooseProject } = useScope()
  const rename = useRenameProject()
  const assign = useAssignProject()
  const forget = useForgetProject()
  const [typed, setTyped] = useState<string | null>(null)
  const [del, setDel] = useState(false)

  const catalogue = projects ?? []
  const project = catalogue.find((one) => one.id === chosen) ?? catalogue[0]
  const mayChange = active?.role === 'owner' || active?.role === 'admin'

  if (!project) {
    return (
      <>
        <H title="General" sub="The project this organisation holds telemetry under." />
        <Card className="px-4 py-3 text-control text-muted-foreground">
          {projects ? <>This organisation holds no project yet. <Link to="/settings/projects">Make one</Link> and come back.</> : nothing}
        </Card>
      </>
    )
  }

  const name = typed ?? project.name
  const problem = typed === null ? null : projectNameProblem(typed)
  const unchanged = name.trim().toLowerCase() === project.name

  return (
    <>
      <H title="General" sub={`Project ${project.name}.`} />
      <Card className="flex flex-col gap-3.5 p-4">
        <div className="grid grid-cols-[repeat(auto-fit,minmax(min(260px,100%),1fr))] gap-3">
          <Field label="Project name" htmlFor="pg-n" hint="Renaming keeps keys and signals: nothing anywhere holds the name.">
            <div className="flex gap-2">
              <Input id="pg-n" mono value={name} disabled={!mayChange} aria-invalid={problem ? true : undefined} onChange={(e) => setTyped(e.target.value)} />
              <Button size="sm" className="h-8" variant="primary" disabled={!mayChange || unchanged || Boolean(problem) || rename.isPending} onClick={() => rename.mutate({ project: project.id, name }, { onSuccess: () => { setTyped(null); toast.success('Saved') }, onError: (error) => toast.error(because(error, 'The project was not renamed')) })}>Save</Button>
            </div>
          </Field>
          <Field label="Owner team" hint={mayChange ? 'A project has one owner team or none.' : 'Only an owner or an administrator can change this.'}>
            <NativeSelect aria-label="Owner team" value={project.team?.id ?? ''} disabled={!mayChange || assign.isPending} onChange={(e) => assign.mutate({ project: project.id, team: e.target.value === '' ? null : e.target.value }, { onError: (error) => toast.error(because(error, 'The project did not change hands')) })}>
              <option value="">No team</option>
              {(teams ?? []).map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
            </NativeSelect>
          </Field>
        </div>
        {problem ? <span className="text-caption text-destructive-text">{problem}</span> : null}
        <span className="text-control"><span className="text-muted-foreground">Project id</span> <span className="font-mono">{project.id}</span></span>
        <span className="text-control"><span className="text-muted-foreground">Api keys that still authenticate</span> <span className="font-mono">{project.keys}</span></span>
      </Card>
      <p className="m-0 text-caption text-muted-foreground">How long a signal is kept is set for the whole organisation, in Settings, Organisation, General. Without a choice there, the deployment default applies.</p>
      {mayChange ? (
        <>
          <Danger title={`Delete ${project.name}`} text="Its api keys stop authenticating at once. Spans already written wait out the retention of the organisation." action="Delete project" onClick={() => setDel(true)} />
          <ConfirmDelete
            open={del}
            onOpenChange={setDel}
            name={project.name}
            what="project"
            body={`Its ${project.keys} api key${project.keys === 1 ? '' : 's'} go with it. Spans already written stay in the span store until their retention expires, unreachable from here.`}
            busy={forget.isPending}
            onConfirm={() =>
              forget.mutate(project.id, {
                onSuccess: () => {
                  setDel(false)
                  chooseProject(undefined)
                  toast(`Project ${project.name} deleted`)
                },
                onError: (error) => toast.error(because(error, 'The project was not deleted')),
              })
            }
          />
        </>
      ) : null}
    </>
  )
}
