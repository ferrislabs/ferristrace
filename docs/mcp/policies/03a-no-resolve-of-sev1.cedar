forbid (
  principal,
  action == FerrisKey::Action::"call_tool",
  resource is FerrisKey::Tool
)
when {
  resource.tool == "resolve_incident"
}
unless {
  context.target has severity &&
  context.target.severity != "sev1"
};
