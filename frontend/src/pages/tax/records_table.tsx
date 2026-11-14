import type { IncomeRecord } from "@/bindings/IncomeRecord"
import type { LineTypeInfo } from "@/bindings/LineTypeInfo"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table"
import { Pencil, Trash2 } from "lucide-react"
import { columnTotals, recordCell, recordMonth, sortRecords, type TableColumns } from "./tax_helpers"
import { Dash, Money } from "./tax_ui"

function ColumnHead({ info }: { info: LineTypeInfo }) {
  const boxText = info.box_ref && info.box_ref.length <= 3 ? `${info.form} box ${info.box_ref}` : info.form
  return (
    <TableHead className="text-right whitespace-nowrap" title={info.note}>
      {info.label}
      {boxText && <span className="block text-[10px] font-normal text-muted-foreground">{boxText}</span>}
    </TableHead>
  )
}

function periodLabel(r: IncomeRecord, byPayDate: boolean): string {
  if (byPayDate) return r.pay_date ?? r.period_end
  return r.kind === "year_end_adjustment" ? r.tax_year : `${r.period_start} to ${r.period_end}`
}

/** Monthly record table with catalogue-driven columns and a totals row. */
export function RecordsTable({
  records, columns, byPayDate, onEdit, onDelete,
}: {
  records: IncomeRecord[]
  columns: TableColumns
  byPayDate: boolean
  onEdit: (record: IncomeRecord) => void
  onDelete: (record: IncomeRecord) => void
}) {
  const all = [...columns.box, ...columns.shown]
  const sorted = sortRecords(records)
  const totals = columnTotals(sorted, all.map((c) => c.line_type))
  const currency = sorted[0]?.currency ?? "GBP"
  return (
    <div className="overflow-x-auto">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>{byPayDate ? "Pay date" : "Period"}</TableHead>
            {all.map((c) => <ColumnHead key={c.line_type} info={c} />)}
            <TableHead />
          </TableRow>
        </TableHeader>
        <TableBody>
          {sorted.map((r) => (
            <TableRow key={r.id} data-record-id={r.id}>
              <TableCell className="whitespace-nowrap">
                <span className="inline-flex items-center gap-1.5">
                  {r.kind === "year_end_adjustment" ? "Year-end adjustment" : periodLabel(r, byPayDate)}
                  {r.kind === "year_end_adjustment" && (
                    <Badge variant="secondary" className="h-5 px-1.5 text-[10px]">{r.tax_year}</Badge>
                  )}
                  {r.kind === "period" && r.origin === "shop_sync" && (
                    <Badge variant="outline" className="h-5 px-1.5 text-[10px]">synced</Badge>
                  )}
                  {r.kind === "period" && byPayDate && recordMonth(r) !== r.period_end.slice(0, 7) && (
                    <span className="text-xs text-muted-foreground">for {r.period_end.slice(0, 7)}</span>
                  )}
                </span>
              </TableCell>
              {all.map((c) => {
                const v = recordCell(r, c.line_type)
                return (
                  <TableCell key={c.line_type} className="text-right">
                    {v === null ? <Dash /> : <Money amount={v} currency={r.currency} />}
                  </TableCell>
                )
              })}
              <TableCell className="whitespace-nowrap text-right">
                {r.origin !== "shop_sync" && (
                  <Button variant="ghost" size="icon" className="h-7 w-7" onClick={() => onEdit(r)} title="Edit">
                    <Pencil className="h-3.5 w-3.5" />
                  </Button>
                )}
                <Button variant="ghost" size="icon" className="h-7 w-7" onClick={() => onDelete(r)} title="Delete">
                  <Trash2 className="h-3.5 w-3.5 text-muted-foreground" />
                </Button>
              </TableCell>
            </TableRow>
          ))}
          <TableRow className="border-t-2 font-medium">
            <TableCell>Total</TableCell>
            {all.map((c) => {
              const v = totals[c.line_type]
              return (
                <TableCell key={c.line_type} className="text-right">
                  {v === undefined ? <Dash /> : <Money amount={v} currency={currency} />}
                </TableCell>
              )
            })}
            <TableCell />
          </TableRow>
        </TableBody>
      </Table>
    </div>
  )
}
