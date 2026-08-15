// Table — shared data table with a header row and selectable/hoverable rows.
// Columns have fixed widths; the row renders highlight for selection/hover.

import { For, Show, type JSX } from "solid-js"
import { theme } from "../theme"

type Width = number | `${number}%`

export type TableColumn<T> = {
  title: string
  width?: Width
  render: (row: T) => JSX.Element
}

export function Table<T = unknown>(props: {
  columns: TableColumn<T>[]
  rows: T[]
  rowKey?: (row: T) => string
  selectedIndex?: number
  hoverIndex?: number | null
  onHover?: (index: number | null) => void
  onSelect?: (index: number) => void
  onRowUp?: (row: T, evt?: { button?: number }) => void
  emptyText?: string
}) {
  return (
    <box flexDirection="column" backgroundColor={theme.backgroundPanel} border={["left", "right"]} borderStyle="rounded" borderColor={theme.border} paddingTop={1} paddingBottom={1}>
      {/* Header */}
      <box flexDirection="row" paddingLeft={3} paddingRight={3} paddingBottom={1}>
        <For each={props.columns}>
          {(col) => (
            <text fg={theme.textMuted} attributes={1} width={col.width}>
              {col.title}
            </text>
          )}
        </For>
      </box>

      {props.rows.length === 0 && (
        <text fg={theme.textDim} paddingLeft={3}>  {props.emptyText ?? "暂无数据"}</text>
      )}

      <For each={props.rows}>
        {(row, i) => {
          const isSel = i() === props.selectedIndex
          const isHover = i() === props.hoverIndex
          return (
            <box
              flexDirection="row"
              backgroundColor={isSel ? theme.primary : isHover ? theme.backgroundElement : theme.backgroundPanel}
              paddingLeft={3}
              paddingRight={3}
              onMouseOver={() => props.onHover?.(i())}
              onMouseOut={() => props.onHover?.(null)}
              onMouseDown={() => props.onSelect?.(i())}
              onMouseUp={(evt: { button?: number }) => props.onRowUp?.(row, evt)}
            >
              <For each={props.columns}>
                {(col) => (
                  <box width={col.width}>
                    {col.render(row)}
                  </box>
                )}
              </For>
            </box>
          )
        }}
      </For>
    </box>
  )
}
