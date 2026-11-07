import {
  Activity, ArrowLeftRight, BellRing, Boxes, CircleAlert, Sparkles, Rocket, Cpu, Database, House, Layers, LayoutGrid, LineChart, Mail, ScrollText, Server, ShieldAlert, Siren, Telescope, Waypoints, Zap, type LucideIcon,
} from 'lucide-react'

export type NavItem = { label: string; to: string; icon: LucideIcon; keys?: string; badge?: boolean }
export type NavAxis = { key: string; label: string; icon: LucideIcon; tabs: NavItem[] }

export const AXES: NavAxis[] = [
  {
    key: 'home', label: 'Home', icon: House,
    tabs: [
      { label: 'Overview', to: '/', icon: House, keys: 'O' },
      { label: 'Services', to: '/services', icon: LayoutGrid, keys: 'V' },
      { label: 'Issues', to: '/issues', icon: CircleAlert, keys: 'I', badge: true },
      { label: 'Releases', to: '/releases', icon: Rocket, keys: 'R' },
      { label: 'AI', to: '/ai', icon: Sparkles, keys: 'C' },
    ],
  },
  {
    key: 'explore', label: 'Explore', icon: Telescope,
    tabs: [
      { label: 'Traces', to: '/traces', icon: Waypoints, keys: 'T' },
      { label: 'Logs', to: '/logs', icon: ScrollText, keys: 'L' },
      { label: 'Metrics', to: '/metrics', icon: LineChart, keys: 'M' },
    ],
  },
  {
    key: 'backend', label: 'Backend', icon: Server,
    tabs: [
      { label: 'Requests', to: '/backend/requests', icon: ArrowLeftRight },
      { label: 'Jobs', to: '/backend/jobs', icon: Layers },
      { label: 'Cache', to: '/backend/cache', icon: Zap },
      { label: 'Emails', to: '/backend/emails', icon: Mail },
      { label: 'Queries', to: '/backend/queries', icon: Database },
      { label: 'System metrics', to: '/backend/system', icon: Cpu },
    ],
  },
  {
    key: 'infra', label: 'Infra', icon: Boxes,
    tabs: [{ label: 'Kubernetes', to: '/infra/kubernetes', icon: Boxes }],
  },
  {
    key: 'reliability', label: 'Reliability', icon: ShieldAlert,
    tabs: [
      { label: 'Alerts', to: '/alerts', icon: BellRing, keys: 'A', badge: true },
      { label: 'Incidents', to: '/incidents', icon: Siren, keys: 'N', badge: true },
      { label: 'Status page', to: '/status-page', icon: Activity, keys: 'P' },
    ],
  },
]

export const ALL_NAV = AXES.flatMap((axis) => axis.tabs)

export const axisOf = (matches: (to: string) => boolean): NavAxis | undefined =>
  AXES.find((axis) => axis.tabs.some((tab) => matches(tab.to)))
