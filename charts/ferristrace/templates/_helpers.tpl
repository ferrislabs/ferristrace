{{- define "ferristrace.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "ferristrace.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{- define "ferristrace.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "ferristrace.labels" -}}
helm.sh/chart: {{ include "ferristrace.chart" . }}
{{ include "ferristrace.selectorLabels" . }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{- define "ferristrace.selectorLabels" -}}
app.kubernetes.io/name: {{ include "ferristrace.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{- define "ferristrace.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "ferristrace.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{- define "ferristrace.image" -}}
{{- $tag := default .Chart.AppVersion .Values.image.tag -}}
{{- if and (eq .Values.edition "ee") (not (hasSuffix "-ee" $tag)) -}}
{{- $tag = printf "%s-ee" $tag -}}
{{- end -}}
{{- printf "%s:%s" .Values.image.repository $tag -}}
{{- end }}

{{- define "ferristrace.secretEnv" -}}
- name: {{ .env }}
  valueFrom:
    secretKeyRef:
      name: {{ .ref.name | quote }}
      key: {{ .ref.key | quote }}
{{- end }}

{{- define "ferristrace.mcpSelectorLabels" -}}
app.kubernetes.io/name: {{ printf "%s-%s" (include "ferristrace.name" .root) .component | trunc 63 | trimSuffix "-" }}
app.kubernetes.io/instance: {{ .root.Release.Name }}
{{- end }}

{{- define "ferristrace.mcpLabels" -}}
helm.sh/chart: {{ include "ferristrace.chart" .root }}
{{ include "ferristrace.mcpSelectorLabels" . }}
app.kubernetes.io/version: {{ .root.Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .root.Release.Service }}
{{- end }}
