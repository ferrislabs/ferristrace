import { LogOut, Menu, Moon, ScrollText, Settings2, UserRound, Waypoints } from 'lucide-react'
import { useState } from 'react'
import { Sheet, SheetContent } from '@/components/ui/sheet'
import { Segmented } from '@/components/ui/segmented'
import { Avatar } from '@/components/ui/misc'
import { ALL_NAV, NAV } from './nav'
import { useIncidents } from '@/data/incidents'
import { useAlerts } from '@/data/alerts'
import { initialsOf, nameOf } from '@/data/profile'
import { useAccount, useSignOut } from '@/auth/session'
import { useTheme, type ThemePref } from '@/lib/theme'
import { Link, NavLink, isActivePath, useLocation } from '@/lib/router'
import { cn } from '@/lib/utils'

const FIRST = ['Reliability', 'Infrastructure']
const ORDERED = [...NAV].sort((a, b) => {
  const rank = (label: string | null) => (label && FIRST.includes(label) ? FIRST.indexOf(label) : FIRST.length)
  return rank(a.label) - rank(b.label)
})

const tab = 'relative flex min-h-[52px] flex-1 flex-col items-center justify-center gap-0.5 text-[11px] font-semibold text-muted-foreground no-underline focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring'

function Count({ value, label }: { value: number; label: string }) {
  if (!value) return null
  return <span aria-label={label} className="absolute left-1/2 top-1.5 ml-2 inline-flex h-4 min-w-4 items-center justify-center rounded-full bg-destructive px-1 font-mono text-[10px] font-semibold leading-none text-destructive-foreground">{value}</span>
}

export function MobileTabs() {
  const { pathname } = useLocation()
  const [open, setOpen] = useState(false)
  const ongoing = useIncidents('open').incidents?.length ?? 0
  const firing = (useAlerts().alerts ?? []).filter((r) => r.enabled && r.state.status === 'firing').length
  const account = useAccount()
  const signingOut = useSignOut()
  const { pref, setPref } = useTheme()
  const incidents = ALL_NAV.find((i) => i.to === '/incidents')
  const tabs = [
    ALL_NAV[0],
    ...(incidents ? [incidents] : []),
    { label: 'Traces', to: '/traces', icon: Waypoints },
    { label: 'Logs', to: '/logs', icon: ScrollText },
  ]
  const elsewhere = !tabs.some((t) => isActivePath(pathname, t.to, t.to === '/'))
  const who = account ? nameOf(account) : null
  const close = () => setOpen(false)

  return (
    <>
      <nav aria-label="Primary" className="fixed inset-x-0 bottom-0 z-30 flex border-t bg-sidebar/95 pb-[env(safe-area-inset-bottom)] backdrop-blur md:hidden">
        {tabs.map((t) => {
          const on = isActivePath(pathname, t.to, t.to === '/')
          return (
            <Link key={t.to} to={t.to} aria-current={on ? 'page' : undefined} className={cn(tab, on && 'text-foreground')}>
              {on ? <span aria-hidden className="absolute inset-x-5 top-0 h-0.5 rounded-full bg-primary" /> : null}
              <t.icon className="size-5" aria-hidden />
              {t.label}
              {t.to === '/incidents' ? <Count value={ongoing} label={`${ongoing} open incidents`} /> : null}
            </Link>
          )
        })}
        <button type="button" onClick={() => setOpen(true)} aria-haspopup="dialog" className={cn(tab, 'cursor-pointer bg-transparent', (open || elsewhere) && 'text-foreground')}>
          {elsewhere ? <span aria-hidden className="absolute inset-x-5 top-0 h-0.5 rounded-full bg-primary" /> : null}
          <Menu className="size-5" aria-hidden />
          Menu
          <Count value={firing} label={`${firing} alerts firing`} />
        </button>
      </nav>

      <Sheet open={open} onOpenChange={setOpen}>
        <SheetContent title="Menu">
          <nav aria-label="All pages" className="flex flex-col pb-2">
            {ORDERED.map((group) => (
              <div key={group.label ?? 'top'} className="flex flex-col">
                {group.label ? <span className="px-3 pb-1 pt-4 text-[11px] font-semibold uppercase tracking-[.05em] text-muted-foreground">{group.label}</span> : null}
                <div className="grid grid-cols-2 gap-1">
                  {group.items.map((item) => (
                    <NavLink
                      key={item.to}
                      to={item.to}
                      end={item.to === '/'}
                      onClick={close}
                      className={({ isActive }) => cn('flex min-h-11 items-center gap-2.5 rounded-lg px-3 text-[15px] font-semibold text-foreground no-underline active:bg-muted', isActive && 'bg-muted')}
                    >
                      <item.icon className="size-[18px] shrink-0 text-muted-foreground" aria-hidden />
                      <span className="min-w-0 flex-1 truncate">{item.label}</span>
                      {item.to === '/alerts' && firing ? <span className="rounded-full bg-destructive-subtle px-1.5 font-mono text-[11px] font-semibold text-destructive-text">{firing}</span> : null}
                      {item.to === '/incidents' && ongoing ? <span className="rounded-full bg-destructive-subtle px-1.5 font-mono text-[11px] font-semibold text-destructive-text">{ongoing}</span> : null}
                    </NavLink>
                  ))}
                </div>
              </div>
            ))}
            <div className="mt-4 flex flex-col gap-1 border-t pt-3">
              <div className="flex items-center gap-3 px-3 py-2">
                <Avatar initials={who ? initialsOf(who) : '??'} src={account?.photo} />
                <span className="flex min-w-0 flex-1 flex-col leading-5">
                  <span className="truncate text-[15px] font-semibold">{who ?? 'Signed in'}</span>
                  <span className="truncate text-caption text-muted-foreground">{account?.email ?? ''}</span>
                </span>
              </div>
              <NavLink to="/settings/profile" onClick={close} className="flex min-h-11 items-center gap-2.5 rounded-lg px-3 text-[15px] font-semibold text-foreground no-underline active:bg-muted">
                <UserRound className="size-[18px] text-muted-foreground" aria-hidden />Profile
              </NavLink>
              <NavLink to="/settings" onClick={close} className="flex min-h-11 items-center gap-2.5 rounded-lg px-3 text-[15px] font-semibold text-foreground no-underline active:bg-muted">
                <Settings2 className="size-[18px] text-muted-foreground" aria-hidden />Settings
              </NavLink>
              <div className="flex min-h-11 items-center gap-2.5 px-3 text-[15px] font-semibold">
                <Moon className="size-[18px] text-muted-foreground" aria-hidden />Theme
                <span className="ml-auto"><Segmented label="Theme" value={pref} onChange={(v: ThemePref) => setPref(v)} options={[{ value: 'system', label: 'System' }, { value: 'light', label: 'Light' }, { value: 'dark', label: 'Dark' }]} /></span>
              </div>
              <button type="button" onClick={() => signingOut.mutate()} className="flex min-h-11 cursor-pointer items-center gap-2.5 rounded-lg bg-transparent px-3 text-left text-[15px] font-semibold text-destructive-text active:bg-muted">
                <LogOut className="size-[18px]" aria-hidden />Sign out
              </button>
            </div>
          </nav>
        </SheetContent>
      </Sheet>
    </>
  )
}
