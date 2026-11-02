import { CircleDashed, Copy, HelpCircle, Rocket } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardHeader, CardTitle } from '@/components/ui/card'
import { Segmented } from '@/components/ui/segmented'
import { Alert } from '@/components/ui/misc'
import { PageHeader } from '@/components/ft/bits'
import { SNIPPETS, ingestKeyCommand, otlpEndpoint, useFirstSignals, type Arrival } from '@/data/onboarding'
import { Link } from '@/lib/router'

const copy = (text: string, what: string) => {
  void navigator.clipboard?.writeText(text)
  toast.success(`${what} copied`)
}

function Copyable({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex flex-col gap-1.5">
      <span className="text-control font-semibold">{label}</span>
      <div className="flex items-center gap-2">
        <code className="min-w-0 flex-1 truncate rounded-md border bg-muted px-3 py-2 font-mono text-[12.5px]">{value}</code>
        <Button size="sm" aria-label={`Copy ${label}`} onClick={() => copy(value, label)}>
          <Copy />
        </Button>
      </div>
    </div>
  )
}

const ARRIVAL: Record<Arrival, { label: string; variant: 'neutral' | 'success' | 'outline' }> = {
  unknown: { label: 'Cannot be checked yet', variant: 'outline' },
  waiting: { label: 'Waiting', variant: 'neutral' },
  arrived: { label: 'Arrived', variant: 'success' },
}

export function Onboarding() {
  const endpoint = otlpEndpoint()
  const signals = useFirstSignals()
  const [language, setLanguage] = useState(SNIPPETS[0].language)
  const snippet = SNIPPETS.find((s) => s.language === language) ?? SNIPPETS[0]
  const code = snippet.code.replaceAll('<endpoint>', endpoint).replaceAll('<base>', endpoint.replace(/\/v1\/traces$/, ''))
  return (
    <>
      <PageHeader title="Send your first trace" meta="Three environment variables and a restart" />
      <div className="grid grid-cols-[repeat(auto-fit,minmax(min(420px,100%),1fr))] items-start gap-3">
        <Card className="flex flex-col gap-4 p-4">
          <Copyable label="OTLP endpoint, HTTP" value={endpoint} />
          <Copyable label="Ingest key, printed once by the server" value={ingestKeyCommand} />
          <Alert variant="info" icon={<HelpCircle />} title="Keys are issued by the server, not here">
            No HTTP route hands out ingest keys yet, so the command above is the way in. Once one exists, this card issues the key and shows it once.
          </Alert>
        </Card>
        <Card className="flex flex-col">
          <CardHeader>
            <CardTitle className="flex-1">First signals</CardTitle>
          </CardHeader>
          <div className="flex flex-col">
            {(['traces', 'logs', 'metrics'] as const).map((kind) => (
              <div key={kind} className="flex items-center gap-2.5 border-b border-border-subtle px-4 py-2.5 last:border-b-0">
                <CircleDashed className="size-4 text-muted-foreground" aria-hidden />
                <span className="flex-1 text-control font-semibold capitalize">{kind}</span>
                <Badge variant={ARRIVAL[signals[kind]].variant}>{ARRIVAL[signals[kind]].label}</Badge>
              </div>
            ))}
          </div>
          <p className="m-0 px-4 py-3 text-caption text-muted-foreground">
            Confirming an arrival needs a route that answers whether anything has been received; the seam waiting for it is <span className="font-mono">useFirstSignals</span> in <span className="font-mono">src/data/onboarding.ts</span>.
          </p>
        </Card>
      </div>
      <Card className="flex flex-col">
        <CardHeader className="flex-wrap">
          <CardTitle className="flex-1">Instrument your service</CardTitle>
          <Segmented label="Language" value={language} onChange={setLanguage} options={SNIPPETS.map((s) => ({ value: s.language, label: s.language }))} />
        </CardHeader>
        <div className="relative">
          <pre className="m-0 overflow-x-auto px-4 py-3 font-mono text-[12.5px] leading-5">{code}</pre>
          <Button size="sm" className="absolute right-3 top-2.5" onClick={() => copy(code, `${snippet.language} snippet`)}>
            <Copy />
            Copy
          </Button>
        </div>
        <p className="m-0 border-t border-border-subtle px-4 py-3 text-caption text-muted-foreground">
          Replace <span className="font-mono">&lt;key&gt;</span> with the key the server printed. Logs and metrics use the same endpoint and key; point their exporters at it too.
        </p>
      </Card>
      <div className="flex flex-wrap gap-2">
        <Button asChild variant="primary" size="sm">
          <Link to="/traces">
            <Rocket />
            Look for traces
          </Link>
        </Button>
        <Button asChild size="sm">
          <Link to="/">Back to Overview</Link>
        </Button>
      </div>
    </>
  )
}
