// Mirror of src-tauri/src/model.rs and settings.rs. Change one, change both.
// Serde is configured camelCase, so field names match exactly.

export type Kind = 'text' | 'image' | 'video' | 'file' | 'other'
export type SubKind = 'plain' | 'rich' | 'code' | 'link' | 'color' | 'animated'

export interface ItemDto {
  id: number
  kind: Kind
  subKind: SubKind | null
  title: string | null
  previewText: string | null
  thumbUrl: string | null
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
}

export const EMPTY_FILTER: Filter = {
  kind: null,
  ext: null,
  pinnedOnly: false,
  subKind: null,
}

export type Sort = 'newest' | 'oldest' | 'nameAsc' | 'nameDesc' | 'sizeAsc' | 'sizeDesc'

export type TabId = 'all' | 'images' | 'text' | 'links' | 'files' | 'pinned'

/// Each tab is just a preset filter; the grid never special-cases a tab.
export const TAB_FILTERS: Record<TabId, Filter> = {
  all: EMPTY_FILTER,
  images: { ...EMPTY_FILTER, kind: 'image' },
  text: { ...EMPTY_FILTER, kind: 'text' },
  links: { ...EMPTY_FILTER, subKind: 'link' },
  files: { ...EMPTY_FILTER, kind: 'file' },
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
    showAge: boolean
    formatLabelSize: 'off' | 'small' | 'medium' | 'large'
    animateGifs: boolean
    reduceMotion: boolean
    accent: string
  }
  privacy: {
    respectClipboardFlags: boolean
    blockedProcesses: string[]
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
} as const
