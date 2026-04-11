import type { Transaction } from "@/types"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Badge } from "@/components/ui/badge"
import { Currency } from "@/components/currency"
import { formatDate } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { ChevronLeft, ChevronRight } from "lucide-react"

interface TransactionTableProps {
  transactions: Transaction[]
  total: number
  page: number
  limit: number
  onPageChange: (page: number) => void
  accountNames?: Record<string, string>
}

export function TransactionTable({
  transactions,
  total,
  page,
  limit,
  onPageChange,
  accountNames = {},
}: TransactionTableProps) {
  const totalPages = Math.ceil(total / limit)

  return (
    <div>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Date</TableHead>
            <TableHead>Merchant</TableHead>
            <TableHead>Category</TableHead>
            <TableHead className="text-right">Amount</TableHead>
            <TableHead>Account</TableHead>
            <TableHead>Source</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {transactions.map((t) => (
            <TableRow key={t.id}>
              <TableCell className="whitespace-nowrap">
                {formatDate(t.date)}
              </TableCell>
              <TableCell>{t.normalized}</TableCell>
              <TableCell>
                {t.category ? (
                  <Badge variant="secondary" className="text-xs">
                    {t.category}
                  </Badge>
                ) : (
                  <Badge variant="outline" className="text-xs text-muted-foreground">
                    Uncategorized
                  </Badge>
                )}
              </TableCell>
              <TableCell className="text-right">
                <Currency amount={t.amount} currency={t.currency} />
              </TableCell>
              <TableCell className="text-sm text-muted-foreground">
                {accountNames[t.account_id] ?? t.account_id}
              </TableCell>
              <TableCell>
                {t.category_source === "claude" && t.confidence && (
                  <Badge variant="outline" className="text-xs">
                    AI {Math.round(t.confidence * 100)}%
                  </Badge>
                )}
                {t.category_source === "manual" && (
                  <Badge variant="outline" className="text-xs">
                    Manual
                  </Badge>
                )}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>

      {/* Pagination */}
      <div className="flex items-center justify-between border-t px-2 py-3">
        <span className="text-sm text-muted-foreground">
          {total} transactions, page {page} of {totalPages}
        </span>
        <div className="flex gap-1">
          <Button
            variant="outline"
            size="sm"
            disabled={page <= 1}
            onClick={() => onPageChange(page - 1)}
          >
            <ChevronLeft className="h-4 w-4" />
          </Button>
          <Button
            variant="outline"
            size="sm"
            disabled={page >= totalPages}
            onClick={() => onPageChange(page + 1)}
          >
            <ChevronRight className="h-4 w-4" />
          </Button>
        </div>
      </div>
    </div>
  )
}
