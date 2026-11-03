import { useQuery } from '@tanstack/react-query'
import { get, json } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { useScope } from '@/lib/scope'

export type Arrival = 'unknown' | 'waiting' | 'arrived'
export type Signal = 'traces' | 'logs' | 'metrics'
export type FirstSignals = Record<Signal, Arrival> & { trace: string | null }

const LOOKBACK_MS = 24 * 60 * 60 * 1000
const POLL_MS = 5000

function useArrived(kind: Signal, path: (org: string, query: URLSearchParams) => string, found: (body: unknown) => string | null | undefined) {
  const { active } = useOrganisations()
  const { project } = useScope()
  const { data, error } = useQuery({
    queryKey: ['first-signal', kind, active?.org, project],
    enabled: Boolean(active),
    refetchInterval: (query) => (query.state.data === undefined || query.state.data === null ? POLL_MS : false),
    queryFn: async () => {
      const to = Date.now()
      const query = new URLSearchParams({ from: String(to - LOOKBACK_MS), to: String(to), limit: '1' })
      if (project) query.set('project', project)
      return (found(await json<unknown>(await get(path(active?.org ?? '', query))))) ?? null
    },
  })
  const arrival: Arrival = error ? 'unknown' : data ? 'arrived' : 'waiting'
  return { arrival, first: data ?? null }
}

export function useFirstSignals(): FirstSignals {
  const traces = useArrived('traces', (org, q) => `/api/orgs/${org}/traces?${q.toString()}`, (b) => (b as { traces: { trace_id: string }[] }).traces[0]?.trace_id)
  const logs = useArrived('logs', (org, q) => `/api/orgs/${org}/logs?${q.toString()}`, (b) => ((b as { records: unknown[] }).records.length ? 'logs' : null))
  const metrics = useArrived('metrics', (org, q) => {
    const kept = new URLSearchParams()
    const project = q.get('project')
    if (project) kept.set('project', project)
    return `/api/orgs/${org}/metrics?${kept.toString()}`
  }, (b) => (b as { metrics: { name: string }[] }).metrics[0]?.name)
  return { traces: traces.arrival, logs: logs.arrival, metrics: metrics.arrival, trace: traces.first }
}

export const otlpBase = () => window.location.origin

export const otlpEndpoint = () => `${otlpBase()}/v1/traces`

export const SNIPPETS: { language: string; code: string }[] = [
  {
    language: 'Environment',
    code: `OTEL_SERVICE_NAME=checkout-api
OTEL_EXPORTER_OTLP_ENDPOINT=<base>
OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf
OTEL_EXPORTER_OTLP_HEADERS="authorization=Bearer <key>"
OTEL_RESOURCE_ATTRIBUTES=deployment.environment=production,service.version=1.0.0`,
  },
  {
    language: 'Rust',
    code: `// Cargo.toml: opentelemetry, opentelemetry_sdk, opentelemetry-otlp = { features = ["http-proto"] }
let exporter = opentelemetry_otlp::SpanExporter::builder()
    .with_http()
    .with_endpoint("<endpoint>")
    .with_headers(HashMap::from([("authorization".into(), "Bearer <key>".into())]))
    .build()?;

let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
    .with_batch_exporter(exporter)
    .with_resource(Resource::builder().with_service_name("checkout-api").build())
    .build();
opentelemetry::global::set_tracer_provider(provider);`,
  },
  {
    language: 'Go',
    code: `// go get go.opentelemetry.io/otel go.opentelemetry.io/otel/exporters/otlp/otlptrace/otlptracehttp
exporter, err := otlptracehttp.New(ctx,
    otlptracehttp.WithEndpointURL("<endpoint>"),
    otlptracehttp.WithHeaders(map[string]string{"authorization": "Bearer <key>"}),
)
if err != nil {
    return err
}

provider := sdktrace.NewTracerProvider(
    sdktrace.WithBatcher(exporter),
    sdktrace.WithResource(resource.NewWithAttributes(semconv.SchemaURL, semconv.ServiceName("checkout-api"))),
)
otel.SetTracerProvider(provider)`,
  },
  {
    language: 'Python',
    code: `# pip install opentelemetry-sdk opentelemetry-exporter-otlp-proto-http
from opentelemetry import trace
from opentelemetry.exporter.otlp.proto.http.trace_exporter import OTLPSpanExporter
from opentelemetry.sdk.resources import Resource
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import BatchSpanProcessor

provider = TracerProvider(resource=Resource.create({"service.name": "checkout-api"}))
provider.add_span_processor(BatchSpanProcessor(OTLPSpanExporter(
    endpoint="<endpoint>",
    headers={"authorization": "Bearer <key>"},
)))
trace.set_tracer_provider(provider)`,
  },
  {
    language: 'Node',
    code: `// npm i @opentelemetry/sdk-node @opentelemetry/exporter-trace-otlp-http @opentelemetry/auto-instrumentations-node
const { NodeSDK } = require('@opentelemetry/sdk-node')
const { OTLPTraceExporter } = require('@opentelemetry/exporter-trace-otlp-http')
const { getNodeAutoInstrumentations } = require('@opentelemetry/auto-instrumentations-node')

new NodeSDK({
  serviceName: 'checkout-api',
  traceExporter: new OTLPTraceExporter({
    url: '<endpoint>',
    headers: { authorization: 'Bearer <key>' },
  }),
  instrumentations: [getNodeAutoInstrumentations()],
}).start()`,
  },
  {
    language: 'Java',
    code: `# Run with the OpenTelemetry Java agent, no code change
java -javaagent:opentelemetry-javaagent.jar \\
  -Dotel.service.name=checkout-api \\
  -Dotel.traces.exporter=otlp \\
  -Dotel.exporter.otlp.protocol=http/protobuf \\
  -Dotel.exporter.otlp.endpoint=<base> \\
  -Dotel.exporter.otlp.headers=authorization=Bearer%20<key> \\
  -jar checkout-api.jar`,
  },
]
