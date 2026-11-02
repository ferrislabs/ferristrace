import { FileText, Save } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { because } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Textarea } from '@/components/ui/input'
import { Segmented } from '@/components/ui/segmented'
import { personOf, type Dossier } from '@/data/incidents'
import {
  useDraftPostmortem, useEditPostmortem, usePostmortem, usePublishPostmortem,
  type Postmortem,
} from '@/data/postmortems'
import { stamp } from '@/lib/format'
import { Markdown } from '@/lib/markdown'

export function PostmortemSection({ incident }: { incident: Dossier }) {
  const { postmortem, pending, failed } = usePostmortem(incident.id)
  const draft = useDraftPostmortem()
  const resolved = incident.status === 'resolved'
  const write = () =>
    draft.mutate({ incident: incident.id }, {
      onSuccess: () => toast.success('Postmortem drafted'),
      onError: (error) => toast.error(because(error, 'The postmortem was not drafted')),
    })

  return (
    <section id="postmortem" aria-label="Postmortem" className="flex flex-col gap-3">
      <h2 className="text-control font-semibold">Postmortem</h2>
      {pending ? (
        <Card className="px-4 py-3 text-control text-muted-foreground">Reading the postmortem…</Card>
      ) : failed ? (
        <Card className="px-4 py-3 text-control text-muted-foreground">{because(failed, 'The postmortem could not be read.')}</Card>
      ) : postmortem ? (
        <Written key={`${postmortem.id}-${postmortem.updated_ms}`} postmortem={postmortem} />
      ) : (
        <Card className="flex flex-wrap items-center gap-3 px-4 py-3">
          <FileText className="size-4 shrink-0 text-muted-foreground" aria-hidden />
          <span className="flex-1 text-control text-muted-foreground">
            {resolved
              ? 'This incident is resolved. Start a postmortem from its timeline, alerts, paging and attachments.'
              : 'The incident is still open. A draft written now will say so, and can be written again until someone edits it.'}
          </span>
          <Button variant="primary" size="sm" disabled={draft.isPending} onClick={write}>Write postmortem</Button>
        </Card>
      )}
    </section>
  )
}

function Written({ postmortem }: { postmortem: Postmortem }) {
  const { active } = useOrganisations()
  const mayPublish = active?.role === 'owner' || active?.role === 'admin'
  const edit = useEditPostmortem()
  const publish = usePublishPostmortem()
  const draft = useDraftPostmortem()
  const [body, setBody] = useState(postmortem.body)
  const [view, setView] = useState<'write' | 'preview'>('write')
  const published = postmortem.status === 'published'
  const changed = body !== postmortem.body

  const save = () =>
    edit.mutate({ incident: postmortem.incident, body }, {
      onSuccess: () => toast.success('Postmortem saved'),
      onError: (error) => toast.error(because(error, 'The postmortem was not saved')),
    })
  const move = (next: boolean) =>
    publish.mutate({ incident: postmortem.incident, publish: next }, {
      onSuccess: () => toast.success(next ? 'Postmortem published' : 'Postmortem unpublished'),
      onError: (error) => toast.error(because(error, next ? 'The postmortem was not published' : 'The postmortem was not unpublished')),
    })
  const regenerate = () => {
    if (!window.confirm('Write the draft again from the timeline? Your edits will be lost.')) return
    draft.mutate({ incident: postmortem.incident, force: postmortem.edited }, {
      onSuccess: () => toast.success('Postmortem drafted again'),
      onError: (error) => toast.error(because(error, 'The postmortem was not drafted again')),
    })
  }

  return (
    <Card className="flex flex-col gap-3 p-4">
      <div className="flex flex-wrap items-center gap-2 text-caption text-muted-foreground">
        <Badge variant={published ? 'success' : 'neutral'}>{postmortem.status}</Badge>
        <span>Written by <span className="text-foreground">{personOf(postmortem.author)}</span></span>
        <span>Updated <span className="font-mono text-foreground">{stamp(postmortem.updated_ms)}</span></span>
        {postmortem.published_ms !== null ? <span>Published <span className="font-mono text-foreground">{stamp(postmortem.published_ms)}</span></span> : null}
        <span className="ml-auto flex flex-wrap items-center gap-2">
          {!published ? <Segmented<'write' | 'preview'> label="Postmortem view" size="sm" value={view} onChange={setView} options={[{ value: 'write', label: 'Write' }, { value: 'preview', label: 'Preview' }]} /> : null}
          {!published && (mayPublish || !postmortem.edited) ? <Button size="sm" disabled={draft.isPending} onClick={regenerate}>Regenerate</Button> : null}
          {mayPublish ? (
            published
              ? <Button size="sm" disabled={publish.isPending} onClick={() => move(false)}>Unpublish</Button>
              : <Button size="sm" variant="primary" disabled={publish.isPending || changed} onClick={() => move(true)}>Publish</Button>
          ) : null}
        </span>
      </div>
      {published ? (
        <Markdown source={postmortem.body} />
      ) : view === 'preview' ? (
        <Markdown source={body} />
      ) : (
        <>
          <Textarea aria-label="Postmortem" value={body} onChange={(e) => setBody(e.target.value)} rows={24} className="font-mono" />
          <Button size="sm" variant="primary" className="self-end" disabled={edit.isPending || !changed || !body.trim()} onClick={save}><Save />Save</Button>
        </>
      )}
    </Card>
  )
}
