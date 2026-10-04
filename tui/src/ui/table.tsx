// Table — shared data table with a header row and selectable/hoverable rows.
// Columns have fixed widths; the row renders highlight for selection/hover.

import { For, Show, createMemo, type JSX } from "solid-js"
import { theme } from "../theme"

type Width = number | `${number}%`

export type TableColumn<T> = {
  title: string
  width?: Width
  render: (row: T, state: { selected: boolean; hovered: boolean }) => JSX.Element
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
  // Track selection/hover reactively so row highlight updates when they change.
  // For each rows.map(...) via a memo so a selectedIndex change re-runs For.
  const rows = createMemo(() =>
    props.rows.map((row, i) => ({ row, isSel: i === props.selectedIndex, isHover: i === props.hoverIndex })),
  )

  return (
    <box flexDirection="column" backgroundColor={theme.backgroundPanel}>
      {/* Header */}
      <box flexDirection="row" paddingLeft={1} paddingRight={1} paddingBottom={1} border={["bottom"]} borderColor={theme.border}>
        <For each={props.columns}>
          {(col) => (
            <text fg={theme.textMuted} attributes={1} width={col.width}>
              {col.title}
            </text>
          )}
        </For>
      </box>

      {props.rows.length === 0 && (
        <text fg={theme.textDim} paddingLeft={1}>{props.emptyText ?? "暂无数据"}</text>
      )}

      <For each={rows()}>
        {(entry, i) => (
          <box
            flexDirection="row"
            backgroundColor={entry.isSel ? theme.primary : entry.isHover ? theme.backgroundElement : theme.backgroundPanel}
            paddingLeft={1}
            paddingRight={1}
            onMouseOver={() => props.onHover?.(i())}
            onMouseOut={() => props.onHover?.(null)}
            onMouseDown={() => props.onSelect?.(i())}
            onMouseUp={(evt: { button?: number }) => props.onRowUp?.(entry.row, evt)}
          >
            <For each={props.columns}>
              {(col) => (
                <box width={col.width}>
                  {col.render(entry.row, { selected: entry.isSel, hovered: entry.isHover })}
                </box>
              )}
            </For>
          </box>
        )}
      </For>
    </box>
  )
}
