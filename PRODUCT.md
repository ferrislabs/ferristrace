# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

Two primary users, served with equal weight:

- The developer on call in a small team with no platform team. They write the code and also run it, and open FerrisTrace when something breaks, often at night and sometimes from a phone.
- The SRE or platform engineer whose job is production. They live in the tool, know OpenTelemetry well, and expect density and speed.

The interface stays simple for the first without slowing the second.

## Product Purpose

FerrisTrace is observability you can host yourself: traces, logs, metrics, Kubernetes, incidents, alerts, a public status page and AI cost tracking, in one application fed over OTLP. Success is the landing promise: find what broke, fix it, and go back to bed.

## Positioning

- Open source and self-hostable as one container; the cloud edition runs the same code and the same UI.
- One timeline from the first alert to the postmortem, with the evidence attached.
- Sign-in goes only through FerrisKey (OIDC). FerrisTrace stores no password.
- AI agents reach telemetry through an MCP gateway that enforces policies the customer writes in FerrisKey (Cedar), with every decision audited.

## Operating Context

Two usage situations, both first-class:

- Incident under pressure: an alert fires, the user finds the cause and declares the incident fast, on desktop or on a phone.
- Investigation at a desk: long sessions on a large screen, exploring traces, logs and metrics to understand or optimise.

Data arrives over OTLP (gRPC 4317, HTTP 4318). Work is scoped by organisation, then project. Roles are owner, admin and member.

## Capabilities and Constraints

- Overview, services, traces, logs, metrics, Kubernetes (nodes, pods), issues, releases, incidents and postmortems, alerts, on-call, status page, AI (LLM cost and latency), settings (organisation, members, teams, projects, api keys, agents, MCP, audit log, usage).
- Some routes are still backed by mock data; the router marks them (`staticData.mock`).
- The cloud and self-hosted editions show the same screens; nothing is reserved to the cloud.
- Installable PWA. It must stay usable on a poor or absent network, notably the incident page while on call.
- The interface is in English only. No translation is planned.

## Brand Commitments

- Name: FerrisTrace. Mascot: Ferris, the crab, as used on ferristrace.fr.
- Voice: plain and factual, short sentences, no hype (landing: "Find what broke. Fix it and go back to bed.").

## Evidence on Hand

- Landing copy and mascot: /opt/ferrislabs/ferristrace.fr/apps/website/src/components/landing.
- No customer testimonials, benchmarks or usage figures exist for the app; do not invent any.

## Product Principles

1. Problems first: what is broken ranks above what is fine, on every screen.
2. One click from symptom to evidence: an alert, a spike or an error opens the traces, logs or pod behind it.
3. Calm under pressure: the incident path stays fast and readable on a phone at night.
4. Dense for experts, legible for newcomers: no screen trades one for the other.
5. Honest data: show what is known and say what is not; never present mock or missing data as real.

## Accessibility & Inclusion

WCAG 2.2 AA is a requirement: contrast, full keyboard navigation and screen reader support on every screen.
