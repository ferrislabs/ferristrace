import { CircleCheck, Send, TriangleAlert } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { toast } from 'sonner'
import { because } from '@/auth/client'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter } from '@/components/ui/dialog'
import { Field, Input, NativeSelect } from '@/components/ui/input'
import { Segmented } from '@/components/ui/segmented'
import { Alert } from '@/components/ui/misc'
import { MetricChart } from '@/components/ft/MetricChart'
import {
  LLM_GROUPS,
  LLM_MEASURES,
  LOG_SEVERITIES,
  NO_DATA,
  OPS,
  SEVERITIES,
  TRACE_MEASURES,
  useChannels,
  useCreateRule,
  usePreviewRule,
  useTestRule,
  useUpdateRule,
  type Op,
  unitOf,
  type Rule,
  type SignalKind,
} from '@/data/alerts'
import { useMetricCatalogue } from '@/data/metrics'
import { useSchedules } from '@/data/oncall'
import { useProjects } from '@/data/projects'
import { useServiceNames } from '@/data/services'
import { useSettled } from '@/lib/settle'
import { BLANK, aggregationsFor, definitionFrom, formOf, type Form } from './form'

const KINDS: { value: SignalKind; label: string }[] = [
  { value: 'traces', label: 'Traces' },
  { value: 'logs', label: 'Logs' },
  { value: 'metrics', label: 'Metrics' },
  { value: 'llm', label: 'LLM spend' },
]

const HOUR_MS = 3_600_000

export function RuleEditor({ rule, open, onClose }: { rule: Rule | null; open: boolean; onClose: () => void }) {
  const projects = useProjects()
  const { channels } = useChannels()
  const { schedules } = useSchedules()
  const { names } = useServiceNames(HOUR_MS * 24)
  const create = useCreateRule()
  const update = useUpdateRule()
  const preview = usePreviewRule()
  const test = useTestRule()
  const [form, setForm] = useState<Form>(BLANK)
  const [shown, setShown] = useState(false)
  const metricQuery = useSettled(form.metricName)
  const { metrics } = useMetricCatalogue(metricQuery, form.project)

  useEffect(() => {
    if (open) {
      setForm(rule ? formOf(rule) : BLANK)
      preview.reset()
      test.reset()
      setShown(false)
    }
  }, [open, rule?.id])

  const set = (part: Partial<Form>) => {
    setForm((was) => ({ ...was, ...part }))
    setShown(false)
  }
  const metric = metrics.find((one) => one.name === form.metricName.trim())
  const allowed = useMemo(() => aggregationsFor(metric?.kind), [metric?.kind])
  useEffect(() => {
    if (form.kind === 'metrics' && !allowed.includes(form.aggregation)) setForm((was) => ({ ...was, aggregation: allowed[0] }))
  }, [allowed, form.kind, form.aggregation])
  const built = definitionFrom(form)
  const unit = 'definition' in built ? unitOf(built.definition.signal, metric?.unit) : form.kind === 'metrics' ? metric?.unit ?? '' : form.kind === 'llm' ? (form.llmMeasure === 'cost' ? '$' : form.llmMeasure === 'p95' ? 'ms' : form.llmMeasure) : form.kind === 'logs' ? 'events' : form.traceMeasure === 'count' ? 'events' : form.traceMeasure === 'error_rate' ? '%' : 'ms'
  const saving = create.isPending || update.isPending

  const toggle = <T,>(held: T[], one: T) => (held.includes(one) ? held.filter((x) => x !== one) : [...held, one])

  const runPreview = () => {
    if ('problem' in built) return toast.error(built.problem)
    setShown(true)
    preview.mutate(built.definition, { onError: (error) => toast.error(because(error, 'The preview failed')) })
  }

  const save = () => {
    if ('problem' in built) return toast.error(built.problem)
    const done = () => { toast.success(rule ? 'Rule saved' : 'Rule created', { description: built.definition.name }); onClose() }
    const failed = (error: unknown) => toast.error(because(error, 'The rule was not saved'))
    if (rule) update.mutate({ id: rule.id, definition: built.definition }, { onSuccess: done, onError: failed })
    else create.mutate(built.definition, { onSuccess: done, onError: failed })
  }

  const channelName = (id: string) => channels?.find((one) => one.id === id)?.name ?? id
  const result = shown && preview.data ? preview.data : null

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent size="lg" title={rule ? 'Edit alert rule' : 'New alert rule'} description="The rule is evaluated every minute over its window.">
        <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); save() }}>
          <div className="flex flex-wrap gap-3">
            <Field label="Name" htmlFor="ar-n" className="flex-[2_1_260px]"><Input id="ar-n" value={form.name} onChange={(e) => set({ name: e.target.value })} autoFocus /></Field>
            <Field label="Project" htmlFor="ar-p" className="flex-[1_1_180px]">
              <NativeSelect id="ar-p" value={form.project} onChange={(e) => set({ project: e.target.value })}>
                <option value="">All projects</option>
                {projects.map((one) => <option key={one.id} value={one.id}>{one.name}</option>)}
              </NativeSelect>
            </Field>
          </div>

          <Field label="Signal"><div><Segmented label="Signal" value={form.kind} onChange={(kind) => set({ kind })} options={KINDS} /></div></Field>

          {form.kind === 'traces' ? (
            <div className="flex flex-wrap gap-3">
              <Field label="Service" htmlFor="ar-s" className="flex-[1_1_160px]">
                <NativeSelect id="ar-s" value={form.service} onChange={(e) => set({ service: e.target.value })}>
                  <option value="">Any service</option>
                  {form.service && !names.includes(form.service) ? <option value={form.service}>{form.service}</option> : null}
                  {names.map((one) => <option key={one} value={one}>{one}</option>)}
                </NativeSelect>
              </Field>
              <Field label="Operation" htmlFor="ar-o" className="flex-[1_1_160px]"><Input id="ar-o" mono value={form.operation} placeholder="any" onChange={(e) => set({ operation: e.target.value })} /></Field>
              <Field label="Environment" htmlFor="ar-e" className="flex-[1_1_140px]"><Input id="ar-e" mono value={form.environment} placeholder="any" onChange={(e) => set({ environment: e.target.value })} /></Field>
              <Field label="Measure" htmlFor="ar-m" className="flex-[1_1_120px]">
                <NativeSelect id="ar-m" value={form.traceMeasure} onChange={(e) => set({ traceMeasure: e.target.value as Form['traceMeasure'] })}>
                  {TRACE_MEASURES.map((one) => <option key={one} value={one}>{one}</option>)}
                </NativeSelect>
              </Field>
            </div>
          ) : null}

          {form.kind === 'logs' ? (
            <div className="flex flex-col gap-3">
              <div className="flex flex-wrap gap-3">
                <Field label="Services" htmlFor="ar-ls" hint="Comma separated, empty for all." className="flex-[1_1_200px]"><Input id="ar-ls" mono value={form.logServices} onChange={(e) => set({ logServices: e.target.value })} /></Field>
                <Field label="Text" htmlFor="ar-lq" className="flex-[1_1_200px]"><Input id="ar-lq" value={form.logText} onChange={(e) => set({ logText: e.target.value })} /></Field>
              </div>
              <fieldset className="flex flex-wrap items-center gap-3">
                <legend className="mb-1 text-control font-semibold">Severities</legend>
                {LOG_SEVERITIES.map((one) => (
                  <label key={one} className="flex items-center gap-1.5 text-control">
                    <input type="checkbox" className="size-4 accent-[var(--primary)]" checked={form.logSeverities.includes(one)} onChange={() => set({ logSeverities: toggle(form.logSeverities, one) })} />
                    <span className="font-mono">{one}</span>
                  </label>
                ))}
              </fieldset>
              <Field label="Attributes" htmlFor="ar-la" hint="Comma separated key=value pairs. The measure is the count of matching records."><Input id="ar-la" mono value={form.logAttrs} onChange={(e) => set({ logAttrs: e.target.value })} /></Field>
            </div>
          ) : null}

          {form.kind === 'metrics' ? (
            <div className="flex flex-col gap-3">
              <div className="flex flex-wrap gap-3">
                <Field label="Metric" htmlFor="ar-mn" className="flex-[2_1_260px]" hint={metric ? `${metric.kind}${metric.unit ? `, ${metric.unit}` : ''}` : 'Pick from the catalogue.'}>
                  <Input id="ar-mn" mono list="ar-metrics" value={form.metricName} onChange={(e) => set({ metricName: e.target.value })} />
                  <datalist id="ar-metrics">{metrics.map((one) => <option key={one.name} value={one.name}>{one.kind}</option>)}</datalist>
                </Field>
                <Field label="Aggregation" htmlFor="ar-ma" className="flex-[1_1_120px]">
                  <NativeSelect id="ar-ma" value={form.aggregation} onChange={(e) => set({ aggregation: e.target.value as Form['aggregation'] })}>
                    {allowed.map((one) => <option key={one} value={one}>{one}</option>)}
                  </NativeSelect>
                </Field>
              </div>
              <div className="flex flex-wrap gap-3">
                <Field label="Group by" htmlFor="ar-mg" hint="Comma separated label keys. One state per series." className="flex-[1_1_220px]"><Input id="ar-mg" mono value={form.groupBy} onChange={(e) => set({ groupBy: e.target.value })} /></Field>
                <Field label="Filter" htmlFor="ar-mf" hint="Comma separated key=value pairs." className="flex-[1_1_220px]"><Input id="ar-mf" mono value={form.filter} onChange={(e) => set({ filter: e.target.value })} /></Field>
              </div>
            </div>
          ) : null}

          {form.kind === 'llm' ? (
            <div className="flex flex-col gap-3">
              <div className="flex flex-wrap gap-3">
                <Field label="Measure" htmlFor="ar-lm" hint={form.llmMeasure === 'cost' ? 'Priced calls only, in dollars. The threshold is in dollars.' : undefined} className="flex-[1_1_140px]">
                  <NativeSelect id="ar-lm" value={form.llmMeasure} onChange={(e) => set({ llmMeasure: e.target.value as Form['llmMeasure'] })}>
                    {LLM_MEASURES.map((one) => <option key={one} value={one}>{one}</option>)}
                  </NativeSelect>
                </Field>
                <Field label="Group by" htmlFor="ar-lg" hint="One state per group." className="flex-[1_1_140px]">
                  <NativeSelect id="ar-lg" value={form.llmGroup} onChange={(e) => set({ llmGroup: e.target.value as Form['llmGroup'] })}>
                    {LLM_GROUPS.map((one) => <option key={one} value={one}>{one}</option>)}
                  </NativeSelect>
                </Field>
              </div>
              <div className="flex flex-wrap gap-3">
                <Field label="Feature" htmlFor="ar-lf" className="flex-[1_1_160px]"><Input id="ar-lf" mono value={form.llmFeature} placeholder="any" onChange={(e) => set({ llmFeature: e.target.value })} /></Field>
                <Field label="Customer" htmlFor="ar-lc" className="flex-[1_1_160px]"><Input id="ar-lc" mono value={form.llmCustomer} placeholder="any" onChange={(e) => set({ llmCustomer: e.target.value })} /></Field>
                <Field label="Model" htmlFor="ar-lmo" className="flex-[1_1_160px]"><Input id="ar-lmo" mono value={form.llmModel} placeholder="any" onChange={(e) => set({ llmModel: e.target.value })} /></Field>
              </div>
            </div>
          ) : null}

          <div className="flex flex-wrap items-end gap-3">
            <Field label="Condition" className="flex-none">
              <div className="flex items-center gap-2">
                <NativeSelect aria-label="Operator" value={form.op} onChange={(e) => set({ op: e.target.value as Op })} className="w-20">{OPS.map((one) => <option key={one} value={one}>{one}</option>)}</NativeSelect>
                <Input aria-label="Threshold" mono inputMode="decimal" value={form.threshold} onChange={(e) => set({ threshold: e.target.value })} className="w-28" />
                {unit ? <span className="text-control text-muted-foreground">{unit}</span> : null}
              </div>
            </Field>
            <Field label="Window (min)" htmlFor="ar-w" className="w-28"><Input id="ar-w" mono inputMode="decimal" value={form.windowMin} onChange={(e) => set({ windowMin: e.target.value })} /></Field>
            <Field label="Pending (min)" htmlFor="ar-pd" className="w-28"><Input id="ar-pd" mono inputMode="decimal" value={form.pendingMin} onChange={(e) => set({ pendingMin: e.target.value })} /></Field>
          </div>

          <div className="flex flex-wrap gap-4">
            <Field label="Severity"><div><Segmented label="Severity" value={form.severity} onChange={(severity) => set({ severity })} options={SEVERITIES.map((one) => ({ value: one, label: one }))} /></div></Field>
            <Field label="When there is no data" htmlFor="ar-nd">
              <NativeSelect id="ar-nd" value={form.noData} onChange={(e) => set({ noData: e.target.value as Form['noData'] })} className="w-40">
                {NO_DATA.map((one) => <option key={one} value={one}>{one === 'no_data' ? 'no data state' : one}</option>)}
              </NativeSelect>
            </Field>
          </div>

          <fieldset className="flex flex-col gap-2">
            <legend className="mb-1 text-control font-semibold">Channels</legend>
            {channels && channels.length > 0 ? (
              <div className="flex flex-wrap gap-x-4 gap-y-1.5">
                {channels.map((one) => (
                  <label key={one.id} className="flex items-center gap-1.5 text-control">
                    <input type="checkbox" className="size-4 accent-[var(--primary)]" checked={form.channels.includes(one.id)} onChange={() => set({ channels: toggle(form.channels, one.id) })} />
                    <span>{one.name}</span><Badge variant="outline">{one.kind}</Badge>
                  </label>
                ))}
              </div>
            ) : <span className="text-control text-muted-foreground">No channel yet. Add one in Settings, Project, Alerts.</span>}
          </fieldset>

          <Field label="Page the on-call of" htmlFor="ar-pg" hint={schedules.length > 0 ? 'When the rule starts firing, the person on call is paged by email and escalated until someone acknowledges.' : 'No schedule yet. Add one in Settings, Teams, On-call.'}>
            <NativeSelect id="ar-pg" value={form.page} onChange={(e) => set({ page: e.target.value })} className="max-w-[360px]">
              <option value="">Nobody</option>
              {schedules.map((one) => <option key={one.id} value={one.id}>{one.teamName} · {one.name}</option>)}
              {form.page && !schedules.some((one) => one.id === form.page) ? <option value={form.page}>A schedule out of reach</option> : null}
            </NativeSelect>
          </Field>

          <div className="flex flex-col gap-2 rounded-lg border p-3">
            <div className="flex flex-wrap items-center gap-2">
              <span className="text-control font-semibold">Preview</span>
              {result ? (result.would_fire ? <Badge variant="destructive"><TriangleAlert />Would fire</Badge> : <Badge variant="success"><CircleCheck />Would not fire</Badge>) : null}
              <Button type="button" size="sm" className="ml-auto" onClick={runPreview} disabled={preview.isPending}>{preview.isPending ? 'Evaluating' : 'Preview'}</Button>
            </div>
            {result ? (
              result.points.some((point) => point[1] !== null)
                ? <MetricChart series={[{ labels: {}, points: result.points }]} exemplars={[]} unit={unit} aria="Preview of the rule over its last window" threshold={result.threshold} />
                : <span className="text-control text-muted-foreground">No data in the last window.</span>
            ) : <span className="text-caption text-muted-foreground">Evaluates this definition over its last window without saving it. The dashed line is the threshold.{unit ? ` Values in ${unit}.` : ''}</span>}
          </div>

          {rule ? (
            <div className="flex flex-col gap-2 rounded-lg border p-3">
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-control font-semibold">Test notification</span>
                <Button type="button" size="sm" className="ml-auto" disabled={test.isPending} onClick={() => test.mutate(rule.id, { onError: (error) => toast.error(because(error, 'The test was not sent')) })}><Send />Send test notification</Button>
              </div>
              {test.data ? (
                test.data.deliveries.length === 0
                  ? <span className="text-control text-muted-foreground">The saved rule has no channel.</span>
                  : <ul className="m-0 flex list-none flex-col gap-1 p-0">{test.data.deliveries.map((one) => (
                    <li key={one.channel} className="flex flex-wrap items-center gap-2 text-control">
                      <Badge variant={one.status === 'sent' ? 'success' : 'destructive'}>{one.status}</Badge>
                      <span className="font-semibold">{channelName(one.channel)}</span>
                      {one.error ? <span className="text-destructive-text">{one.error}</span> : null}
                    </li>
                  ))}</ul>
              ) : <span className="text-caption text-muted-foreground">Sends to the channels of the saved rule, not to unsaved changes.</span>}
            </div>
          ) : null}

          {'problem' in built && form.name ? <Alert variant="warning" icon={<TriangleAlert />}>{built.problem}</Alert> : null}
          <DialogFooter>
            <Button type="button" onClick={onClose}>Cancel</Button>
            <Button variant="primary" type="submit" disabled={saving || 'problem' in built}>{rule ? 'Save rule' : 'Create rule'}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
