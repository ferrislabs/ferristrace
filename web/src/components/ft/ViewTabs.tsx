import { NavLink } from '@/lib/router'
import { cn } from '@/lib/utils'

export type ViewTab = { label: string; to: string }

export function ViewTabs({ label, tabs }: { label: string; tabs: ViewTab[] }) {
  return (
    <nav aria-label={label} className="scrollbar-none -mx-4 flex gap-1 overflow-x-auto border-b px-4 md:-mx-5 md:px-5">
      {tabs.map((tab) => (
        <NavLink
          key={tab.to}
          to={tab.to}
          end
          className={({ isActive }) =>
            cn(
              'relative -mb-px shrink-0 border-b-2 border-transparent px-3 py-2 text-control font-semibold text-muted-foreground no-underline hover:text-foreground focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring',
              isActive && 'border-primary text-foreground',
            )
          }
        >
          {tab.label}
        </NavLink>
      ))}
    </nav>
  )
}
