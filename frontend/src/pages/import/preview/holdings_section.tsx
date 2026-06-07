import { useMemo, useState, useEffect } from "react"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Button } from "@/components/ui/button"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { Input } from "@/components/ui/input"
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command"
import { Trash2, RotateCcw, ChevronDown, Plus } from "lucide-react"
import { cn, formatDate } from "@/lib/utils"
import type { HoldingsIngestionResult } from "@/bindings/HoldingsIngestionResult"
import type { HoldingsImportPayload } from "@/bindings/HoldingsImportPayload"
import type { Holding } from "@/bindings/Holding"
import type { HoldingType } from "@/bindings/HoldingType"
import type { KnownHolding } from "@/bindings/KnownHolding"
import type { Currency } from "@/types"
import { Badge } from "@/components/ui/badge"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { StatusBadge } from "./status_badge"
import { DateCell, SelectCell, TextCell } from "./editors"
import { SectionShell, useSectionControls } from "./section_shell"
import { SortHeader } from "./sort_header"
import { InlineDateRange } from "@/components/inline_date_range"
import { useUrlState } from "@/hooks/use_url_state"
import {
  Select as UiSelect,
  SelectContent as UiSelectContent,
  SelectItem as UiSelectItem,
  SelectTrigger as UiSelectTrigger,
} from "@/components/ui/select"

const STATUS_RANK: Record<string, number> = {
  new: 0,
  modify: 1,
  duplicate: 2,
  error: 3,
  removed: 4,
}

interface HldEntryShape {
  row: { symbol: string; sub_account: string | null; value: string; currency: string; as_of: string; status: string; derived: boolean }
  payloadIdx: number | null
}

function hldSortValue(
  entry: HldEntryShape,
  column: string,
  payload: HoldingsImportPayload | null,
  marked: Set<number>
): string | number {
  const edited = entry.payloadIdx !== null ? payload?.holdings[entry.payloadIdx] : undefined
  switch (column) {
    case "as_of":
      return edited?.as_of ?? entry.row.as_of
    case "action": {
      const s = entry.payloadIdx !== null && marked.has(entry.payloadIdx) ? "removed" : entry.row.status
      return STATUS_RANK[s] ?? 99
    }
    case "symbol": {
      const sym = edited?.symbol ?? entry.row.symbol
      const sub = edited?.sub_account ?? entry.row.sub_account ?? ""
      return `${sym}::${sub}`
    }
    case "currency":
      return edited?.currency ?? entry.row.currency
    case "value": {
      const raw = edited?.value ?? entry.row.value
      const n = parseFloat(raw)
      return Number.isFinite(n) ? n : 0
    }
    case "source":
      return entry.row.derived ? 1 : 0
    default:
      return 0
  }
}

function holdingKey(symbol: string, subAccount: string | null | undefined): string {
  return `${symbol}::${subAccount ?? ""}`
}

interface PrevRef {
  value: string
  /** ISO date or full ISO datetime — caller normalizes for display. */
  as_of: string
  /** Whether the previous snapshot lives in the DB (true) or earlier in the same batch (false). */
  fromDb: boolean
}

/**
 * For each preview row, find the most recent "previous" snapshot to diff
 * against. Search order: earlier rows in the SAME batch (sorted by as_of),
 * then the latest open snapshot in the DB. Returns null when no previous
 * exists — that's a brand-new position.
 */
function buildPrevRefs(
  rows: { symbol: string; sub_account: string | null; value: string; as_of: string }[],
  knownHoldings: KnownHolding[]
): (PrevRef | null)[] {
  const knownByKey = new Map<string, KnownHolding>()
  for (const k of knownHoldings) {
    knownByKey.set(holdingKey(k.symbol, k.sub_account ?? null), k)
  }
  // Group every batch row by holding key, indexed for "earlier in batch" lookups.
  const indexed = rows.map((r, i) => ({ ...r, _i: i }))
  return rows.map((r, i) => {
    const key = holdingKey(r.symbol, r.sub_account)
    let best: { value: string; as_of: string; fromDb: boolean } | null = null
    for (const other of indexed) {
      if (other._i === i) continue
      if (holdingKey(other.symbol, other.sub_account) !== key) continue
      if (other.as_of >= r.as_of) continue
      if (!best || other.as_of > best.as_of) {
        best = { value: other.value, as_of: other.as_of, fromDb: false }
      }
    }
    if (!best) {
      const dbMatch = knownByKey.get(key)
      if (dbMatch && dbMatch.last_as_of < (r.as_of.split("T")[0] ?? r.as_of)) {
        best = { value: dbMatch.last_value, as_of: dbMatch.last_as_of, fromDb: true }
      }
    }
    return best
  })
}

function diffStr(curr: string, prev: string): { text: string; positive: boolean | null } {
  const c = parseFloat(curr)
  const p = parseFloat(prev)
  if (!Number.isFinite(c) || !Number.isFinite(p)) return { text: "—", positive: null }
  const d = c - p
  if (d === 0) return { text: "±0.00", positive: null }
  const sign = d > 0 ? "+" : ""
  return { text: `${sign}${d.toFixed(2)}`, positive: d > 0 }
}

function PrevDiffCell({
  currentValue,
  prev,
  currency,
}: {
  currentValue: string
  prev: PrevRef | null
  currency: string
}) {
  if (!prev) {
    return (
      <Tooltip>
        <TooltipTrigger
          render={
            <Badge
              variant="outline"
              className={cn(
                "border-violet-500/40 bg-violet-500/10 text-violet-700 dark:text-violet-400 hover:bg-violet-500/10 cursor-help"
              )}
            >
              New position
            </Badge>
          }
        />
        <TooltipContent>
          First time we've seen this symbol on this account. Double-check the symbol before committing.
        </TooltipContent>
      </Tooltip>
    )
  }
  const { text, positive } = diffStr(currentValue, prev.value)
  const dateStr = prev.as_of.split("T")[0] ?? prev.as_of
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <span
            className={cn(
              "text-xs tabular-nums cursor-help",
              positive === true && "text-emerald-600 dark:text-emerald-400",
              positive === false && "text-rose-600 dark:text-rose-400",
              positive === null && "text-muted-foreground"
            )}
          >
            {text}
          </span>
        }
      />
      <TooltipContent>
        Previous: {prev.value} {currency} on {formatDate(dateStr)}{" "}
        {prev.fromDb ? "(in DB)" : "(earlier row in this import)"}.
      </TooltipContent>
    </Tooltip>
  )
}

/**
 * Symbol picker for an editable holding row. Lists existing open symbols on
 * the account first; final option lets the user fall through to free-text for
 * a genuinely new position. Picking an existing symbol fills name / type /
 * currency / sub_account from the known snapshot.
 */
function SymbolCombobox({
  value,
  knownHoldings,
  onPickExisting,
  onPickCustom,
}: {
  value: string
  knownHoldings: KnownHolding[]
  onPickExisting: (kh: KnownHolding) => void
  onPickCustom: (symbol: string) => void
}) {
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState("")
  const sorted = useMemo(
    () => [...knownHoldings].sort((a, b) => a.symbol.localeCompare(b.symbol)),
    [knownHoldings]
  )
  const trimmed = query.trim()
  const knownMatchesQuery = trimmed
    ? sorted.some((k) => k.symbol.toLowerCase() === trimmed.toLowerCase())
    : false

  return (
    <Popover
      open={open}
      onOpenChange={(o) => {
        setOpen(o)
        if (!o) setQuery("")
      }}
    >
      <PopoverTrigger
        render={
          <button
            type="button"
            className={cn(
              "inline-flex w-full items-center justify-between gap-1.5",
              "h-7 px-1.5 py-0.5 text-xs font-mono rounded-md border bg-transparent",
              "hover:border-ring focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            )}
          >
            <span className="truncate">{value || "—"}</span>
            <ChevronDown className="h-3 w-3 text-muted-foreground" />
          </button>
        }
      />
      <PopoverContent className="w-[260px] p-0" align="start">
        <Command>
          <CommandInput
            placeholder="Search or type a new symbol…"
            value={query}
            onValueChange={setQuery}
            className="h-9 text-xs"
          />
          <CommandList>
            {sorted.length === 0 && (
              <CommandEmpty>No existing holdings on this account.</CommandEmpty>
            )}
            {sorted.length > 0 && (
              <CommandGroup heading="Existing on this account">
                {sorted.map((k) => (
                  <CommandItem
                    key={holdingKey(k.symbol, k.sub_account ?? null)}
                    value={`${k.symbol} ${k.name}`}
                    onSelect={() => {
                      onPickExisting(k)
                      setOpen(false)
                      setQuery("")
                    }}
                  >
                    <span className="font-mono text-xs">{k.symbol}</span>
                    <span className="ml-2 truncate text-xs text-muted-foreground">{k.name}</span>
                  </CommandItem>
                ))}
              </CommandGroup>
            )}
            {trimmed && !knownMatchesQuery && (
              <CommandGroup heading="New">
                <CommandItem
                  value={`__new__${trimmed}`}
                  onSelect={() => {
                    onPickCustom(trimmed)
                    setOpen(false)
                    setQuery("")
                  }}
                >
                  <Plus className="mr-2 h-3 w-3" />
                  Add new symbol "{trimmed}"
                </CommandItem>
              </CommandGroup>
            )}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  )
}

function SourceBadge({ derived }: { derived: boolean }) {
  if (derived) {
    return (
      <Tooltip>
        <TooltipTrigger
          render={
            <Badge
              variant="outline"
              className={cn(
                "border-sky-500/40 bg-sky-500/10 text-sky-700 dark:text-sky-400 hover:bg-sky-500/10 cursor-help"
              )}
            >
              Derived
            </Badge>
          }
        />
        <TooltipContent>
          Computed by the agent from other data (e.g. interpolated from neighbouring snapshots or rolled up from transactions).
        </TooltipContent>
      </Tooltip>
    )
  }
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Badge
            variant="outline"
            className={cn(
              "border-emerald-500/40 bg-emerald-500/10 text-emerald-700 dark:text-emerald-400 hover:bg-emerald-500/10 cursor-help"
            )}
          >
            From doc
          </Badge>
        }
      />
      <TooltipContent>
        Read directly from the source document for this exact date.
      </TooltipContent>
    </Tooltip>
  )
}

const HOLDING_TYPES: HoldingType[] = [
  "stock", "etf", "fund", "bond", "crypto", "cash", "property", "loan", "credit",
]

const DECIMAL_RE = /^-?\d*\.?\d*$/

function toNum(s: string): number {
  const n = parseFloat(s)
  return Number.isFinite(n) ? n : 0
}

function trimZeros(s: string): string {
  if (!s.includes(".")) return s
  return s.replace(/\.?0+$/, "")
}

/**
 * Value editor for a holding. The backend always wants `quantity` and `value`
 * (and optionally `price_per_unit`) with the invariant `value = qty * price`.
 * The table only ever shows `value`; this popover lets the user edit value
 * directly, OR drill into quantity + price-per-unit. Editing any field
 * propagates per `value = qty * price`.
 */
function HoldingValuePopover({
  value,
  quantity,
  pricePerUnit,
  currency,
  disabled,
  onChange,
}: {
  value: string
  quantity: string
  pricePerUnit: string | null
  currency: string
  disabled?: boolean
  onChange: (next: { value: string; quantity: string; pricePerUnit: string | null }) => void
}) {
  const [open, setOpen] = useState(false)
  const [v, setV] = useState(value)
  const [q, setQ] = useState(quantity || "1")
  const [p, setP] = useState(pricePerUnit ?? "")

  useEffect(() => {
    if (open) {
      setV(value)
      setQ(quantity || "1")
      setP(pricePerUnit ?? "")
    }
  }, [open, value, quantity, pricePerUnit])

  function setVRecalcPrice(nv: string) {
    if (!(nv === "" || nv === "-" || DECIMAL_RE.test(nv))) return
    setV(nv)
    const qn = toNum(q)
    if (qn > 0 && nv && nv !== "-") {
      setP(trimZeros((toNum(nv) / qn).toFixed(6)))
    }
  }

  function setQRecalcValue(nq: string) {
    if (!(nq === "" || nq === "-" || DECIMAL_RE.test(nq))) return
    setQ(nq)
    const pn = toNum(p)
    if (nq && nq !== "-" && p) {
      setV(trimZeros((toNum(nq) * pn).toFixed(2)))
    }
  }

  function setPRecalcValue(np: string) {
    if (!(np === "" || np === "-" || DECIMAL_RE.test(np))) return
    setP(np)
    const qn = toNum(q)
    if (np && np !== "-" && q) {
      setV(trimZeros((qn * toNum(np)).toFixed(2)))
    }
  }

  function commitAndClose() {
    onChange({
      value: v || "0",
      quantity: q || "1",
      pricePerUnit: p ? p : null,
    })
    setOpen(false)
  }

  return (
    <Popover
      open={open}
      onOpenChange={(o) => {
        if (o) setOpen(true)
        else commitAndClose()
      }}
    >
      <PopoverTrigger
        render={
          <button
            type="button"
            disabled={disabled}
            className={cn(
              "h-7 px-1.5 py-0.5 text-xs text-right tabular-nums rounded-md border bg-transparent w-full",
              "hover:border-ring focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
              "disabled:opacity-50 disabled:cursor-not-allowed"
            )}
          >
            {value || "0"}
          </button>
        }
      />
      <PopoverContent className="w-[260px] p-3" align="end">
        <p className="text-xs font-medium text-muted-foreground mb-2">
          Value <span className="text-muted-foreground/60">({currency})</span>
        </p>
        <Input
          className="h-8 text-sm text-right tabular-nums mb-3"
          inputMode="decimal"
          value={v}
          onChange={(e) => setVRecalcPrice(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter") commitAndClose() }}
          autoFocus
        />
        <p className="text-[10px] uppercase tracking-wider text-muted-foreground mb-1.5">
          or set quantity × price
        </p>
        <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-1.5">
          <Input
            className="h-7 text-xs text-right tabular-nums"
            inputMode="decimal"
            value={q}
            onChange={(e) => setQRecalcValue(e.target.value)}
            placeholder="Qty"
            aria-label="Quantity"
            onKeyDown={(e) => { if (e.key === "Enter") commitAndClose() }}
          />
          <span className="text-xs text-muted-foreground">×</span>
          <Input
            className="h-7 text-xs text-right tabular-nums"
            inputMode="decimal"
            value={p}
            onChange={(e) => setPRecalcValue(e.target.value)}
            placeholder="Price"
            aria-label="Price per unit"
            onKeyDown={(e) => { if (e.key === "Enter") commitAndClose() }}
          />
        </div>
        <div className="mt-3 flex justify-end">
          <Button size="sm" className="h-7 text-xs" onClick={commitAndClose}>Done</Button>
        </div>
      </PopoverContent>
    </Popover>
  )
}

interface Props {
  result: HoldingsIngestionResult
  payload: HoldingsImportPayload | null
  setPayload: (p: HoldingsImportPayload | null) => void
  markedForDeletion: Set<number>
  setMarkedForDeletion: (s: Set<number>) => void
  currencyOptions: Currency[]
}

export function HoldingsSection({
  result,
  payload,
  setPayload,
  markedForDeletion,
  setMarkedForDeletion,
  currencyOptions,
}: Props) {
  const ctrls = useSectionControls()
  const url = useUrlState()

  const { minDate, maxDate } = useMemo(() => {
    const dates = result.rows
      .map((r) => r.as_of.split("T")[0] ?? "")
      .filter(Boolean)
      .sort()
    return { minDate: dates[0] ?? "", maxDate: dates[dates.length - 1] ?? "" }
  }, [result.rows])

  const dateFrom = url.get("hldDateFrom", minDate)
  const dateTo = url.get("hldDateTo", maxDate)
  const holdingFilter = url.get("hldKey", "__all__")
  const sortColumn = url.get("hldSort", "")
  const sortDir = (url.get("hldDir", "asc") === "desc" ? "desc" : "asc") as "asc" | "desc"

  function setDateRange(start: string, end: string) {
    url.set({
      hldDateFrom: start === minDate ? null : start,
      hldDateTo: end === maxDate ? null : end,
    })
    ctrls.setPage(1)
  }
  function setHoldingFilter(v: string) {
    url.set({ hldKey: v === "__all__" ? null : v })
    ctrls.setPage(1)
  }
  function cycleSort(col: string) {
    if (sortColumn !== col) {
      url.set({ hldSort: col, hldDir: null })
    } else if (sortDir === "asc") {
      url.set({ hldSort: col, hldDir: "desc" })
    } else {
      url.set({ hldSort: null, hldDir: null })
    }
  }

  const holdingOptions = useMemo(() => {
    const map = new Map<string, string>()
    result.rows.forEach((r) => {
      const key = `${r.symbol}::${r.sub_account ?? ""}`
      const label = r.sub_account ? `${r.symbol} / ${r.sub_account}` : r.symbol
      map.set(key, label)
    })
    return [...map.entries()].sort(([, a], [, b]) => a.localeCompare(b))
  }, [result.rows])

  /**
   * Holdings payload contains both "new" and "modify" rows in the same
   * order as `rows[]`. Every preview row maps to a payload entry.
   */
  const rowPayloadIndex = useMemo(() => {
    let n = 0
    return result.rows.map((r) =>
      r.status === "new" || r.status === "modify" ? n++ : null
    )
  }, [result.rows])

  const filteredEntries = useMemo(() => {
    const fromIso = dateFrom ? `${dateFrom}T00:00:00` : null
    const toIso = dateTo ? `${dateTo}T23:59:59` : null
    const filtered = result.rows
      .map((row, displayIdx) => ({ row, displayIdx, payloadIdx: rowPayloadIndex[displayIdx] }))
      .filter(({ row }) => {
        if (fromIso && row.as_of < fromIso) return false
        if (toIso && row.as_of > toIso) return false
        if (holdingFilter !== "__all__") {
          const key = `${row.symbol}::${row.sub_account ?? ""}`
          if (key !== holdingFilter) return false
        }
        return true
      })
    if (!sortColumn) return filtered
    const sign = sortDir === "asc" ? 1 : -1
    return [...filtered].sort((a, b) => {
      const av = hldSortValue(a, sortColumn, payload, markedForDeletion)
      const bv = hldSortValue(b, sortColumn, payload, markedForDeletion)
      if (av < bv) return -1 * sign
      if (av > bv) return 1 * sign
      return 0
    })
  }, [result.rows, rowPayloadIndex, dateFrom, dateTo, holdingFilter, sortColumn, sortDir, markedForDeletion, payload])

  const totalRows = filteredEntries.length
  const start = (ctrls.page - 1) * ctrls.pageSize
  const pageEntries = filteredEntries.slice(start, start + ctrls.pageSize)

  // Map each displayIdx → its previous-snapshot reference. Computed once
  // against the unfiltered batch so chaining works even when the user has
  // filtered the visible rows.
  const prevRefsByDisplayIdx = useMemo(
    () => buildPrevRefs(result.rows, result.known_holdings),
    [result.rows, result.known_holdings]
  )

  function updatePayloadAt(idx: number, patch: Partial<Holding>) {
    if (!payload) return
    const next: Holding[] = payload.holdings.map((h, i) => (i === idx ? { ...h, ...patch } : h))
    setPayload({ ...payload, holdings: next })
  }

  function toggleDelete(idx: number) {
    const next = new Set(markedForDeletion)
    if (next.has(idx)) next.delete(idx)
    else next.add(idx)
    setMarkedForDeletion(next)
  }

  const willCommit = (payload?.holdings.length ?? 0) - markedForDeletion.size

  const summary = (
    <>
      {result.new} new · {result.modify} update
      {markedForDeletion.size > 0 && (
        <> · <span className="text-foreground">{markedForDeletion.size} marked skip</span></>
      )}
      <> · <span className="text-foreground">{Math.max(0, willCommit)} will commit</span></>
    </>
  )

  const holdingTypeOpts = HOLDING_TYPES.map((t) => ({ value: t, label: t }))
  const currencyOpts = currencyOptions.map((c) => ({ value: c.code, label: c.code }))

  const holdingFilterLabel = holdingFilter === "__all__"
    ? "All holdings"
    : holdingOptions.find(([k]) => k === holdingFilter)?.[1] ?? holdingFilter

  const filterSlot = (
    <>
      <InlineDateRange
        start={dateFrom}
        end={dateTo}
        onChange={({ start, end }) => setDateRange(start, end)}
      />
      <UiSelect
        value={holdingFilter}
        onValueChange={(v) => { if (v) setHoldingFilter(v) }}
      >
        <UiSelectTrigger className="h-8 text-xs min-w-[10rem]">
          <span>{holdingFilterLabel}</span>
        </UiSelectTrigger>
        <UiSelectContent>
          <UiSelectItem value="__all__">All holdings</UiSelectItem>
          {holdingOptions.map(([key, label]) => (
            <UiSelectItem key={key} value={key}>{label}</UiSelectItem>
          ))}
        </UiSelectContent>
      </UiSelect>
    </>
  )

  return (
    <SectionShell
      title="Holdings"
      summary={summary}
      filterSlot={filterSlot}
      totalRows={totalRows}
      pageSize={ctrls.pageSize}
      page={ctrls.page}
      onPageChange={ctrls.setPage}
      onPageSizeChange={(s) => {
        ctrls.setPageSize(s)
        ctrls.setPage(1)
      }}
    >
      <Table>
        <TableHeader>
          <TableRow>
            <SortHeader label="Action" columnId="action" activeColumn={sortColumn} direction={sortDir} onClick={() => cycleSort("action")} className="w-24" />
            <SortHeader label="Source" columnId="source" activeColumn={sortColumn} direction={sortDir} onClick={() => cycleSort("source")} className="w-24" />
            <SortHeader label="Symbol" columnId="symbol" activeColumn={sortColumn} direction={sortDir} onClick={() => cycleSort("symbol")} className="w-28" />
            <TableHead>Name</TableHead>
            <TableHead className="w-28">Type</TableHead>
            <SortHeader label="Value" columnId="value" activeColumn={sortColumn} direction={sortDir} onClick={() => cycleSort("value")} className="w-32" align="right" />
            <TableHead className="w-28 text-right">vs prev</TableHead>
            <SortHeader label="Currency" columnId="currency" activeColumn={sortColumn} direction={sortDir} onClick={() => cycleSort("currency")} className="w-20" />
            <SortHeader label="As of" columnId="as_of" activeColumn={sortColumn} direction={sortDir} onClick={() => cycleSort("as_of")} className="w-36" />
            <TableHead className="w-10" />
          </TableRow>
        </TableHeader>
        <TableBody>
          {pageEntries.length === 0 && (
            <TableRow>
              <TableCell colSpan={10} className="text-center text-xs text-muted-foreground py-6">
                No rows match
              </TableCell>
            </TableRow>
          )}
          {pageEntries.map(({ row, displayIdx, payloadIdx }) => {
            const editable = payloadIdx !== null && payload !== null
            const h = editable ? payload.holdings[payloadIdx] : null
            const marked = payloadIdx !== null && markedForDeletion.has(payloadIdx)
            return (
              <TableRow
                key={`${displayIdx}-${row.symbol}`}
                className={cn(
                  marked && "opacity-40 line-through",
                  !editable && "bg-muted/30 text-muted-foreground"
                )}
              >
                <TableCell>
                  <StatusBadge status={marked ? "removed" : row.status} />
                </TableCell>
                <TableCell>
                  <SourceBadge derived={row.derived} />
                </TableCell>
                <TableCell>
                  {editable && h && !marked ? (
                    <SymbolCombobox
                      value={h.symbol}
                      knownHoldings={result.known_holdings}
                      onPickExisting={(kh) =>
                        updatePayloadAt(payloadIdx, {
                          symbol: kh.symbol,
                          name: kh.name,
                          holding_type: kh.holding_type,
                          currency: kh.currency,
                          sub_account: kh.sub_account ?? null,
                        })
                      }
                      onPickCustom={(sym) => updatePayloadAt(payloadIdx, { symbol: sym })}
                    />
                  ) : (
                    <span className="text-xs font-mono">{row.symbol}</span>
                  )}
                </TableCell>
                <TableCell>
                  {editable && h && !marked ? (
                    <TextCell value={h.name} onChange={(v) => updatePayloadAt(payloadIdx, { name: v })} />
                  ) : (
                    <span className="text-xs text-muted-foreground">—</span>
                  )}
                </TableCell>
                <TableCell>
                  {editable && h && !marked ? (
                    <SelectCell
                      value={h.holding_type}
                      options={holdingTypeOpts}
                      onChange={(v) => updatePayloadAt(payloadIdx, { holding_type: v as HoldingType })}
                    />
                  ) : (
                    <span className="text-xs">—</span>
                  )}
                </TableCell>
                <TableCell className="text-right">
                  {editable && h && !marked ? (
                    <HoldingValuePopover
                      value={h.value}
                      quantity={h.quantity}
                      pricePerUnit={h.price_per_unit}
                      currency={h.currency}
                      onChange={({ value, quantity, pricePerUnit }) =>
                        updatePayloadAt(payloadIdx, {
                          value,
                          quantity,
                          price_per_unit: pricePerUnit,
                        })
                      }
                    />
                  ) : (
                    <span className="text-xs tabular-nums">
                      {row.value}
                      {row.status === "modify" && row.existing_value && (
                        <span className="ml-1 text-muted-foreground">(was {row.existing_value})</span>
                      )}
                    </span>
                  )}
                </TableCell>
                <TableCell className="text-right">
                  <PrevDiffCell
                    currentValue={editable && h && !marked ? h.value : row.value}
                    prev={prevRefsByDisplayIdx[displayIdx] ?? null}
                    currency={editable && h && !marked ? h.currency : row.currency}
                  />
                </TableCell>
                <TableCell>
                  {editable && h && !marked ? (
                    <SelectCell
                      value={h.currency}
                      options={currencyOpts}
                      onChange={(v) => updatePayloadAt(payloadIdx, { currency: v })}
                    />
                  ) : (
                    <span className="text-xs">{row.currency}</span>
                  )}
                </TableCell>
                <TableCell>
                  {editable && h && !marked ? (
                    <DateCell value={h.as_of} onChange={(v) => updatePayloadAt(payloadIdx, { as_of: v })} />
                  ) : (
                    <span className="text-xs text-muted-foreground tabular-nums">{formatDate(row.as_of.split("T")[0] ?? row.as_of)}</span>
                  )}
                </TableCell>
                <TableCell>
                  {editable && payloadIdx !== null && (
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7"
                      onClick={() => toggleDelete(payloadIdx)}
                      aria-label={marked ? "Unmark deletion" : "Mark for deletion"}
                    >
                      {marked ? <RotateCcw className="h-3.5 w-3.5" /> : <Trash2 className="h-3.5 w-3.5" />}
                    </Button>
                  )}
                </TableCell>
              </TableRow>
            )
          })}
        </TableBody>
      </Table>
    </SectionShell>
  )
}
