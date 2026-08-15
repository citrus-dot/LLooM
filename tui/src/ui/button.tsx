// Button — shared interactive control.
// The caller controls colors via fg/bg props (any RGBA); `variant` is only a
// convenience default for common cases. Hover highlighting is opt-in.

import { createSignal, type ParentProps } from "solid-js"
import { theme } from "../theme"
import type { ColorInput } from "@opentui/core"

export type ButtonVariant = "primary" | "ghost" | "danger" | "success"

export function Button(props: ParentProps<{
  variant?: ButtonVariant
  fg?: ColorInput
  bg?: ColorInput
  borderColor?: ColorInput
  disabled?: boolean
  hover?: boolean
  bold?: boolean
  onClick?: (evt?: { button?: number }) => void
  onRightClick?: () => void
  title?: string
}>) {
  const [hovered, setHovered] = createSignal(false)

  // Explicit fg/bg win over the variant defaults; fall back to transparent so
  // the surrounding row background shows through.
  const fg = () => {
    if (props.disabled) return theme.textDim
    if (props.fg) return props.fg
    switch (props.variant ?? "ghost") {
      case "primary": return theme.primary
      case "danger": return theme.error
      case "success": return theme.success
      default: return theme.textMuted
    }
  }
  const bg = () => {
    if (props.disabled) return theme.backgroundPanel
    if (props.bg) return props.bg
    if (props.hover && hovered()) return theme.backgroundElement
    return "transparent"
  }
  const border = () => {
    if (props.borderColor) return props.borderColor
    if (props.hover && hovered()) return theme.borderActive
    return theme.border
  }

  return (
    <box
      backgroundColor={bg()}
      border={["left"]}
      borderColor={border()}
      borderStyle="rounded"
      paddingLeft={2}
      paddingRight={2}
      onMouseOver={() => setHovered(true)}
      onMouseOut={() => setHovered(false)}
      onMouseDown={(evt: { button?: number }) => {
        if (evt?.button === 2) {
          if (!props.disabled) props.onRightClick?.()
        }
      }}
      onMouseUp={(evt: { button?: number }) => {
        if (props.disabled) return
        if (evt?.button === 2) return
        props.onClick?.()
      }}
    >
      <text fg={fg()} attributes={props.bold ? 1 : 0}>{props.children}</text>
    </box>
  )
}
