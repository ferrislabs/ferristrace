permit (
  principal is FerrisKey::User,
  action == FerrisKey::Action::"call_tool",
  resource is FerrisKey::Tool
)
when {
  resource.server == "ferristrace" &&
  [
    "list_services",
    "service_health",
    "search_traces",
    "get_trace",
    "search_logs",
    "query_metric",
    "cluster_status",
    "list_alerts",
    "alert_history",
    "list_incidents",
    "get_incident",
    "who_is_on_call"
  ].contains(resource.tool)
};
