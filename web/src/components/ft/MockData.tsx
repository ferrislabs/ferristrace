import { FlaskConical } from 'lucide-react'
import { Badge } from '@/components/ui/badge'

export function MockData({ className }: { className?: string }) {
  return (
    <Badge variant="warning" className={className} title="Sample data bundled with the app. Nothing here comes from the server yet.">
      <FlaskConical />
      Mock data
    </Badge>
  )
}
