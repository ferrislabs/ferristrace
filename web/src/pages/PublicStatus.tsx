import { useEffect } from 'react'
import { ApiError } from '@/auth/client'
import { StatusPageView } from '@/components/ft/StatusPageView'
import { Markdown } from '@/lib/markdown'
import { useParams, useSearchParam } from '@/lib/router'
import { usePublicPostmortem, usePublicStatus, useSubscribe } from '@/data/status-page'

const NOTICES: Record<string, string> = {
  confirmed: 'Your subscription is confirmed. You will get an email when an incident is posted here.',
  unsubscribed: 'You are unsubscribed and will not get emails from this page any more.',
}

function useNoIndex(hidden: boolean) {
  useEffect(() => {
    if (!hidden) return
    const meta = document.createElement('meta')
    meta.name = 'robots'
    meta.content = 'noindex'
    document.head.appendChild(meta)
    return () => {
      meta.remove()
    }
  }, [hidden])
}

function Missing({ failed }: { failed: unknown }) {
  const gone = failed instanceof ApiError && failed.status === 404
  return (
    <div className="flex min-h-screen items-center justify-center bg-sidebar px-6 text-center">
      <p className="m-0 text-control text-muted-foreground">{gone ? 'There is no status page at this address.' : 'The status page could not be loaded. Try again in a minute.'}</p>
    </div>
  )
}

export function PublicStatus() {
  const { slug } = useParams()
  const notice = useSearchParam('notice')
  const { status, failed } = usePublicStatus(slug)
  const subscribe = useSubscribe(slug)
  useNoIndex(status?.page.hide_from_search ?? true)
  useEffect(() => {
    if (status) document.title = `${status.page.name} status`
  }, [status])
  if (failed) return <Missing failed={failed} />
  if (!status) return <div className="min-h-screen bg-sidebar" aria-busy />
  return (
    <>
      {notice && NOTICES[notice] ? <div role="status" className="bg-accent-subtle px-4 py-2.5 text-center text-control text-accent-text">{NOTICES[notice]}</div> : null}
      <StatusPageView view={status} subscribe={async (email) => subscribe.mutateAsync(email)} />
    </>
  )
}

export function PublicPostmortem() {
  const { slug, incident } = useParams()
  const { postmortem, failed } = usePublicPostmortem(slug, incident)
  const { status } = usePublicStatus(slug)
  useNoIndex(status?.page.hide_from_search ?? true)
  if (failed) return <Missing failed={failed} />
  if (!postmortem) return <div className="min-h-screen bg-sidebar" aria-busy />
  return (
    <div className="min-h-screen bg-sidebar text-foreground">
      <main className="mx-auto flex max-w-[880px] flex-col gap-4 px-6 pb-16 pt-8">
        <a className="text-control" href={`/status/${slug}`}>Back to the status page</a>
        <h1 className="text-[24px] font-bold">{postmortem.title}</h1>
        <span className="text-caption text-muted-foreground">Published {new Date(postmortem.published_ms).toLocaleString('en-US')}</span>
        <article className="rounded-lg border bg-card px-6 py-5"><Markdown source={postmortem.body} /></article>
      </main>
    </div>
  )
}
