<script lang="ts">
  import type { Facet, Filter, Kind, Sort } from '../types'

  interface Props {
    query: string
    sort: Sort
    filter: Filter
    facets: Facet[]
    onquery?: (q: string) => void
    onsort?: (s: Sort) => void
    onfilter?: (f: Filter) => void
    onadd?: () => void
  }

  let { query, sort, filter, facets, onquery, onsort, onfilter, onadd }: Props = $props()

  const SORTS: Array<{ value: Sort; label: string }> = [
    { value: 'newest', label: 'Newest' },
    { value: 'oldest', label: 'Oldest' },
    { value: 'nameAsc', label: 'Name A to Z' },
    { value: 'nameDesc', label: 'Name Z to A' },
    { value: 'sizeAsc', label: 'Size, small first' },
    { value: 'sizeDesc', label: 'Size, large first' },
  ]

  const filterValue = $derived.by(() => {
    if (filter.ext) return `e:${filter.ext}`
    if (filter.subKind === 'link') return 'k:link'
    if (filter.kind) return `k:${filter.kind}`
    return 'all'
  })

  function onFilterChange(e: Event): void {
    const v = (e.currentTarget as HTMLSelectElement).value
    if (v === 'all') {
      onfilter?.({ ...filter, kind: null, subKind: null, ext: null })
      return
    }
    if (v === 'k:link') {
      onfilter?.({ ...filter, kind: null, subKind: 'link', ext: null })
      return
    }
    if (v.startsWith('k:')) {
      onfilter?.({ ...filter, kind: v.slice(2) as Kind, subKind: null, ext: null })
      return
    }
    onfilter?.({ ...filter, kind: null, subKind: null, ext: v.slice(2) })
  }

  function onQueryInput(e: Event): void {
    onquery?.((e.currentTarget as HTMLInputElement).value)
  }
</script>

<div class="toolbar">
  <button type="button" class="add-btn" onclick={() => onadd?.()}>
    <svg viewBox="0 0 12 12" width="10" height="10" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round">
      <path d="M6 1v10M1 6h10" />
    </svg>
    <span>Add</span>
  </button>

  <label class="search">
    <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round">
      <circle cx="11" cy="11" r="6.5" />
      <path d="m20 20-4.2-4.2" />
    </svg>
    <input
      type="text"
      placeholder="Search clipboard"
      spellcheck="false"
      autocomplete="off"
      value={query}
      oninput={onQueryInput}
    />
  </label>

  <label class="select">
    <select
      value={sort}
      aria-label="Sort"
      onchange={(e) => onsort?.((e.currentTarget as HTMLSelectElement).value as Sort)}
    >
      {#each SORTS as s (s.value)}
        <option value={s.value}>{s.label}</option>
      {/each}
    </select>
  </label>

  <label class="select">
    <select value={filterValue} aria-label="Filter" onchange={onFilterChange}>
      <optgroup label="Type">
        <option value="all">All</option>
        <option value="k:image">Images</option>
        <option value="k:text">Text</option>
        <option value="k:link">Links</option>
        <option value="k:file">Files</option>
        <option value="k:video">Video</option>
        <option value="k:other">Other</option>
      </optgroup>
      {#if facets.length > 0}
        <optgroup label="Extension">
          {#each facets as f (f.ext)}
            <option value="e:{f.ext}">{f.ext} ({f.count})</option>
          {/each}
        </optgroup>
      {/if}
    </select>
  </label>
</div>

<style>
  .toolbar {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .add-btn {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 7px 14px;
    border: none;
    border-radius: var(--r-sm);
    background: var(--accent);
    color: var(--on-accent);
    font-size: var(--fs-sm);
    font-weight: 600;
    cursor: pointer;
    transition: transform var(--dur-fast) var(--ease-out), opacity var(--dur-fast) var(--ease-out);
  }

  .add-btn:hover {
    opacity: 0.9;
  }

  .add-btn:active {
    transform: scale(0.97);
  }

  .search {
    position: relative;
    flex: 1;
    min-width: 0;
  }

  .search svg {
    position: absolute;
    left: 10px;
    top: 50%;
    translate: 0 -50%;
    color: var(--text-3);
    pointer-events: none;
  }

  .search input {
    width: 100%;
    padding: 7px 12px 7px 30px;
    border-radius: var(--r-sm);
    border: 1px solid var(--border-1);
    background: var(--surface-2);
    color: var(--text-1);
    font-size: var(--fs-sm);
    outline: none;
  }

  .search input::placeholder {
    color: var(--text-3);
  }

  .search input:hover {
    background: var(--surface-4);
  }

  .search input:focus {
    border-color: var(--accent);
  }

  .select {
    position: relative;
    display: inline-flex;
  }

  .select select {
    appearance: none;
    -webkit-appearance: none;
    padding: 7px 30px 7px 12px;
    border-radius: var(--r-sm);
    border: 1px solid var(--border-1);
    background: var(--surface-2);
    color: var(--text-1);
    font-size: var(--fs-sm);
    cursor: pointer;
    outline: none;
  }

  .select select:hover {
    background: var(--surface-4);
  }

  .select select:focus {
    border-color: var(--accent);
  }

  .select::after {
    content: '';
    position: absolute;
    right: 11px;
    top: 50%;
    translate: 0 -50%;
    width: 9px;
    height: 5px;
    background: var(--text-3);
    pointer-events: none;
    mask-image: var(--chevron-mask);
    -webkit-mask-image: var(--chevron-mask);
  }
</style>