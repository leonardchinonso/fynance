import { useRef, useState } from "react"
import { COLOR_PALETTE } from "@/lib/colors"
import { Input } from "@/components/ui/input"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"

interface Props {
  /** Display label inside the popover, e.g. "Tomi color". */
  label: string
  color: string
  onChange: (color: string) => void
  /** Optional size override (defaults to 5 = 20px). */
  sizeClassName?: string
}

/**
 * A small round swatch that opens a popover with palette swatches + a native
 * color picker + a hex input. Used by Categories and Profiles to let the user
 * tint their tags. Shared across surfaces so the picker UI stays consistent.
 */
export function ColorSwatchPicker({ label, color, onChange, sizeClassName }: Props) {
  const [open, setOpen] = useState(false)
  const [hexInput, setHexInput] = useState(color)
  const nativeRef = useRef<HTMLInputElement>(null)

  function applyHex(raw: string) {
    const hex = raw.startsWith("#") ? raw : "#" + raw
    if (/^#[0-9a-fA-F]{6}$/.test(hex)) onChange(hex)
  }

  return (
    <Popover open={open} onOpenChange={(o) => { setOpen(o); if (o) setHexInput(color) }}>
      <PopoverTrigger
        nativeButton={false}
        render={
          <div
            role="button"
            tabIndex={0}
            className={`${sizeClassName ?? "h-5 w-5"} rounded-full border-2 border-white/20 shadow-sm ring-1 ring-black/10 shrink-0 transition-transform hover:scale-110 cursor-pointer`}
            style={{ backgroundColor: color }}
            title={label}
            onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") setOpen(true) }}
          />
        }
      />
      <PopoverContent className="w-[220px] p-3" align="start">
        <p className="text-xs font-medium text-muted-foreground mb-2">{label}</p>

        <div className="grid grid-cols-8 gap-1 mb-3">
          {COLOR_PALETTE.map((c) => (
            <button
              key={c}
              className="h-6 w-6 rounded-full border-2 transition-transform hover:scale-110"
              style={{
                backgroundColor: c,
                borderColor: c === color ? "white" : "transparent",
                boxShadow: c === color ? `0 0 0 1px ${c}` : undefined,
              }}
              onClick={() => { onChange(c); setHexInput(c); setOpen(false) }}
            />
          ))}
        </div>

        <div className="flex items-center gap-2">
          <button
            className="h-8 w-8 rounded border shrink-0 overflow-hidden p-0"
            style={{ backgroundColor: color }}
            onClick={() => nativeRef.current?.click()}
            title="Open color wheel"
          >
            <input
              ref={nativeRef}
              type="color"
              className="opacity-0 w-full h-full cursor-pointer"
              value={color}
              onChange={(e) => { onChange(e.target.value); setHexInput(e.target.value) }}
            />
          </button>
          <Input
            className="h-8 font-mono text-xs"
            value={hexInput}
            maxLength={7}
            onChange={(e) => setHexInput(e.target.value)}
            onBlur={(e) => applyHex(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter") applyHex(hexInput) }}
          />
        </div>
      </PopoverContent>
    </Popover>
  )
}
