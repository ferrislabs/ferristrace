import { BellRing } from 'lucide-react'
import { toast } from 'sonner'
import { because } from '@/auth/client'
import { useAccount, useOrganisations } from '@/auth/session'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { personOf } from '@/data/incidents'
import { pageLink, pageStateVariant, useAcknowledgePage, useMyPages, type Page } from '@/data/oncall'
import { stamp } from '@/lib/format'
import { Link } from '@/lib/router'
import { cn } from '@/lib/utils'

export function PagesButton() {
  const { mine, all } = useMyPages()
  const { active } = useOrganisations()
  if (!active) return null
  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button variant={mine.length > 0 ? 'primary' : 'ghost'} size="sm" aria-label={`Pages, ${mine.length} waiting for you`}>
          <BellRing /><span className="max-sm:sr-only">Pages</span>{mine.length > 0 ? <span className="font-mono text-[11px]">{mine.length}</span> : null}
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-[380px] p-0">
        <PagesPanel pages={all} />
      </PopoverContent>
    </Popover>
  )
}

function PagesPanel({ pages }: { pages: Page[] }) {
  const account = useAccount()
  const { active } = useOrganisations()
  const acknowledge = useAcknowledgePage()
  const mayAnswerAny = active?.role === 'owner' || active?.role === 'admin'
  return (
    <section aria-label="Open pages" className="flex flex-col">
      <h2 className="m-0 border-b px-3.5 py-2.5 text-[15px] font-bold">Open pages</h2>
      <div className="max-h-[420px] overflow-y-auto">
        {pages.length === 0 ? <p className="m-0 px-4 py-8 text-center text-control text-muted-foreground">Nobody is being paged.</p> : null}
        {pages.map((page) => {
          const mine = page.target.id === account?.account
          const waiting = page.state === 'triggered' || page.state === 'escalated'
          const link = pageLink(page)
          return (
            <div key={page.id} className={cn('flex flex-col gap-1 border-b border-border-subtle px-3.5 py-2.5', mine && waiting && 'bg-accent-subtle')}>
              <span className="flex items-center gap-2">
                <Link to={link.to} search={link.search} className="flex-1 truncate text-control font-semibold text-foreground">{page.headline}</Link>
                <Badge variant={pageStateVariant(page.state)}>{page.state}</Badge>
              </span>
              <span className="text-caption text-muted-foreground">
                {page.source.kind === 'incident' ? 'Incident' : 'Alert'} · {page.schedule.name ?? 'a deleted schedule'} · {mine ? 'you' : personOf(page.target)}{page.step > 0 ? ` · step ${page.step}` : ''}
              </span>
              <span className="flex items-center gap-2">
                <time className="flex-1 font-mono text-[11px] text-faint-foreground">{stamp(page.created_ms)}</time>
                {waiting && (mine || mayAnswerAny) ? (
                  <Button size="sm" variant="primary" disabled={acknowledge.isPending} onClick={() => acknowledge.mutate(page.id, {
                    onSuccess: () => toast.success('Page acknowledged', { description: 'Its escalation stopped.' }),
                    onError: (error) => toast.error(because(error, 'The page was not acknowledged')),
                  })}>Acknowledge</Button>
                ) : null}
              </span>
            </div>
          )
        })}
      </div>
      <p className="m-0 px-3.5 py-2.5 text-caption text-muted-foreground">An unacknowledged page moves to the next person in the rotation, then to the administrators.</p>
    </section>
  )
}
