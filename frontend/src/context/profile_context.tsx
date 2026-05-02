import { createContext, useContext } from "react"
import type { Profile } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { useProfilesData } from "@/hooks/data/use_profiles"

interface ProfileContextValue {
  /** Full async state for profiles — use when you need to handle loading/error states. */
  profilesData: RemoteData<Profile[]>
  /** Convenience refresh — call after creating or deleting a profile. */
  refreshProfiles: () => void
}

const ProfileContext = createContext<ProfileContextValue | null>(null)

export function ProfileProvider({ children }: { children: React.ReactNode }) {
  const [profilesData, refreshProfiles] = useProfilesData()

  return (
    <ProfileContext.Provider value={{ profilesData, refreshProfiles }}>
      {children}
    </ProfileContext.Provider>
  )
}

export function useProfiles(): ProfileContextValue {
  const ctx = useContext(ProfileContext)
  if (!ctx) throw new Error("useProfiles must be used inside ProfileProvider")
  return ctx
}
