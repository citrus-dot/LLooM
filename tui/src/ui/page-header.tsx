// PageHeader — consistent page title row with optional action buttons.

import type { JSX } from "solid-js"
import { theme } from "../theme"

export function PageHeader(props: { title: string; children?: JSX.Element; onRightClick?: (evt?: { button?: number }) => void }) {
  return (
    <box flexDirection="row" gap={1} paddingBottom={1}>
      <text
        fg={theme.textMuted}
        attributes={1}
        onMouseUp={(evt: { button?: number }) => props.onRightClick?.(evt)}
      >
        {props.title}
      </text>
      {props.children}
    </box>
  )
}
