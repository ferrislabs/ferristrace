import { Activity, CircleMinus, KeyRound, Gauge, TriangleAlert, UserPlus } from 'lucide-react'
import { useState } from 'react'
import { Segmented } from '@/components/ui/segmented'
import { Button } from '@/components/ui/button'
import { MockData } from '@/components/ft/MockData'
import { useNotifications } from '@/data/notifications'
import { Link } from '@/lib/router'
import { cn } from '@/lib/utils'

const ICON = { n1: TriangleAlert, n2: Activity, n3: CircleMinus, n4: KeyRound, n5: Gauge, n6: UserPlus } as Record<string, typeof Activity>

export function NotificationsPanel() {
  const { items, unread, markAll, mark } = useNotifications()
  const [filter, setFilter] = useState<'unread' | 'all'>(unread ? 'unread' : 'all')
  const list = filter === 'unread' ? items.filter((n) => n.unread) : items
  return (
    <section aria-label="Notifications" className="flex flex-col">
      <div className="flex items-center gap-2 border-b px-3.5 py-2.5">
        <h2 className="flex-1 text-[15px] font-bold">Notifications</h2>
        <MockData />
        <Segmented size="sm" label="Filter" value={filter} onChange={setFilter} options={[{ value: 'unread', label: `Unread ${unread}` }, { value: 'all', label: 'All' }]} />
        <Button variant="ghost" size="sm" onClick={markAll} disabled={!unread}>Mark all read</Button>
      </div>
      <div className="max-h-[420px] overflow-y-auto">
        {list.length === 0 ? <p className="m-0 px-4 py-8 text-center text-control text-muted-foreground">You are all caught up.</p> : null}
        {list.map((n) => {
          const I = ICON[n.id] ?? Activity
          return (
            <Link key={n.id} to={n.to} onClick={() => mark(n.id)} className={cn('grid grid-cols-[20px_minmax(0,1fr)_auto] gap-x-2.5 border-b border-border-subtle px-3.5 py-2.5 text-foreground no-underline hover:bg-row-hover', n.unread && 'bg-accent-subtle')}>
              <I className={cn('mt-0.5 size-3.5', n.kind === 'warn' ? 'text-warning-text' : n.kind === 'info' ? 'text-accent-text' : 'text-muted-foreground')} aria-hidden />
              <span className="flex min-w-0 flex-col gap-0.5">
                <span className="text-control font-semibold">{n.title}</span>
                <span className="text-caption text-muted-foreground">{n.meta}</span>
                <time className="font-mono text-[11px] text-faint-foreground">{n.time}</time>
              </span>
              {n.unread ? <span className="mt-[7px] size-1.5 rounded-full bg-primary" aria-label="Unread" /> : <span />}
            </Link>
          )
        })}
      </div>
      <div className="flex items-center gap-2 px-3.5 py-2.5 text-caption text-muted-foreground">
        <span className="flex-1">Kept 30 days. Alerts also go to email and webhooks if set.</span>
        <Link to="/settings/preferences">Notification settings</Link>
      </div>
    </section>
  )
}
