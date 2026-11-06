import { AXES } from './nav'
import { useNavCounts } from './useNavCounts'
import { NavLink, isActivePath, useLocation } from '@/lib/router'
import { cn } from '@/lib/utils'

export function SubNav() {
  const { pathname } = useLocation()
  const counts = useNavCounts()
  const axis = AXES.find((a) => a.tabs.some((t) => isActivePath(pathname, t.to)))
  if (!axis || axis.tabs.length < 2) return null
  return (
    <nav aria-label={axis.label} className="scrollbar-none sticky top-12 z-10 flex h-10 shrink-0 gap-1 overflow-x-auto border-b bg-background/95 px-[max(12px,env(safe-area-inset-left))] backdrop-blur md:px-4">
      {axis.tabs.map((t) => {
        const count = t.badge ? counts[t.to] : 0
        return (
          <NavLink
            key={t.to}
            to={t.to}
            end={t.to === '/'}
            className={({ isActive }) =>
              cn(
                'relative flex shrink-0 items-center gap-1.5 px-2.5 text-[13.5px] font-semibold text-muted-foreground no-underline hover:text-foreground focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring',
                isActive && 'text-foreground after:absolute after:inset-x-2 after:bottom-0 after:h-0.5 after:rounded-full after:bg-primary',
              )
            }
          >
            <t.icon className="size-3.5" aria-hidden />
            {t.label}
            {count ? <span className="rounded-full bg-destructive-subtle px-1.5 font-mono text-[11px] font-semibold leading-[18px] text-destructive-text" aria-label={`${count} ${t.to === '/issues' ? 'new issues' : t.to === '/alerts' ? 'alerts firing' : 'open incidents'}`}>{count}</span> : null}
          </NavLink>
        )
      })}
    </nav>
  )
}
