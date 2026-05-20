import { useState } from "react"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import type { ClarificationRequest } from "@/bindings/ClarificationRequest"

interface Props {
  requests: ClarificationRequest[]
  retrying: boolean
  onRetry: (answers: Record<string, string>) => void
  onCancel: () => void
}

export function ClarificationDialog({ requests, retrying, onRetry, onCancel }: Props) {
  const [answers, setAnswers] = useState<Record<string, string>>({})
  const allAnswered = requests.every((r) => (answers[r.file]?.trim().length ?? 0) > 0)

  return (
    <Dialog open={requests.length > 0} onOpenChange={(open) => { if (!open) onCancel() }}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>We need a bit more info</DialogTitle>
          <DialogDescription>
            The parser couldn't fully identify the uploaded {requests.length === 1 ? "file" : "files"}.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          {requests.map((req) => (
            <div key={req.file} className="space-y-2">
              <p className="text-sm font-medium">{req.file}</p>
              <p className="text-xs text-muted-foreground">{req.question}</p>
              {req.suggestions.length > 0 && (
                <div className="flex flex-wrap gap-1.5">
                  {req.suggestions.map((s) => (
                    <Button
                      key={s}
                      variant={answers[req.file] === s ? "default" : "outline"}
                      size="sm"
                      className="h-7 text-xs"
                      onClick={() => setAnswers((prev) => ({ ...prev, [req.file]: s }))}
                    >
                      {s}
                    </Button>
                  ))}
                </div>
              )}
              <Input
                placeholder="Or type your answer…"
                value={
                  req.suggestions.includes(answers[req.file] ?? "")
                    ? ""
                    : answers[req.file] ?? ""
                }
                onChange={(e) => setAnswers((prev) => ({ ...prev, [req.file]: e.target.value }))}
                className="h-8 text-xs"
              />
            </div>
          ))}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onCancel} disabled={retrying}>
            Cancel
          </Button>
          <Button
            onClick={() => onRetry(answers)}
            disabled={!allAnswered || retrying}
          >
            {retrying ? "Re-parsing…" : "Retry"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
