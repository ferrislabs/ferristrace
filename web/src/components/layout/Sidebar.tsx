import { Bell, Keyboard, LogOut, Moon, Settings2, UserRound } from 'lucide-react'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Segmented } from '@/components/ui/segmented'
import { Kbd, Avatar } from '@/components/ui/misc'
import { Badge } from '@/components/ui/badge'
import { AXES } from './nav'
import { useNavCounts } from './useNavCounts'
import { initialsOf, nameOf } from '@/data/profile'
import { useUnreadCount } from '@/data/notifications'
import { useAccount, useOrganisations, useSignOut } from '@/auth/session'
import { useTheme, type ThemePref } from '@/lib/theme'
import { Link, isActivePath, useLocation, useNavigate } from '@/lib/router'
import { cn } from '@/lib/utils'
import { Logo } from '@/components/ft/Logo'

const item = 'relative flex w-14 flex-col items-center justify-center gap-0.5 rounded-lg py-1.5 text-[10.5px] font-semibold text-muted-foreground no-underline hover:bg-muted hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring'

export function Sidebar({ onOpenShortcuts }: { onOpenShortcuts: () => void }) {
  const { pathname } = useLocation()
  const counts = useNavCounts()
  const account = useAccount()
  const { active } = useOrganisations()
  const signingOut = useSignOut()
  const { pref, setPref } = useTheme()
  const navigate = useNavigate()
  const unread = useUnreadCount()
  const who = account ? nameOf(account) : null
  const initials = who ? initialsOf(who) : '??'
  const onSettings = isActivePath(pathname, '/settings')
  return (
    <aside className="sticky top-0 hidden h-screen w-[72px] shrink-0 flex-col items-center border-r bg-sidebar md:flex" aria-label="Primary">
      <Link to="/" className="flex h-12 w-full shrink-0 items-center justify-center border-b text-foreground no-underline" aria-label="FerrisTrace home">
        <Logo className="size-7" />
      </Link>
      <nav aria-label="Main" className="scrollbar-none flex min-h-0 flex-1 flex-col items-center gap-1 overflow-y-auto py-2">
        {AXES.map((axis) => {
          const on = axis.tabs.some((t) => isActivePath(pathname, t.to))
          const alert = axis.tabs.reduce((n, t) => n + (t.badge ? (counts[t.to] ?? 0) : 0), 0)
          return (
            <Link key={axis.key} to={axis.tabs[0].to} aria-current={on ? 'page' : undefined} title={axis.label} className={cn(item, on && 'bg-muted text-foreground')}>
              {on ? <span aria-hidden className="absolute -left-[9px] top-2 h-[calc(100%-16px)] w-0.5 rounded-full bg-primary" /> : null}
              <axis.icon className="size-5" aria-hidden />
              {axis.label}
              {alert ? <span aria-label={`${alert} need attention`} className="absolute right-1.5 top-1 size-2 rounded-full bg-destructive" /> : null}
            </Link>
          )
        })}
      </nav>
      <div className="flex flex-col items-center gap-1 border-t py-2">
        <Link to="/settings" title="Settings" aria-current={onSettings ? 'page' : undefined} className={cn(item, onSettings && 'bg-muted text-foreground')}>
          <Settings2 className="size-5" aria-hidden />
          Settings
        </Link>
        <Popover>
          <PopoverTrigger asChild>
            <button type="button" aria-label="Account menu" className="flex size-10 cursor-pointer items-center justify-center rounded-full bg-transparent hover:bg-muted data-[state=open]:bg-muted focus-visible:outline-2 focus-visible:outline-ring">
              <Avatar initials={initials} src={account?.photo} />
            </button>
          </PopoverTrigger>
          <PopoverContent side="right" align="end" className="w-[264px] p-1">
            <div className="mb-1 flex items-center gap-2.5 border-b px-2 pb-2.5 pt-2">
              <Avatar initials={initials} src={account?.photo} />
              <div className="flex min-w-0 flex-1 flex-col leading-[17px]">
                <span className="truncate text-control font-semibold">{who ?? 'Signed in'}</span>
                <span className="truncate text-caption text-muted-foreground">{account?.email ?? '—'}</span>
              </div>
              {active ? <Badge variant="accent">{active.role}</Badge> : null}
            </div>
            <MenuItem icon={<Bell />} onClick={() => navigate('/notifications')} hint={unread > 0 ? <Badge variant="accent">{unread}</Badge> : undefined}>Notifications</MenuItem>
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
