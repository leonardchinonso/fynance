import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import type { ReactNode } from "react"

interface ViewMode {
  value: string
  label: string
  icon: ReactNode
}

interface ViewModeSwitcherProps {
  modes: ViewMode[]
  value: string
  onChange: (value: string) => void
}

export function ViewModeSwitcher({
  modes,
  value,
  onChange,
}: ViewModeSwitcherProps) {
  return (
    <ToggleGroup
      value={[value]}
      onValueChange={(v) => {
        if (v) onChange(v[0])
      }}
    >
      {modes.map((mode) => (
        <ToggleGroupItem
          key={mode.value}
          value={mode.value}
          aria-label={mode.label}
          title={mode.label}
        >
          {mode.icon}
          <span className="ml-1.5 hidden sm:inline">{mode.label}</span>
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  )
}
