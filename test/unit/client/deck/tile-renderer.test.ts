import { describe, expect, it } from 'vitest'
import { MINI_CAPS, PLUS_CAPS } from '@/deck/fake-deck-device'
import {
  containIconRect,
  cropPreviewLines, drawRing, fitLabel, iconLayout, keyFrameGeometry, previewGeometry, renderKey, renderStrip, truncateTitle,
  APPROVE_COLOR, ACTIVE_COLOR, DISABLED_ACTION_COLOR, EMPTY_BG, PREVIEW_TEXT_COLOR, PREVIEW_BG, RING_COLORS,
  TILE_BG, TILE_FILL_GREEN, BANNER_FILL, BAR_TOP_BORDER, CONTROL_BG, CONTROL_DIM, STOP_COLOR,
  CONTROL_LABEL_FONT_SIZE, CONTROL_VALUE_FONT_SIZE, ICONS_TITLE_FONT_SIZE, STRIP_FONT_SIZE,
  MAX_KEY_PANE_ICONS, OVERFLOW_FONT_SIZE, BANNER_HEIGHT, ICON_ROW_SIDE_INSET,
  TEXT_LETTER_SPACING, TITLE_SIDE_PADDING,
  ensureRoundRect,
} from '@/deck/tile-renderer'
import { STATUS_GREEN, STATUS_BLUE, STATUS_AMBER, STATUS_RED, STATUS_MUTED, STATUS_MUTED_DIM, PANE_TINT_COLORS } from '@/deck/pane-tint-colors'
import { providerIconDataUrl } from '@/deck/provider-icon-svg'
import type { Ctx2D, IconSource } from '@/deck/tile-renderer'
import { repoAvatarColor, REPO_AVATAR_FONT_RATIO } from '@/components/icons/RepoIcon'
import { DECK_FONT_STACK } from '@/deck/deck-font'
import type { KeySpec, RingColor } from '@/deck/frame'

type Rect = { x: number; y: number; w: number; h: number; style: string }
type Text = { text: string; x: number; y: number; style: string; font: string; letterSpacing: string }
type Img = { x: number; y: number; w: number; h: number }
type Circle = { cx: number; cy: number; r: number; style: string }
type RRect = { x: number; y: number; w: number; h: number; r: number }
type Stroke = RRect & { style: string; lineWidth: number }
type Measure = { text: string; letterSpacing: string }

function recordingCtx(width: number, height: number) {
  const rects: Rect[] = []
  const texts: Text[] = []
  const images: Img[] = []
  const circles: Circle[] = []
  const clips: RRect[] = []
  const strokes: Stroke[] = []
  const measures: Measure[] = []
  let pendingArc: { cx: number; cy: number; r: number } | null = null
  let pendingRound: RRect | null = null
  let saves = 0
  let restores = 0
  const ctx = {
    fillStyle: '#000000' as string,
    strokeStyle: '#000000' as string,
    lineWidth: 0,
    font: '',
    letterSpacing: '',
    textBaseline: 'alphabetic' as CanvasTextBaseline,
    fillRect(x: number, y: number, w: number, h: number) {
      rects.push({ x, y, w, h, style: String(this.fillStyle) })
    },
    fillText(text: string, x: number, y: number) {
      texts.push({ text, x, y, style: String(this.fillStyle), font: this.font, letterSpacing: this.letterSpacing })
    },
    drawImage(_src: CanvasImageSource, x: number, y: number, w: number, h: number) {
      images.push({ x, y, w, h })
    },
    beginPath() {
      pendingArc = null
      pendingRound = null
    },
    arc(cx: number, cy: number, r: number) {
      pendingArc = { cx, cy, r }
    },
    roundRect(x: number, y: number, w: number, h: number, r = 0) {
      pendingRound = { x, y, w, h, r }
    },
    clip() {
      if (pendingRound) clips.push(pendingRound)
      pendingRound = null
      pendingArc = null
    },
    stroke() {
      if (pendingRound) strokes.push({ ...pendingRound, style: String(this.strokeStyle), lineWidth: this.lineWidth })
      pendingRound = null
      pendingArc = null
    },
    fill() {
      if (pendingArc) circles.push({ ...pendingArc, style: String(this.fillStyle) })
      pendingArc = null
      pendingRound = null
    },
    save() {
      saves++
    },
    restore() {
      restores++
    },
    measureText(t: string) {
      measures.push({ text: t, letterSpacing: this.letterSpacing })
      const ls = parseFloat(this.letterSpacing) || 0
      return { width: t.length * (6 + ls) } as TextMetrics
    },
    getImageData() {
      return { data: new Uint8ClampedArray(width * height * 4) } as ImageData
    },
  } as unknown as Ctx2D
  return { ctx, rects, texts, images, circles, clips, strokes, measures, getSaves: () => saves, getRestores: () => restores }
}

describe('title fitting', () => {
  it('truncateTitle caps at 10 chars with ellipsis', () => {
    expect(truncateTitle('short')).toBe('short')
    expect(truncateTitle('exactly-10')).toBe('exactly-10')
    expect(truncateTitle('longer-than-ten')).toBe('longer-th…')
  })
  it('fitLabel pixel-fits with ellipsis', () => {
    const measure = (t: string) => t.length * 6
    expect(fitLabel(measure, 'abcdef', 100)).toBe('abcdef')
    expect(fitLabel(measure, 'abcdefghij', 30)).toBe('abcd…')
  })
  it('icons banner title is 14px and vertically centered in the 20px banner', () => {
    // Literal pins on purpose: an interpolated pin cannot catch drift.
    expect(ICONS_TITLE_FONT_SIZE).toBe(14)
    const { texts } = renderTab(tabSpec({ title: 'build' }))
    const title = texts.find((t) => t.text === 'build')
    expect(title?.font).toBe('400 14px Inter, sans-serif')
    // y = Math.round((BANNER_HEIGHT - ICONS_TITLE_FONT_SIZE) / 2) = (20 - 14) / 2 = 3
    expect(title?.y).toBe(3)
  })
  it('classic preview banner stays PINNED at literal 16px sans-serif (does not follow the icons shrink)', () => {
    const { texts } = renderTab(previewSpec({ title: 'build', previewLines: ['$ ls'] }))
    expect(texts.find((t) => t.text === 'build')?.font).toBe('16px sans-serif')
  })
})

describe('drawRing', () => {
  it('drawRing strokes a rounded rect that follows the key frame', () => {
    const rec = recordingCtx(80, 80)
    drawRing(rec.ctx, 80, 80, '#ffffff', 3, 0)
    // margin 3, radius 10 => off = 3 + 0 + 1.5 = 4.5, r = 10 - 1.5 = 8.5
    expect(rec.strokes[0]).toEqual({ x: 4.5, y: 4.5, w: 71, h: 71, r: 8.5, style: '#ffffff', lineWidth: 3 })
    expect(rec.rects).toHaveLength(0)

    const inner = recordingCtx(80, 80)
    drawRing(inner.ctx, 80, 80, '#ffffff', 2, 3)
    // off = 3 + 3 + 1 = 7, r = 10 - 3 - 1 = 6
    expect(inner.strokes[0]).toEqual({ x: 7, y: 7, w: 66, h: 66, r: 6, style: '#ffffff', lineWidth: 2 })
  })
})

describe('previewGeometry', () => {
  it('matches the hardware-anchored values', () => {
    expect(previewGeometry(120, 120)).toEqual({ lines: 8, columns: 21 })
    expect(previewGeometry(80, 80)).toEqual({ lines: 5, columns: 14 })
    expect(previewGeometry(72, 72)).toEqual({ lines: 4, columns: 12 })
  })
})

describe('cropPreviewLines', () => {
  it('drops trailing blanks, keeps last N lines and first M columns', () => {
    const lines = ['one', 'two-is-longer-than-five', 'three', '', '   ']
    expect(cropPreviewLines(lines, 2, 5)).toEqual(['two-i', 'three'])
  })
})

const tabSpec = (over: Partial<Extract<KeySpec, { kind: 'tab'; style: 'icons' }>> = {}): KeySpec => ({
  kind: 'tab', style: 'icons', tabId: 't1', title: 'build',
  active: false, fill: 'none', paneIcons: [], icons: [], ...over,
})

function previewSpec(overrides: Partial<Extract<KeySpec, { kind: 'tab'; style: 'preview' }>> = {}): KeySpec {
  return {
    kind: 'tab' as const, style: 'preview' as const, tabId: 't1', title: 'Tab 1',
    active: false, previewLines: ['$ npm test', 'PASS'], ring: null as RingColor,
    ...overrides,
  }
}

function renderTab(spec: KeySpec, getIcon?: IconSource) {
  let captured: ReturnType<typeof recordingCtx> | null = null
  const factory = (w: number, h: number) => {
    captured = recordingCtx(w, h)
    return captured.ctx
  }
  const out = renderKey(spec, MINI_CAPS, factory, getIcon)
  const { rects, texts, images, circles, clips, strokes, measures, getSaves, getRestores } = captured!
  return { out, rects, texts, images, circles, clips, strokes, measures, getSaves, getRestores }
}

describe('renderKey', () => {
  it('no-fill tile: pure-black bg, banner, white title, no rings, no dot, no preview text', () => {
    const { out, rects, texts, strokes } = renderTab(tabSpec())
    expect(out).toBeInstanceOf(Uint8ClampedArray)
    expect(rects[1]).toMatchObject({ x: 0, y: 0, w: 80, h: 80, style: TILE_BG })
    expect(rects.some((r) => r.y === 0 && r.h === 20 && r.style.startsWith('rgba'))).toBe(true) // banner
    expect(texts.some((t) => t.text === 'build' && t.style === '#ffffff')).toBe(true)           // title
    expect(strokes).toHaveLength(0) // no rings
    expect(texts.filter((t) => t.style === PREVIEW_TEXT_COLOR)).toHaveLength(0) // no preview text anywhere on the tile
  })

  it('green fill state paints the darker composited green background', () => {
    const { rects } = renderTab(tabSpec({ fill: 'green' }))
    expect(rects[1]).toMatchObject({ x: 0, y: 0, w: 80, h: 80, style: TILE_FILL_GREEN })
  })

  it('barTop state paints the darker composited green background + full-strength 3px green border ring', () => {
    const { rects, strokes } = renderTab(tabSpec({ fill: 'barTop', active: true }))
    expect(rects[1].style).toBe(TILE_FILL_GREEN)
    expect(strokes).toContainEqual({ x: 4.5, y: 4.5, w: 71, h: 71, r: 8.5, style: BAR_TOP_BORDER, lineWidth: 3 })
    // active tab keeps its white ring nested inside the border
    expect(strokes).toContainEqual({ x: 7, y: 7, w: 66, h: 66, r: 6, style: ACTIVE_COLOR, lineWidth: 2 })
  })

  it('active tab without fill gets the plain white ring', () => {
    const { strokes } = renderTab(tabSpec({ active: true }))
    expect(strokes).toContainEqual({ x: 4.5, y: 4.5, w: 71, h: 71, r: 8.5, style: ACTIVE_COLOR, lineWidth: 3 })
  })

  it('ready icon draws via drawImage at the centered layout slot', () => {
    const bitmap = {} as CanvasImageSource
    const { images } = renderTab(
      tabSpec({ icons: [{ url: '/i/a', letter: 'A', hue: 120, ready: true }] }),
      (url) => (url === '/i/a' ? bitmap : null),
    )
    const [slot] = iconLayout(80, 80, 1)
    expect(images).toEqual([{ x: slot.x, y: slot.y, w: slot.size, h: slot.size }])
  })

  it('unready or letter-only icon draws RepoIcon\'s circle avatar + centered white letter', () => {
    const { rects, texts, images, circles } = renderTab(
      tabSpec({ icons: [{ url: null, letter: 'B', hue: 200, ready: false }] }),
    )
    expect(images).toHaveLength(0)
    const slot = iconLayout(80, 80, 1)[0]
    // Exact replica of RepoIcon's SVG: full-slot circle, shared color function.
    expect(circles).toEqual([
      { cx: slot.x + slot.size / 2, cy: slot.y + slot.size / 2, r: slot.size / 2, style: repoAvatarColor(200) },
    ])
    // The old square swatch is gone.
    expect(rects.some((r) => r.style === repoAvatarColor(200))).toBe(false)
    const letter = texts.find((t) => t.text === 'B')
    expect(letter?.style).toBe('#ffffff')
    // 9/16 of the diameter, weight 600 (slot.size is 45 on the 80x80 Mini -> 25px).
    expect(letter?.font).toBe(`600 ${Math.round(slot.size * REPO_AVATAR_FONT_RATIO)}px ${DECK_FONT_STACK}`)
  })

  it('no status dot: a plain icons tile draws only the frame surround, the background and the banner', () => {
    const { rects } = renderTab(tabSpec())
    // frame surround + background + banner — nothing else (the dot used to be an extra rect)
    expect(rects).toHaveLength(3)
    expect(rects[1]).toMatchObject({ x: 0, y: 0, w: 80, h: 80, style: TILE_BG })
    expect(rects[2]).toMatchObject({ x: 0, y: 0, w: 80, h: 20, style: BANNER_FILL })
  })

  it('iconLayout: 1 icon centered ~50% larger; 3 icons clamp to fit inside the rounded frame', () => {
    const one = iconLayout(80, 80, 1)
    expect(one[0].size).toBe(45) // round(min(80, 60) * 0.75)
    expect(one[0].x).toBe(Math.round((80 - 45) / 2)) // 18
    expect(one[0].y).toBe(Math.round(20 + (60 - 45) / 2)) // 28
    expect(one[0].y).toBeGreaterThanOrEqual(BANNER_HEIGHT) // clear of the banner

    const two = iconLayout(80, 80, 2)
    expect(two).toHaveLength(2)
    expect(two.every((s) => s.size === 27)).toBe(true) // round(60 * 0.45), fits unclamped

    const three = iconLayout(80, 80, 3)
    expect(three).toHaveLength(3)
    expect(three.every((s) => s.size === 20)).toBe(true) // clamped: floor((80 - 12 - 2*3) / 3)
    expect(three[1].x - three[0].x).toBe(20 + 3) // size + gap
    const last = three[2]
    expect(last.x + last.size).toBeLessThanOrEqual(80 - ICON_ROW_SIDE_INSET) // on-frame guarantee

    const threeSmall = iconLayout(72, 72, 3)
    expect(threeSmall).toHaveLength(3)
    expect(threeSmall.every((s) => s.size === 18)).toBe(true) // floor((72 - 12 - 6) / 3)
    expect(threeSmall[0].x).toBeGreaterThanOrEqual(ICON_ROW_SIDE_INSET)
  })

  it('pager key renders PAGE / n/m / NEXT > on the control background', () => {
    let cap: ReturnType<typeof recordingCtx> | null = null
    renderKey({ kind: 'pager', page: 2, pageCount: 3 }, MINI_CAPS, (w, h) => (cap = recordingCtx(w, h)).ctx)
    const { rects, texts } = cap!
    expect(rects[1].style).toBe(CONTROL_BG)
    expect(texts.map((t) => t.text)).toEqual(expect.arrayContaining(['PAGE', '2/3', 'NEXT >']))
  })

  it('disabled action key gets the grey ring; enabled approve gets green', () => {
    const strokesFor = (enabled: boolean) => {
      let cap: ReturnType<typeof recordingCtx> | null = null
      renderKey({ kind: 'action', action: 'approve', enabled }, MINI_CAPS, (w, h) => (cap = recordingCtx(w, h)).ctx)
      return cap!.strokes
    }
    expect(strokesFor(false)).toContainEqual({ x: 4.5, y: 4.5, w: 71, h: 71, r: 8.5, style: DISABLED_ACTION_COLOR, lineWidth: 3 })
    expect(strokesFor(true)).toContainEqual({ x: 4.5, y: 4.5, w: 71, h: 71, r: 8.5, style: APPROVE_COLOR, lineWidth: 3 })
  })
})

describe('rounded key frame', () => {
  it('keyFrameGeometry: margin 3 below 96px (else 4), radius 12% of key size', () => {
    expect(keyFrameGeometry(72, 72)).toEqual({ margin: 3, radius: 9 })
    expect(keyFrameGeometry(80, 80)).toEqual({ margin: 3, radius: 10 })
    expect(keyFrameGeometry(96, 96)).toEqual({ margin: 4, radius: 12 })
    expect(keyFrameGeometry(120, 120)).toEqual({ margin: 4, radius: 14 })
  })

  it('every key kind paints a pure-black surround then clips to the rounded frame', () => {
    // 80x80 Mini caps => frame margin 3, radius 10, inner 74x74.
    const empty = renderTab({ kind: 'empty' })
    for (const rec of [
      empty,
      renderTab(tabSpec()),
      renderTab(previewSpec()),
      renderTab({ kind: 'pager', page: 2, pageCount: 3 }),
      renderTab({ kind: 'action', action: 'approve', enabled: true }),
    ]) {
      expect(rec.rects[0]).toMatchObject({ x: 0, y: 0, w: 80, h: 80, style: EMPTY_BG })
      expect(rec.clips[0]).toEqual({ x: 3, y: 3, w: 74, h: 74, r: 10 })
      // Verify save/restore balance: each key rendering saves and restores
      const saves = typeof rec.getSaves === 'function' ? rec.getSaves() : 0
      const restores = typeof rec.getRestores === 'function' ? rec.getRestores() : 0
      expect(saves).toBe(restores)
      expect(saves).toBeGreaterThanOrEqual(1)
    }
    // Empty kind: the frame surround plus the empty case's own fill (now
    // clipped and pixel-identical) - two full-bleed EMPTY_BG rects.
    expect(empty.rects).toHaveLength(2)
    expect(empty.rects[1]).toMatchObject({ x: 0, y: 0, w: 80, h: 80, style: EMPTY_BG })
  })
})

describe('renderKey preview style', () => {
  it('draws preview text in the preview color under the title banner', () => {
    const { texts } = renderTab(previewSpec())
    const previewTexts = texts.filter((t) => t.style === PREVIEW_TEXT_COLOR)
    expect(previewTexts.map((t) => t.text)).toEqual(['$ npm test', 'PASS'])
  })

  it('status ring + active tab draws the status ring plus the white inner ring', () => {
    const { strokes } = renderTab(previewSpec({ ring: 'green', active: true }))
    expect(strokes).toContainEqual({ x: 4.5, y: 4.5, w: 71, h: 71, r: 8.5, style: RING_COLORS.green, lineWidth: 3 })
    expect(strokes).toContainEqual({ x: 7, y: 7, w: 66, h: 66, r: 6, style: ACTIVE_COLOR, lineWidth: 2 }) // white inner ring
  })

  it('amber ring renders for a waiting-for-approval tab', () => {
    const { strokes } = renderTab(previewSpec({ ring: 'amber' }))
    expect(strokes).toContainEqual({ x: 5, y: 5, w: 70, h: 70, r: 8, style: RING_COLORS.amber, lineWidth: 4 })
  })

  it('icons style still renders fills (dispatch regression)', () => {
    const { rects } = renderTab(tabSpec({ fill: 'green' }))
    expect(rects.some((r) => r.style === TILE_FILL_GREEN)).toBe(true) // composited green fill (emerald-100 @ 50% over black)
  })
})

describe('fonts (Inter)', () => {
  it('icons tile: banner title renders regular-weight (400) Inter; avatar letter keeps RepoIcon 600', () => {
    const { texts } = renderTab(
      tabSpec({ title: 'build', icons: [{ url: null, letter: 'B', hue: 200, ready: false }] }),
    )
    const title = texts.find((t) => t.text === 'build')
    expect(title?.font).toBe('400 14px Inter, sans-serif')
    const letter = texts.find((t) => t.text === 'B')
    const slot = iconLayout(80, 80, 1)[0]
    expect(letter?.font).toBe(`600 ${Math.round(slot.size * (9 / 16))}px ${DECK_FONT_STACK}`)
  })

  it('pager: labels and page count render 400 Inter', () => {
    const { texts } = renderTab({ kind: 'pager', page: 2, pageCount: 3 })
    expect(texts.find((t) => t.text === 'PAGE')?.font).toBe(`400 ${CONTROL_LABEL_FONT_SIZE}px ${DECK_FONT_STACK}`)
    expect(texts.find((t) => t.text === 'NEXT >')?.font).toBe(`400 ${CONTROL_LABEL_FONT_SIZE}px ${DECK_FONT_STACK}`)
    expect(texts.find((t) => t.text === '2/3')?.font).toBe(`400 ${CONTROL_VALUE_FONT_SIZE}px ${DECK_FONT_STACK}`)
  })

  it('action key labels render in 400 Inter', () => {
    const { texts } = renderTab({ kind: 'action', action: 'approve', enabled: true })
    expect(texts.find((t) => t.text === 'APPROVE')?.font).toBe(`400 ${CONTROL_VALUE_FONT_SIZE}px ${DECK_FONT_STACK}`)
  })

  it('strip text renders in 400 Inter', () => {
    let captured: ReturnType<typeof recordingCtx> | null = null
    const factory = (w: number, h: number) => {
      captured = recordingCtx(w, h)
      return captured.ctx
    }
    renderStrip('hello', 800, 100, factory)
    expect(captured!.texts.find((t) => t.text === 'hello')?.font).toBe(`400 ${STRIP_FONT_SIZE}px ${DECK_FONT_STACK}`)
  })

  it('classic preview tile is PINNED: monospace body, sans-serif banner', () => {
    const { texts } = renderTab(previewSpec({ title: 'build', previewLines: ['$ ls'] }))
    expect(texts.find((t) => t.text === '$ ls')?.font).toBe('11px monospace')
    expect(texts.find((t) => t.text === 'build')?.font).toBe('16px sans-serif')
  })
})

describe('letter spacing', () => {
  it('icons banner, pager, action, and strip text carry TEXT_LETTER_SPACING; avatar, badge, and classic preview do not', () => {
    // Icons tile: letter avatar + enough agent panes that two fold into a +2 badge.
    const paneIcons = Array.from({ length: 3 }, () => ({ provider: 'claude', tint: 'green' as const, ready: true }))
    const icons = renderTab(
      tabSpec({ title: 'build', icons: [{ url: null, letter: 'B', hue: 200, ready: false }], paneIcons }),
      () => ({} as CanvasImageSource),
    )
    expect(icons.texts.find((t) => t.text === 'build')?.letterSpacing).toBe(TEXT_LETTER_SPACING)
    expect(icons.texts.find((t) => t.text === 'B')?.letterSpacing).toBe('')  // avatar keeps default
    expect(icons.texts.find((t) => t.text === '+2')?.letterSpacing).toBe('') // badge keeps default

    // Pager: all three texts carry the tracking.
    const pager = renderTab({ kind: 'pager', page: 2, pageCount: 3 })
    for (const label of ['PAGE', '2/3', 'NEXT >']) {
      expect(pager.texts.find((t) => t.text === label)?.letterSpacing).toBe(TEXT_LETTER_SPACING)
    }

    // Action label carries the tracking.
    const action = renderTab({ kind: 'action', action: 'approve', enabled: true })
    expect(action.texts.find((t) => t.text === 'APPROVE')?.letterSpacing).toBe(TEXT_LETTER_SPACING)

    // Touch-strip text carries the tracking.
    let strip: ReturnType<typeof recordingCtx> | null = null
    renderStrip('hello', 800, 100, (w, h) => (strip = recordingCtx(w, h)).ctx)
    expect(strip!.texts.find((t) => t.text === 'hello')?.letterSpacing).toBe(TEXT_LETTER_SPACING)

    // Classic preview is PINNED: body lines AND banner keep the default ''.
    const preview = renderTab(previewSpec({ title: 'build', previewLines: ['$ ls', 'PASS'] }))
    expect(preview.texts.length).toBeGreaterThan(0)
    for (const t of preview.texts) expect(t.letterSpacing).toBe('')
  })

  it('the icons title is measured with spacing applied and fits within w - 2 * TITLE_SIDE_PADDING', () => {
    // 72px-wide caps: maxWidth = 72 - 2 * 6 = 60. With spacing set the stub
    // measures chars * 6.4, so a 10-char title (64 > 60) must truncate to
    // 'ABCDEFGH…' (9 * 6.4 = 57.6 <= 60).
    expect(72 - 2 * TITLE_SIDE_PADDING).toBe(60)
    const caps = { ...MINI_CAPS, keyPixelWidth: 72, keyPixelHeight: 72 }
    const rec = recordingCtx(72, 72)
    renderKey(tabSpec({ title: 'ABCDEFGHIJ' }), caps, () => rec.ctx)
    const title = rec.texts.find((t) => t.text.includes('…'))
    expect(title?.text).toBe('ABCDEFGH…')
    // Measurement happened WITH the spacing already set:
    expect(rec.measures.find((m) => m.text === 'ABCDEFGHIJ')?.letterSpacing).toBe(TEXT_LETTER_SPACING)

    // On the 80px key the same 10-char title still fits: 64 <= 80 - 12.
    const wide = renderTab(tabSpec({ title: 'ABCDEFGHIJ' }))
    expect(wide.texts.find((t) => t.text === 'ABCDEFGHIJ')).toBeDefined()
  })
})

describe('palette derives from the app UI tokens (mapping block in tile-renderer.ts)', () => {
  it('matches the documented app-token values', () => {
    expect(TILE_BG).toBe('#000000')          // deck-only pure black (matches EMPTY_BG surround)
    expect(TILE_FILL_GREEN).toBe('#697d73')  // emerald-100 #d1fae5 @ 50% over black, precomputed
    expect(BAR_TOP_BORDER).toBe('#21c45d')   // --success: hsl(142 71% 45%)
    expect(STATUS_GREEN).toBe('#21c45d')     // text-success (pane running tint)
    expect(STATUS_BLUE).toBe('#3b82f6')      // text-blue-500 (pane busy tint)
    expect(STATUS_AMBER).toBe('#f59f0a')     // --warning: hsl(38 92% 50%) (text-warning)
    expect(STATUS_RED).toBe('#dc2828')       // --destructive light: hsl(0 72% 51%) (text-destructive)
    expect(STATUS_MUTED).toBe('#a1a1aa')     // text-muted-foreground dark: hsl(240 5% 65%)
    expect(STATUS_MUTED_DIM).toBe('rgba(161,161,170,0.4)') // text-muted-foreground/40 dark
    expect(ACTIVE_COLOR).toBe('#ffffff')     // white active ring
    expect(CONTROL_BG).toBe('#27272a')       // bg-muted dark
    expect(CONTROL_DIM).toBe('#a1a1aa')      // text-muted-foreground dark
    expect(APPROVE_COLOR).toBe('#21c45d')    // --success
    expect(STOP_COLOR).toBe('#dc2828')       // --destructive light: hsl(0 72% 51%)
  })

  it('TILE_FILL_GREEN is emerald-100 composited at 50% opacity over black (round(c/2) per channel)', () => {
    // #d1fae5 = rgb(209,250,229) -> rgb(105,125,115) = #697d73. Same hue, darker.
    const composite = ['d1', 'fa', 'e5']
      .map((h) => Math.round(parseInt(h, 16) / 2).toString(16).padStart(2, '0'))
      .join('')
    expect(TILE_FILL_GREEN).toBe(`#${composite}`)
    // Borders stay full strength: the barTop BORDER is NOT darkened.
    expect(BAR_TOP_BORDER).toBe('#21c45d')
    expect(ACTIVE_COLOR).toBe('#ffffff')
  })

  it('classic previews palette is PINNED', () => {
    expect(PREVIEW_BG).toBe('#0a0a0a')
    expect(PREVIEW_TEXT_COLOR).toBe('#a8a8a8')
    expect(RING_COLORS).toEqual({ amber: '#f59e0b', green: '#22c55e', blue: '#3b82f6' })
  })
})

describe('agent pane icons (tab-bar presentation)', () => {
  const repoIcon = { url: null, letter: 'B', hue: 200, ready: false }
  const bitmap = {} as CanvasImageSource

  it('requests the tinted provider icon (only when ready) and draws it beside the repo avatar', () => {
    const requested: string[] = []
    const { images, circles } = renderTab(
      tabSpec({ icons: [repoIcon], paneIcons: [{ provider: 'claude', tint: 'blue', ready: true }] }),
      (url) => { requested.push(url); return bitmap },
    )
    expect(requested).toEqual([providerIconDataUrl('claude', STATUS_BLUE)])
    const slots = iconLayout(80, 80, 2)
    // Slot 0: letter avatar circle; slot 1: the tinted agent icon.
    expect(circles[0].cx).toBe(slots[0].x + slots[0].size / 2)
    expect(images).toEqual([{ x: slots[1].x, y: slots[1].y, w: slots[1].size, h: slots[1].size }])
  })

  it('folds hidden agent icons into a +N badge that occupies a centered, on-key row slot', () => {
    const paneIcons = [
      { provider: 'claude', tint: 'green' as const, ready: true },
      { provider: 'codex', tint: 'green' as const, ready: true },
      { provider: 'gemini', tint: 'green' as const, ready: true },
      { provider: 'opencode', tint: 'blue' as const, ready: true },
    ]
    const requested: string[] = []
    const { texts } = renderTab(tabSpec({ icons: [repoIcon], paneIcons }), (url) => { requested.push(url); return bitmap })
    // The badge occupies the third row slot, so with a repo icon present only
    // ONE agent icon is drawn (repo + 1 agent + badge = MAX_ROW_SLOTS = 3);
    // the other three panes fold into +3.
    expect(requested).toEqual([providerIconDataUrl('claude', PANE_TINT_COLORS.green)])
    const badge = texts.find((t) => t.text === '+3')
    expect(badge).toBeDefined()
    // A hidden pane is busy -> blue badge (TabItem's overflow rule).
    expect(badge?.style).toBe(STATUS_BLUE)
    expect(badge?.font).toBe(`600 ${OVERFLOW_FONT_SIZE}px ${DECK_FONT_STACK}`)
    // Badge geometry: horizontally centered in the reserved LAST slot of the
    // 3-slot row, vertically at slot middle - so the whole composition (badge
    // included) is centered and never clips off the key. The harness stubs
    // measureText at 6px/char ('+3' -> 12).
    const last = iconLayout(80, 80, 3)[2]
    expect(badge?.x).toBe(Math.round(last.x + (last.size - 12) / 2))
    expect(badge?.y).toBe(last.y + last.size / 2)
    expect((badge?.x ?? Number.NaN) + 12).toBeLessThanOrEqual(80 - ICON_ROW_SIDE_INSET) // fully inside the rounded frame
  })

  it('+N badge font scales with its slot (Plus-size key exercises the scaling branch)', () => {
    // 120px key, repo avatar + 4 agent panes => repo + 1 agent + badge = 3 slots.
    const paneIcons = [
      { provider: 'claude', tint: 'green' as const, ready: true },
      { provider: 'codex', tint: 'green' as const, ready: true },
      { provider: 'gemini', tint: 'green' as const, ready: true },
      { provider: 'opencode', tint: 'green' as const, ready: true },
    ]
    let cap: ReturnType<typeof recordingCtx> | null = null
    renderKey(tabSpec({ icons: [repoIcon], paneIcons }), PLUS_CAPS, (w, h) => (cap = recordingCtx(w, h)).ctx, () => bitmap)
    const badge = cap!.texts.find((t) => t.text === '+3')
    const slot = iconLayout(120, 120, 3)[2]
    expect(badge?.font).toBe(`600 ${Math.max(OVERFLOW_FONT_SIZE, Math.round(slot.size / 2))}px ${DECK_FONT_STACK}`)
    // The scaling branch is live here: slot 34 => 17px, not the 10px floor.
    expect(badge?.font).toBe(`600 17px ${DECK_FONT_STACK}`)
  })

  it('badge is muted when no hidden pane is busy; MAX_KEY_PANE_ICONS binds when no repo icon competes for slots', () => {
    const paneIcons = Array.from({ length: 3 }, () => ({ provider: 'claude', tint: 'green' as const, ready: true }))
    const { texts, images } = renderTab(tabSpec({ icons: [], paneIcons }), () => bitmap)
    // No repo icon: 2 agent icons + badge fill the 3 slots exactly.
    expect(images).toHaveLength(MAX_KEY_PANE_ICONS)
    expect(texts.find((t) => t.text === '+1')?.style).toBe(STATUS_MUTED)
  })

  it('with agent icons present, repo icons collapse to the first one', () => {
    const icons = [repoIcon, { ...repoIcon, letter: 'C', hue: 10 }, { ...repoIcon, letter: 'D', hue: 20 }]
    const { circles } = renderTab(tabSpec({ icons, paneIcons: [{ provider: 'claude', tint: 'green', ready: true }] }), () => bitmap)
    expect(circles).toHaveLength(1) // only the first repo avatar
  })

  it('without agent panes, up to 3 repo icons render exactly as before', () => {
    const icons = [repoIcon, { ...repoIcon, letter: 'C', hue: 10 }, { ...repoIcon, letter: 'D', hue: 20 }]
    const { circles } = renderTab(tabSpec({ icons, paneIcons: [] }))
    expect(circles).toHaveLength(3)
  })

  it('an unready pane icon draws nothing and never hits the icon source (slot fills on the cache-notify repaint, which flips ready in the spec)', () => {
    const requested: string[] = []
    const { images } = renderTab(
      tabSpec({ icons: [], paneIcons: [{ provider: 'claude', tint: 'green', ready: false }] }),
      (url) => { requested.push(url); return bitmap },
    )
    expect(images).toHaveLength(0)
    expect(requested).toHaveLength(0) // buildFrame owns load-starting; the renderer only draws
  })

  it('a ready flag with a cache miss (probe-failed/evicted bitmap) still draws nothing', () => {
    const { images } = renderTab(tabSpec({ icons: [], paneIcons: [{ provider: 'claude', tint: 'green', ready: true }] }), () => null)
    expect(images).toHaveLength(0)
  })
})

describe('aspect-preserving icon fit', () => {
  // Single-icon slot on the 80x80 Mini: { x: 18, y: 28, size: 45 }.
  const slot = iconLayout(80, 80, 1)[0]

  it('containIconRect letterboxes wide bitmaps and pillarboxes tall ones, centered in the slot', () => {
    const wide = { width: 200, height: 100 } as unknown as CanvasImageSource
    // scale = 45/200 -> 45x23 (round(22.5) = 23), vertically centered.
    expect(containIconRect(wide, 10, 20, 45)).toEqual({ x: 10, y: 31, w: 45, h: 23 })
    const tall = { width: 100, height: 200 } as unknown as CanvasImageSource
    expect(containIconRect(tall, 10, 20, 45)).toEqual({ x: 21, y: 20, w: 23, h: 45 })
  })

  it('containIconRect keeps square AND dimensionless bitmaps at the full square (drawn-blank trap)', () => {
    const square = { width: 64, height: 64 } as unknown as CanvasImageSource
    expect(containIconRect(square, 10, 20, 45)).toEqual({ x: 10, y: 20, w: 45, h: 45 })
    // Dimensionless (viewBox-only) SVGs report 0x0 - explicit square dims MUST survive.
    const dimensionless = { naturalWidth: 0, naturalHeight: 0 } as unknown as CanvasImageSource
    expect(containIconRect(dimensionless, 10, 20, 45)).toEqual({ x: 10, y: 20, w: 45, h: 45 })
    expect(containIconRect({} as CanvasImageSource, 10, 20, 45)).toEqual({ x: 10, y: 20, w: 45, h: 45 })
  })

  it('containIconRect prefers intrinsic naturalWidth/naturalHeight over layout width/height', () => {
    const img = { naturalWidth: 200, naturalHeight: 100, width: 50, height: 50 } as unknown as CanvasImageSource
    expect(containIconRect(img, 0, 0, 45)).toEqual({ x: 0, y: 11, w: 45, h: 23 })
  })

  it('containIconRect never collapses below 1px on extreme aspect ratios', () => {
    const sliver = { width: 1000, height: 10 } as unknown as CanvasImageSource
    expect(containIconRect(sliver, 0, 0, 45)).toEqual({ x: 0, y: 22, w: 45, h: 1 })
  })

  it('a wide repo icon draws contained in its slot, not stretched to the square', () => {
    const bitmap = { naturalWidth: 200, naturalHeight: 100 } as unknown as CanvasImageSource
    const { images } = renderTab(
      tabSpec({ icons: [{ url: '/i/wide', letter: 'W', hue: 120, ready: true }] }),
      (url) => (url === '/i/wide' ? bitmap : null),
    )
    expect(images).toEqual([{ x: slot.x, y: slot.y + 11, w: 45, h: 23 }])
  })

  it('a non-square agent pane icon draws contained in its slot', () => {
    const bitmap = { width: 32, height: 16 } as unknown as CanvasImageSource
    const { images } = renderTab(
      tabSpec({ icons: [], paneIcons: [{ provider: 'claude', tint: 'green', ready: true }] }),
      () => bitmap,
    )
    expect(images).toEqual([{ x: slot.x, y: slot.y + 11, w: 45, h: 23 }])
  })
})

describe('ensureRoundRect', () => {
  it('installs roundRect delegate on stubs lacking it', () => {
    const recta: unknown[] = []
    const stub = {
      rect: function (x: number, y: number, w: number, h: number) {
        recta.push({ x, y, w, h })
      },
    } as unknown as CanvasRenderingContext2D
    ensureRoundRect(stub)
    ;(stub.roundRect as any)(10, 20, 30, 40, 5)
    expect(recta).toEqual([{ x: 10, y: 20, w: 30, h: 40 }])
  })

  it('leaves existing roundRect function untouched', () => {
    const calls: unknown[] = []
    const existing = function (x: number) {
      calls.push({ x })
    }
    const stub = { roundRect: existing } as unknown as CanvasRenderingContext2D
    ensureRoundRect(stub)
    expect(stub.roundRect).toBe(existing)
    ;(stub.roundRect as any)(42)
    expect(calls).toEqual([{ x: 42 }])
  })
})
