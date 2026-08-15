// MessageBubble — shared chat message block.
// User messages: filled panel with a left accent + "你" label.
// Assistant messages: markdown-rendered content with a model/cache detail line.

import { createSignal, Show } from "solid-js"
import { theme, syntax } from "../theme"

export function UserBubble(props: { content: string; label?: string }) {
  const [hover, setHover] = createSignal(false)
  return (
    <box
      paddingTop={1}
      paddingBottom={1}
      paddingLeft={2}
      backgroundColor={hover() ? theme.backgroundElement : theme.backgroundPanel}
      flexShrink={0}
      border={["left"]}
      borderStyle="rounded"
      borderColor={theme.primary}
      onMouseOver={() => setHover(true)}
      onMouseOut={() => setHover(false)}
    >
      <text fg={theme.textMuted} attributes={1}>{props.label ?? "你"}</text>
      <text></text>
      <text fg={theme.text}>{props.content}</text>
    </box>
  )
}

export function AssistantBubble(props: { content: string; detail?: string }) {
  return (
    <>
      <box paddingLeft={3} paddingTop={1} paddingBottom={1} flexShrink={0}>
        <markdown content={props.content.trim()} streaming={true} syntaxStyle={syntax} fg={theme.text} />
      </box>
      <box paddingLeft={3}>
        <text>
          <span style={{ fg: theme.secondary }}>▣ </span>
          <span style={{ fg: theme.text }}>LLooM</span>
          <Show when={props.detail}>
            <span style={{ fg: theme.textMuted }}> · {props.detail}</span>
          </Show>
        </text>
      </box>
    </>
  )
}
