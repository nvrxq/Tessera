import { invoke } from "@tauri-apps/api/core";

/**
 * Sync the native overlay's bounds + visibility to a DOM element's
 * client rect. Returns a cleanup function the caller should invoke on
 * unmount.
 *
 * Coordinates are converted to physical pixels via devicePixelRatio.
 * The Tauri window's desktop position is added (window.screenX/Y) so the
 * overlay can be placed in absolute screen coordinates by winit.
 *
 * Plan 4 will wire this to the real terminal pane element. For Plan 3
 * the helper exists; manual verification (Plan 3 T10) can invoke it
 * against a temporary element.
 */
export function syncOverlayToElement(el: HTMLElement): () => void {
  let lastBounds = { x: -1, y: -1, w: 0, h: 0 };
  let visible = false;

  function sync() {
    const r = el.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    const x = Math.round(r.left * dpr) + Math.round(window.screenX * dpr);
    const y = Math.round(r.top * dpr) + Math.round(window.screenY * dpr);
    const w = Math.max(1, Math.round(r.width * dpr));
    const h = Math.max(1, Math.round(r.height * dpr));

    if (x !== lastBounds.x || y !== lastBounds.y || w !== lastBounds.w || h !== lastBounds.h) {
      lastBounds = { x, y, w, h };
      void invoke("overlay_set_bounds", { x, y, w, h });
    }

    const shouldBeVisible = r.width > 0 && r.height > 0;
    if (shouldBeVisible !== visible) {
      visible = shouldBeVisible;
      void invoke("overlay_set_visible", { visible });
    }
  }

  sync();

  const ro = new ResizeObserver(() => sync());
  ro.observe(el);

  const onScroll = () => sync();
  const onResize = () => sync();
  window.addEventListener("scroll", onScroll, { passive: true });
  window.addEventListener("resize", onResize);

  // rAF tick — catches the Tauri window moving across the desktop
  // (no DOM event fires for that). 60 Hz polling; gated by diff above.
  let raf = 0;
  function tick() {
    sync();
    raf = requestAnimationFrame(tick);
  }
  raf = requestAnimationFrame(tick);

  return () => {
    ro.disconnect();
    window.removeEventListener("scroll", onScroll);
    window.removeEventListener("resize", onResize);
    cancelAnimationFrame(raf);
    void invoke("overlay_set_visible", { visible: false });
  };
}

export async function selectOverlaySession(sessionId: string | null): Promise<void> {
  await invoke("overlay_select_session", { sessionId });
}

export async function resizeOverlayGrid(cols: number, rows: number): Promise<void> {
  await invoke("overlay_resize_grid", { cols, rows });
}
