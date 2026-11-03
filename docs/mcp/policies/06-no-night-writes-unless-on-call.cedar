forbid (
  principal,
  action == FerrisKey::Action::"call_tool",
  resource is FerrisKey::Tool
)
when {
  (context.time.hour >= 22 || context.time.hour < 7) &&
  !(resource has access && resource.access == "read")
}
unless {
  principal has on_call &&
  principal.on_call
};
