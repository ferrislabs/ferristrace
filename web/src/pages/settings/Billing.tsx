import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { NativeSelect } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { Table, Td, Th, Tr } from '@/components/ui/misc'
import { H, Row } from './parts'

export function Billing() {
  return (
    <>
      <H title="Billing" sub="Cloud · Team plan · renews Nov 1" />
      <Card className="flex flex-wrap items-center gap-4 p-4">
        <div className="flex flex-[1_1_280px] flex-col gap-0.5"><span className="text-control text-muted-foreground">Current plan</span><span className="text-heading">Team · €[PRICE] / month</span><span className="text-caption text-muted-foreground">75 M spans, 200 M log records, 100 M metric points included. 14-day retention.</span></div>
        <Button size="sm">Change plan</Button>
      </Card>
      <Card className="overflow-hidden">
        <Row title="Projected bill" sub="Based on usage so far this month."><span className="font-mono text-heading">€[PRICE]</span></Row>
        <Row title="Usage alert" sub="Email admins at 80 % of included."><Switch defaultChecked aria-label="Usage alert" /></Row>
        <Row title="Hard limit" sub="Ingest returns 429 above it; queries keep working."><NativeSelect className="w-[200px]" aria-label="Hard limit"><option>Off</option><option>150 % of included</option><option>200 % of included</option></NativeSelect></Row>
        <Row title="Payment method" sub="Visa ending 4242 · expires 08/28"><Button size="sm">Update</Button></Row>
      </Card>
      <Card className="overflow-x-auto">
        <Table aria-label="Invoices"><thead><tr><Th>Invoice</Th><Th className="w-[130px]">Date</Th><Th numeric className="w-[120px]">Amount</Th><Th className="w-[100px]">Status</Th><Th className="w-[100px]"><span className="sr-only">Download</span></Th></tr></thead>
          <tbody>{['FT-2026-0010', 'FT-2026-0009', 'FT-2026-0008'].map((n, i) => <Tr key={n}><Td className="font-mono">{n}</Td><Td className="font-mono">2026-{String(10 - i).padStart(2, '0')}-01</Td><Td numeric>€[PRICE]</Td><Td><Badge variant="success">Paid</Badge></Td><Td numeric><Button variant="ghost" size="sm">PDF</Button></Td></Tr>)}</tbody></Table>
      </Card>
    </>
  )
}
