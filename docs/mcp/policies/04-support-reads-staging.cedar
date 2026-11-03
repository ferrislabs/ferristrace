permit (
  principal in FerrisKey::Group::"Support",
  action == FerrisKey::Action::"call_tool",
  resource is FerrisKey::Tool
)
when {
  resource.server == "ferristrace" &&
  resource has access &&
  resource.access == "read" &&
  context.target has environment &&
  context.target.environment == "staging"
};
