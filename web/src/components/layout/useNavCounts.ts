import { useAlerts } from '@/data/alerts'
import { useIncidents } from '@/data/incidents'
import { useIssues } from '@/data/issues'

export function useNavCounts(): Record<string, number> {
  const { issues } = useIssues()
  const { alerts } = useAlerts()
  const ongoing = useIncidents('open').incidents?.length ?? 0
  return {
    '/issues': issues.filter((x) => x.status === 'unresolved' || x.regressed).length,
    '/alerts': (alerts ?? []).filter((r) => r.enabled && r.state.status === 'firing').length,
    '/incidents': ongoing,
  }
}
