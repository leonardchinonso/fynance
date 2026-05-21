import { useState } from "react"
import { format, parse } from "date-fns"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Input } from "@/components/ui/input"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { Calendar } from "@/components/ui/calendar"
import { formatDate, cn } from "@/lib/utils"

const cellInputClass = "h-7 px-1.5 py-0.5 text-xs"

export function TextCell({
  value,
  onChange,
  disabled,
  placeholder,
  align,
}: {
  value: string
  onChange: (v: string) => void
  disabled?: boolean
  placeholder?: string
  align?: "left" | "right"
}) {
  return (
    <Input
      value={value}
      onChange={(e) => onChange(e.target.value)}
      disabled={disabled}
      placeholder={placeholder}
      className={cn(cellInputClass, align === "right" && "text-right tabular-nums")}
    />
  )
}

const DECIMAL_RE = /^-?\d*\.?\d*$/

/**
 * Color a money string green (positive / income) or red (negative / spend).
 * Returns an empty string for zero or unparseable values so callers can opt
 * out by just spreading the result into a className.
 */
export function moneyDirectionClass(amount: string): string {
  const n = parseFloat(amount)
  if (!Number.isFinite(n) || n === 0) return ""
  return n > 0 ? "text-green-500" : "text-red-500"
}

export function DecimalCell({
  value,
  onChange,
  disabled,
  colorize,
}: {
  value: string
  onChange: (v: string) => void
  disabled?: boolean
  /** Tint green for positive, red for negative. Use on money columns. */
  colorize?: boolean
}) {
  return (
    <Input
      inputMode="decimal"
      value={value}
      onChange={(e) => {
        const v = e.target.value
        if (v === "" || v === "-" || DECIMAL_RE.test(v)) onChange(v)
      }}
      disabled={disabled}
      className={cn(
        cellInputClass,
        "text-right tabular-nums",
        colorize && moneyDirectionClass(value)
      )}
    />
  )
}

/**
 * Inline date editor: renders the date as plain (humanised) text that
 * opens a Calendar popover on click. Keeps the same readonly-style look
 * as a static cell so the table doesn't read as a forest of input boxes.
 */
export function DateCell({
  value,
  onChange,
  disabled,
}: {
  /** ISO 8601 datetime string. Only the date portion is editable. */
  value: string
  onChange: (isoDateTime: string) => void
  disabled?: boolean
}) {
  const [open, setOpen] = useState(false)
  const datePart = value.split("T")[0] ?? value
  const parsed = datePart ? parse(datePart, "yyyy-MM-dd", new Date()) : undefined
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        disabled={disabled}
        className={cn(
          "inline-flex items-center text-xs tabular-nums cursor-pointer",
          "hover:text-foreground hover:underline transition-colors",
          "text-muted-foreground",
          "disabled:opacity-50 disabled:cursor-not-allowed disabled:no-underline"
        )}
      >
        {datePart ? formatDate(datePart) : "—"}
      </PopoverTrigger>
      <PopoverContent className="w-auto p-0" align="start">
        <Calendar
          mode="single"
          selected={parsed}
          onSelect={(date) => {
            if (date) {
              onChange(`${format(date, "yyyy-MM-dd")}T00:00:00`)
              setOpen(false)
            }
          }}
          defaultMonth={parsed}
        />
      </PopoverContent>
    </Popover>
  )
}

export function SelectCell({
  value,
  options,
  onChange,
  disabled,
  placeholder,
  tintColor,
}: {
  value: string | null | undefined
  options: { value: string; label: string }[]
  onChange: (v: string) => void
  disabled?: boolean
  placeholder?: string
  /** Hex color (e.g. "#3b82f6"). When set + a value is chosen, the trigger
   * picks up a subtle tint matching that color. Used to visually associate
   * category picks with the user's category palette. */
  tintColor?: string
}) {
  const tinted = tintColor && value
  const triggerStyle = tinted
    ? {
        // 24 = ~14% alpha → soft tint that keeps text readable on both
        // light and dark themes.
        backgroundColor: `${tintColor}24`,
        borderColor: `${tintColor}66`,
        color: tintColor,
      }
    : undefined
  return (
    <Select
      // Always pass a defined value so the Select stays controlled for its
      // entire lifetime. `undefined` here would let the user's first pick
      // flip uncontrolled → controlled and trip a base-ui warning.
      value={value ?? ""}
      onValueChange={(v) => { if (v) onChange(v) }}
      disabled={disabled}
    >
      <SelectTrigger
        className={cn(cellInputClass, "min-w-[8rem]")}
        style={triggerStyle}
      >
        <SelectValue placeholder={placeholder ?? "—"} />
      </SelectTrigger>
      <SelectContent
        // base-ui sizes the popup to anchor width by default and tries to fit
        // available viewport height. With long lists (categories) and narrow
        // cell triggers, both break: items wrap or extend past the screen.
        // Force a sane min width and max height so the popup stays usable.
        alignItemWithTrigger={false}
        className="max-h-[300px] min-w-[14rem] w-auto"
      >
        {options.map((opt) => (
          <SelectItem key={opt.value} value={opt.value}>
            {opt.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}
