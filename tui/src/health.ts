// Global service-health polling.
// A single background poller (started by App) keeps the service status and
// server-connection state fresh for every page, so no page needs its own
// interval. Pages read the shared signals below.

import { createSignal } from "solid-js"
import { getServicesStatus } from "./api"

export const [healthServices, setHealthServices] = createSignal<
  { name: string; status: string; healthy: boolean; detail?: string }[] | null
>(null)
export const [serverConnected, setServerConnected] = createSignal(true)

let timer: ReturnType<typeof setInterval> | null = null

/// One probe of the server + service health. Async: performs the REST call,
/// updates the shared signals, never rejects (failures set serverConnected
/// to false). Callers may `await` it or fire-and-forget.
export async function pollHealth(): Promise<void> {
  try {
    const svc = await getServicesStatus()
    setHealthServices(svc.services)
    setServerConnected(true)
  } catch {
    setServerConnected(false)
  }
}

/// Start global polling (30s). Runs one poll immediately, then every
/// intervalMs. Returns a dispose function; also cleans up on unmount.
export function startHealthPolling(intervalMs = 30000): () => void {
  void pollHealth()
  if (timer) clearInterval(timer)
  timer = setInterval(() => void pollHealth(), intervalMs)
  return () => {
    if (timer) clearInterval(timer)
    timer = null
  }
}
