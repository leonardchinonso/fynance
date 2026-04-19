import type { ImportResult } from "@/types"
import { Button } from "@/components/ui/button"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Badge } from "@/components/ui/badge"
import { CheckCircle2, AlertTriangle } from "lucide-react"

interface Props {
  result: ImportResult
  accountName: string
  onConfirm: () => void
  onCancel: () => void
}

function bigintToNumber(val: bigint): number {
  return Number(val)
}

export function PreviewTable({ result, accountName, onConfirm, onCancel }: Props) {
  const total = bigintToNumber(result.rows_total)
  const inserted = bigintToNumber(result.rows_inserted)
  const duplicates = bigintToNumber(result.rows_duplicate)
  const hasErrors = result.errors.length > 0

  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-lg font-semibold">Import Preview</h2>
        <p className="text-sm text-muted-foreground">{accountName} &middot; {result.filename}</p>
      </div>

      {/* Stats */}
      <div className="grid grid-cols-3 gap-3">
        <div className="rounded-lg border p-3 text-center">
          <p className="text-2xl font-bold tabular-nums">{total}</p>
          <p className="text-xs text-muted-foreground">Total rows</p>
        </div>
        <div className="rounded-lg border p-3 text-center">
          <p className="text-2xl font-bold tabular-nums text-green-600">{inserted}</p>
          <p className="text-xs text-muted-foreground">New</p>
        </div>
        <div className="rounded-lg border p-3 text-center">
          <p className="text-2xl font-bold tabular-nums text-amber-600">{duplicates}</p>
          <p className="text-xs text-muted-foreground">Duplicates</p>
        </div>
      </div>

      {/* Bank detection */}
      <div className="flex items-center gap-2">
        <Badge variant="outline" className="capitalize">{result.detected_bank}</Badge>
        <span className="text-xs text-muted-foreground">
          Detection confidence: {(result.detection_confidence * 100).toFixed(0)}%
        </span>
      </div>

      {/* Errors */}
      {hasErrors && (
        <div className="rounded-lg border border-destructive/50 bg-destructive/5 p-3">
          <div className="flex items-center gap-2 mb-2">
            <AlertTriangle className="h-4 w-4 text-destructive" />
            <p className="text-sm font-medium text-destructive">
              {result.errors.length} row{result.errors.length > 1 ? "s" : ""} had errors
            </p>
          </div>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-20">Row</TableHead>
                <TableHead>Reason</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {result.errors.slice(0, 10).map((err) => (
                <TableRow key={err.index}>
                  <TableCell className="tabular-nums">{err.index + 1}</TableCell>
                  <TableCell className="text-sm">{err.reason}</TableCell>
                </TableRow>
              ))}
              {result.errors.length > 10 && (
                <TableRow>
                  <TableCell colSpan={2} className="text-center text-muted-foreground text-sm">
                    ...and {result.errors.length - 10} more
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </div>
      )}

      {/* Success indicator */}
      {!hasErrors && inserted > 0 && (
        <div className="flex items-center gap-2 rounded-lg border border-green-200 bg-green-50 dark:border-green-900 dark:bg-green-950 p-3">
          <CheckCircle2 className="h-4 w-4 text-green-600" />
          <p className="text-sm text-green-700 dark:text-green-400">
            Import completed successfully. {inserted} new transaction{inserted > 1 ? "s" : ""} added.
          </p>
        </div>
      )}

      {/* Actions */}
      <div className="flex justify-end gap-2">
        <Button variant="outline" onClick={onCancel}>Cancel</Button>
        <Button onClick={onConfirm}>Done</Button>
      </div>
    </div>
  )
}
