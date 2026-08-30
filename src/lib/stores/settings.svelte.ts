// OWNER: worker W6. One instance per window; loaded once, patched through
// update_settings, re-synced live on `settings-changed` so an external edit of
// settings.json shows up without a reload.

import { getSettings, onSettingsChanged, updateSettings } from '../ipc'
import type { Settings, SettingsPatch } from '../types'

/** Mirrors src-tauri/src/settings.rs defaults so the UI renders before the
 * first successful get_settings, and so it has sane values if the Rust side
 * is not up yet. */
export const DEFAULT_SETTINGS: Settings = {
  version: 1,
  hotkey: { binding: 'Alt+V', aggressiveMode: false },
  storage: {
    path: '',
    retentionDays: 30,
    maxItemBytes: 256 * 1024 * 1024,
    maxStoreBytes: null,
    notifyWhenFull: true,
  },
  window: {
    sizeMode: 'percent',
    percentOfMonitor: 40,
    fixed: { width: 1100, height: 700 },
    zoomStep: 3,
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
    accent: '#7aa2ff',
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
  },
}

class SettingsStore {
  current = $state<Settings>(DEFAULT_SETTINGS)

  private loading: Promise<void> | null = null

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
    } catch {
      // Backend not reachable (parallel build); keep what we have.
    }
  }

  private async loadOnce(): Promise<void> {
    try {
      this.current = await getSettings()
    } catch {
      // Backend not reachable (parallel build); defaults stand in.
    }
    await onSettingsChanged((next) => {
      this.current = next
    })
  }

  /** Sends a deep-partial patch; the returned full settings become current. */
  async patch(p: SettingsPatch): Promise<void> {
    const next = await updateSettings(p)
    this.current = next
  }
}

export const settings = new SettingsStore()