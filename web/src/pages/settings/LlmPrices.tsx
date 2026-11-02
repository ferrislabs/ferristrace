import { Plus } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { because } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Dialog, DialogContent, DialogFooter } from '@/components/ui/dialog'
import { Field, Input } from '@/components/ui/input'
import { Table, Td, Th, Tr } from '@/components/ui/misc'
import {
  dollarsToMicros,
  formatRate,
  modelProblem,
  providerProblem,
  rateProblem,
  useAddPrice,
  useDeletePrice,
  usePrices,
  useUnpriced,
  type ModelPrice,
} from '@/data/llm'
import { num, stamp } from '@/lib/format'
import { H } from './parts'

type Form = { provider: string; model: string; input: string; output: string; cached: string; from: string }

const EMPTY: Form = { provider: '', model: '', input: '', output: '', cached: '', from: '' }

const fromMs = (date: string) => (date === '' ? 0 : Date.parse(`${date}T00:00:00Z`))

export function LlmPrices() {
  const { active } = useOrganisations()
  const { prices, nothing } = usePrices()
  const unpriced = useUnpriced() ?? []
  const add = useAddPrice()
  const remove = useDeletePrice()
  const [form, setForm] = useState<Form | null>(null)
  const [removing, setRemoving] = useState<ModelPrice | null>(null)
  const mayChange = active?.role === 'owner' || active?.role === 'admin'

  const set = (part: Partial<Form>) => setForm((now) => (now ? { ...now, ...part } : now))
  const problems = form
    ? [providerProblem(form.provider), modelProblem(form.model), rateProblem(form.input), rateProblem(form.output), rateProblem(form.cached, true), form.from !== '' && Number.isNaN(Date.parse(form.from)) ? 'the date is not a date' : null]
    : []
  const invalid = problems.some((one) => one !== null)
  const problem = problems.find((one) => one !== null) ?? null
  const touched = form !== null && (form.provider !== '' || form.model !== '' || form.input !== '' || form.output !== '')

  return (
    <>
      <H title="LLM prices" sub="What each model costs per million tokens, from an effective date. A call of a model without a price is shown as unpriced, never as free.">
        {mayChange ? <Button variant="primary" size="sm" onClick={() => setForm(EMPTY)}><Plus />Add price</Button> : null}
      </H>
      {mayChange ? null : <span className="text-control text-muted-foreground">Only an owner or an administrator can add or delete a price.</span>}
      <Card className="overflow-x-auto">
        {prices ? (
          <Table aria-label="Model prices">
            <thead><tr><Th className="w-[110px]">Provider</Th><Th>Model</Th><Th numeric className="w-[110px]">Input / M</Th><Th numeric className="w-[110px]">Output / M</Th><Th numeric className="w-[120px]">Cached / M</Th><Th className="w-[170px]">Effective from</Th>{mayChange ? <Th className="w-[90px]"><span className="sr-only">Actions</span></Th> : null}</tr></thead>
            <tbody>
              {prices.length === 0 ? <Tr><Td colSpan={mayChange ? 7 : 6} className="text-muted-foreground">No price yet. Add the models you use; a model ending in * prices every model with that prefix.</Td></Tr> : null}
              {prices.map((p) => (
                <Tr key={p.id}>
                  <Td className="font-mono">{p.provider}</Td>
                  <Td className="font-mono font-semibold">{p.model}</Td>
                  <Td numeric className="font-mono">{formatRate(p.input)}</Td>
                  <Td numeric className="font-mono">{formatRate(p.output)}</Td>
                  <Td numeric className="font-mono">{p.cachedInput === null ? <span className="text-muted-foreground" title="Cached tokens cost the input price">input price</span> : formatRate(p.cachedInput)}</Td>
                  <Td className="font-mono">{p.effectiveFromMs === 0 ? 'always' : stamp(p.effectiveFromMs)}</Td>
                  {mayChange ? <Td numeric><Button variant="ghost" size="sm" className="text-destructive-text" onClick={() => setRemoving(p)}>Delete</Button></Td> : null}
                </Tr>
              ))}
            </tbody>
          </Table>
        ) : <div className="px-4 py-3 text-control text-muted-foreground">{nothing}</div>}
      </Card>
      <H title="Models without a price" sub="Seen on calls in the last 30 days. Their cost is not counted until you price them." />
      <Card className="overflow-x-auto">
        <Table aria-label="Models without a price">
          <thead><tr><Th className="w-[110px]">Provider</Th><Th>Model</Th><Th numeric className="w-[100px]">Calls</Th><Th className="w-[170px]">Last seen</Th>{mayChange ? <Th className="w-[140px]"><span className="sr-only">Actions</span></Th> : null}</tr></thead>
          <tbody>
            {unpriced.length === 0 ? <Tr><Td colSpan={mayChange ? 5 : 4} className="text-muted-foreground">Every model seen on a call has a price.</Td></Tr> : null}
            {unpriced.map((m) => (
              <Tr key={`${m.provider}/${m.model}`}>
                <Td className="font-mono">{m.provider || '—'}</Td>
                <Td className="font-mono font-semibold">{m.model}</Td>
                <Td numeric>{num(m.calls)}</Td>
                <Td className="font-mono">{stamp(m.lastSeenMs)}</Td>
                {mayChange ? <Td numeric><Button size="sm" onClick={() => setForm({ ...EMPTY, provider: m.provider, model: m.model })}>Add a price</Button></Td> : null}
              </Tr>
            ))}
          </tbody>
        </Table>
      </Card>
      <Dialog open={form !== null} onOpenChange={(o) => !o && setForm(null)}>
        <DialogContent size="sm" title="Add a price" description="Dollars per million tokens. The price applies to calls from its effective date on.">
          {form ? (
            <form
              className="flex flex-col gap-4"
              onSubmit={(e) => {
                e.preventDefault()
                add.mutate(
                  {
                    provider: form.provider.trim(),
                    model: form.model.trim(),
                    input: dollarsToMicros(Number(form.input)),
                    output: dollarsToMicros(Number(form.output)),
                    cachedInput: form.cached.trim() === '' ? null : dollarsToMicros(Number(form.cached)),
                    effectiveFromMs: fromMs(form.from),
                  },
                  { onSuccess: () => { setForm(null); toast.success('Price added') }, onError: (error) => toast.error(because(error, 'The price was not added')) },
                )
              }}
            >
              <Field label="Provider" htmlFor="lp-p" hint="As the spans name it, like openai or anthropic."><Input id="lp-p" value={form.provider} onChange={(e) => set({ provider: e.target.value })} autoFocus={form.provider === ''} /></Field>
              <Field label="Model" htmlFor="lp-m" hint="An exact model, or a prefix ending in *, like gpt-4o*."><Input id="lp-m" mono value={form.model} onChange={(e) => set({ model: e.target.value })} autoFocus={form.provider !== ''} /></Field>
              <div className="grid grid-cols-3 gap-3">
                <Field label="Input $ / M" htmlFor="lp-i"><Input id="lp-i" inputMode="decimal" value={form.input} onChange={(e) => set({ input: e.target.value })} /></Field>
                <Field label="Output $ / M" htmlFor="lp-o"><Input id="lp-o" inputMode="decimal" value={form.output} onChange={(e) => set({ output: e.target.value })} /></Field>
                <Field label="Cached $ / M" htmlFor="lp-c" hint="Blank: the input price."><Input id="lp-c" inputMode="decimal" value={form.cached} onChange={(e) => set({ cached: e.target.value })} /></Field>
              </div>
              <Field label="Effective from" htmlFor="lp-d" hint="Blank: from the beginning."><Input id="lp-d" type="date" value={form.from} onChange={(e) => set({ from: e.target.value })} /></Field>
              {problem && touched ? <span className="text-caption text-destructive-text">{problem}</span> : null}
              <DialogFooter><Button type="button" onClick={() => setForm(null)}>Cancel</Button><Button variant="primary" type="submit" disabled={add.isPending || invalid}>Add price</Button></DialogFooter>
            </form>
          ) : null}
        </DialogContent>
      </Dialog>
      <Dialog open={removing !== null} onOpenChange={(o) => !o && setRemoving(null)}>
        <DialogContent role="alertdialog" title={`Delete the price of ${removing?.model ?? 'this model'}?`} description="Calls it priced are shown as unpriced again, unless an earlier price covers them. This is recorded in the audit log.">
          <DialogFooter>
            <Button onClick={() => setRemoving(null)}>Cancel</Button>
            <Button
              variant="destructive"
              disabled={remove.isPending}
              onClick={() => {
                if (!removing) return
                remove.mutate(removing.id, {
                  onSuccess: () => { setRemoving(null); toast.success('Price deleted') },
                  onError: (error) => toast.error(because(error, 'The price was not deleted')),
                })
              }}
            >Delete price</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}
