import { AuthSection } from "./auth_section"

export function SettingsAuthPage() {
  return (
    <>
      <div>
        <h1 className="text-2xl font-semibold">Auth</h1>
        <p className="text-sm text-muted-foreground mt-1">
          API tokens for programmatic access.
        </p>
      </div>

      <AuthSection />
    </>
  )
}
