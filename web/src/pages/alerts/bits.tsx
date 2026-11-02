import { Badge } from '@/components/ui/badge'
import type { AlertStatus } from '@/data/alerts'

const VARIANT: Record<AlertStatus, 'destructive' | 'warning' | 'neutral' | 'success'> = {
  firing: 'destructive',
  pending: 'warning',
  no_data: 'neutral',
  ok: 'success',
}

export const StateBadge = ({ status }: { status: AlertStatus }) => <Badge variant={VARIANT[status]}>{status.replace('_', ' ')}</Badge>
