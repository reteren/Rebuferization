// OWNER: worker W6. One instance per window; loaded once, patched through
// update_settings, re-synced live on `settings-changed` so an external edit of
// settings.json shows up without a reload.

import { getSettings, onSettingsChanged, updateSettings } from '../ipc'
import { THEMES, type Settings, type SettingsPatch, type Theme } from '../types'

/** Mirrors the appearance fields that drive document-level styling. */
function applyAppearance(a: Settings['appearance']): void {
  const root = document.documentElement

  // An unknown name would leave data-theme pointing at a block that does not
  // exist, and the UI would silently keep the previous theme's colours; fall
  // back to the built-in dark instead. 'dark' IS :root, so it carries no
  // attribute.
  const theme: Theme = (THEMES as readonly string[]).includes(a.theme)
    ? (a.theme as Theme)
    : 'darkblue'
  if (theme === 'darkblue') root.removeAttribute('data-theme')
  else root.setAttribute('data-theme', theme)

  // The accent belongs to the theme. An inline property is the highest-priority
  // source in CSS, so setting it unconditionally meant no theme could ever
  // change its own accent — which is exactly what went wrong. An empty value
  // means "follow the theme"; anything else is the user overriding it, and
  // only then does the inline property go on.
  if (a.accent) root.style.setProperty('--accent', a.accent)
  else root.style.removeProperty('--accent')
  // global.css ships a reduced-motion override keyed on this attribute.
  if (a.reduceMotion) root.setAttribute('data-reduce-motion', '')
  else root.removeAttribute('data-reduce-motion')
}

/** Mirrors src-tauri/src/settings.rs defaults so the UI renders before the
 * first successful get_settings, and so it has sane values if the Rust side
 * is not up yet. */
export const DEFAULT_SETTINGS: Settings = {
  version: 1,
  hotkey: { binding: 'Alt+V', aggressiveMode: false },
  storage: {
    path: '',
    retentionDays: 30,
    tempFilesDays: 7,
    maxItemBytes: 256 * 1024 * 1024,
    maxStoreBytes: null,
    notifyWhenFull: true,
  },
  window: {
    sizeMode: 'percent',
    percentOfMonitor: 40,
    fixed: { width: 1100, height: 700 },
    zoomStep: 3,
    dragBar: true,
  },
  behavior: {
    autoPaste: false,
    pasteAsPlainText: false,
    closeOnCopy: true,
    launchOnStartup: true,
    silentStart: true,
    captureEnabled: true,
  },
  appearance: {
    showAge: true,
    formatLabelSize: 'medium',
    animateGifs: true,
    reduceMotion: false,
    theme: 'darkblue',
    accent: '',
  },
  privacy: {
    respectClipboardFlags: true,
    blockedProcesses: [
      'keepass.exe',
      'keepassxc.exe',
      '1password.exe',
      'bitwarden.exe',
      'lastpass.exe',
      'dashlane.exe',
      'protonpass.exe',
    ],
    linkPreviews: true,
  },
}

class SettingsStore {
  current = $state<Settings>(DEFAULT_SETTINGS)

  private loading: Promise<void> | null = null
  private unlisten: (() => void) | null = null

  /** Loads once; safe to call from both windows. */
  init(): Promise<void> {
    if (this.loading) return this.loading
    this.loading = this.loadOnce().finally(() => {
      this.loading = null
    })
    return this.loading
  }

  /** Refetches from the backend after operations that mutate settings on the
   * Rust side (relocation, import). */
  async reload(): Promise<void> {
    try {
      this.current = await getSettings()
      applyAppearance(this.current.appearance)
    } catch {
      // Backend not reachable (parallel build); keep what we have.
    }
  }

  private async loadOnce(): Promise<void> {
    // The backend manages AppState as the very last step of setup, after the
    // store is opened and the watcher, hotkeys and tray are up; the windows
    // are created before that and their scripts run immediately. So on a cold
    // start get_settings can arrive before there is any state to answer it,
    // and it fails. Settling for the defaults there is what made a chosen
    // theme revert to dark blue on every restart: nothing ever asked again,
    // and the settings only reappeared once something emitted a change.
    // Retry instead — the window is still hidden at this point, so waiting
    // costs nothing visible.
    for (let attempt = 0; attempt < 60; attempt++) {
      try {
        this.current = await getSettings()
        break
      } catch {
        await new Promise((r) => setTimeout(r, 250))
      }
    }
    applyAppearance(this.current.appearance)
    this.unlisten?.()
    this.unlisten = await onSettingsChanged((next) => {
      this.current = next
      applyAppearance(this.current.appearance)
    })
  }

  /** Sends a deep-partial patch; the returned full settings become current. */
  async patch(p: SettingsPatch): Promise<void> {
    const next = await updateSettings(p)
    this.current = next
    applyAppearance(this.current.appearance)
  }
}

export const settings = new SettingsStore()