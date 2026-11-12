import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useAppStore } from './app'
import { addReadingStats } from '../api/readingStats'

vi.mock('../api/readingStats', () => ({
  addReadingStats: vi.fn(() => Promise.resolve({ saved: true })),
}))

function installBrowserGlobals() {
  const values = new Map<string, string>()
  const documentListeners = new Map<string, () => void>()
  const windowListeners = new Map<string, () => void>()
  vi.stubGlobal('localStorage', {
    get length() { return values.size },
    key: (index: number) => [...values.keys()][index] ?? null,
    getItem: vi.fn((key: string) => values.get(key) ?? null),
    setItem: vi.fn((key: string, value: string) => values.set(key, value)),
    removeItem: vi.fn((key: string) => values.delete(key)),
  })
  vi.stubGlobal('navigator', { onLine: true })
  const style = () => ({ setProperty: vi.fn() })
  const head = { appendChild: vi.fn() }
  const documentStub = {
    visibilityState: 'visible',
    documentElement: { setAttribute: vi.fn(), style: style() },
    body: { style: style() },
    head,
    getElementById: vi.fn(() => null),
    querySelector: vi.fn(() => null),
    createElement: vi.fn(() => ({ setAttribute: vi.fn(), parentNode: null })),
    addEventListener: vi.fn((type: string, handler: () => void) => {
      documentListeners.set(type, handler)
    }),
    removeEventListener: vi.fn(),
  }
  vi.stubGlobal('document', documentStub)
  vi.stubGlobal('requestAnimationFrame', vi.fn((callback: FrameRequestCallback) => {
    callback(0)
    return 0
  }))
  vi.stubGlobal('window', {
    addEventListener: vi.fn((type: string, handler: () => void) => {
      windowListeners.set(type, handler)
    }),
    removeEventListener: vi.fn(),
  })
  return {
    values,
    /** 触发窗口失焦/聚焦, 对应真实的 pause → resume 路径。 */
    blur: () => windowListeners.get('blur')?.(),
    focus: () => windowListeners.get('focus')?.(),
    setVisibility: (state: 'visible' | 'hidden') => {
      documentStub.visibilityState = state
      documentListeners.get('visibilitychange')?.()
    },
  }
}

describe('app reading session clock', () => {
  let browser: ReturnType<typeof installBrowserGlobals>

  beforeEach(() => {
    vi.restoreAllMocks()
    vi.mocked(addReadingStats).mockReset().mockResolvedValue({ saved: true })
    setActivePinia(createPinia())
    browser = installBrowserGlobals()
    vi.useFakeTimers({ toFake: ['Date', 'performance', 'setTimeout', 'clearTimeout', 'setInterval', 'clearInterval'] })
  })

  afterEach(() => {
    vi.useRealTimers()
    vi.unstubAllGlobals()
  })

  it('keeps counting after switching to another book', () => {
    const store = useAppStore()
    store.startReadingSession('book-a', '第一本', '作者')
    vi.advanceTimersByTime(10_000)

    // 换书: setReadingSessionBook 会 flush 旧书时长并立刻 resume 新书计时。
    store.setReadingSessionBook('book-b', '第二本', '作者')
    vi.advanceTimersByTime(5_000)

    store.stopReadingSession()

    // 10s(第一本) + 5s(第二本) 都要计入；换书后计时不能冻结。
    expect(store.readingStats.totalSeconds).toBe(15)
  })

  it('does not double count while the session keeps running', () => {
    const store = useAppStore()
    store.startReadingSession('book-a', '第一本', '作者')
    vi.advanceTimersByTime(4_000)

    // 同一本书重复启动只更新书名, 不重置也不重复累加。
    store.startReadingSession('book-a', '第一本(改名)', '作者')
    vi.advanceTimersByTime(2_000)

    store.stopReadingSession()

    expect(store.readingStats.totalSeconds).toBe(6)
  })

  /** 失焦会 flush 并暂停; 重新聚焦必须能继续累计。
   *
   *  修复前 `flushReadingSession` 把起点归零却留着「正在计时」标志, 于是
   *  `resumeReadingSessionClock` 空转 —— 真实表现是切走一次后台再回来, 时长
   *  不再增长。 */
  it('resumes counting after the window regains focus', () => {
    const store = useAppStore()
    store.startReadingSession('book-a', '第一本', '作者')
    vi.advanceTimersByTime(6_000)

    browser.blur()
    // 失焦期间的时间不计入。
    vi.advanceTimersByTime(30_000)
    browser.focus()
    vi.advanceTimersByTime(4_000)

    store.stopReadingSession()

    expect(store.readingStats.totalSeconds).toBe(10)
  })

  /** 页面隐藏同样暂停, 回到可见后继续。 */
  it('resumes counting after the page becomes visible again', () => {
    const store = useAppStore()
    store.startReadingSession('book-a', '第一本', '作者')
    vi.advanceTimersByTime(5_000)

    browser.setVisibility('hidden')
    vi.advanceTimersByTime(20_000)
    browser.setVisibility('visible')
    vi.advanceTimersByTime(5_000)

    store.stopReadingSession()

    expect(store.readingStats.totalSeconds).toBe(10)
  })

  it('splits a long session into server-sized chunks', async () => {
    const store = useAppStore()
    store.startReadingSession('book-a', '第一本', '作者')
    vi.advanceTimersByTime(2 * 60 * 60 * 1000)

    store.stopReadingSession()
    await vi.waitFor(() => expect(addReadingStats).toHaveBeenCalledTimes(2))

    expect(store.readingStats.totalSeconds).toBe(7200)
    expect(vi.mocked(addReadingStats).mock.calls.map(([params]) => params.seconds)).toEqual([3600, 3600])
    expect(vi.mocked(addReadingStats).mock.calls.every(([params]) => params.bookUrl === 'book-a')).toBe(true)
  })

  it('marks a chapter as read only once', () => {
    const store = useAppStore()

    expect(store.markChapterRead('book-a', 2, 10)).toBe(true)
    expect(store.markChapterRead('book-a', 2, 10)).toBe(false)
    expect(store.readingStats.readChapters.filter((key) => key === 'book-a#2')).toHaveLength(1)
  })
  it('excludes covered time even after focus and visibility events', () => {
    const store = useAppStore()
    store.startReadingSession('book-a', '第一本', '作者')
    vi.advanceTimersByTime(5_000)
    store.setReadingSessionSuspended(true)
    vi.advanceTimersByTime(20_000)
    browser.blur()
    browser.focus()
    browser.setVisibility('hidden')
    browser.setVisibility('visible')
    vi.advanceTimersByTime(10_000)
    expect(store.readingSessionActive).toBe(false)
    store.setReadingSessionSuspended(false)
    vi.advanceTimersByTime(3_000)
    store.stopReadingSession()
    expect(store.readingStats.totalSeconds).toBe(8)
  })

  it('counts non-activating hover reading while Codex keeps focus', () => {
    const store = useAppStore()
    store.setReadingSessionHoverVisible(false)
    store.startReadingSession('book-a', '第一本', '作者')
    browser.blur()
    vi.advanceTimersByTime(4_000)
    store.setReadingSessionHoverVisible(true)
    vi.advanceTimersByTime(6_000)
    store.setReadingSessionHoverVisible(false)
    browser.focus()
    vi.advanceTimersByTime(10_000)
    store.stopReadingSession()
    expect(store.readingStats.totalSeconds).toBe(6)
  })

  it.each([400, 600])('accumulates repeated %dms hover visits without rounding each visit', async (duration) => {
    const store = useAppStore()
    store.setReadingSessionHoverVisible(false)
    store.startReadingSession('book-a', '第一本')
    for (let i = 0; i < 10; i++) {
      store.setReadingSessionHoverVisible(true)
      vi.advanceTimersByTime(duration)
      store.setReadingSessionHoverVisible(false)
      vi.advanceTimersByTime(1700)
    }
    await store.settleReadingSession()
    expect(store.readingStats.totalSeconds).toBe(duration / 100)
    expect(vi.mocked(addReadingStats).mock.calls.reduce((sum, [p]) => sum + (p.seconds || 0), 0)).toBe(duration / 100)
  })

  it('continues timing visible hover text through focus loss', () => {
    const store = useAppStore()
    store.setReadingSessionHoverVisible(true)
    store.startReadingSession('book-a')
    vi.advanceTimersByTime(3000)
    browser.blur()
    vi.advanceTimersByTime(4000)
    store.stopReadingSession()
    expect(store.readingStats.totalSeconds).toBe(7)
  })

  it('does not count wall clock corrections as reading', () => {
    const store = useAppStore()
    store.startReadingSession('book-a')
    vi.advanceTimersByTime(2000)
    vi.setSystemTime(Date.now() + 3600_000)
    vi.advanceTimersByTime(3000)
    store.stopReadingSession()
    expect(store.readingStats.totalSeconds).toBe(5)
  })

  it('retains subsecond remainders through frequent settlements', async () => {
    const store = useAppStore()
    store.startReadingSession('book-a')
    for (let i = 0; i < 10; i++) {
      vi.advanceTimersByTime(350)
      await store.settleReadingSession()
    }
    expect(store.readingStats.totalSeconds).toBe(3)
    vi.advanceTimersByTime(500)
    store.stopReadingSession()
    await store.settleReadingSession()
    expect(store.readingStats.totalSeconds).toBe(4)
    expect(vi.mocked(addReadingStats).mock.calls.reduce((sum, [p]) => sum + (p.seconds || 0), 0)).toBe(4)
  })

  it('keeps fractional time attached to its book when switching books', async () => {
    const store = useAppStore()
    store.startReadingSession('book-a')
    vi.advanceTimersByTime(600)
    store.startReadingSession('book-b')
    vi.advanceTimersByTime(600)
    store.startReadingSession('book-a')
    vi.advanceTimersByTime(400)
    store.stopReadingSession()
    await store.settleReadingSession()
    expect(vi.mocked(addReadingStats).mock.calls.map(([p]) => [p.bookUrl, p.seconds])).toEqual([['book-a', 1]])
  })

  it('retains a failed save and retries with the same receipt id', async () => {
    vi.mocked(addReadingStats).mockRejectedValueOnce(new Error('reply lost'))
    const store = useAppStore()
    store.startReadingSession('book-a')
    vi.advanceTimersByTime(2000)
    store.stopReadingSession()
    await store.settleReadingSession()
    const first = vi.mocked(addReadingStats).mock.calls[0][0]
    expect(first.eventId).toEqual(expect.any(String))
    await store.settleReadingSession()
    expect(addReadingStats).toHaveBeenCalledTimes(2)
    expect(vi.mocked(addReadingStats).mock.calls[1][0]).toEqual(first)
    expect(store.readingStats.totalSeconds).toBe(2)
  })

  it('recovers an unsaved event and fractional time after reopening the reader', async () => {
    vi.mocked(addReadingStats).mockRejectedValue(new Error('backend unavailable'))
    const store = useAppStore()
    store.startReadingSession('book-a')
    vi.advanceTimersByTime(2600)
    store.stopReadingSession()
    await store.settleReadingSession()
    const first = vi.mocked(addReadingStats).mock.calls[0][0]
    setActivePinia(createPinia())
    vi.mocked(addReadingStats).mockResolvedValue({ saved: true }).mockClear()
    const reopened = useAppStore()
    reopened.startReadingSession('book-a')
    vi.advanceTimersByTime(400)
    reopened.stopReadingSession()
    await reopened.settleReadingSession()
    expect(vi.mocked(addReadingStats).mock.calls.map(([p]) => p.seconds)).toEqual([2, 1])
    expect(vi.mocked(addReadingStats).mock.calls[0][0]).toEqual(first)
    expect(reopened.readingStats.totalSeconds).toBe(3)
    expect([...browser.values.keys()].filter(k => k.startsWith('reader-reading-event:'))).toEqual([])
  })

  it('excludes a hidden document even when hover visibility and focus change', () => {
    const store = useAppStore()
    store.setReadingSessionHoverVisible(true)
    store.startReadingSession('book-a')
    vi.advanceTimersByTime(2000)
    browser.setVisibility('hidden')
    browser.focus()
    store.setReadingSessionHoverVisible(true)
    vi.advanceTimersByTime(20_000)
    browser.setVisibility('visible')
    vi.advanceTimersByTime(3000)
    store.stopReadingSession()
    expect(store.readingStats.totalSeconds).toBe(5)
  })

  it('saves time on LAN HTTP without crypto.randomUUID', async () => {
    const getRandomValues = crypto.getRandomValues.bind(crypto)
    vi.stubGlobal('crypto', { getRandomValues })
    const store = useAppStore()
    store.startReadingSession('book-a')
    vi.advanceTimersByTime(2000)
    store.stopReadingSession()
    await store.settleReadingSession()
    expect(addReadingStats).toHaveBeenCalledOnce()
    expect(vi.mocked(addReadingStats).mock.calls[0][0].eventId).toMatch(/^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$/)
  })

  it('replays pending time on startup without requiring any new reading', async () => {
    const event = { eventId: 'saved-event', seconds: 7, bookUrl: 'book-a', date: '2026-11-11' }
    browser.values.set('reader-reading-event:saved-event', JSON.stringify(event))
    useAppStore()
    await vi.waitFor(() => expect(addReadingStats).toHaveBeenCalledWith(event))
    await vi.waitFor(() => expect(browser.values.has('reader-reading-event:saved-event')).toBe(false))
  })

})
