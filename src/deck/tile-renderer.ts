import type { DeckCapabilities } from './deck-device'
import type { DeckAction, KeySpec, RingColor } from './frame'
import { repoAvatarColor, REPO_AVATAR_FONT_RATIO } from '@/components/icons/RepoIcon'
import { DECK_FONT_STACK } from './deck-font'
import { providerIconDataUrl } from './provider-icon-svg'
import { PANE_TINT_COLORS, STATUS_BLUE, STATUS_MUTED } from './pane-tint-colors'

// Canvas draw layer: converts a KeySpec into an RGBA pixel buffer via an
// injectable 2D-context factory (jsdom returns null from getContext, so tests
// always inject a fake context; defaultCtxFactory is runtime-only).

export type Ctx2D = Pick<
  CanvasRenderingContext2D,
  | 'fillRect'
  | 'fillText'
  | 'measureText'
  | 'getImageData'
  | 'drawImage'
  | 'beginPath'
  | 'arc'
  | 'fill'
  | 'save'
  | 'restore'
  | 'clip'
  | 'stroke'
> & {
  fillStyle: string | CanvasGradient | CanvasPattern
  strokeStyle: string | CanvasGradient | CanvasPattern
  lineWidth: number
  font: string
  /** Chromium-only canvas API (the deck runs in Chromium/Electron; WebHID requires it). */
  letterSpacing: string
  textBaseline: CanvasTextBaseline
  roundRect(x: number, y: number, w: number, h: number, radii?: number): void
}

export type IconSource = (url: string) => CanvasImageSource | null
export type CtxFactory = (width: number, height: number) => Ctx2D
export type KeyRenderer = (spec: KeySpec, caps: DeckCapabilities) => Uint8ClampedArray
export type StripRenderer = (text: string, width: number, height: number) => Uint8ClampedArray

export const PREVIEW_BG = '#0a0a0a'
export const PREVIEW_TEXT_COLOR = '#a8a8a8'
export const PREVIEW_FONT_SIZE = 11
export const PREVIEW_LINE_HEIGHT = 13
export const PREVIEW_CHAR_WIDTH = 5.5
export const PREVIEW_LEFT_MARGIN = 3
export const RING_COLORS: Record<Exclude<RingColor, null>, string> = {
  amber: '#f59e0b',
  green: '#22c55e',
  blue: '#3b82f6',
}
export const BANNER_HEIGHT = 20
export const BANNER_FILL = 'rgba(0,0,0,0.667)'
export const TITLE_FONT_SIZE = 16
/** Icons-style banner title. 2px smaller than the classic banner: 16px read
 * too large on 72-80px keys. The classic terminal-previews banner is PINNED
 * and keeps TITLE_FONT_SIZE = 16. Fit math needs no change: fitLabel is
 * measure-driven and Chromium's measureText scales with ctx.font. */
export const ICONS_TITLE_FONT_SIZE = 14
/** Subtle tracking for deck text. Chromium-only canvas API — the deck's only
 * supported surface. Set BEFORE measureText so fitLabel includes the tracking
 * (Chromium adds it after every glyph, matching the test stub's model). */
export const TEXT_LETTER_SPACING = '0.4px'
/** Side padding for the icons-style banner label inside the rounded frame. */
export const TITLE_SIDE_PADDING = 6

// ============================================================================
// DECK PALETTE — derived from freshell's own UI palette so the deck reads as
// part of the app. KEEP IN SYNC: when an app token changes, update the deck
// constant to match. Lightness/opacity may be tuned for the small dark LCD;
// hues must stay the app's (see docs/plans/2026-07-29-deck-icons-polish.md).
//
//   deck constant     <- app source token (where it lives)                 value
//   TILE_BG           <- deck-only pure black (was --background dark #09090b).
//                        Matches EMPTY_BG, so unfilled tiles read as plain
//                        black keys: only banner/icons/rings carry state.     #000000
//   TILE_FILL_GREEN   <- bg-emerald-100     (TabItem.tsx green-filled tab)    #d1fae5
//                        precomposited at 50% opacity over the black tile:
//                        round(c/2) per channel -> rgb(105,125,115) = #697d73.
//                        Same hue as the tab bar's green, dark enough for the
//                        LCD. Both fill states (green + barTop) use it; the
//                        barTop BORDER stays full-strength BAR_TOP_BORDER.
//   BAR_TOP_BORDER    <- border-t-success / --success (TabItem bar-on-top) hsl(142 71% 45%) = #21c45d
//   STATUS_* pane-icon tints (text-success, text-blue-500, ...) live in
//   pane-tint-colors.ts — shared with frame.ts, which computes the tinted
//   data URLs at frame-build time.
//   ACTIVE_COLOR      <- white active ring (deck-only affordance)          #ffffff
//   BANNER_FILL       <- black scrim over the tile (shared w/ previews)    rgba(0,0,0,0.667)
//   CONTROL_BG        <- bg-muted dark      (src/theme-variables.css)      hsl(240 4% 16%)  = #27272a
//   CONTROL_DIM       <- text-muted-foreground dark                        hsl(240 5% 65%)  = #a1a1aa
//   APPROVE_COLOR     <- --success                                         #21c45d
//   STOP_COLOR        <- --destructive light: hsl(0 72% 51%)               #dc2828
//                        (light variant: the dark-theme destructive is too
//                        dull for an action ring on the LCD)
//   PREVIEW_* / RING_COLORS: classic terminal-previews style — PINNED,
//   deliberately not re-derived (that style must not change).
// ============================================================================

export const TILE_BG = '#000000'
export const TILE_FILL_GREEN = '#697d73'
export const BAR_TOP_BORDER = '#21c45d'
export const ACTIVE_COLOR = '#ffffff'
export const ICON_GAP = 3
export const CONTROL_BG = '#27272a'
export const CONTROL_DIM = '#a1a1aa'
export const APPROVE_COLOR = '#21c45d'
export const STOP_COLOR = '#dc2828'
export const DISABLED_ACTION_COLOR = '#555555'
export const EMPTY_BG = '#000000'
export const STRIP_FONT_SIZE = 22
export const CONTROL_LABEL_FONT_SIZE = 11
export const CONTROL_VALUE_FONT_SIZE = 15
export const MAX_TITLE_CHARS = 10
export const MAX_KEY_PANE_ICONS = 2
export const OVERFLOW_FONT_SIZE = 10
/**
 * Max row slots (icons + badge). iconLayout's row-fit clamp guarantees any
 * slot count fits inside the rounded frame, but 3 remains the visual maximum
 * to mirror TabItem's row, so the +N badge COUNTS AS A SLOT and drawn agent
 * icons shrink to make room for it.
 */
const MAX_ROW_SLOTS = 3

/** Rounded key frame: every key draws inside a rounded rect with pure black
 * outside it, so each button reads as a rounded tile floating on the deck. */
export const KEY_FRAME_RADIUS_RATIO = 0.12

export function keyFrameGeometry(w: number, h: number): { margin: number; radius: number } {
  const s = Math.min(w, h)
  return { margin: s >= 96 ? 4 : 3, radius: Math.round(s * KEY_FRAME_RADIUS_RATIO) }
}

function beginKeyFrame(ctx: Ctx2D, w: number, h: number): void {
  ctx.fillStyle = EMPTY_BG
  ctx.fillRect(0, 0, w, h)
  const { margin, radius } = keyFrameGeometry(w, h)
  ctx.save()
  ctx.beginPath()
  ctx.roundRect(margin, margin, w - 2 * margin, h - 2 * margin, radius)
  ctx.clip()
}

export function previewGeometry(width: number, height: number): { lines: number; columns: number } {
  return {
    lines: Math.max(1, Math.floor((height - BANNER_HEIGHT - 2) / PREVIEW_LINE_HEIGHT) + 1),
    columns: Math.max(1, Math.floor((width - PREVIEW_LEFT_MARGIN) / PREVIEW_CHAR_WIDTH)),
  }
}

export function cropPreviewLines(lines: string[], maxLines: number, maxColumns: number): string[] {
  const out = [...lines]
  while (out.length > 0 && out[out.length - 1].trim() === '') out.pop()
  return out.slice(-maxLines).map((l) => l.slice(0, maxColumns))
}

export function truncateTitle(title: string): string {
  return title.length > MAX_TITLE_CHARS ? `${title.slice(0, MAX_TITLE_CHARS - 1)}…` : title
}

export function fitLabel(measure: (t: string) => number, text: string, maxWidth: number): string {
  if (measure(text) <= maxWidth) return text
  let t = text
  while (t.length > 0 && measure(`${t}…`) > maxWidth) t = t.slice(0, -1)
  return `${t}…`
}

/** Keeps multi-icon rows clear of the rounded key frame's corners/edges. */
export const ICON_ROW_SIDE_INSET = 6

/** Centered icon slots in the area below the title banner. */
export function iconLayout(w: number, h: number, count: number): Array<{ x: number; y: number; size: number }> {
  if (count <= 0) return []
  const areaTop = BANNER_HEIGHT
  const areaH = h - areaTop
  const scale = count === 1 ? 0.75 : 0.45
  let size = Math.round(Math.min(w, areaH) * scale)
  // Row-fit clamp: the whole row (icons + gaps) stays inside the rounded frame.
  const innerW = w - 2 * ICON_ROW_SIDE_INSET
  size = Math.min(size, Math.floor((innerW - (count - 1) * ICON_GAP) / count))
  const rowW = count * size + (count - 1) * ICON_GAP
  const x0 = Math.round((w - rowW) / 2)
  const y = Math.round(areaTop + (areaH - size) / 2)
  return Array.from({ length: count }, (_, i) => ({ x: x0 + i * (size + ICON_GAP), y, size }))
}

/** Intrinsic pixel size of a decoded bitmap. Prefers naturalWidth/naturalHeight
 * (HTMLImageElement intrinsics) over width/height (layout/attribute values);
 * returns 0x0 when unknown — dimensionless (viewBox-only) SVGs report 0. */
function intrinsicIconSize(bitmap: CanvasImageSource): { width: number; height: number } {
  const src = bitmap as { naturalWidth?: unknown; naturalHeight?: unknown; width?: unknown; height?: unknown }
  const dim = (v: unknown): number => (typeof v === 'number' && Number.isFinite(v) && v > 0 ? v : 0)
  const nw = dim(src.naturalWidth)
  const nh = dim(src.naturalHeight)
  if (nw > 0 && nh > 0) return { width: nw, height: nh }
  return { width: dim(src.width), height: dim(src.height) }
}

/** Aspect-preserving contain fit: the largest centered rect with the bitmap's
 * intrinsic aspect ratio inside the square icon slot — icons only ever scale
 * SYMMETRICALLY, never stretched to the square. Falls back to the full square
 * when the intrinsic size is unknown (dimensionless SVGs report 0x0), so
 * explicit destination dims are always available for drawImage. */
export function containIconRect(
  bitmap: CanvasImageSource,
  x: number,
  y: number,
  size: number,
): { x: number; y: number; w: number; h: number } {
  const { width, height } = intrinsicIconSize(bitmap)
  if (width <= 0 || height <= 0 || width === height) return { x, y, w: size, h: size }
  const scale = size / Math.max(width, height)
  const w = Math.max(1, Math.round(width * scale))
  const h = Math.max(1, Math.round(height * scale))
  return { x: x + Math.round((size - w) / 2), y: y + Math.round((size - h) / 2), w, h }
}

/** Draws an icon bitmap contain-fitted into its square slot. ALWAYS uses the
 * 5-arg drawImage form with explicit destination width AND height:
 * dimensionless (viewBox-only) SVGs draw blank without them (verified headless
 * Chromium 145; the server serves dimensionless SVGs first-class -
 * repo_icon_detect.rs:51-52). Never call the 3-arg drawImage(image, dx, dy)
 * form anywhere in this module. */
function drawIconContained(ctx: Ctx2D, bitmap: CanvasImageSource, x: number, y: number, size: number): void {
  const rect = containIconRect(bitmap, x, y, size)
  ctx.drawImage(bitmap, rect.x, rect.y, rect.w, rect.h)
}

/** Ring/border that follows the rounded key frame: a rounded-rect stroke of
 * `width` px, `inset` px inside the frame edge. (Replaces the old nested
 * 1px square fillRect frames — strokes keep the rounded shape at corners.) */
export function drawRing(ctx: Ctx2D, w: number, h: number, color: string, width: number, inset = 0): void {
  const { margin, radius } = keyFrameGeometry(w, h)
  const off = margin + inset + width / 2
  ctx.strokeStyle = color
  ctx.lineWidth = width
  ctx.beginPath()
  ctx.roundRect(off, off, w - 2 * off, h - 2 * off, Math.max(1, radius - inset - width / 2))
  ctx.stroke()
}

function drawCenteredText(ctx: Ctx2D, text: string, w: number, y: number): void {
  const x = (w - ctx.measureText(text).width) / 2
  ctx.fillText(text, x, y)
}

const ACTION_LABELS: Record<DeckAction, string> = { back: 'BACK', approve: 'APPROVE', stop: 'STOP' }
const ACTION_RING: Record<DeckAction, string> = { back: ACTIVE_COLOR, approve: APPROVE_COLOR, stop: STOP_COLOR }

function drawPreviewTab(ctx: Ctx2D, w: number, h: number, spec: Extract<KeySpec, { kind: 'tab'; style: 'preview' }>): void {
  ctx.fillStyle = PREVIEW_BG
  ctx.fillRect(0, 0, w, h)

  const { lines, columns } = previewGeometry(w, h)
  const body = cropPreviewLines(spec.previewLines, lines, columns)
  ctx.font = `${PREVIEW_FONT_SIZE}px monospace`
  ctx.textBaseline = 'top'
  ctx.fillStyle = PREVIEW_TEXT_COLOR
  const baseY = h - body.length * PREVIEW_LINE_HEIGHT - 2
  body.forEach((line, i) => {
    if (line.trim() === '') return
    ctx.fillText(line, PREVIEW_LEFT_MARGIN, baseY + i * PREVIEW_LINE_HEIGHT)
  })

  ctx.fillStyle = BANNER_FILL
  ctx.fillRect(0, 0, w, BANNER_HEIGHT)

  ctx.font = `${TITLE_FONT_SIZE}px sans-serif`
  ctx.textBaseline = 'top'
  ctx.fillStyle = ACTIVE_COLOR
  const label = fitLabel((t) => ctx.measureText(t).width, truncateTitle(spec.title), w - 4)
  drawCenteredText(ctx, label, w, 2)

  const ring = spec.ring ? RING_COLORS[spec.ring] : null
  if (ring && spec.active) {
    drawRing(ctx, w, h, ring, 3, 0)
    drawRing(ctx, w, h, ACTIVE_COLOR, 2, 3)
  } else if (ring) {
    drawRing(ctx, w, h, ring, 4, 0)
  } else if (spec.active) {
    drawRing(ctx, w, h, ACTIVE_COLOR, 3, 0)
  }
}

function drawIconsTab(ctx: Ctx2D, w: number, h: number, spec: Extract<KeySpec, { kind: 'tab'; style: 'icons' }>, getIcon: IconSource): void {
  // 1. Background mirrors the tab bar state: no fill / green fill / barTop (fill + border below).
  ctx.fillStyle = spec.fill === 'none' ? TILE_BG : TILE_FILL_GREEN
  ctx.fillRect(0, 0, w, h)

  // 2. Center row mirrors the tab bar's pane-icon presentation (TabItem.tsx
  //    renderIcons): repo icon (or circle letter avatar) first, then up to
  //    MAX_KEY_PANE_ICONS tinted agent icons, then a +N badge for hidden agent
  //    panes (blue when a hidden pane is busy). Unlike TabItem's flex row
  //    (badge = additive 4th sibling), canvas has no auto-layout and
  //    iconLayout only fits 3 slots on-key, so the badge OCCUPIES A ROW SLOT:
  //    when it appears, drawn agent icons shrink so repo + agents + badge
  //    never exceed MAX_ROW_SLOTS — the whole row (badge included) stays
  //    centered and fully on-key. Tabs with no agent panes keep the
  //    repo-icons-only row (up to 3, as before).
  const repoIcons = spec.paneIcons.length > 0 ? spec.icons.slice(0, 1) : spec.icons
  let drawnCount = Math.min(spec.paneIcons.length, MAX_KEY_PANE_ICONS)
  if (spec.paneIcons.length > drawnCount && repoIcons.length + drawnCount + 1 > MAX_ROW_SLOTS) {
    drawnCount = MAX_ROW_SLOTS - 1 - repoIcons.length // give the badge its slot
  }
  const paneIcons = spec.paneIcons.slice(0, drawnCount)
  const hidden = spec.paneIcons.slice(drawnCount)
  const slots = iconLayout(w, h, repoIcons.length + paneIcons.length + (hidden.length > 0 ? 1 : 0))
  repoIcons.forEach((icon, i) => {
    const { x, y, size } = slots[i]
    const bitmap = icon.url && icon.ready ? getIcon(icon.url) : null
    if (bitmap) {
      // Contain-fitted so non-square icons scale symmetrically (never stretched).
      // drawIconContained owns the mandatory 5-arg drawImage rule (dimensionless-SVG
      // drawn-blank trap) - never call drawImage directly for icons.
      drawIconContained(ctx, bitmap, x, y, size)
      return
    }
    // Letter avatar: exact canvas replica of RepoIcon's SVG — circle filling
    // the slot, letter at 9/16 of the diameter, weight 600, white, with
    // RepoIcon's +0.5/16 optical nudge below true center (y=8.5 in a 16-unit box).
    const cx = x + size / 2
    const cy = y + size / 2
    ctx.fillStyle = repoAvatarColor(icon.hue)
    ctx.beginPath()
    ctx.arc(cx, cy, size / 2, 0, Math.PI * 2)
    ctx.fill()
    ctx.font = `600 ${Math.round(size * REPO_AVATAR_FONT_RATIO)}px ${DECK_FONT_STACK}`
    ctx.textBaseline = 'middle'
    ctx.fillStyle = '#ffffff'
    const letterWidth = ctx.measureText(icon.letter).width
    ctx.fillText(icon.letter, Math.round(cx - letterWidth / 2), Math.round(cy + size * (0.5 / 16)))
  })
  paneIcons.forEach((icon, i) => {
    const { x, y, size } = slots[repoIcons.length + i]
    // Mirror the repo-icon pattern above: draw ONLY when buildFrame stamped
    // ready (it consulted the cache with this same memoized URL, which also
    // started the async load). An unready slot stays empty; when the decode
    // completes, the cache notify repaints, buildFrame flips `ready` in the
    // spec JSON, and the controller's diff un-skips this key. Contain-fitted;
    // drawIconContained owns the mandatory 5-arg drawImage rule.
    const bitmap = icon.ready ? getIcon(providerIconDataUrl(icon.provider, PANE_TINT_COLORS[icon.tint])) : null
    if (bitmap) drawIconContained(ctx, bitmap, x, y, size)
  })
  if (hidden.length > 0) {
    // The badge renders horizontally centered inside the row slot reserved
    // for it above (slots.length >= 1 is guaranteed: the badge itself was
    // counted in the iconLayout call), so it can never clip off the key.
    const slot = slots[slots.length - 1]
    const label = `+${hidden.length}`
    ctx.font = `600 ${Math.max(OVERFLOW_FONT_SIZE, Math.round(slot.size / 2))}px ${DECK_FONT_STACK}`
    ctx.textBaseline = 'middle'
    ctx.fillStyle = hidden.some((p) => p.tint === 'blue') ? STATUS_BLUE : STATUS_MUTED
    ctx.fillText(label, Math.round(slot.x + (slot.size - ctx.measureText(label).width) / 2), slot.y + slot.size / 2)
  }

  // 3. Title banner across the top. Tracking is set HERE — after the
  //    icons/avatar/badge draws above (they keep the default spacing) and
  //    before measurement, so fitLabel accounts for it.
  ctx.fillStyle = BANNER_FILL
  ctx.fillRect(0, 0, w, BANNER_HEIGHT)
  ctx.letterSpacing = TEXT_LETTER_SPACING
  // Weight rule: 400 everywhere, EXCEPT where the deck mirrors the app UI —
  // the letter avatar and +N badge keep RepoIcon's 600 (see RepoIcon.tsx).
  ctx.font = `400 ${ICONS_TITLE_FONT_SIZE}px ${DECK_FONT_STACK}`
  ctx.textBaseline = 'top'
  ctx.fillStyle = ACTIVE_COLOR
  const label = fitLabel((t) => ctx.measureText(t).width, truncateTitle(spec.title), w - 2 * TITLE_SIDE_PADDING)
  drawCenteredText(ctx, label, w, Math.round((BANNER_HEIGHT - ICONS_TITLE_FONT_SIZE) / 2))

  // 4. Borders/rings: barTop green border; white ring marks the active tab.
  if (spec.fill === 'barTop') {
    drawRing(ctx, w, h, BAR_TOP_BORDER, 3, 0)
    if (spec.active) drawRing(ctx, w, h, ACTIVE_COLOR, 2, 3)
  } else if (spec.active) {
    drawRing(ctx, w, h, ACTIVE_COLOR, 3, 0)
  }
}

function drawTab(ctx: Ctx2D, w: number, h: number, spec: Extract<KeySpec, { kind: 'tab' }>, getIcon: IconSource): void {
  if (spec.style === 'preview') return drawPreviewTab(ctx, w, h, spec)
  drawIconsTab(ctx, w, h, spec, getIcon)
}

function drawPager(
  ctx: Ctx2D, w: number, h: number,
  spec: Extract<KeySpec, { kind: 'pager' }>,
): void {
  ctx.fillStyle = CONTROL_BG
  ctx.fillRect(0, 0, w, h)
  ctx.letterSpacing = TEXT_LETTER_SPACING
  ctx.textBaseline = 'top'

  ctx.font = `400 ${CONTROL_LABEL_FONT_SIZE}px ${DECK_FONT_STACK}`
  ctx.fillStyle = CONTROL_DIM
  drawCenteredText(ctx, 'PAGE', w, 2)

  ctx.font = `400 ${CONTROL_VALUE_FONT_SIZE}px ${DECK_FONT_STACK}`
  ctx.fillStyle = ACTIVE_COLOR
  drawCenteredText(ctx, `${spec.page}/${spec.pageCount}`, w, (h - CONTROL_VALUE_FONT_SIZE) / 2)

  ctx.font = `400 ${CONTROL_LABEL_FONT_SIZE}px ${DECK_FONT_STACK}`
  ctx.fillStyle = CONTROL_DIM
  drawCenteredText(ctx, 'NEXT >', w, h - CONTROL_LABEL_FONT_SIZE - 4)
}

function drawAction(
  ctx: Ctx2D, w: number, h: number,
  spec: Extract<KeySpec, { kind: 'action' }>,
): void {
  ctx.fillStyle = CONTROL_BG
  ctx.fillRect(0, 0, w, h)
  ctx.letterSpacing = TEXT_LETTER_SPACING

  ctx.font = `400 ${CONTROL_VALUE_FONT_SIZE}px ${DECK_FONT_STACK}`
  ctx.textBaseline = 'top'
  ctx.fillStyle = ACTIVE_COLOR
  drawCenteredText(ctx, ACTION_LABELS[spec.action], w, (h - CONTROL_VALUE_FONT_SIZE) / 2)

  drawRing(ctx, w, h, spec.enabled ? ACTION_RING[spec.action] : DISABLED_ACTION_COLOR, 3, 0)
}

export function renderKey(
  spec: KeySpec,
  caps: DeckCapabilities,
  createCtx: CtxFactory,
  getIcon: IconSource = () => null,
): Uint8ClampedArray {
  const w = caps.keyPixelWidth
  const h = caps.keyPixelHeight
  const ctx = createCtx(w, h)
  beginKeyFrame(ctx, w, h)
  switch (spec.kind) {
    case 'empty':
      ctx.fillStyle = EMPTY_BG
      ctx.fillRect(0, 0, w, h)
      break
    case 'tab':
      drawTab(ctx, w, h, spec, getIcon)
      break
    case 'pager':
      drawPager(ctx, w, h, spec)
      break
    case 'action':
      drawAction(ctx, w, h, spec)
      break
  }
  ctx.restore()
  return ctx.getImageData(0, 0, w, h).data
}

export function renderStrip(text: string, width: number, height: number, createCtx: CtxFactory): Uint8ClampedArray {
  const ctx = createCtx(width, height)
  ctx.fillStyle = EMPTY_BG
  ctx.fillRect(0, 0, width, height)
  ctx.letterSpacing = TEXT_LETTER_SPACING
  ctx.font = `400 ${STRIP_FONT_SIZE}px ${DECK_FONT_STACK}`
  ctx.textBaseline = 'top'
  ctx.fillStyle = ACTIVE_COLOR
  drawCenteredText(ctx, text, width, (height - STRIP_FONT_SIZE) / 2)
  return ctx.getImageData(0, 0, width, height).data
}

/** Pre-Baseline-2023 canvases (Firefox <=111, Safari <=15) lack roundRect; an
 * unguarded call would crash VirtualDeckPanel rendering in those browsers.
 * Degrade to square corners instead. (ctx.letterSpacing needs no guard:
 * assigning it on a non-supporting canvas is an inert expando — no throw, and
 * text stays self-consistent since measureText excludes the tracking there too.) */
export function ensureRoundRect(ctx: CanvasRenderingContext2D): void {
  const c = ctx as CanvasRenderingContext2D & { roundRect?: unknown }
  if (typeof c.roundRect !== 'function') {
    c.roundRect = function (this: CanvasRenderingContext2D, x: number, y: number, w: number, h: number) {
      this.rect(x, y, w, h)
    }
  }
}

export function defaultCtxFactory(width: number, height: number): Ctx2D {
  const canvas = document.createElement('canvas')
  canvas.width = width
  canvas.height = height
  const ctx = canvas.getContext('2d')
  if (!ctx) throw new Error('Canvas 2D context unavailable (defaultCtxFactory is runtime-only; inject a CtxFactory in tests)')
  ensureRoundRect(ctx)
  // lib.dom may predate ctx.letterSpacing; the runtime (Chromium) always has it.
  return ctx as unknown as Ctx2D
}
