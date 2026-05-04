import { api } from "@/api/client"
import type { Currency } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { useRemoteData } from "@/hooks/use_remote_data"

export function useCurrencies(): [RemoteData<Currency[]>, () => void] {
  return useRemoteData(
    () => api.getCurrencies(),
    { hard: [], soft: [] },
  )
}
