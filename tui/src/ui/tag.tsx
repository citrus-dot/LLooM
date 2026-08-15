// Tag — small status label with a colored indicator.

import type { JSX } from "solid-js"
import { theme } from "../theme"

export type TagTone = "default" | "primary" | "success" | "warning" | "error" | "info"

const toneColor = (tone: TagTone) => {
  switch (tone) {
    case "primary": return theme.primary
    case "success": return theme.success
    case "warning": return theme.warning
    case "error": return theme.error
    case "info": return theme.info
    default: return theme.textMuted
  }
}

export function Tag(props: { children: JSX.Element; tone?: TagTone }) {
  const color = toneColor(props.tone ?? "default")
  return (
    <text fg={color} attributes={1}>
      {props.children}
    </text>
  )
}
