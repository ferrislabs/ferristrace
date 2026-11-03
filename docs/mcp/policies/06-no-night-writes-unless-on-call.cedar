forbid (
  principal,
  action == FerrisKey::Action::"call_tool",
  resource is FerrisKey::Tool
)
when {
  resource has access &&
  resource.access == "write" &&
  (context.time.hour >= 22 || context.time.hour < 7) &&
  !(principal has on_call && principal.on_call)
};
