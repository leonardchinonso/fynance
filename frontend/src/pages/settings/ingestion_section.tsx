import { useState, useEffect } from "react"
import { api } from "@/api/client"
import type { Account } from "@/types"
import { useIngestionPreferences } from "@/hooks/use_ingestion_preferences"
import { DraggableList, DragHandle } from "@/components/draggable_list"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { X, Plus, Building2 } from "lucide-react"

export function IngestionSection() {
  const [accounts, setAccounts] = useState<Account[]>([])
  const [loading, setLoading] = useState(true)
  const [dragId, setDragId] = useState<string | null>(null)
  const {
    orderedAccountIds,
    getOrderedAccounts,
    getHiddenAccounts,
    showAccount,
    hideAccount,
    reorderAccounts,
    setOrderedAccountIds,
  } = useIngestionPreferences()

  useEffect(() => {
    api.getAccounts().then((data) => {
      setAccounts(data)
      setLoading(false)
    }).catch(() => setLoading(false))
  }, [])

  // Initialize preferences if not set: show all accounts
  const visibleAccounts = getOrderedAccounts(accounts)
  const hiddenAccounts = getHiddenAccounts(accounts)
  const hasPreferences = orderedAccountIds !== null

  function initializeOrder() {
    setOrderedAccountIds(accounts.map((a) => a.id))
  }

  if (loading) {
    return (
      <Card id="ingestion">
        <CardHeader>
          <CardTitle className="text-lg">Data Ingestion</CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-sm text-muted-foreground py-4 text-center">Loading accounts...</p>
        </CardContent>
      </Card>
    )
  }

  // Wrap visible accounts with id field for DraggableList
  const draggableItems = visibleAccounts.map((a) => ({ ...a, id: a.id }))

  return (
    <Card id="ingestion">
      <CardHeader>
        <CardTitle className="text-lg">Data Ingestion</CardTitle>
        <p className="text-sm text-muted-foreground">
          Set the order and visibility of accounts in the monthly ingestion wizard. Drag to reorder. Remove accounts you do not want in the flow.
        </p>
      </CardHeader>
      <CardContent className="space-y-4">
        {/* Ingestion order */}
        <div>
          <h4 className="text-xs font-medium text-muted-foreground uppercase tracking-wider mb-2">
            Ingestion Order ({visibleAccounts.length} accounts)
          </h4>
          {!hasPreferences && accounts.length > 0 && (
            <div className="rounded-lg border border-dashed p-3 mb-2 text-center">
              <p className="text-sm text-muted-foreground mb-2">
                No custom order set. The wizard will show all accounts in the default order.
              </p>
              <Button size="sm" variant="outline" onClick={initializeOrder}>
                Customize order
              </Button>
            </div>
          )}
          {hasPreferences && visibleAccounts.length === 0 && (
            <p className="text-sm text-muted-foreground py-2 text-center">
              All accounts are hidden. The wizard will have no accounts to process.
            </p>
          )}
          {hasPreferences && visibleAccounts.length > 0 && (
            <DraggableList
              items={draggableItems}
              dragId={dragId}
              onDragChange={setDragId}
              onReorder={reorderAccounts}
              listClassName="space-y-1"
              renderItem={(account) => (
                <div className="flex items-center gap-2 rounded-lg border p-2.5 bg-background group">
                  <DragHandle />
                  <Building2 className="h-3.5 w-3.5 text-muted-foreground shrink-0" />
                  <div className="flex-1 min-w-0">
                    <p className="text-sm font-medium truncate">{account.name}</p>
                    <p className="text-xs text-muted-foreground">{account.institution}</p>
                  </div>
                  <Badge variant="secondary" className="text-[10px] capitalize shrink-0">{account.type}</Badge>
                  <Button variant="ghost" size="icon" className="h-7 w-7 opacity-0 group-hover:opacity-100 shrink-0"
                    onClick={() => hideAccount(account.id, accounts)} title="Remove from ingestion flow">
                    <X className="h-3.5 w-3.5" />
                  </Button>
                </div>
              )}
            />
          )}
        </div>

        {/* Hidden accounts */}
        {hasPreferences && hiddenAccounts.length > 0 && (
          <div>
            <h4 className="text-xs font-medium text-muted-foreground uppercase tracking-wider mb-2">
              Hidden ({hiddenAccounts.length})
            </h4>
            <div className="space-y-1">
              {hiddenAccounts.map((a) => (
                <div key={a.id} className="flex items-center gap-2 rounded-lg border border-dashed p-2.5 opacity-60 hover:opacity-100 transition-opacity">
                  <Building2 className="h-3.5 w-3.5 text-muted-foreground shrink-0" />
                  <div className="flex-1 min-w-0">
                    <p className="text-sm truncate">{a.name}</p>
                    <p className="text-xs text-muted-foreground">{a.institution}</p>
                  </div>
                  <Button variant="ghost" size="sm" className="h-7 gap-1"
                    onClick={() => showAccount(a.id, accounts)} title="Add back to ingestion flow">
                    <Plus className="h-3 w-3" /> Add
                  </Button>
                </div>
              ))}
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  )
}
