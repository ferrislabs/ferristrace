import {
  Activity, ArrowLeftRight, BellRing, Boxes, Bug, CircleAlert, Sparkles, Rocket, Cpu, Database, Globe, House, Layers, LayoutGrid, LineChart, Mail, MonitorSmartphone, Package, PanelTop, ScrollText, Siren, Users, Waypoints, Zap, type LucideIcon,
} from 'lucide-react'

export type NavItem = { label: string; to: string; icon: LucideIcon; keys?: string; badge?: boolean }
export type NavGroup = { label: string | null; collapsible?: boolean; items: NavItem[] }

export const NAV: NavGroup[] = [
  { label: null, items: [{ label: 'Overview', to: '/', icon: House, keys: 'O' }, { label: 'Services', to: '/services', icon: LayoutGrid, keys: 'V' }, { label: 'Issues', to: '/issues', icon: CircleAlert, keys: 'I', badge: true }, { label: 'Releases', to: '/releases', icon: Rocket, keys: 'R' }, { label: 'AI', to: '/ai', icon: Sparkles, keys: 'C' }] },
  { label: 'Observability', items: [{ label: 'Traces', to: '/traces', icon: Waypoints, keys: 'T' }, { label: 'Logs', to: '/logs', icon: ScrollText, keys: 'L' }, { label: 'Metrics', to: '/metrics', icon: LineChart, keys: 'M' }] },
  {
    label: 'Backend', collapsible: true,
    items: [
      { label: 'Requests', to: '/backend/requests', icon: ArrowLeftRight }, { label: 'Jobs', to: '/backend/jobs', icon: Layers }, { label: 'Cache', to: '/backend/cache', icon: Zap },
      { label: 'Emails', to: '/backend/emails', icon: Mail }, { label: 'Queries', to: '/backend/queries', icon: Database }, { label: 'System metrics', to: '/backend/system', icon: Cpu },
    ],
  },
  {
    label: 'Frontend', collapsible: true,
    items: [
      { label: 'Pages', to: '/frontend/pages', icon: PanelTop }, { label: 'Errors', to: '/frontend/errors', icon: Bug }, { label: 'Network', to: '/frontend/network', icon: Globe },
      { label: 'Sessions', to: '/frontend/sessions', icon: Users }, { label: 'Resources', to: '/frontend/resources', icon: Package }, { label: 'Browsers & devices', to: '/frontend/devices', icon: MonitorSmartphone },
    ],
  },
  { label: 'Infrastructure', collapsible: true, items: [{ label: 'Kubernetes', to: '/infra/kubernetes', icon: Boxes }] },
  { label: 'Reliability', items: [{ label: 'Alerts', to: '/alerts', icon: BellRing, keys: 'A', badge: true }, { label: 'Incidents', to: '/incidents', icon: Siren, keys: 'N', badge: true }, { label: 'Status page', to: '/status-page', icon: Activity, keys: 'P' }] },
]
export const ALL_NAV = NAV.flatMap((g) => g.items)
