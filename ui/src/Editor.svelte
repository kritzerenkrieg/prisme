<script lang="ts">
  // The editor for one file (B5). Its state (text, undo history, selection)
  // lives in the buffer, so switching tabs and back loses nothing.
  import { onDestroy, onMount } from 'svelte'
  import type { Buffer } from './lib/buffers'
  import { onPinch } from './lib/api'
  import { EditorView, openSearchPanel, redo, undo } from './lib/editor'

  interface Props {
    buffer: Buffer
    /** Source line at the top of the view, with a fraction for the way into it. */
    onscrollline: (line: number) => void
    /** The text size (`--doc-scale`), and a touchpad pinch asking for another. */
    scale?: number
    onzoom?: (scale: number) => void
  }
  let { buffer, onscrollline, scale = 1, onzoom }: Props = $props()

  let host = $state<HTMLElement>()
  let view: EditorView | undefined
  let quiet = false

  onMount(() => {
    view = new EditorView({ state: buffer.state, parent: host! })
    view.scrollDOM.addEventListener('scroll', onscroll)
    if (buffer.scroll) requestAnimationFrame(() => scrollToLine(buffer.scroll))
    view.focus()
  })

  // A touchpad pinch (sent here by App.svelte) sizes the text; the line at
  // the top stays there.
  $effect(() => {
    const el = host
    if (!el || !onzoom) return
    return onPinch(el, () => scale, (next) => {
      const keep = topLine()
      onzoom(next)
      requestAnimationFrame(() => {
        view?.requestMeasure()
        scrollToLine(keep)
      })
    })
  })

  // The text size changed (Ctrl+=, a pinch, Settings): CodeMirror measures its lines again.
  $effect(() => {
    void scale
    view?.requestMeasure()
  })

  onDestroy(() => {
    if (!view) return
    // The scroll position is kept as it changes: by now the view is detached.
    buffer.state = view.state
    view.destroy()
  })

  function topLine(): number {
    if (!view) return 1
    const top = view.scrollDOM.scrollTop
    const block = view.lineBlockAtHeight(top)
    const line = view.state.doc.lineAt(block.from).number
    return line + Math.max(0, Math.min(1, (top - block.top) / (block.height || 1)))
  }

  let queued = false
  let report = false
  function onscroll() {
    // Scrolls asked for by `scrollToLine` are not reported back.
    if (!quiet) report = true
    if (queued) return
    queued = true
    requestAnimationFrame(() => {
      queued = false
      buffer.scroll = topLine()
      if (report) onscrollline(buffer.scroll)
      report = false
    })
  }

  /** Scroll so `line` (fraction allowed) is at the top; does not report back. */
  export function scrollToLine(line: number) {
    if (!view) return
    const doc = view.state.doc
    const n = Math.min(Math.max(1, Math.floor(line)), doc.lines)
    const block = view.lineBlockAt(doc.line(n).from)
    quiet = true
    view.scrollDOM.scrollTop = block.top + (line - Math.floor(line)) * block.height
    requestAnimationFrame(() => (quiet = false))
  }

  /** Put the cursor on `line` and show it (a path clicked in a terminal). */
  export function goToLine(line: number) {
    if (!view) return
    const doc = view.state.doc
    const pos = doc.line(Math.min(Math.max(1, line), doc.lines)).from
    view.dispatch({ selection: { anchor: pos }, effects: EditorView.scrollIntoView(pos, { y: 'start' }) })
    view.focus()
  }

  /** Replace the whole text (the file was loaded again from disk). */
  export function setText(text: string) {
    if (!view) return
    const head = Math.min(view.state.selection.main.head, text.length)
    view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text }, selection: { anchor: head } })
  }

  export function focus() {
    view?.focus()
  }

  /** CodeMirror's find and replace panel. */
  export function find() {
    if (view) openSearchPanel(view)
  }

  // The Edit menu (N2).
  export function undoEdit() {
    if (view) undo(view)
    view?.focus()
  }

  export function redoEdit() {
    if (view) redo(view)
    view?.focus()
  }

  /** The selected text ('' when nothing is selected). */
  export function selected(): string {
    if (!view) return ''
    return view.state.sliceDoc(view.state.selection.main.from, view.state.selection.main.to)
  }

  /** Type `text` over the selection. */
  export function insert(text: string) {
    if (!view) return
    view.dispatch(view.state.replaceSelection(text), { scrollIntoView: true, userEvent: 'input.paste' })
    view.focus()
  }
</script>

<div class="editor" bind:this={host}></div>
