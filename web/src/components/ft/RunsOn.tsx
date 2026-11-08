import { Boxes, Container, Server } from 'lucide-react'
import { Link } from '@/lib/router'
import { Card, CardHeader, CardTitle } from '@/components/ui/card'
import { useInfrastructure, type InfraResource } from '@/data/infra'

const ICON = { cluster: Boxes, host: Server, container: Container } as const

function Target({ resource }: { resource: InfraResource }) {
  const Icon = ICON[resource.kind]
  const body = (
    <>
      <Icon className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
      <span className="truncate font-mono">{resource.name}</span>
      <span className="text-caption text-muted-foreground">{resource.kind}</span>
      {resource.silent ? <span className="text-caption font-semibold text-warning-text">silent</span> : null}
    </>
  )
  const cls = 'flex items-center gap-2 border-b border-border-subtle px-4 py-2 text-control text-foreground no-underline hover:bg-muted'
  if (resource.kind === 'cluster') return <Link to="/infra/kubernetes" search={{ cluster: resource.name }} className={cls}>{body}</Link>
  return <Link to="/infra" search={{ kind: resource.kind }} className={cls}>{body}</Link>
}

export function RunsOn({ service }: { service: string }) {
  const { resources } = useInfrastructure()
  const found = resources.filter((resource) => resource.services.includes(service))
  if (found.length === 0) return null
  return (
    <Card className="overflow-hidden">
      <CardHeader><CardTitle className="flex-1">Runs on</CardTitle><span className="text-caption text-muted-foreground">seen on its spans in the last 10 minutes</span></CardHeader>
      {found.map((resource) => <Target key={`${resource.kind}/${resource.id}`} resource={resource} />)}
    </Card>
  )
}
