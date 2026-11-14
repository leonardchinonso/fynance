import type { TaxYearSummary } from "@/bindings/TaxYearSummary"
import type { RemoteData } from "@/lib/remote_data"
import { usePreferredCurrency } from "@/context/preferred_currency_context"
import { Badge } from "@/components/ui/badge"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table"
import { MoneyDisplay } from "@/components/currency"
import { CardState } from "./tax_ui"

interface CardProps {
  data: RemoteData<TaxYearSummary>
  taxYear: string
  onRetry: () => void
}

export function InterestCard({ data, taxYear, onRetry }: CardProps) {
  const preferred = usePreferredCurrency()
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">Interest in {taxYear}</CardTitle>
      </CardHeader>
      <CardContent>
        <CardState data={data} onRetry={onRetry}>
          {(summary) => {
            const interest = summary.interest
            if (interest.by_account.length === 0) {
              return (
                <p className="text-sm text-muted-foreground">
                  No interest totals for this tax year. Interest is captured from
                  transactions categorized as taxable or tax-free interest.
                </p>
              )
            }
            return (
              <div className="relative">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Account</TableHead>
                      <TableHead className="text-right">Taxable</TableHead>
                      <TableHead className="text-right">Tax-free</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {interest.by_account.map((a) => (
                      <TableRow key={a.account_id}>
                        <TableCell className="max-w-[180px] truncate">
                          {a.account_name}
                          {a.joint_share_of > 1 && (
                            <Badge variant="outline" className="ml-1.5 h-5 px-1.5 text-[10px]">
                              joint, 1/{a.joint_share_of} share
                            </Badge>
                          )}
                        </TableCell>
                        <TableCell className="text-right">
                          <MoneyDisplay amount={a.taxable} currency={preferred} colorize={false} />
                        </TableCell>
                        <TableCell className="text-right">
                          <MoneyDisplay amount={a.non_taxable} currency={preferred} colorize={false} />
                        </TableCell>
                      </TableRow>
                    ))}
                    <TableRow className="border-t-2 font-medium">
                      <TableCell>Total</TableCell>
                      <TableCell className="text-right">
                        <MoneyDisplay amount={interest.taxable_total} currency={preferred} colorize={false} />
                      </TableCell>
                      <TableCell className="text-right">
                        <MoneyDisplay amount={interest.non_taxable_total} currency={preferred} colorize={false} />
                      </TableCell>
                    </TableRow>
                  </TableBody>
                </Table>
              </div>
            )
          }}
        </CardState>
      </CardContent>
    </Card>
  )
}

export function GiftAidCard({ data, taxYear, onRetry }: CardProps) {
  const preferred = usePreferredCurrency()
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">Donations in {taxYear}</CardTitle>
      </CardHeader>
      <CardContent>
        <CardState data={data} onRetry={onRetry}>
          {(summary) => (
            <div className="space-y-3">
              <div className="flex gap-8">
                <div>
                  <p className="text-xs text-muted-foreground">Gift Aid</p>
                  <MoneyDisplay
                    amount={summary.donations.gift_aid_total}
                    currency={preferred}
                    colorize={false}
                    className="text-sm font-medium"
                  />
                </div>
                <div>
                  <p className="text-xs text-muted-foreground">Non-Gift-Aid</p>
                  <MoneyDisplay
                    amount={summary.donations.non_gift_aid_total}
                    currency={preferred}
                    colorize={false}
                    className="text-sm font-medium"
                  />
                </div>
              </div>
              <p className="text-xs text-muted-foreground">
                Gift Aid is the donation_taxable category type; non-reclaimable donations are donation_non_taxable.
              </p>
            </div>
          )}
        </CardState>
      </CardContent>
    </Card>
  )
}
