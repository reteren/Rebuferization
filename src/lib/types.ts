// Mirror of src-tauri/src/model.rs and settings.rs. Change one, change both.
// Serde is configured camelCase, so field names match exactly.

/// Selectable themes. 'darkblue' is the built-in one defined on :root; every other
/// name has a file under lib/styles/themes/ and is applied via data-theme.
export const THEMES = [
  'darkblue',
  'black',
  'light',
  'grey',
  'skyblue',
  'dark-green',
  'dark-purple',
  'ember',
  'ocean',
  'wine',
  'paper',
] as const
export type Theme = (typeof THEMES)[number]

export const THEME_LABELS: Record<Theme, string> = {
  darkblue: 'Dark blue',
  black: 'Black',
  light: 'Light',
  grey: 'Grey',
  skyblue: 'Sky blue',
  'dark-green': 'Dark green',
  'dark-purple': 'Dark purple',
  ember: 'Ember',
  ocean: 'Ocean',
  wine: 'Wine',
  paper: 'Paper',
}

export type Kind = 'text' | 'image' | 'video' | 'file' | 'other'
export type SubKind = 'plain' | 'rich' | 'code' | 'link' | 'color' | 'animated'

export interface ItemDto {
  id: number
  kind: Kind
  subKind: SubKind | null
  title: string | null
  previewText: string | null
  thumbUrl: string | null
  /// The original blob, present only for animated items. A thumbnail is one
  /// decoded frame re-encoded as static WebP, so rendering an animated GIF from
  /// thumbUrl can never move it.
  animatedUrl: string | null
  ext: string | null
  byteSize: number
  width: number | null
  height: number | null
  durationMs: number | null
  createdAt: number
  pinned: boolean
  isReference: boolean
  refPath: string | null
  sourceApp: string | null
  copyCount: number
  missing: boolean
  fileNames: string[]
}

export interface Filter {
  kind: Kind | null
  ext: string | null
  pinnedOnly: boolean
  subKind: SubKind | null
  /// The Added Files tab: shelf items, stored by path and never copied.
  referencesOnly: boolean
}

export const EMPTY_FILTER: Filter = {
  kind: null,
  ext: null,
  pinnedOnly: false,
  subKind: null,
  referencesOnly: false,
}

export type Sort = 'newest' | 'oldest' | 'nameAsc' | 'nameDesc' | 'sizeAsc' | 'sizeDesc'

export type TabId = 'all' | 'images' | 'text' | 'links' | 'files' | 'references' | 'pinned'

/// Each tab is just a preset filter; the grid never special-cases a tab.
export const TAB_FILTERS: Record<TabId, Filter> = {
  all: EMPTY_FILTER,
  images: { ...EMPTY_FILTER, kind: 'image' },
  text: { ...EMPTY_FILTER, kind: 'text' },
  links: { ...EMPTY_FILTER, subKind: 'link' },
  files: { ...EMPTY_FILTER, kind: 'file' },
  references: { ...EMPTY_FILTER, referencesOnly: true },
  pinned: { ...EMPTY_FILTER, pinnedOnly: true },
}

export interface Facet {
  ext: string
  count: number
}

/// Counts for the tab bar. Not derivable on the client: `links` is a sub-kind,
/// `pinned` cuts across every kind, and extension facets miss items with no
/// extension — so the store counts them in one query.
export interface TabCounts {
  all: number
  images: number
  text: number
  links: number
  files: number
  references: number
  pinned: number
}

export interface KindStat {
  kind: string
  count: number
  bytes: number
}

export interface StorageStats {
  totalItems: number
  totalBytes: number
  dbBytes: number
  byKind: KindStat[]
  capBytes: number | null
}

export interface CleanupResult {
  removedItems: number
  freedBytes: number
}

export type ImportMode = 'merge' | 'replace'

/// What the janitor is allowed to delete. Pushed into the store from settings;
/// the store does not read settings.json itself.
export interface RetentionPolicy {
  retentionDays: number
  maxStoreBytes: number | null
}

/// Payload of the `storage-warning` event. `removedItems` is 0 for the 90%
/// warning fired before anything is deleted.
export interface StorageWarning {
  usedBytes: number
  capBytes: number
  removedItems: number
  freedBytes: number
}

export interface StoreProgress {
  phase: string
  done: number
  total: number
}

// ---------------------------------------------------------------------------
// settings
// ---------------------------------------------------------------------------

export interface Settings {
  version: number
  hotkey: { binding: string; aggressiveMode: boolean }
  storage: {
    path: string
    retentionDays: number
    maxItemBytes: number
    maxStoreBytes: number | null
    notifyWhenFull: boolean
  }
  window: {
    sizeMode: 'percent' | 'fixed'
    percentOfMonitor: number
    fixed: { width: number; height: number }
    zoomStep: number
    dragBar: boolean
  }
  behavior: {
    autoPaste: boolean
    pasteAsPlainText: boolean
    closeOnCopy: boolean
    launchOnStartup: boolean
    silentStart: boolean
    captureEnabled: boolean
  }
  appearance: {
    theme: Theme
    showAge: boolean
    formatLabelSize: 'off' | 'small' | 'medium' | 'large'
    animateGifs: boolean
    reduceMotion: boolean
    accent: string
  }
  privacy: {
    respectClipboardFlags: boolean
    blockedProcesses: string[]
    /// Whether a copied link may be looked up at the site that owns it. The
    /// only setting in the app that lets it talk to anyone; on by default.
    linkPreviews: boolean
  }
}

/// A deep-partial patch, matching `update_settings` on the Rust side.
export type SettingsPatch = {
  [K in keyof Settings]?: Partial<Settings[K]>
}

// ---------------------------------------------------------------------------
// events
// ---------------------------------------------------------------------------

export const EVENTS = {
  itemAdded: 'item-added',
  itemUpdated: 'item-updated',
  itemsDeleted: 'items-deleted',
  settingsChanged: 'settings-changed',
  storeProgress: 'store-progress',
  storageWarning: 'storage-warning',
  /// Carries the id of the item now on the clipboard, so exactly one card can
  /// be marked as live.
  clipboardCurrent: 'clipboard-current',
  /// The configured store could not be opened; the app fell back to the
  /// default root. Payload is the path that failed.
  storeUnavailable: 'store-unavailable',
} as const
