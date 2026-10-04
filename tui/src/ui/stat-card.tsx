// StatCard — a labeled metric card (value + caption), used in stat rows.

import { theme } from "../theme"

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
      paddingLeft={1}
      paddingRight={2}
    >
      <text fg={theme.textDim}>{props.label.toUpperCase()}</text>
      <text fg={valueColor()} attributes={1}>{props.value}</text>
    </box>
  )
}
