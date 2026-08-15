// LLooM TUI theme — tokyonight-inspired (from OpenCode).
// Colors are RGBA instances from @opentui/core.

import { RGBA, SyntaxStyle, type BorderStyle } from "@opentui/core"

export interface Theme {
  background: RGBA
  backgroundPanel: RGBA
  backgroundElement: RGBA
  backgroundMenu: RGBA
  border: RGBA
  borderActive: RGBA
  text: RGBA
  textMuted: RGBA
  textDim: RGBA
  primary: RGBA
  secondary: RGBA
  accent: RGBA
  error: RGBA
  warning: RGBA
  success: RGBA
  info: RGBA
}

// tokyonight palette (dark)
export const theme: Theme = {
  background: RGBA.fromInts(0x1a, 0x1b, 0x26),
  backgroundPanel: RGBA.fromInts(0x1e, 0x20, 0x30),
  backgroundElement: RGBA.fromInts(0x22, 0x24, 0x36),
  backgroundMenu: RGBA.fromInts(0x22, 0x24, 0x36),
  border: RGBA.fromInts(0x3b, 0x42, 0x61),
  borderActive: RGBA.fromInts(0x73, 0x7a, 0xa2),
  text: RGBA.fromInts(0xc8, 0xd3, 0xf5),
  textMuted: RGBA.fromInts(0x82, 0x8b, 0xb8),
  textDim: RGBA.fromInts(0x54, 0x5c, 0x7e),
  primary: RGBA.fromInts(0x82, 0xaa, 0xff),
  secondary: RGBA.fromInts(0xc0, 0x99, 0xff),
  accent: RGBA.fromInts(0xff, 0x96, 0x6c),
  error: RGBA.fromInts(0xff, 0x75, 0x7f),
  warning: RGBA.fromInts(0xff, 0xc7, 0x77),
  success: RGBA.fromInts(0xc3, 0xe8, 0x8d),
  info: RGBA.fromInts(0x86, 0xe1, 0xfc),
}

// ── Design tokens ──

/** Border style used across the UI (rounded corners). */
export const borderStyle: BorderStyle = "rounded"

/** Spacing scale (cells). */
export const spacing = {
  xs: 1,
  sm: 2,
  md: 3,
  lg: 4,
  xl: 6,
} as const

/** Typography: 1 = bold. */
export const textAttrs = {
  normal: 0,
  bold: 1,
} as const

// Default syntax style for markdown rendering (pure text chat).
export const syntax = SyntaxStyle.create()

