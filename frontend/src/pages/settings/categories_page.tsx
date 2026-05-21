import { CategoriesSection } from "./categories_section"

export function SettingsCategoriesPage() {
  return (
    <>
      <div>
        <h1 className="text-2xl font-semibold">Categories</h1>
        <p className="text-sm text-muted-foreground mt-1">
          Organize transactions into categories. Set a color for each parent.
        </p>
      </div>

      <CategoriesSection />
    </>
  )
}
