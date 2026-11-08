import { useNavigate as useRouterNavigate } from '@tanstack/react-router'
import { Boxes, Container, Cloud, CircleAlert, Server } from 'lucide-react'
import { useMemo } from 'react'
import { Link, useSearchParam } from '@/lib/router'
import { Card } from '@/components/ui/card'
import { NativeSelect } from '@/components/ui/input'
import { Segmented } from '@/components/ui/segmented'
import { Table, Td, Th } from '@/components/ui/misc'
import { EmptyState, PageHeader } from '@/components/ft/bits'
import { Seen } from '@/pages/Infra'
import { bytes, collectorConfig, KIND_LABEL, RECEIVER, useCluster, useInfrastructure, type ClusterPod, type InfraResource, type ResourceKind } from '@/data/infra'
import { ApiError, because } from '@/auth/client'
import { cn } from '@/lib/utils'

const HERE = '/infra'
const KINDS: ResourceKind[] = ['cluster', 'host', 'container']
type KindFilter = ResourceKind | 'all'
type StatusFilter = 'all' | 'active' | 'silent'

function useOverviewSearch() {
  const navigate = useRouterNavigate()
  const read = {
    kind: (useSearchParam('kind') as KindFilter | undefined) ?? 'all',
    provider: useSearchParam('provider') ?? '',
    region: useSearchParam('region') ?? '',
    environment: useSearchParam('environment') ?? '',
    status: (useSearchParam('status') as StatusFilter | undefined) ?? 'all',
  }
  const set = (part: Record<string, string | undefined>) =>
    void navigate({
      to: HERE as never,
      replace: true,
      search: ((was: Record<string, unknown>) => {
        const next: Record<string, unknown> = { ...was, ...part }
        for (const key of Object.keys(next)) if (next[key] === undefined || next[key] === '' || next[key] === 'all') delete next[key]
        return next
      }) as never,
    })
  return { ...read, set }
}

const distinct = (values: (string | null)[]) => [...new Set(values.filter((value): value is string => Boolean(value)))].sort()

function Status({ resource }: { resource: InfraResource }) {
  if (resource.silent) {
    return (
      <span className="inline-flex items-center gap-1.5 font-semibold text-warning-text">
        <CircleAlert className="size-3.5" aria-hidden />
        Silent
        <span className="font-normal text-muted-foreground">· <Seen ms={resource.last_seen_at_ms} /></span>
      </span>
    )
  }
  return (
    <span className="inline-flex items-center gap-1.5 text-muted-foreground">
      <span aria-hidden className="size-1.5 rounded-full bg-status-ok" />
      Active
    </span>
  )
}

function Where({ resource }: { resource: InfraResource }) {
  const parts = [resource.provider, resource.region, resource.host_type].filter(Boolean)
  return parts.length ? <span className="font-mono text-muted-foreground">{parts.join(' · ')}</span> : <span className="text-faint-foreground">—</span>
}

function Muted({ children }: { children: React.ReactNode }) {
  return children ? <span className="text-muted-foreground">{children}</span> : <span className="text-faint-foreground">—</span>
}

function ClusterCard({ resource, pods }: { resource: InfraResource; pods: ClusterPod[] }) {
  const ill = pods.filter((pod) => pod.unhealthy).length
  const tone = resource.silent ? 'text-warning-text' : ill ? 'text-destructive-text' : 'text-success-text'
  const verdict = resource.silent ? 'Silent' : ill ? `${ill} unhealthy pod${ill === 1 ? '' : 's'}` : 'Healthy'
  return (
    <Link
      to="/infra/kubernetes"
      search={{ cluster: resource.name }}
      aria-label={`Open cluster ${resource.name}`}
      className="flex min-w-0 flex-col gap-3 rounded-lg border bg-card px-4 py-3.5 text-foreground no-underline hover:border-foreground/30 focus-visible:outline-2 focus-visible:outline-ring"
    >
      <span className="flex items-center gap-2">
        <Boxes className="size-4 shrink-0 text-muted-foreground" aria-hidden />
        <span className="truncate font-mono text-[13px] font-semibold">{resource.name}</span>
        {resource.environment ? <span className="rounded-sm border px-1.5 font-mono text-[10.5px] text-muted-foreground">{resource.environment}</span> : null}
      </span>
      <span className={cn('inline-flex items-center gap-1.5 text-control font-semibold', tone)}>
        {resource.silent || ill ? <CircleAlert className="size-3.5" aria-hidden /> : <span aria-hidden className="size-1.5 rounded-full bg-status-ok" />}
        {verdict}
      </span>
      <span className="flex items-baseline gap-4 text-control text-muted-foreground">
        <span><span className="tabular text-[20px] font-semibold text-foreground">{resource.silent ? '—' : pods.length}</span> pods</span>
        {resource.provider || resource.region ? <span className="truncate font-mono">{[resource.provider, resource.region].filter(Boolean).join(' · ')}</span> : null}
      </span>
      <span className="text-[11.5px] text-faint-foreground">{resource.silent ? <Seen ms={resource.last_seen_at_ms} /> : 'Open the Kubernetes view'}</span>
    </Link>
  )
}

function Section({ id, title, icon, count, children }: { id: string; title: string; icon: React.ReactNode; count: number; children: React.ReactNode }) {
  return (
    <section aria-labelledby={id} className="flex flex-col gap-3">
      <h2 id={id} className="text-heading flex items-center gap-2 [&_svg]:size-4 [&_svg]:text-muted-foreground">
        {icon}
        {title}
        <span className="font-normal text-muted-foreground">{count}</span>
      </h2>
      {children}
    </section>
  )
}

function HostTable({ hosts }: { hosts: InfraResource[] }) {
  return (
    <Card className="overflow-x-auto">
      <Table aria-label="Hosts" className="min-w-[640px]">
        <thead>
          <tr>
            <Th className="min-w-[200px]">Host</Th>
            <Th>Where</Th>
            <Th className="w-[130px]">Environment</Th>
            <Th className="w-[200px]">Status</Th>
            <Th className="w-[130px]"><span className="sr-only">Open</span></Th>
          </tr>
        </thead>
        <tbody>
          {hosts.map((host) => (
            <tr key={host.id}>
              <Td className="font-mono">{host.name}</Td>
              <Td><Where resource={host} /></Td>
              <Td><Muted>{host.environment}</Muted></Td>
              <Td><Status resource={host} /></Td>
              <Td><Link to="/backend/system" className="text-control text-accent-text no-underline hover:underline" aria-label={`System metrics for ${host.name}`}>System metrics</Link></Td>
            </tr>
          ))}
        </tbody>
      </Table>
    </Card>
  )
}

function ContainerTable({ containers }: { containers: InfraResource[] }) {
  return (
    <Card className="overflow-x-auto">
      <Table aria-label="Containers" className="min-w-[700px]">
        <thead>
          <tr>
            <Th className="min-w-[200px]">Container</Th>
            <Th>Where</Th>
            <Th className="w-[130px]">Environment</Th>
            <Th className="w-[80px]">CPU</Th>
            <Th className="w-[90px]">Memory</Th>
            <Th className="w-[200px]">Status</Th>
          </tr>
        </thead>
        <tbody>
          {containers.map((container) => (
            <tr key={container.id}>
              <Td className="font-mono">{container.name}</Td>
              <Td><Where resource={container} /></Td>
              <Td><Muted>{container.environment}</Muted></Td>
              <Td className="tabular">{container.cpu_percent === null ? '—' : `${container.cpu_percent.toFixed(1)} %`}</Td>
              <Td className="tabular">{bytes(container.memory_bytes)}</Td>
              <Td><Status resource={container} /></Td>
            </tr>
          ))}
        </tbody>
      </Table>
    </Card>
  )
}

function NothingReporting() {
  return (
    <EmptyState icon={<Boxes />} title="No infrastructure is reporting" code={collectorConfig(window.location.origin)}>
      A cluster, a host or a container shows up as soon as its OpenTelemetry Collector sends metrics for it. Clusters come from {RECEIVER.cluster}. Hosts come from {RECEIVER.host}. Containers come from {RECEIVER.container}.
    </EmptyState>
  )
}

export function InfraOverview() {
  const { kind, provider, region, environment, status, set } = useOverviewSearch()
  const { resources, pending, failed } = useInfrastructure()
  const { view } = useCluster('', '')

  const providers = useMemo(() => distinct(resources.map((r) => r.provider)), [resources])
  const regions = useMemo(() => distinct(resources.map((r) => r.region)), [resources])
  const environments = useMemo(() => distinct(resources.map((r) => r.environment)), [resources])
  const shown = useMemo(
    () => resources.filter((r) =>
      (kind === 'all' || r.kind === kind)
      && (!provider || r.provider === provider)
      && (!region || r.region === region)
      && (!environment || r.environment === environment)
      && (status === 'all' || (status === 'silent') === r.silent)),
    [resources, kind, provider, region, environment, status],
  )
  const silent = resources.filter((r) => r.silent).length
  const of = (wanted: ResourceKind) => shown.filter((r) => r.kind === wanted)
  const count = (wanted: ResourceKind) => resources.filter((r) => r.kind === wanted).length

  const header = (
    <PageHeader
      title="Infrastructure"
      meta={resources.length ? (
        <span className="inline-flex flex-wrap items-center gap-x-1.5">
          <span className={cn(silent ? 'font-semibold text-warning-text' : 'text-success-text')}>{silent ? `${silent} silent` : 'All reporting'}</span>
          <span aria-hidden>·</span><span>{resources.length} resource{resources.length === 1 ? '' : 's'}</span>
        </span>
      ) : undefined}
    />
  )

  if (failed) {
    const unconfigured = failed instanceof ApiError && failed.status === 503
    return (
      <>
        {header}
        <EmptyState icon={<Boxes />} title={unconfigured ? 'No metric store is configured' : 'The infrastructure could not be read'}>{because(failed, 'Try again in a moment.')}</EmptyState>
      </>
    )
  }
  if (pending) return header
  if (resources.length === 0) {
    return (
      <>
        {header}
        <NothingReporting />
      </>
    )
  }

  return (
    <>
      {header}
      <div className="flex flex-wrap items-center gap-2">
        <Segmented
          label="Kind"
          value={kind}
          onChange={(value) => set({ kind: value })}
          options={[{ value: 'all', label: `All ${resources.length}` }, ...KINDS.filter(count).map((value) => ({ value, label: `${KIND_LABEL[value]} ${count(value)}` }))]}
        />
        <Segmented
          label="Status"
          value={status}
          onChange={(value) => set({ status: value })}
          options={[{ value: 'all', label: 'Any status' }, { value: 'active', label: 'Active' }, { value: 'silent', label: 'Silent' }]}
        />
        <span className="flex-1" />
        {providers.length ? (
          <NativeSelect aria-label="Provider" value={provider} onChange={(e) => set({ provider: e.target.value || undefined })} className="w-[140px]">
            <option value="">All providers</option>
            {providers.map((name) => <option key={name} value={name}>{name}</option>)}
          </NativeSelect>
        ) : null}
        {regions.length ? (
          <NativeSelect aria-label="Region" value={region} onChange={(e) => set({ region: e.target.value || undefined })} className="w-[150px] font-mono">
            <option value="">All regions</option>
            {regions.map((name) => <option key={name} value={name}>{name}</option>)}
          </NativeSelect>
        ) : null}
        {environments.length ? (
          <NativeSelect aria-label="Environment" value={environment} onChange={(e) => set({ environment: e.target.value || undefined })} className="w-[150px]">
            <option value="">All environments</option>
            {environments.map((name) => <option key={name} value={name}>{name}</option>)}
          </NativeSelect>
        ) : null}
      </div>
      <p className="sr-only" aria-live="polite">{shown.length} of {resources.length} resources shown</p>

      {shown.length === 0 ? (
        <EmptyState icon={<Cloud />} title="Nothing matches these filters">Clear a filter to see the other resources.</EmptyState>
      ) : null}
      {of('cluster').length ? (
        <Section id="clusters" title="Clusters" icon={<Boxes aria-hidden />} count={of('cluster').length}>
          <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
            {of('cluster').map((cluster) => (
              <ClusterCard key={cluster.id} resource={cluster} pods={view.pods.filter((pod) => pod.cluster === cluster.name)} />
            ))}
          </div>
        </Section>
      ) : null}
      {of('host').length ? (
        <Section id="hosts" title="Hosts" icon={<Server aria-hidden />} count={of('host').length}>
          <HostTable hosts={of('host')} />
        </Section>
      ) : null}
      {of('container').length ? (
        <Section id="containers" title="Containers" icon={<Container aria-hidden />} count={of('container').length}>
          <ContainerTable containers={of('container')} />
        </Section>
      ) : null}
    </>
  )
}
