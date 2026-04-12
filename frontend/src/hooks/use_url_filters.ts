import { useSearchParams } from "react-router-dom"
import { useCallback } from "react"
import type { Granularity } from "@/types"
import { format, subMonths, subYears, startOfMonth, startOfYear } from "date-fns"

export type Preset =
  | "this-month"
  | "last-3-months"
  | "last-12-months"
  | "ytd"
  | "3-years"
  | "5-years"
  | "10-years"
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
  const search = searchParams.get("search") || ""

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
    (p: Preset) => {
      const range = getPresetRange(p)
      setFilter({ preset: p, start: range.start, end: range.end, page: "1" })
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

  const setSearch = useCallback(
    (q: string) => setFilter({ search: q || undefined, page: "1" }),
    [setFilter]
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
    search,
    setFilter,
    setPreset,
    setSearch,
    setView,
    setGranularity,
    setPage,
    setProfileId,
    setAccounts,
    setCategories,
  }
}
