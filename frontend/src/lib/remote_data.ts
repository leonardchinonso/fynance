/**
 * 5-state discriminated union for async data.
 *
 * - `idle`      — not yet requested
 * - `loading`   — first fetch in progress, no value available
 * - `reloading` — background fetch in progress, previous value still available
 * - `failed`    — last fetch failed with an error message
 * - `succeeded` — last fetch succeeded with a value
 */
export type RemoteData<T> =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "reloading"; value: T }
  | { status: "failed"; error: string }
  | { status: "succeeded"; value: T }

// ── Constructors ──────────────────────────────────────────────────────────────

/** Convenience constructors for building `RemoteData` values. */
export const RemoteData = {
  /** Data has not been requested yet. */
  idle: <T>(): RemoteData<T> => ({ status: "idle" }),
  /** First fetch is in progress; no value available. */
  loading: <T>(): RemoteData<T> => ({ status: "loading" }),
  /** Background re-fetch is in progress; previous value is still available. */
  reloading: <T>(value: T): RemoteData<T> => ({ status: "reloading", value }),
  /** Last fetch failed with an error message. */
  failed: <T>(error: string): RemoteData<T> => ({ status: "failed", error }),
  /** Last fetch succeeded with a value. */
  succeeded: <T>(value: T): RemoteData<T> => ({ status: "succeeded", value }),
}

// ── Visitors ──────────────────────────────────────────────────────────────────

type RemoteValues<T extends readonly RemoteData<unknown>[]> = {
  [K in keyof T]: T[K] extends RemoteData<infer V> ? V : never
}

/**
 * Combine several queries into one `RemoteData`: failed if any part failed,
 * a mapped value once every part has one (reloading if any part is
 * refetching), otherwise loading. Lets view hooks compose per-endpoint
 * cache entries while consumers keep seeing a single async state.
 */
export function combineRemoteData<T extends readonly RemoteData<unknown>[], R>(
  parts: readonly [...T],
  map: (values: RemoteValues<T>) => R,
): RemoteData<R> {
  for (const p of parts) {
    if (p.status === "failed") return RemoteData.failed(p.error)
  }
  if (parts.every((p) => p.status === "succeeded" || p.status === "reloading")) {
    const values = parts.map((p) => (p as { value: unknown }).value) as RemoteValues<T>
    const value = map(values)
    return parts.some((p) => p.status === "reloading")
      ? RemoteData.reloading(value)
      : RemoteData.succeeded(value)
  }
  if (parts.every((p) => p.status === "idle")) return RemoteData.idle()
  return RemoteData.loading()
}

/**
 * Visit a `RemoteData` value with a 3-branch handler (the common case).
 *
 * - `notLoaded` — covers `idle` and `loading` (no value available yet)
 * - `failed`    — covers `failed`
 * - `hasValue`  — covers `reloading` and `succeeded` (a value is available;
 *                 check `data.status === "reloading"` if you need to distinguish)
 *
 * Use this in ~90% of components. For full 5-state control, use `visitRemoteDataFull`.
 */
export function visitRemoteData<T, R>(
  data: RemoteData<T>,
  handlers: {
    notLoaded: () => R
    failed: (error: string) => R
    hasValue: (value: T) => R
  }
): R {
  return visitRemoteDataFull(data, {
    idle: handlers.notLoaded,
    loading: handlers.notLoaded,
    reloading: handlers.hasValue,
    failed: handlers.failed,
    succeeded: handlers.hasValue,
  })
}

/**
 * Visit a `RemoteData` value with a full 5-branch handler.
 *
 * TypeScript enforces that every state is handled. Use this when you need
 * to treat `reloading` differently from `succeeded` (e.g. showing a subtle
 * refresh indicator while keeping the previous value visible).
 */
export function visitRemoteDataFull<T, R>(
  data: RemoteData<T>,
  handlers: {
    idle: () => R
    loading: () => R
    reloading: (value: T) => R
    failed: (error: string) => R
    succeeded: (value: T) => R
  }
): R {
  switch (data.status) {
    case "idle":      return handlers.idle()
    case "loading":   return handlers.loading()
    case "reloading": return handlers.reloading(data.value)
    case "failed":    return handlers.failed(data.error)
    case "succeeded": return handlers.succeeded(data.value)
  }
}
