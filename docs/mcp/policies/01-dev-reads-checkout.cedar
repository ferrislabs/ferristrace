permit (
  principal in FerrisKey::Group::"Dev",
  action == FerrisKey::Action::"call_tool",
  resource is FerrisKey::Tool
)
when {
  resource.server == "ferristrace" &&
  ["search_logs", "search_traces"].contains(resource.tool) &&
  context.target has service &&
  context.target.service == "checkout" &&
  context.time.weekday >= 1 &&
  context.time.weekday <= 5 &&
  context.time.hour >= 9 &&
  context.time.hour < 18
};
