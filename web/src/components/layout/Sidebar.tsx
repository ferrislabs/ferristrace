import { Bell, ChevronDown, ChevronsUpDown, Keyboard, LogOut, Moon, Settings2, UserRound } from 'lucide-react'
import { useState } from 'react'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Segmented } from '@/components/ui/segmented'
import { Kbd, Avatar } from '@/components/ui/misc'
import { Badge } from '@/components/ui/badge'
import { NAV } from './nav'
import { NotificationsPanel } from './Notifications'
import { useNotifications } from '@/data/notifications'
import { useIssues } from '@/data/issues'
import { useAlerts } from '@/data/alerts'
import { initialsOf, nameOf } from '@/data/profile'
import { useAccount, useOrganisations, useSignOut } from '@/auth/session'
import { useTheme, type ThemePref } from '@/lib/theme'
import { Link, NavLink, isActivePath, useLocation, useNavigate } from '@/lib/router'
import { cn } from '@/lib/utils'
import { Logo } from '@/components/ft/Logo'

export function Sidebar({ onOpenShortcuts }: { onOpenShortcuts: () => void }) {
  const { pathname } = useLocation()
  const [open, setOpen] = useState<Record<string, boolean>>({})
  const notifs = useNotifications()
  const { issues } = useIssues()
  const { alerts } = useAlerts()
  const account = useAccount()
  const { active } = useOrganisations()
  const signingOut = useSignOut()
  const newIssues = issues.filter((x) => x.status === 'unresolved' || x.regressed).length
  const firing = (alerts ?? []).filter((r) => r.enabled && r.state.status === 'firing').length
  const { pref, setPref } = useTheme()
  const navigate = useNavigate()
  const who = account ? nameOf(account) : null
  const initials = who ? initialsOf(who) : '??'
  return (
    <aside className="sticky top-0 hidden h-screen w-[224px] shrink-0 flex-col border-r bg-sidebar md:flex" aria-label="Primary">
      <Link to="/" className="flex h-12 shrink-0 items-center gap-2 border-b px-4 text-[15px] font-bold text-foreground no-underline">
        <Logo className="size-5" /> FerrisTrace
      </Link>
      <nav aria-label="Main" className="scrollbar-none flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto p-2">
        {NAV.map((g) => {
          const inGroup = g.items.some((i) => isActivePath(pathname, i.to))
          const expanded = !g.collapsible || (open[g.label!] ?? inGroup)
          return (
            <div key={g.label ?? 'top'} className="flex flex-col gap-0.5">
              {g.label && g.collapsible ? (
                <button
                  type="button"
                  aria-expanded={expanded}
                  onClick={() => setOpen((o) => ({ ...o, [g.label!]: !expanded }))}
                  className="flex w-full cursor-pointer items-center rounded-sm px-2.5 pb-1 pt-3 text-[11px] font-semibold uppercase tracking-[.05em] text-muted-foreground hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring"
                >
                  <span className="flex-1 text-left">{g.label}</span>
                  {!expanded ? <span className="mr-1.5 font-mono text-[10.5px] font-medium normal-case tracking-normal">{g.items.length}</span> : null}
                  <ChevronDown className={cn('size-3 transition-transform', !expanded && '-rotate-90')} aria-hidden />
                </button>
              ) : g.label ? (
                <span className="px-2.5 pb-1 pt-3 text-[11px] font-semibold uppercase tracking-[.05em] text-muted-foreground">{g.label}</span>
              ) : null}
              {expanded &&
                g.items.map((i) => (
                  <NavLink
                    key={i.to}
                    to={i.to}
                    end={i.to === '/'}
                    className={({ isActive: a }) =>
                      cn('flex h-8 items-center gap-2.5 rounded-md px-2.5 text-[13.5px] font-semibold text-muted-foreground no-underline hover:bg-muted hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring', a && 'bg-muted text-foreground')
                    }
                  >
                    <i.icon className="size-4 shrink-0" aria-hidden />
                    <span className="flex-1 truncate">{i.label}</span>
                    {i.badge && i.to === '/issues' && newIssues ? <span className="rounded-full bg-destructive-subtle px-1.5 font-mono text-[11px] font-semibold leading-[18px] text-destructive-text" aria-label={`${newIssues} new issues`}>{newIssues} new</span> : null}
                    {i.badge && i.to === '/alerts' && firing ? <span className="rounded-full bg-destructive-subtle px-1.5 font-mono text-[11px] font-semibold leading-[18px] text-destructive-text" aria-label={`${firing} alerts firing`}>{firing} firing</span> : null}
                    {!i.badge && i.keys ? <span className="flex gap-0.5"><Kbd>G</Kbd><Kbd>{i.keys}</Kbd></span> : null}
                  </NavLink>
                ))}
            </div>
          )
        })}
      </nav>
      <div className="flex flex-col gap-0.5 border-t p-2">
        <Popover>
          <PopoverTrigger asChild>
            <button type="button" aria-label={`Notifications, ${notifs.unread} unread`} className="sb-row data-[state=open]:bg-muted">
              <span className="relative inline-flex size-7 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground">
                <Bell className="size-3.5" aria-hidden />
                {notifs.unread ? <span className="absolute -right-0.5 -top-0.5 size-2 rounded-full bg-primary ring-2 ring-sidebar" /> : null}
              </span>
              <span className="flex min-w-0 flex-1 flex-col leading-4">
                <span className="truncate text-control font-semibold">Notifications</span>
                <span className="truncate text-caption text-muted-foreground">{notifs.items.find((n) => n.unread)?.title ?? 'All caught up'}</span>
              </span>
              {notifs.unread ? <span className="inline-flex h-[18px] min-w-[18px] items-center justify-center rounded-full bg-primary px-1.5 font-mono text-[11px] font-semibold text-primary-foreground">{notifs.unread}</span> : null}
            </button>
          </PopoverTrigger>
          <PopoverContent side="right" align="end" className="w-[400px] overflow-hidden p-0">
            <NotificationsPanel />
          </PopoverContent>
        </Popover>
        <NavLink to="/settings" className={({ isActive: a }) => cn('sb-row no-underline', a && 'bg-muted')}>
          <span className="inline-flex size-7 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground"><Settings2 className="size-3.5" aria-hidden /></span>
          <span className="flex min-w-0 flex-1 flex-col leading-4">
            <span className="truncate text-control font-semibold">Settings</span>
            <span className="truncate text-caption text-muted-foreground">Personal, organisation, project</span>
          </span>
          <span className="flex gap-0.5"><Kbd>G</Kbd><Kbd>S</Kbd></span>
        </NavLink>
        <Popover>
          <PopoverTrigger asChild>
            <button type="button" aria-label="Account menu" className="sb-row data-[state=open]:bg-muted">
              <Avatar initials={initials} src={account?.photo} />
              <span className="flex min-w-0 flex-1 flex-col leading-4">
                <span className="truncate text-control font-semibold">{who ?? 'Signed in'}</span>
                <span className="truncate text-caption text-muted-foreground">{account?.email ?? '—'}</span>
              </span>
              <ChevronsUpDown className="size-3.5 text-muted-foreground" aria-hidden />
            </button>
          </PopoverTrigger>
          <PopoverContent side="top" align="start" className="w-[264px] p-1">
            <div className="mb-1 flex items-center gap-2.5 border-b px-2 pb-2.5 pt-2">
              <Avatar initials={initials} src={account?.photo} />
              <div className="flex min-w-0 flex-1 flex-col leading-[17px]">
                <span className="truncate text-control font-semibold">{who ?? 'Signed in'}</span>
                <span className="truncate text-caption text-muted-foreground">{account?.email ?? '—'}</span>
              </div>
              {active ? <Badge variant="accent">{active.role}</Badge> : null}
            </div>
            <MenuItem icon={<UserRound />} onClick={() => navigate('/settings/profile')}>Profile</MenuItem>
            <div className="flex h-[30px] items-center gap-2 px-2 text-control">
              <Moon className="size-3.5" aria-hidden />Theme
              <span className="ml-auto"><Segmented size="sm" label="Theme" value={pref} onChange={(v: ThemePref) => setPref(v)} options={[{ value: 'system', label: 'System' }, { value: 'light', label: 'Light' }, { value: 'dark', label: 'Dark' }]} /></span>
            </div>
            <MenuItem icon={<Keyboard />} onClick={onOpenShortcuts} hint={<Kbd>?</Kbd>}>Keyboard shortcuts</MenuItem>
            <div className="my-1 h-px bg-border" />
            <MenuItem icon={<LogOut />} onClick={() => signingOut.mutate()}>Sign out</MenuItem>
          </PopoverContent>
        </Popover>
      </div>
    </aside>
  )
}

function MenuItem({ icon, children, hint, onClick }: { icon: React.ReactNode; children: React.ReactNode; hint?: React.ReactNode; onClick?: () => void }) {
  return (
    <button type="button" onClick={onClick} className="flex h-[30px] w-full cursor-pointer items-center gap-2 rounded-sm px-2 text-left text-control text-foreground hover:bg-muted focus-visible:bg-muted focus-visible:outline-none [&>svg]:size-3.5">
      {icon}
      <span className="flex-1">{children}</span>
      {hint}
    </button>
  )
}
