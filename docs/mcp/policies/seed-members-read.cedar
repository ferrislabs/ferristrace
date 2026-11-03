permit (
  principal is FerrisKey::User,
  action == FerrisKey::Action::"call_tool",
  resource is FerrisKey::Tool
)
when {
  resource.server == "ferristrace" &&
  resource has access &&
  resource.access == "read"
};
