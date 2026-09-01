<script lang="ts">
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import {
    getCaptureEnabled,
    hideTrayMenu,
    quitApp,
    setCaptureEnabled,
    showSettingsWindow,
  } from '../lib/ipc'
  import { settings } from '../lib/stores/settings.svelte'

  let captureOn = $state(true)
  let busy = $state(false)

  // The window is shown and hidden rather than created per click, so its state
  // has to be refreshed every time it appears, not once at mount.
  $effect(() => {
    void settings.init()
    const win = getCurrentWindow()
    const refresh = (): void => {
      void getCaptureEnabled().then((v) => {
        captureOn = v
      })
    }
    refresh()

    let unfocus: (() => void) | null = null
    void win
      .onFocusChanged(({ payload: focused }) => {
        if (focused) refresh()
        // Losing focus IS the dismissal: a menu that outlives the click that
        // opened it is the bug the popup's own context menu had.
        else void hideTrayMenu()
      })
      .then((fn) => {
        unfocus = fn
      })

    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'Escape') void hideTrayMenu()
    }
    window.addEventListener('keydown', onKey)
    return () => {
      unfocus?.()
      window.removeEventListener('keydown', onKey)
    }
  })

  async function run(action: () => Promise<unknown>): Promise<void> {
    if (busy) return
    busy = true
    try {
      await action()
    } finally {
      await hideTrayMenu()
      busy = false
    }
  }
</script>

<nav class="menu">
  <button type="button" onclick={() => void run(showSettingsWindow)}>
    <!-- The same traced gear the popup uses, taken from one source so the two
         can never drift apart. -->
    <svg viewBox="0 0 24 24" width="15" height="15" aria-hidden="true">
      <path fill="currentColor" fill-rule="evenodd" d="M9.94 3.75 L9.82 0.23 L14.18 0.23 L14.06 3.75 A8.50 8.50 0 0 1 16.38 4.71 L18.78 2.14 L21.86 5.22 L19.29 7.62 A8.50 8.50 0 0 1 20.25 9.94 L23.77 9.82 L23.77 14.18 L20.25 14.06 A8.50 8.50 0 0 1 19.29 16.38 L21.86 18.78 L18.78 21.86 L16.38 19.29 A8.50 8.50 0 0 1 14.06 20.25 L14.18 23.77 L9.82 23.77 L9.94 20.25 A8.50 8.50 0 0 1 7.62 19.29 L5.22 21.86 L2.14 18.78 L4.71 16.38 A8.50 8.50 0 0 1 3.75 14.06 L0.23 14.18 L0.23 9.82 L3.75 9.94 A8.50 8.50 0 0 1 4.71 7.62 L2.14 5.22 L5.22 2.14 L7.62 4.71 Z M16.09 12.00 A4.09 4.09 0 1 0 7.91 12.00 A4.09 4.09 0 1 0 16.09 12.00 Z" />
    </svg>
    Settings
  </button>

  <button
    type="button"
    onclick={() => void run(() => setCaptureEnabled(!captureOn))}
  >
    <span class="dot" class:on={captureOn}></span>
    {captureOn ? 'Disable capture' : 'Enable capture'}
  </button>

  <hr />

  <button type="button" class="danger" onclick={() => void run(quitApp)}>
    <svg viewBox="0 0 24 24" width="15" height="15" aria-hidden="true">
      <path
        fill="none"
        stroke="currentColor"
        stroke-width="1.9"
        stroke-linecap="round"
        d="M9 4.6H6.2a1.6 1.6 0 0 0-1.6 1.6v11.6a1.6 1.6 0 0 0 1.6 1.6H9M15.4 15.6 19 12l-3.6-3.6M19 12H9.4"
      />
    </svg>
    Quit Rebuffer
  </button>
</nav>

<style>
  .menu {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 6px;
    height: 100vh;
    box-sizing: border-box;
    border-radius: var(--r-lg);
    background: var(--surface-3);
    border: 1px solid var(--window-border);
    box-shadow: var(--shadow-2);
    backdrop-filter: blur(var(--glass-blur));
    font-family: var(--font-ui);
    overflow: hidden;
  }

  button {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 7px 10px;
    border: none;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--text-1);
    font-size: var(--fs-md);
    font-family: inherit;
    text-align: left;
    cursor: pointer;
    transition:
      background var(--dur-fast) var(--ease-out),
      color var(--dur-fast) var(--ease-out);
  }

  button:hover {
    background: var(--accent-soft);
    color: var(--accent-strong);
  }

  button.danger:hover {
    background: var(--danger-soft);
    color: var(--danger);
  }

  /* Green while capturing, hollow while paused: the tray icon says the same
     thing by going muted, and the two should never disagree. */
  .dot {
    width: 9px;
    height: 9px;
    margin: 0 3px;
    border-radius: var(--r-pill);
    border: 1.5px solid var(--text-3);
  }

  .dot.on {
    background: var(--ok);
    border-color: var(--ok);
  }

  hr {
    margin: 3px 6px;
    border: none;
    border-top: 1px solid var(--border-1);
  }
</style>
