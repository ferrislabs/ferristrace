permit (
  principal is FerrisKey::User,
  action == FerrisKey::Action::"call_tool",
  resource is FerrisKey::Tool
)
when {
  resource.server == "ferristrace" &&
  ["resolve_incident", "acknowledge_page"].contains(resource.tool) &&
  principal has on_call &&
  principal.on_call
};
