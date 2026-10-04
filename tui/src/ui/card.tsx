// Card — shared panel with an optional title row.
// Uniform border, padding, and background across all pages.

import type { JSX } from "solid-js"
import { theme } from "../theme"

export function Card(props: {
  title?: string
  children: JSX.Element
  actions?: JSX.Element
  borderColor?: typeof theme.primary | typeof theme.secondary | typeof theme.border
  flexGrow?: boolean
  paddingLeft?: number
  paddingRight?: number
}) {
  const border = props.borderColor ?? theme.border
  return (
    <box
      flexDirection="column"
      flexGrow={props.flexGrow ? 1 : 0}
      backgroundColor={theme.backgroundPanel}
      border={["left"]}
      borderColor={border}
      paddingLeft={props.paddingLeft ?? 1}
      paddingRight={props.paddingRight ?? 1}
    >
      {props.title !== undefined && (
        <>
          <box flexDirection="row" paddingLeft={1} paddingRight={1} paddingTop={1}>
            <text fg={theme.text} attributes={1}>{props.title}</text>
            <box flexGrow={1} />
            {props.actions}
          </box>
        </>
      )}
      <box paddingTop={props.title ? 1 : 0} paddingBottom={1}>
        {props.children}
      </box>
    </box>
  )
}
