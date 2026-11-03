forbid (
  principal,
  action == FerrisKey::Action::"call_tool",
  resource is FerrisKey::Tool
)
when {
  ["comment_incident", "resolve_incident", "acknowledge_page"].contains(resource.tool) &&
  (context.time.hour >= 22 || context.time.hour < 7) &&
  !(principal has on_call && principal.on_call)
};
