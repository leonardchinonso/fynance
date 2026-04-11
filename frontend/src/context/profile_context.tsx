import { createContext, useContext, useState, useEffect } from "react"
import type { Profile } from "@/types"
import { api } from "@/api/client"

interface ProfileContextValue {
  profiles: Profile[]
  loading: boolean
}

const ProfileContext = createContext<ProfileContextValue>({
  profiles: [],
  loading: true,
})

export function ProfileProvider({ children }: { children: React.ReactNode }) {
  const [profiles, setProfiles] = useState<Profile[]>([])
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    api.getProfiles().then((p) => {
      setProfiles(p)
      setLoading(false)
    })
  }, [])

  return (
    <ProfileContext.Provider value={{ profiles, loading }}>
      {children}
    </ProfileContext.Provider>
  )
}

export function useProfiles() {
  return useContext(ProfileContext)
}
