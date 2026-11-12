import { ref, onMounted, onBeforeUnmount } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { isTauriRuntime } from '../api/transport'
import { useAppStore } from '../stores/app'

type Status = { visible: boolean; suspended: boolean }

export function useHoverReader(enabled: boolean, persist: () => Promise<unknown>) {
  const app = useAppStore()
  const visible = ref(false)
  const menu = ref(false)
  // Current Codex content: 16 logical px at the user's 120% window zoom.
  const codexFontSize = 19.2
  const savedFontSize = Number(localStorage.getItem('hover-reader-font-size-v2'))
  const fontSize = ref(Number.isFinite(savedFontSize) && savedFontSize >= 11 && savedFontSize <= 32 ? savedFontSize : codexFontSize)
  const lightText = ref(localStorage.getItem('hover-reader-light-text') !== 'false')
  let unlisten: UnlistenFn | undefined
  let disposed = false
  let lastCheckpointAt = performance.now()

  function apply(status: Status) {
    if (disposed) return
    const now = performance.now()
    // Native status heartbeats also checkpoint a stationary reader, so an
    // unexpected process exit loses at most the last few uncheckpointed seconds.
    if (status.visible && now - lastCheckpointAt >= 5000) {
      lastCheckpointAt = now
      void app.settleReadingSession()
    }
    if (visible.value === status.visible) return
    visible.value = status.visible
    if (!status.visible) menu.value = false
    app.setReadingSessionHoverVisible(status.visible)
    if (!status.visible) void persist()
  }
  onMounted(async () => {
    if (!enabled) return
    app.setReadingSessionHoverVisible(false)
    if (!isTauriRuntime()) return
    unlisten = await listen<Status>('hover-reader-visibility', event => apply(event.payload))
    if (disposed) { unlisten(); return }

  })
  onBeforeUnmount(() => { disposed = true; unlisten?.() })

  async function ready() {
    if (!enabled || !isTauriRuntime()) return
    try { await invoke<Status>('hover_reader_ready') }
    catch { await invoke('return_from_hover_reader') }
  }
  function leave() {
    if (!enabled) return
    // Used by an explicit hide action; native tracking controls pointer exits.
    apply({ visible:false, suspended:false })
  }
  async function drag(event?: MouseEvent) {
    if (!enabled || !isTauriRuntime() || (event && !event.altKey)) return
    event?.preventDefault()
    menu.value = false
    await getCurrentWindow().startDragging()
  }
  async function resize(edge: string) {
    if (!enabled || !isTauriRuntime()) return
    menu.value = false
    try { await invoke('resize_hover_reader', { edge }) }
    catch { app.showToast('无法调整悬浮窗口，请重试', 'error') }
  }
  function changeFont(delta: number) {
    fontSize.value = Math.round(Math.max(11, Math.min(32, fontSize.value + delta)) * 10) / 10
    localStorage.setItem('hover-reader-font-size-v2', String(fontSize.value))
  }
  function resetFont() {
    fontSize.value = codexFontSize
    localStorage.removeItem('hover-reader-font-size-v2')
  }
  function toggleTextColor() {
    lightText.value = !lightText.value
    localStorage.setItem('hover-reader-light-text', String(lightText.value))
  }
  async function back() {
    visible.value = false
    app.setReadingSessionHoverVisible(false)
    await Promise.race([persist(), new Promise(resolve => setTimeout(resolve, 1500))])
    await app.settleReadingSession()
    app.stopReadingSession()
    await invoke('return_from_hover_reader')
  }
  return { visible, menu, ready, fontSize, lightText, leave, drag, resize, changeFont, resetFont, toggleTextColor, back }
}
