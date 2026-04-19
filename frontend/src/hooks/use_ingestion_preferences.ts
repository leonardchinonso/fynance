import { useState } from "react"
import type { Account } from "@/types"

const STORAGE_KEY = "fynance-ingestion-prefs"

/**
 * Manages the ordered list of account IDs for the ingestion wizard.
 *
 * - Accounts in the list appear in the wizard in the stored order.
 * - Accounts NOT in the list are hidden from the ingestion flow.
 * - If no preferences are set (null), the wizard shows ALL accounts
 *   in the order they come from the API.
 */
export function useIngestionPreferences() {
  const [orderedAccountIds, setRaw] = useState<string[] | null>(() => {
    try {
      const stored = localStorage.getItem(STORAGE_KEY)
      if (!stored) return null
      const parsed = JSON.parse(stored)
      return Array.isArray(parsed) ? parsed : null
    } catch {
      return null
    }
  })

  function setOrderedAccountIds(ids: string[] | null) {
    setRaw(ids)
    if (ids === null) {
      localStorage.removeItem(STORAGE_KEY)
    } else {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(ids))
    }
  }

  /** Is a specific account visible in the ingestion flow? */
  function isAccountVisible(id: string): boolean {
    if (orderedAccountIds === null) return true // No preferences = all visible
    return orderedAccountIds.includes(id)
  }

  /**
   * Get accounts ordered for the ingestion wizard.
   * - If preferences are set: returns only visible accounts in the stored order.
   * - If no preferences: returns all accounts in API order.
   */
  function getOrderedAccounts(allAccounts: Account[]): Account[] {
    if (orderedAccountIds === null) return allAccounts
    const map = new Map(allAccounts.map((a) => [a.id, a]))
    return orderedAccountIds
      .filter((id) => map.has(id))
      .map((id) => map.get(id)!)
  }

  /** Get accounts that are hidden from the ingestion flow. */
  function getHiddenAccounts(allAccounts: Account[]): Account[] {
    if (orderedAccountIds === null) return []
    const visibleSet = new Set(orderedAccountIds)
    return allAccounts.filter((a) => !visibleSet.has(a.id))
  }

  /** Add an account to the ingestion list (appends to end). */
  function showAccount(id: string, allAccounts: Account[]) {
    const current = orderedAccountIds ?? allAccounts.map((a) => a.id)
    if (current.includes(id)) return
    setOrderedAccountIds([...current, id])
  }

  /** Remove an account from the ingestion list (hides it). */
  function hideAccount(id: string, allAccounts: Account[]) {
    const current = orderedAccountIds ?? allAccounts.map((a) => a.id)
    setOrderedAccountIds(current.filter((i) => i !== id))
  }

  /** Reorder accounts within the visible list. */
  function reorderAccounts(fromIndex: number, toIndex: number) {
    if (!orderedAccountIds) return
    const updated = [...orderedAccountIds]
    const [moved] = updated.splice(fromIndex, 1)
    updated.splice(toIndex, 0, moved)
    setOrderedAccountIds(updated)
  }

  return {
    orderedAccountIds,
    setOrderedAccountIds,
    isAccountVisible,
    getOrderedAccounts,
    getHiddenAccounts,
    showAccount,
    hideAccount,
    reorderAccounts,
  }
}
