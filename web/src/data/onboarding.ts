import { useMock } from './seam'

export type Arrival = 'unknown' | 'waiting' | 'arrived'
export type FirstSignals = { traces: Arrival; logs: Arrival; metrics: Arrival }

export function useFirstSignals(): FirstSignals {
  return useMock(['onboarding', 'first-signals'], () => ({ traces: 'unknown', logs: 'unknown', metrics: 'unknown' }) as FirstSignals)
}

export const otlpEndpoint = () => `${window.location.origin}/v1/traces`

export const ingestKeyCommand = 'docker compose exec server ferristrace issue-key --org <org-uuid>'

export const SNIPPETS: { language: string; code: string }[] = [
  {
    language: 'Environment',
    code: `OTEL_SERVICE_NAME=checkout-api
OTEL_EXPORTER_OTLP_ENDPOINT=<endpoint>
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
