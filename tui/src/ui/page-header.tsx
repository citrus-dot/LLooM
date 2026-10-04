// PageHeader — consistent page title row with optional action buttons.

import type { JSX } from "solid-js"
import { theme } from "../theme"

export function PageHeader(props: { title: string; subtitle?: string; children?: JSX.Element; onRightClick?: (evt?: { button?: number }) => void }) {
  return (
    <box flexDirection="row" paddingBottom={1}>
      <box flexDirection="column">
        <text fg={theme.text} attributes={1} onMouseUp={(evt: { button?: number }) => props.onRightClick?.(evt)}>{props.title}</text>
        {props.subtitle && <text fg={theme.textDim}>{props.subtitle}</text>}
      </box>
      <box flexGrow={1} />
      {props.children}
    </box>
  )
}
