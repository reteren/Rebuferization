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
    <svg viewBox="0 0 24 24" width="15" height="15" aria-hidden="true">
      <path
        fill="currentColor"
        d="M12 8.6a3.4 3.4 0 1 0 0 6.8 3.4 3.4 0 0 0 0-6.8Zm8.6 3.4c0-.4 0-.8-.1-1.2l2-1.5-1.6-2.8-2.4.9c-.6-.5-1.2-.9-1.9-1.2L16.2 3h-3.2l-.4 2.4h-1.2L11 3H7.8l-.4 2.4c-.7.3-1.3.7-1.9 1.2l-2.4-.9L1.5 8.5l2 1.5c-.1.4-.1.8-.1 1.2s0 .8.1 1.2l-2 1.5 1.6 2.8 2.4-.9c.6.5 1.2.9 1.9 1.2l.4 2.4h3.2l.4-2.4h1.2l.4 2.4h3.2l.4-2.4c.7-.3 1.3-.7 1.9-1.2l2.4.9 1.6-2.8-2-1.5c.1-.4.1-.8.1-1.2Z"
      />
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
