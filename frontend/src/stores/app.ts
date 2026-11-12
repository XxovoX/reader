import { defineStore } from 'pinia'
import { ref, watch, computed } from 'vue'
import {
  applyDesktopVersionUpdate,
  cancelDesktopVersionUpdate,
  dismissVersionUpdate,
  getVersionUpdate,
} from '../api/update'
import { addReadingStats } from '../api/readingStats'
import { isTauriRuntime, isWebUiMode } from '../api/transport'
import type { DesktopUpdateProgress, VersionUpdateInfo } from '../types'
import { applySystemTheme } from '../utils/systemUi'
import type { LegadoWebdavConfig } from '../api/webdav'
import { invokeRaw } from '../api/invoke'
import { normalizeBossKeyShortcut } from '../utils/bossKey'

export const useAppStore = defineStore('app', () => {
  const STATS_KEY = 'reader-stats'
  const CLOSE_TO_TRAY_KEY = 'reader-close-to-tray'
  const LEGADO_WEBDAV_KEY = 'reader-legado-webdav'
  const LEGADO_SYNC_ENABLED_KEY = 'reader-legado-sync-enabled'
  const BOSS_KEY_KEY = 'reader-boss-key'
  const NETWORK_PROXY_MODE_KEY = 'reader-network-proxy-mode'
  const NETWORK_PROXY_URL_KEY = 'reader-network-proxy-url'
  // ─── Theme ───
  const savedTheme = localStorage.getItem('theme') as 'light' | 'dark' | null
  const legacyReaderNight = localStorage.getItem('reader-isNight') === 'true'
  const theme = ref<'light' | 'dark'>(
    savedTheme || (legacyReaderNight ? 'dark' : 'light')
  )

  function setTheme(value: 'light' | 'dark') {
    theme.value = value
    localStorage.setItem('theme', value)
    applySystemTheme(value)
  }

  function toggleTheme() {
    setTheme(theme.value === 'light' ? 'dark' : 'light')
  }

  watch(theme, (val) => {
    localStorage.setItem('theme', val)
    applySystemTheme(val)
  }, { immediate: true })

  // ─── Window close behavior ───
  const closeToTray = ref(localStorage.getItem(CLOSE_TO_TRAY_KEY) === 'true')

  function setCloseToTray(value: boolean) {
    closeToTray.value = value
    localStorage.setItem(CLOSE_TO_TRAY_KEY, String(value))
  }

  function toggleCloseToTray() {
    setCloseToTray(!closeToTray.value)
  }

  const legadoWebdavConfig = ref<LegadoWebdavConfig>(loadLegadoWebdavConfig())

  function loadLegadoWebdavConfig(): LegadoWebdavConfig {
    try {
      const value = JSON.parse(localStorage.getItem(LEGADO_WEBDAV_KEY) || '{}')
      return {
        url: typeof value.url === 'string' ? value.url : '',
        account: typeof value.account === 'string' ? value.account : '',
        password: typeof value.password === 'string' ? value.password : '',
        directory: typeof value.directory === 'string' && value.directory.trim() ? value.directory : 'legado',
      }
    } catch {
      return { url: '', account: '', password: '', directory: 'legado' }
    }
  }

  function setLegadoWebdavConfig(value: LegadoWebdavConfig) {
    const next = {
      url: value.url.trim(),
      account: value.account.trim(),
      password: value.password,
      directory: value.directory?.trim() || 'legado',
    }
    legadoWebdavConfig.value = next
    localStorage.setItem(LEGADO_WEBDAV_KEY, JSON.stringify(next))
  }

  const legadoSyncEnabled = ref(localStorage.getItem(LEGADO_SYNC_ENABLED_KEY) !== 'disabled')

  function setLegadoSyncEnabled(value: boolean) {
    legadoSyncEnabled.value = value
    localStorage.setItem(LEGADO_SYNC_ENABLED_KEY, value ? 'enabled' : 'disabled')
  }

  const bossKeyEnabled = ref(localStorage.getItem(BOSS_KEY_KEY) === 'enabled')
  const savedBossKeyShortcut = localStorage.getItem(`${BOSS_KEY_KEY}-shortcut`)
  const bossKeyShortcut = ref(normalizeBossKeyShortcut(savedBossKeyShortcut))
  if (savedBossKeyShortcut !== bossKeyShortcut.value) {
    localStorage.setItem(`${BOSS_KEY_KEY}-shortcut`, bossKeyShortcut.value)
  }

  async function applyBossKey() {
    if (!isTauriRuntime()) return
    await invokeRaw('configure_boss_key', {
      shortcut: bossKeyEnabled.value ? bossKeyShortcut.value : null,
    })
  }

  async function setBossKeyEnabled(value: boolean) {
    const previous = bossKeyEnabled.value
    bossKeyEnabled.value = value
    try {
      await applyBossKey()
      localStorage.setItem(BOSS_KEY_KEY, value ? 'enabled' : 'disabled')
    } catch (error) {
      bossKeyEnabled.value = previous
      throw error
    }
  }

  async function setBossKeyShortcut(value: string) {
    const normalizedValue = normalizeBossKeyShortcut(value)
    const previous = bossKeyShortcut.value
    bossKeyShortcut.value = normalizedValue
    try {
      await applyBossKey()
      localStorage.setItem(`${BOSS_KEY_KEY}-shortcut`, normalizedValue)
    } catch (error) {
      bossKeyShortcut.value = previous
      await applyBossKey().catch(() => undefined)
      throw error
    }
  }

  // ─── Network proxy ───
  type NetworkProxyMode = 'system' | 'manual'
  type NetworkProxyStatus = {
    mode: NetworkProxyMode
    active: boolean
    address?: string | null
  }
  const savedProxyMode = localStorage.getItem(NETWORK_PROXY_MODE_KEY)
  const networkProxyMode = ref<NetworkProxyMode>(savedProxyMode === 'manual' ? 'manual' : 'system')
  const networkProxyUrl = ref(localStorage.getItem(NETWORK_PROXY_URL_KEY) || '')
  const networkProxyStatus = ref<NetworkProxyStatus | null>(null)

  async function applyNetworkProxy(
    mode: NetworkProxyMode = networkProxyMode.value,
    proxyUrl: string = networkProxyUrl.value,
  ) {
    if (!isTauriRuntime()) return null
    const status = await invokeRaw<NetworkProxyStatus>('configure_network_proxy', {
      mode,
      proxyUrl: mode === 'manual' ? proxyUrl.trim() : null,
    })
    networkProxyStatus.value = status
    return status
  }

  async function setNetworkProxy(mode: NetworkProxyMode, proxyUrl: string) {
    const normalizedUrl = proxyUrl.trim()
    const status = await applyNetworkProxy(mode, normalizedUrl)
    networkProxyMode.value = mode
    networkProxyUrl.value = normalizedUrl
    localStorage.setItem(NETWORK_PROXY_MODE_KEY, mode)
    localStorage.setItem(NETWORK_PROXY_URL_KEY, normalizedUrl)
    return status
  }

  // ─── 隐藏功能（在设置里可隐藏统计、RSS 等导航入口）───
  const HIDDEN_FEATURES_KEY = 'reader-hidden-features'
  const hiddenFeatures = ref<Set<string>>(new Set(readHiddenFeatures()))

  function readHiddenFeatures(): string[] {
    try {
      const raw = localStorage.getItem(HIDDEN_FEATURES_KEY)
      if (!raw) return []
      const parsed = JSON.parse(raw)
      return Array.isArray(parsed) ? parsed.filter((x) => typeof x === 'string') : []
    } catch {
      return []
    }
  }

  function persistHiddenFeatures() {
    localStorage.setItem(HIDDEN_FEATURES_KEY, JSON.stringify(Array.from(hiddenFeatures.value)))
  }

  function isFeatureHidden(key: string): boolean {
    return hiddenFeatures.value.has(key)
  }

  function toggleHiddenFeature(key: string) {
    if (hiddenFeatures.value.has(key)) {
      hiddenFeatures.value.delete(key)
    } else {
      hiddenFeatures.value.add(key)
    }
    persistHiddenFeatures()
  }

  // ─── User (single-user desktop: no auth) ───
  const AUTO_CHECK_UPDATE_KEY = 'reader-auto-check-update'
  const initialAutoCheckUpdate = (() => {
    try {
      return localStorage.getItem(AUTO_CHECK_UPDATE_KEY) !== '0'
    } catch {
      return true
    }
  })()
  /** 启动时自动检查版本更新并在有更新时弹窗提示。 */
  const autoCheckUpdate = ref(initialAutoCheckUpdate)

  function setAutoCheckUpdate(value: boolean) {
    autoCheckUpdate.value = value
    try {
      localStorage.setItem(AUTO_CHECK_UPDATE_KEY, value ? '1' : '0')
    } catch {
      // Ignore unavailable storage; keep runtime state anyway.
    }
  }

  const versionUpdate = ref<VersionUpdateInfo | null>(null)
  const versionUpdateLoading = ref(false)
  const desktopUpdateLoading = ref(false)
  const desktopUpdateProgress = ref<DesktopUpdateProgress | null>(null)
  const versionUpdateChecked = ref(false)
  let versionUpdateToastVersion = ''
  const canCheckVersionUpdate = computed(() => true)
  const hasVersionUpdateReminder = computed(() => !!versionUpdate.value?.shouldRemind)

  async function checkVersionUpdate(
    force = false,
    opts: { notify?: 'toast' | 'dialog'; silent?: boolean } = {},
  ) {
    if (versionUpdateLoading.value) return versionUpdate.value
    versionUpdateLoading.value = true
    try {
      const info = await getVersionUpdate(force)
      versionUpdate.value = info
      versionUpdateChecked.value = true
      if (info.shouldRemind && info.latestVersion && versionUpdateToastVersion !== info.latestVersion) {
        versionUpdateToastVersion = info.latestVersion
        if (opts.notify === 'dialog') {
          void promptVersionUpdate(info)
        } else {
          showToast(`发现新版本 ${info.latestVersion}`, 'warning')
        }
      }
      return info
    } catch (error) {
      if (force && !opts.silent) {
        showToast((error as Error).message || '检查更新失败', 'error')
      }
      return null
    } finally {
      versionUpdateLoading.value = false
    }
  }

  async function promptVersionUpdate(info: VersionUpdateInfo) {
    const go = await confirmDialog(
      `最新版本 ${info.latestVersion} 已发布，是否立即更新？\n（下载已自动接入系统/应用代理，直连失败时会尝试镜像加速）`,
      { title: '发现新版本' },
    )
    if (!go) return
    // 直接在更新进度弹窗中展示下载进度并开始应用内更新。
    updateDialogVisible.value = true
    const result = await applyDesktopUpdate()
    if (!result && desktopUpdateProgress.value?.stage !== 'failed') {
      // 未真正开始下载(如已是最新版本), 不停留在进度弹窗。
      updateDialogVisible.value = false
    }
  }

  /** 应用启动时的自动检查, 受「自动检查更新」开关控制。 */
  async function runStartupVersionCheck() {
    if (!autoCheckUpdate.value) return
    // 强制拉新: 启动检查若读缓存, 上次检查后新发布的版本要等缓存过期才可见,
    // 用户感知就是「打开了应用却没有更新弹窗」。每次启动一次请求远低于限额。
    // 失败静默: 离线/网络波动时不弹错误打扰。
    await checkVersionUpdate(true, { notify: 'dialog', silent: true })
  }

  async function dismissVersionUpdateReminder(version = versionUpdate.value?.latestVersion || '') {
    if (!version) {
      showToast('当前没有可忽略的版本', 'warning')
      return null
    }
    versionUpdateLoading.value = true
    try {
      const info = await dismissVersionUpdate(version)
      versionUpdate.value = info
      versionUpdateToastVersion = version
      showToast('已忽略当前版本更新提醒', 'success')
      return info
    } catch (error) {
      showToast((error as Error).message || '忽略版本失败', 'error')
      return null
    } finally {
      versionUpdateLoading.value = false
    }
  }

  // ─── UI State ───
  const showLoginModal = ref(false)
  const showSettingsDrawer = ref(false)
  /** 更新进度弹窗(确认更新后直接展示下载进度, 不跳转设置抽屉)。 */
  const updateDialogVisible = ref(false)

  function closeUpdateProgressDialog() {
    updateDialogVisible.value = false
  }
  const showSourceManager = ref(false)
  const showUserManager = ref(false)
  const showWebdavManager = ref(false)
  const showBookmarkManager = ref(false)
  const isOnline = ref(typeof navigator !== 'undefined' ? navigator.onLine : true)

  const initialReadingStats = (() => {
    const fallback = {
      totalSeconds: 0,
      openedBooks: [],
      readChapters: [],
      completedBooks: [],
    }
    try {
      const saved = JSON.parse(localStorage.getItem(STATS_KEY) || '{}')
      return {
        ...fallback,
        ...saved,
        totalSeconds: Number.isFinite(saved?.totalSeconds) ? Math.max(0, saved.totalSeconds) : 0,
        openedBooks: Array.isArray(saved?.openedBooks) ? saved.openedBooks.filter((item: unknown) => typeof item === 'string') : [],
        readChapters: Array.isArray(saved?.readChapters) ? saved.readChapters.filter((item: unknown) => typeof item === 'string') : [],
        completedBooks: Array.isArray(saved?.completedBooks) ? saved.completedBooks.filter((item: unknown) => typeof item === 'string') : [],
      }
    } catch {
      return fallback
    }
  })()

  const readingStats = ref<{
    totalSeconds: number
    openedBooks: string[]
    readChapters: string[]
    completedBooks: string[]
  }>(initialReadingStats)
  let readingSessionStartedAt: number | null = null
  // Keep each book's fractional second across hide/show, settlement and book switches.
  const READING_REMAINDER_PREFIX = 'reader-reading-remainder:'
  const READING_EVENT_PREFIX = 'reader-reading-event:'
  type ReadingEvent = Parameters<typeof addReadingStats>[0] & { eventId: string }
  let readingStatsQueuedFlushes = 0
  let readingSessionBook: { bookUrl: string; bookName: string; bookAuthor: string } | null = null
  const readingSessionActive = ref(false)
  let readingSessionVisibilityBound = false
  let readingSessionHasWindowFocus = true
  let readingSessionSuspended = false
  let readingSessionHoverVisible: boolean | null = null
  let readingStatsFlushQueue = Promise.resolve()

  /** 单次上报上限: 服务端同样限制 24h, 长会话分段避免整段被截断。 */
  const READING_STATS_FLUSH_CHUNK_SECONDS = 60 * 60

  function persistStats() {
    localStorage.setItem(STATS_KEY, JSON.stringify(readingStats.value))
  }

  function readingSessionShouldRun() {
    if (!readingSessionBook?.bookUrl || readingSessionSuspended) return false
    if (typeof document !== 'undefined' && document.visibilityState === 'hidden') return false
    if (readingSessionHoverVisible !== null) return readingSessionHoverVisible
    return readingSessionHasWindowFocus
  }

  function resumeReadingSessionClock() {
    if (!readingSessionShouldRun()) {
      pauseReadingSessionClock()
      return
    }
    if (readingSessionStartedAt === null) {
      readingSessionStartedAt = performance.now()
    }
    readingSessionActive.value = true
  }

  function pauseReadingSessionClock() {
    flushReadingSession()
    readingSessionActive.value = false
  }

  function handleReadingSessionVisibilityChange() {
    if (document.visibilityState === 'hidden') {
      pauseReadingSessionClock()
    } else {
      resumeReadingSessionClock()
    }
  }

  function handleReadingSessionWindowBlur() {
    readingSessionHasWindowFocus = false
    // Non-activating overlays deliberately leave keyboard focus in Codex.
    resumeReadingSessionClock()
  }

  function handleReadingSessionWindowFocus() {
    readingSessionHasWindowFocus = true
    resumeReadingSessionClock()
  }

  function bindReadingSessionVisibility() {
    if (readingSessionVisibilityBound || typeof document === 'undefined') return
    document.addEventListener('visibilitychange', handleReadingSessionVisibilityChange)
    window.addEventListener('blur', handleReadingSessionWindowBlur)
    window.addEventListener('focus', handleReadingSessionWindowFocus)
    window.addEventListener('pagehide', pauseReadingSessionClock)
    window.addEventListener('beforeunload', pauseReadingSessionClock)
    window.addEventListener('pageshow', resumeReadingSessionClock)
    readingSessionVisibilityBound = true
  }

  function flushReadingSession() {
    if (readingSessionStartedAt === null || !readingSessionBook) return
    const book = readingSessionBook
    const elapsed = Math.max(0, performance.now() - readingSessionStartedAt)
    readingSessionStartedAt = null
    readingSessionActive.value = false
    const remainderKey = READING_REMAINDER_PREFIX + book.bookUrl
    const savedRemainder = Number(localStorage.getItem(remainderKey))
    const milliseconds = (Number.isFinite(savedRemainder) && savedRemainder >= 0 && savedRemainder < 1000 ? savedRemainder : 0) + elapsed
    const seconds = Math.floor(milliseconds / 1000)
    localStorage.setItem(remainderKey, String(milliseconds - seconds * 1000))
    if (seconds > 0) flushReadingSessionStats(book, seconds)
  }

  function newReadingEventId() {
    if (typeof crypto.randomUUID === 'function') return crypto.randomUUID()
    // getRandomValues also works on the supported LAN HTTP web reader.
    const bytes = crypto.getRandomValues(new Uint8Array(16))
    bytes[6] = (bytes[6]! & 0x0f) | 0x40
    bytes[8] = (bytes[8]! & 0x3f) | 0x80
    const hex = Array.from(bytes, byte => byte.toString(16).padStart(2, '0')).join('')
    return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
  }

  function flushReadingSessionStats(book: NonNullable<typeof readingSessionBook>, remaining: number) {
    const total = remaining
    readingStats.value.totalSeconds += total
    persistStats()

    const now = new Date()
    const date = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`
    while (remaining > 0) {
      const seconds = Math.min(remaining, READING_STATS_FLUSH_CHUNK_SECONDS)
      const event: ReadingEvent = { eventId: newReadingEventId(), seconds, date, ...book }
      // One key per event prevents main/hover windows overwriting each other's queue.
      localStorage.setItem(READING_EVENT_PREFIX + event.eventId, JSON.stringify(event))
      remaining -= seconds
    }
    queueReadingStatsFlush()
  }

  function pendingReadingEventKeys() {
    const keys: string[] = []
    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i)
      if (key?.startsWith(READING_EVENT_PREFIX)) keys.push(key)
    }
    return keys
  }

  function queueReadingStatsFlush() {
    readingStatsQueuedFlushes++
    readingStatsFlushQueue = readingStatsFlushQueue.then(async () => {
      for (const key of pendingReadingEventKeys()) {
        const saved = localStorage.getItem(key)
        if (!saved) continue
        try {
          const event = JSON.parse(saved) as ReadingEvent
          await addReadingStats(event)
          localStorage.removeItem(key)
        } catch {
          // Retain the event for the next settlement/restart. The server receipt
          // makes replay safe even when a write succeeded but its reply was lost.
          break
        }
      }
    }).finally(() => { readingStatsQueuedFlushes-- })
  }

  // Recover unfinished writes even when reopening only the library/stats page.
  if (pendingReadingEventKeys().length > 0) queueReadingStatsFlush()

  async function applyDesktopUpdate() {
    if (desktopUpdateLoading.value) return null
    if (!versionUpdate.value?.updateAvailable) {
      showToast('当前没有可安装的新版本', 'warning')
      return null
    }
    desktopUpdateLoading.value = true
    desktopUpdateProgress.value = {
      stage: 'checking',
      percent: 0,
      downloaded: 0,
      total: 0,
      message: '正在确认最新版本',
    }
    try {
      const result = await applyDesktopVersionUpdate((progress) => {
        desktopUpdateProgress.value = progress
      })
      showToast(result.message, 'success')
      return result
    } catch (error) {
      if (desktopUpdateProgress.value?.stage !== 'failed') {
        desktopUpdateProgress.value = {
          stage: 'failed',
          percent: null,
          downloaded: desktopUpdateProgress.value?.downloaded || 0,
          total: desktopUpdateProgress.value?.total || 0,
          message: '更新失败',
        }
      }
      showToast((error as Error).message || '更新失败', 'error')
      return null
    } finally {
      desktopUpdateLoading.value = false
    }
  }

  async function cancelDesktopUpdate() {
    if (!desktopUpdateLoading.value) return false
    try {
      const result = await cancelDesktopVersionUpdate()
      return Boolean(result?.cancelled)
    } catch {
      return false
    }
  }

  function setReadingSessionBook(bookUrl?: string, bookName?: string, bookAuthor?: string) {
    const normalizedUrl = bookUrl?.trim() || ''
    const nextBook = normalizedUrl
      ? { bookUrl: normalizedUrl, bookName: bookName?.trim() || '未命名书籍', bookAuthor: bookAuthor?.trim() || '' }
      : null

    if (readingSessionBook?.bookUrl === nextBook?.bookUrl) {
      readingSessionBook = nextBook
      return
    }

    const wasRunning = readingSessionStartedAt !== null
    if (wasRunning) flushReadingSession()
    readingSessionBook = nextBook
    if (wasRunning) resumeReadingSessionClock()
  }

  function startReadingSession(bookUrl?: string, bookName?: string, bookAuthor?: string) {
    setReadingSessionBook(bookUrl, bookName, bookAuthor)
    bindReadingSessionVisibility()
    resumeReadingSessionClock()
  }

  function setReadingSessionHoverVisible(visible: boolean | null) {
    readingSessionHoverVisible = visible
    if (visible === false) pauseReadingSessionClock()
    else resumeReadingSessionClock()
  }

  function setReadingSessionSuspended(suspended: boolean) {
    readingSessionSuspended = suspended
    if (suspended) pauseReadingSessionClock()
    else resumeReadingSessionClock()
  }

  async function settleReadingSession() {
    const wasRunning = readingSessionActive.value
    flushReadingSession()
    if (wasRunning) resumeReadingSessionClock()
    if (readingStatsQueuedFlushes === 0) queueReadingStatsFlush()
    await readingStatsFlushQueue
  }

  function stopReadingSession() {
    pauseReadingSessionClock()
    readingSessionBook = null
    readingSessionSuspended = false
    readingSessionHoverVisible = null
    readingSessionActive.value = false
  }

  function markBookOpened(bookUrl: string) {
    if (!readingStats.value.openedBooks.includes(bookUrl)) {
      readingStats.value.openedBooks.push(bookUrl)
      persistStats()
    }
  }

  function markChapterRead(bookUrl: string, index: number, totalChapters: number) {
    const key = `${bookUrl}#${index}`
    const isFirstRead = !readingStats.value.readChapters.includes(key)
    if (isFirstRead) readingStats.value.readChapters.push(key)
    if (totalChapters > 0 && index >= totalChapters - 1 && !readingStats.value.completedBooks.includes(bookUrl)) {
      readingStats.value.completedBooks.push(bookUrl)
    }
    persistStats()
    return isFirstRead
  }

  const readingStatsSummary = computed(() => {
    const totalMinutes = Math.floor(readingStats.value.totalSeconds / 60)
    const hours = Math.floor(totalMinutes / 60)
    const minutes = totalMinutes % 60
    return {
      totalSeconds: readingStats.value.totalSeconds,
      totalTimeText: hours ? `${hours}小时${minutes}分钟` : `${Math.max(1, totalMinutes)}分钟`,
      openedBooks: readingStats.value.openedBooks.length,
      readChapters: readingStats.value.readChapters.length,
      completedBooks: readingStats.value.completedBooks.length,
    }
  })

  // ─── Toast ───
  const toasts = ref<Array<{ id: number; message: string; type: string }>>([])
  let toastId = 0
  /** 记录自动消失的定时器, 点击关闭时一并清掉, 避免留下悬空回调。 */
  const toastTimers = new Map<number, ReturnType<typeof setTimeout>>()

  /** 手动关闭一条提示(点击提示本身)。 */
  function dismissToast(id: number) {
    const timer = toastTimers.get(id)
    if (timer !== undefined) {
      clearTimeout(timer)
      toastTimers.delete(id)
    }
    toasts.value = toasts.value.filter((t) => t.id !== id)
  }

  function showToast(message: string, type: 'success' | 'error' | 'warning' = 'success') {
    const id = ++toastId
    toasts.value.push({ id, message, type })
    toastTimers.set(id, setTimeout(() => dismissToast(id), 3000))
  }

  function setOnlineStatus(value: boolean) {
    isOnline.value = value
  }

  // ─── 全局确认弹窗（替代原生 confirm，避免 Tauri 里显示 tauri.localhost）───
  const confirmState = ref<{
    message: string
    title?: string
    danger?: boolean
    resolve: (ok: boolean) => void
  } | null>(null)

  function confirmDialog(message: string, opts: { title?: string; danger?: boolean } = {}): Promise<boolean> {
    return new Promise((resolve) => {
      confirmState.value = { message, title: opts.title, danger: opts.danger, resolve }
    })
  }

  function resolveConfirm(ok: boolean) {
    confirmState.value?.resolve(ok)
    confirmState.value = null
  }

  // ─── 迷你模式(小窗置顶) ───
  // 状态以 Rust 侧 window-state.json 为准(桌面壳启动时要据此决定建窗尺寸),
  // 前端只镜像一份驱动紧凑布局, 因此不写 localStorage。
  const miniMode = ref(false)

  async function syncWindowMode() {
    if (!isTauriRuntime()) return
    try {
      const mode = await invokeRaw<{ miniMode: boolean; alwaysOnTop: boolean }>('get_window_mode')
      miniMode.value = Boolean(mode?.miniMode)
    } catch {
      // 查询失败时保持默认(非迷你), 不影响正常使用。
    }
  }

  async function toggleMiniMode() {
    if (!isTauriRuntime()) {
      throw new Error('迷你模式仅在桌面应用可用')
    }
    const mode = await invokeRaw<{ miniMode: boolean; alwaysOnTop: boolean }>('toggle_mini_mode')
    miniMode.value = Boolean(mode?.miniMode)
    showToast(miniMode.value ? '已进入迷你模式' : '已退出迷你模式', 'success')
    return mode
  }

  // ─── Web 精简模式 ───
  /**
   * 是否运行在浏览器里(由 `src/server` 托管, 不是 Tauri WebView)。
   *
   * Web 端的目标只是"用手机躺着看书", 因此**砍掉一切与阅读无关的入口**:
   * 书海(需多源搜索, 走 ipc::Channel 未实现)、统计、RSS、书源管理、WebDAV、
   * 版本更新、迷你模式、老板键……留着它们只会让人以为功能坏了。
   *
   * 判据统一走 `api/transport` 的 `isWebUiMode()` —— 项目里曾经散着五份
   * `'__TAURI_INTERNALS__' in window` 字面量, 改一处漏一处就会出现"界面按
   * 桌面端渲染、接口却按浏览器发"这种自相矛盾的状态。
   */
  const isWebMode = computed(() => isWebUiMode())

  /** 集成的导航项在 Web 端是否该显示。 */
  function isNavVisible(key: 'home' | 'explore' | 'recent' | 'rss'): boolean {
    if (isWebMode.value) {
      // Web 端只保留书架与最近阅读 —— 其余都没有可用的后端能力。
      return key === 'home' || key === 'recent'
    }
    if (key === 'recent') return !isFeatureHidden('stats')
    if (key === 'rss') return !isFeatureHidden('rss')
    return true
  }

  return {
    theme, setTheme, toggleTheme,
    miniMode, syncWindowMode, toggleMiniMode,
    closeToTray, setCloseToTray, toggleCloseToTray,
    legadoWebdavConfig, setLegadoWebdavConfig, legadoSyncEnabled, setLegadoSyncEnabled,
    bossKeyEnabled, bossKeyShortcut, applyBossKey, setBossKeyEnabled, setBossKeyShortcut,
    networkProxyMode, networkProxyUrl, networkProxyStatus, applyNetworkProxy, setNetworkProxy,
    versionUpdate, versionUpdateLoading, desktopUpdateLoading, desktopUpdateProgress, versionUpdateChecked, canCheckVersionUpdate, hasVersionUpdateReminder,
    autoCheckUpdate, setAutoCheckUpdate, runStartupVersionCheck,
    checkVersionUpdate, dismissVersionUpdateReminder, applyDesktopUpdate, cancelDesktopUpdate,
    showLoginModal, showSettingsDrawer, showSourceManager, showUserManager, showWebdavManager,
    showBookmarkManager,
    updateDialogVisible, closeUpdateProgressDialog,
    isOnline, setOnlineStatus,
    readingStats, readingStatsSummary, startReadingSession, stopReadingSession, setReadingSessionBook,
    setReadingSessionSuspended, setReadingSessionHoverVisible, settleReadingSession, readingSessionActive,
    markBookOpened, markChapterRead,
    toasts, showToast, dismissToast,
    confirmState, confirmDialog, resolveConfirm,
    hiddenFeatures, isFeatureHidden, toggleHiddenFeature,
    isWebMode, isNavVisible,
  }
})
