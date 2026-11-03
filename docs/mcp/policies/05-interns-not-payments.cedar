forbid (
  principal in FerrisKey::Group::"Interns",
  action == FerrisKey::Action::"call_tool",
  resource is FerrisKey::Tool
)
when {
  context.target has namespace &&
  context.target.namespace == "payments"
};
