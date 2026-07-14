import { useSearchParams } from "react-router-dom"
import { useCallback } from "react"
import type { Granularity } from "@/types"
import type { AssetClass } from "@/bindings/AssetClass"

export const ASSET_CLASSES: AssetClass[] = ["Investments", "Cash", "Pension", "Property"]

export type AssetClassSettings = Record<AssetClass, { show: boolean; merge: boolean }>
import { format, subMonths, subYears, startOfMonth, startOfYear } from "date-fns"

export type Preset =
  | "this-month"
  | "last-3-months"
  | "last-12-months"
  | "ytd"
  | "3-years"
  | "5-years"
  | "10-years"
  | "tax-2025-26"
  | "tax-2024-25"
  | "tax-2023-24"
  | "custom"

function todayStr(): string {
  return format(new Date(), "yyyy-MM-dd")
}

function getPresetRange(preset: Preset): { start: string; end: string } {
  const now = new Date()
  switch (preset) {
    case "this-month":
      return { start: format(startOfMonth(now), "yyyy-MM-dd"), end: todayStr() }
    case "last-3-months":
      return { start: format(startOfMonth(subMonths(now, 2)), "yyyy-MM-dd"), end: todayStr() }
    case "last-12-months":
      return { start: format(startOfMonth(subMonths(now, 11)), "yyyy-MM-dd"), end: todayStr() }
    case "ytd":
      return { start: format(startOfYear(now), "yyyy-MM-dd"), end: todayStr() }
    case "3-years":
      return { start: format(subYears(now, 3), "yyyy-MM-dd"), end: todayStr() }
    case "5-years":
      return { start: format(subYears(now, 5), "yyyy-MM-dd"), end: todayStr() }
    case "10-years":
      return { start: format(subYears(now, 10), "yyyy-MM-dd"), end: todayStr() }
    // UK tax years run 6 April to 5 April
    case "tax-2025-26":
      return { start: "2025-04-06", end: "2026-04-05" }
    case "tax-2024-25":
      return { start: "2024-04-06", end: "2025-04-05" }
    case "tax-2023-24":
      return { start: "2023-04-06", end: "2024-04-05" }
    case "custom":
      return { start: format(startOfMonth(subMonths(now, 5)), "yyyy-MM-dd"), end: todayStr() }
  }
}

export function useUrlFilters() {
  const [searchParams, setSearchParams] = useSearchParams()

  const preset = (searchParams.get("preset") as Preset) || "last-12-months"
  const defaultRange = getPresetRange(preset)

  const start = searchParams.get("start") || defaultRange.start
  const end = searchParams.get("end") || defaultRange.end
  const view = searchParams.get("view") || "table"
  const granularity = (searchParams.get("granularity") as Granularity) || "monthly"
  // Profile persists across page navigation via localStorage
  const urlProfile = searchParams.get("profile")
  const storedProfile = typeof window !== "undefined" ? localStorage.getItem("fynance-profile") : null
  const profileId = urlProfile || storedProfile || undefined
  const page = parseInt(searchParams.get("page") || "1", 10)

  const accounts = searchParams.get("accounts")
    ? searchParams.get("accounts")!.split(",")
    : []
  const categories = searchParams.get("categories")
    ? searchParams.get("categories")!.split(",")
    : []
  // Category-type filter (shared across Budget views). Empty = all types.
  const categoryTypes = searchParams.get("category_types")
    ? searchParams.get("category_types")!.split(",").filter(Boolean)
    : []
  // Charts "Group by" dimension. Default (omitted) = parent category.
  const groupBy = searchParams.get("group_by") || "parent_category"
  const search = searchParams.get("search") || ""

  // Transactions table sort. `txSort` unset = backend default (newest-first).
  const sortRaw = searchParams.get("txSort")
  const txSort: "date" | "amount" | "category" | undefined =
    sortRaw === "date" || sortRaw === "amount" || sortRaw === "category"
      ? sortRaw
      : undefined
  const dirRaw = searchParams.get("txDir")
  const txDir: "asc" | "desc" = dirRaw === "asc" ? "asc" : "desc"

  // Portfolio pie settings — default true (omitted from URL = true)
  const hideSmall = searchParams.get("hide_small") !== "0"
  const assetClassSettings: AssetClassSettings = Object.fromEntries(
    ASSET_CLASSES.map(cls => {
      const key = cls.toLowerCase()
      return [cls, {
        show:  searchParams.get(`show_${key}`) !== "0",
        merge: searchParams.get(`merge_${key}`) !== "0",
      }]
    })
  ) as AssetClassSettings

  const setFilter = useCallback(
    (updates: Record<string, string | undefined>) => {
      setSearchParams((prev) => {
        const next = new URLSearchParams(prev)
        for (const [key, value] of Object.entries(updates)) {
          if (value === undefined || value === "") {
            next.delete(key)
          } else {
            next.set(key, value)
          }
        }
        return next
      })
    },
    [setSearchParams]
  )

  const setPreset = useCallback(
    // Only the preset goes in the URL; start/end are derived at read time so
    // bookmarks and pinned views stay evergreen ("last 12 months" tracks today
    // instead of freezing at the dates it resolved to when pinned).
    (p: Preset) => {
      setFilter({ preset: p, start: undefined, end: undefined, page: "1" })
    },
    [setFilter]
  )

  const setView = useCallback(
    (v: string) => setFilter({ view: v }),
    [setFilter]
  )

  const setGranularity = useCallback(
    (g: Granularity) => setFilter({ granularity: g }),
    [setFilter]
  )

  const setPage = useCallback(
    (p: number) => setFilter({ page: p.toString() }),
    [setFilter]
  )

  const setProfileId = useCallback(
    (id: string | undefined) => {
      if (id) {
        localStorage.setItem("fynance-profile", id)
      } else {
        localStorage.removeItem("fynance-profile")
      }
      setFilter({ profile: id })
    },
    [setFilter]
  )

  const setAccounts = useCallback(
    (ids: string[]) =>
      setFilter({ accounts: ids.length > 0 ? ids.join(",") : undefined }),
    [setFilter]
  )

  const setCategories = useCallback(
    (cats: string[]) =>
      setFilter({ categories: cats.length > 0 ? cats.join(",") : undefined }),
    [setFilter]
  )

  const setCategoryTypes = useCallback(
    (types: string[]) =>
      setFilter({ category_types: types.length > 0 ? types.join(",") : undefined }),
    [setFilter]
  )

  const setGroupBy = useCallback(
    // Omit the default from the URL to keep it clean.
    (g: string) => setFilter({ group_by: g === "parent_category" ? undefined : g }),
    [setFilter]
  )

  const setSearch = useCallback(
    (q: string) => setFilter({ search: q || undefined, page: "1" }),
    [setFilter]
  )

  /**
   * Cycle the transactions table sort by column. State machine per column:
   * inactive → asc → desc → inactive. The active column's direction is
   * tracked separately so toggling to another column resets the direction.
   * Resets pagination to page 1 so the user sees the new first page.
   */
  const cycleTxSort = useCallback(
    (col: "date" | "amount" | "category") => {
      if (txSort !== col) {
        setFilter({ txSort: col, txDir: "asc", page: "1" })
      } else if (txDir === "asc") {
        setFilter({ txSort: col, txDir: "desc", page: "1" })
      } else {
        setFilter({ txSort: undefined, txDir: undefined, page: "1" })
      }
    },
    [setFilter, txSort, txDir]
  )

  return {
    start,
    end,
    preset,
    view,
    granularity,
    profileId,
    page,
    accounts,
    categories,
    categoryTypes,
    groupBy,
    search,
    hideSmall,
    assetClassSettings,
    txSort,
    txDir,
    cycleTxSort,
    setFilter,
    setPreset,
    setSearch,
    setView,
    setGranularity,
    setPage,
    setProfileId,
    setAccounts,
    setCategories,
    setCategoryTypes,
    setGroupBy,
  }
}
