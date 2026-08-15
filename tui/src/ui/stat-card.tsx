// StatCard — a labeled metric card (value + caption), used in stat rows.

import { theme, spacing } from "../theme"

export function StatCard(props: {
  value: string
  label: string
  tone?: "primary" | "success" | "warning" | "secondary" | "text"
  flexGrow?: boolean
}) {
  const valueColor = () => {
    switch (props.tone ?? "text") {
      case "primary": return theme.primary
      case "success": return theme.success
      case "warning": return theme.warning
      case "secondary": return theme.secondary
      default: return theme.text
    }
  }
  return (
    <box
      flexDirection="column"
      flexGrow={props.flexGrow ? 1 : 0}
      backgroundColor={theme.backgroundPanel}
      border={["left"]}
      borderStyle="rounded"
      borderColor={theme.border}
      paddingLeft={spacing.md}
      paddingRight={spacing.md}
      paddingTop={spacing.sm}
      paddingBottom={spacing.sm}
    >
      <text fg={valueColor()} attributes={1}>{props.value}</text>
      <text fg={theme.textMuted}>{props.label}</text>
    </box>
  )
}
