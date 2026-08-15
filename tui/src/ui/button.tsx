// Button — shared interactive control.
// Variants: primary (filled accent), ghost (bordered, transparent), danger.
// Hover highlights, selected state for toggles. Text-based, OpenCode-style.

import { createSignal, type ParentProps } from "solid-js"
import { theme } from "../theme"

export type ButtonVariant = "primary" | "ghost" | "danger" | "success"

export function Button(props: ParentProps<{
  variant?: ButtonVariant
  disabled?: boolean
  selected?: boolean
  /** Invert colors: for buttons rendered on a highlighted (selected) row. */
  inverse?: boolean
  onClick?: (evt?: { button?: number }) => void
  onRightClick?: () => void
  title?: string
}>) {
  const [hover, setHover] = createSignal(false)

  const fg = () => {
    if (props.disabled) return theme.textDim
    if (props.inverse) return theme.background
    if (props.selected) return theme.background
    switch (props.variant ?? "ghost") {
      case "primary": return theme.primary
      case "danger": return theme.error
      case "success": return theme.success
      default: return theme.textMuted
    }
  }
  const bg = () => {
    if (props.disabled) return theme.backgroundPanel
    if (props.inverse) return "transparent"
    if (props.selected) return theme.primary
    if (hover()) return theme.backgroundElement
    return theme.backgroundPanel
  }

  return (
    <box
      backgroundColor={bg()}
      border={["left"]}
      borderColor={props.inverse ? theme.background : props.selected ? theme.primary : hover() ? theme.borderActive : theme.border}
      borderStyle="rounded"
      paddingLeft={2}
      paddingRight={2}
      onMouseOver={() => setHover(true)}
      onMouseOut={() => setHover(false)}
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
      <text fg={fg()} attributes={props.selected ? 1 : 0}>{props.children}</text>
    </box>
  )
}
