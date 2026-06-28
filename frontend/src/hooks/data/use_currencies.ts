import { api } from "@/api/client"
import type { Currency } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { useQuery } from "@/hooks/use_query"

/** FX rates / preferred currency. Reference data, cached session-stable. */
export function useCurrencies(): [RemoteData<Currency[]>, () => void] {
  return useQuery(
    () => api.getCurrencies(),
    { tag: "currencies", hard: [], soft: [], static: true },
  )
}
