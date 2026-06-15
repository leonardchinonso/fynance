/**
 * Smoke test for the import → parse → preview review flow.
 * Drives the UI in mock mode (no real backend calls).
 *
 * Run from the `frontend` directory:
 *   node scripts/smoke_preview.mjs
 *
 * Screenshots are written to a dated run folder under ../.playwright-mcp/,
 * e.g. ../.playwright-mcp/smoke_preview_2026-06-14/ . Set SMOKE_RUN to
 * override the folder name.
 */
import { chromium } from "playwright"
import { mkdirSync } from "node:fs"
import { resolve } from "node:path"

const BASE = process.env.SMOKE_BASE || "http://localhost:5173"
const RUN = process.env.SMOKE_RUN || `smoke_preview_${new Date().toISOString().slice(0, 10)}`
const OUT = resolve(process.cwd(), "..", ".playwright-mcp", RUN)
mkdirSync(OUT, { recursive: true })

async function shot(page, name) {
  await page.screenshot({ path: `${OUT}/${name}.png`, fullPage: true })
}

async function runView(browser, view) {
  const label = `${view.name}_${view.theme}`
  const ctx = await browser.newContext({
    viewport: view.viewport,
    deviceScaleFactor: 1,
    // Match the app's `prefers-color-scheme` to the theme we're testing so
    // CSS media queries and the theme hook's `system` fallback agree.
    colorScheme: view.theme,
  })
  await ctx.addInitScript((theme) => {
    localStorage.setItem("fynance-api-mode", "mock")
    localStorage.setItem("fynance-theme", theme)
  }, view.theme)
  const page = await ctx.newPage()
  page.on("pageerror", (err) => console.log(`[${label}] page error:`, err))
  page.on("console", (msg) => {
    if (msg.type() === "error") {
      console.log(`[${label}] console error:`, msg.text())
    }
  })

  // 1. Land on the import page (landing screen with two cards).
  await page.goto(`${BASE}/import`, { waitUntil: "domcontentloaded" })
  await page.getByRole("heading", { name: /^import$/i }).waitFor({ timeout: 15000 })
  await shot(page, `preview_${label}_1_landing`)

  // 2. Click the "Import to specific account" card.
  await page.getByRole("button", { name: /import to specific account/i }).click()
  await page.getByRole("heading", { name: /select account/i }).waitFor({ timeout: 5000 })
  await shot(page, `preview_${label}_1b_account_select`)

  // 3. Open the account combobox INSIDE the import card and pick the first
  //    account. Scope to the import card to avoid the navbar profile combobox.
  const card = page.locator("main, [data-slot='card']").filter({ hasText: "Select Account" }).first()
  const trigger = card.getByRole("combobox").first()
  await trigger.click()
  await page.getByRole("option").first().waitFor({ timeout: 5000 })
  await shot(page, `preview_${label}_1c_dropdown_open`)
  // The select options render in a portal at the document root.
  await page.getByRole("option").first().click()
  await shot(page, `preview_${label}_1d_account_picked`)
  await page.getByRole("button", { name: /^continue$/i }).click()

  // 3. Wait for the file upload screen.
  await page.getByText(/drop files here/i).waitFor({ timeout: 5000 })
  await shot(page, `preview_${label}_2_upload`)

  // 4. Inject a synthetic CSV file into the hidden input.
  const fileInput = page.locator('input[type="file"]')
  await fileInput.waitFor({ state: "attached" })
  await fileInput.setInputFiles({
    name: "monzo_may.csv",
    mimeType: "text/csv",
    buffer: Buffer.from("date,description,amount\n2026-05-15,TfL,-2.80\n"),
  })
  await shot(page, `preview_${label}_3_filepicked`)

  // 4a. Expand the optional "additional context" disclosure and type a hint.
  //     This is passed to the agent via hints.hint; the textarea only renders
  //     once the disclosure is open.
  await page.getByRole("button", { name: /add additional context/i }).click()
  const contextBox = page.locator("textarea[data-slot='textarea']").first()
  await contextBox.waitFor({ state: "visible", timeout: 5000 })
  await contextBox.fill("Amounts are in EUR; ignore the summary page.")
  await shot(page, `preview_${label}_3a_additional_context`)

  // 5. Trigger the parse → preview flow.
  //    Scope to the import card to avoid the navbar's "Import" button.
  await page.getByRole("main").getByRole("button", { name: /^import$/i }).click()

  // 5a. While parsing, the ReloadingOverlay shows a determinate progress bar plus a
  //     live label, driven by the mock service's scripted progress timeline. Assert
  //     the bar actually advances and the label reflects the stream (live token count),
  //     so the feature is exercised, not just rendered.
  const parseBar = page.locator("[data-slot='progress']").first()
  await parseBar.waitFor({ state: "visible", timeout: 5000 })
  await page.waitForTimeout(900) // let the mock progress advance past the pre segment
  const valueNow = Number(await parseBar.getAttribute("aria-valuenow"))
  if (!(valueNow > 0)) {
    throw new Error(`Parse progress bar did not advance (aria-valuenow=${valueNow})`)
  }
  const progressLabel = page
    .getByText(/extracting (transactions|holdings|investments)|found|reading your statement/i)
    .first()
  await progressLabel.waitFor({ state: "visible", timeout: 5000 })
  await shot(page, `preview_${label}_3b_parsing_progress`)

  await page.getByRole("heading", { name: /^review /i }).waitFor({ timeout: 15000 })
  await page.waitForTimeout(800) // allow tables to render
  await shot(page, `preview_${label}_4_preview`)

  // 5b. Validate that the CostTag is rendered in the review header.
  //     The pill is a `<button>` (TooltipTrigger) carrying a £ / $ / € symbol
  //     and tabular-nums. Match by class + currency-symbol text.
  const costTagInHeader = page
    .locator("button.tabular-nums")
    .filter({ hasText: /[£$€¥]/ })
    .first()
  await costTagInHeader.waitFor({ timeout: 5000 })
  if ((await costTagInHeader.count()) === 0) {
    throw new Error("CostTag not rendered in review header")
  }

  // 5c. Hover the CostTag to expand the breakdown tooltip.
  await costTagInHeader.hover()
  await page.waitForTimeout(400)
  // The tooltip lives in a portal at the document root.
  const tooltipTable = page.locator("[data-slot='tooltip-content'] table").first()
  await tooltipTable.waitFor({ timeout: 5000 })
  await shot(page, `preview_${label}_4b_cost_tooltip`)

  // 5d. Confidence is now embedded inline next to the Category select in
  //     unified mode. Confirm at least one row shows the inline % chip when
  //     the agent's pick is intact (we haven't overridden anything yet).
  const inlineConfidence = page
    .locator("td span.tabular-nums")
    .filter({ hasText: /%$/ })
    .first()
  await inlineConfidence.waitFor({ timeout: 5000 })
  if ((await inlineConfidence.count()) === 0) {
    throw new Error("Inline category-confidence indicator missing in unified-mode preview")
  }
  await shot(page, `preview_${label}_4c_inline_confidence`)

  // 6. Submit → confirm dialog.
  await page.getByRole("button", { name: /^submit/i }).click()
  await page.getByText(/confirm import/i).waitFor({ timeout: 5000 })
  await shot(page, `preview_${label}_5_confirm`)

  // 7. Confirm → completion screen.
  await page.getByRole("button", { name: /^confirm$/i }).click()
  await page.getByText(/import complete/i).waitFor({ timeout: 10000 })
  await shot(page, `preview_${label}_6_complete`)

  // 8. Wizard prep flow — verify the prep step renders with queued + hidden lists.
  await page.goto(`${BASE}/import`, { waitUntil: "domcontentloaded" })
  await page.getByRole("button", { name: /monthly ingestion wizard/i }).click()
  await page.getByRole("heading", { name: /plan this session/i }).waitFor({ timeout: 5000 })
  await shot(page, `preview_${label}_7_wizard_prep`)

  // 9. Navigate to /import/single (the account-select step renders the
  //    RecentImportsList) and verify the just-completed entry shows a CostTag.
  //    The plain /import URL renders <ImportLanding /> which doesn't show
  //    recents, so we have to go one level deeper.
  await page.goto(`${BASE}/import/single`, { waitUntil: "domcontentloaded" })
  await page.getByRole("heading", { name: /select account/i }).waitFor({ timeout: 10000 })
  await page.waitForTimeout(400)
  await shot(page, `preview_${label}_8a_account_select_with_recents`)
  const recentHeading = page.getByText(/recent imports/i)
  if ((await recentHeading.count()) === 0) {
    throw new Error("Recent-imports section not present on account-select after a completed import")
  }
  const costInRecentList = page
    .locator("ul")
    .locator("button.tabular-nums")
    .filter({ hasText: /[£$€¥]/ })
    .first()
  await costInRecentList.waitFor({ timeout: 5000 })
  if ((await costInRecentList.count()) === 0) {
    throw new Error("CostTag missing on recent-imports list card")
  }
  await shot(page, `preview_${label}_8_recent_imports_with_cost`)

  // 10. Reports → CGT landing → generate → saved-report URL → history list.
  await page.goto(`${BASE}/reports`, { waitUntil: "domcontentloaded" })
  await page.getByRole("heading", { name: /^reports$/i }).waitFor({ timeout: 10000 })
  await page.getByText(/capital gains tax report/i).waitFor({ timeout: 5000 })
  await page.getByText(/^documents$/i).first().waitFor({ timeout: 5000 })
  await page.getByText(/more reports coming soon/i).waitFor({ timeout: 5000 })
  await shot(page, `preview_${label}_9_reports_landing`)

  await page.getByRole("button", { name: /capital gains tax report/i }).click()
  await page.waitForURL(/\/reports\/cgt$/, { timeout: 5000 })
  await page.getByRole("heading", { name: /capital gains tax report/i }).waitFor({ timeout: 5000 })
  await page.getByText(/^filters$/i).first().waitFor({ timeout: 5000 })
  await shot(page, `preview_${label}_9a_cgt_filter_default`)

  // Generate is gated on a profile. Select one only if none is pre-selected (the
  // page may default to the first profile), then generate.
  const profileTrigger = page.getByRole("combobox").filter({ hasText: /select profile/i }).first()
  if ((await profileTrigger.count()) > 0) {
    await profileTrigger.click()
    await page.getByRole("option").first().click()
  }
  await page.getByRole("button", { name: /^generate$/i }).click()
  await page.waitForURL(/\/reports\/cgt\/[\w-]+$/, { timeout: 10000 })
  await page.getByText(/disposal proceeds/i).waitFor({ timeout: 10000 })
  await page.getByText(/disposal schedule/i).waitFor({ timeout: 5000 })
  await shot(page, `preview_${label}_9b_cgt_results`)

  // The Generate PDF button is rendered but we never click it (would trigger
  // a download). Just assert it's present and enabled.
  const pdfButton = page.getByRole("button", { name: /generate pdf|preparing pdf/i })
  await pdfButton.waitFor({ timeout: 5000 })

  // Back to the listing — the just-saved report should appear in Recent reports.
  await page.getByRole("button", { name: /all reports/i }).click()
  await page.waitForURL(/\/reports\/cgt$/, { timeout: 5000 })
  await page.getByText(/recent reports/i).waitFor({ timeout: 5000 })
  await page.getByText(/tax year/i).first().waitFor({ timeout: 5000 })
  await shot(page, `preview_${label}_9c_cgt_history`)

  // 11. Reports → Documents page. Verify the table renders and the mock's
  //     orphaned document shows the "Orphaned" badge.
  await page.goto(`${BASE}/reports`, { waitUntil: "domcontentloaded" })
  await page.getByRole("button", { name: /documents/i }).first().click()
  await page.waitForURL(/\/reports\/documents$/, { timeout: 5000 })
  await page.getByRole("heading", { name: /^documents$/i }).waitFor({ timeout: 5000 })
  await page.getByText(/monzo_may_2026\.csv/i).waitFor({ timeout: 5000 })
  const orphanBadge = page.getByText(/^orphaned$/i).first()
  await orphanBadge.waitFor({ timeout: 5000 })
  if ((await orphanBadge.count()) === 0) {
    throw new Error("Orphaned badge missing on the Documents page")
  }
  await shot(page, `preview_${label}_10_documents`)

  // 12. Budget grid — verify the "Show empty categories" toggle and the
  //     per-period spending-trend tooltip on the Average cell (V0 cleanup).
  await page.goto(`${BASE}/budget?view=spreadsheet`, { waitUntil: "domcontentloaded" })
  await page.getByText(/show empty categories/i).waitFor({ timeout: 10000 })
  const emptySwitch = page.locator("[data-slot='switch']").first()
  await emptySwitch.click()
  await shot(page, `preview_${label}_11_budget_toggle`)

  const avgTrigger = page.getByRole("table").locator("[data-slot='tooltip-trigger']").first()
  await avgTrigger.waitFor({ timeout: 5000 })
  await avgTrigger.hover()
  await page.waitForTimeout(300)
  const trendTable = page.locator("[data-slot='tooltip-content'] table").first()
  await trendTable.waitFor({ timeout: 5000 })
  if ((await trendTable.count()) === 0) {
    throw new Error("Budget spending-trend tooltip table did not render on hover")
  }
  await shot(page, `preview_${label}_11b_budget_trend_tooltip`)

  await ctx.close()
  console.log(`[${label}] OK`)
}

// Verify the `system` theme resolves and live-updates with the OS color scheme,
// specifically on a mobile viewport (the burndown reported this broken on
// mobile). Sets theme=system, flips prefers-color-scheme at runtime, and asserts
// the <html> dark class follows the change listener.
async function checkSystemTheme(browser, viewport, label) {
  const ctx = await browser.newContext({ viewport, colorScheme: "dark" })
  await ctx.addInitScript(() => {
    localStorage.setItem("fynance-api-mode", "mock")
    localStorage.setItem("fynance-theme", "system")
  })
  const page = await ctx.newPage()
  await page.goto(`${BASE}/budget?view=spreadsheet`, { waitUntil: "domcontentloaded" })
  await page.waitForFunction(() => document.documentElement.classList.contains("dark"), undefined, { timeout: 6000 })
  await page.emulateMedia({ colorScheme: "light" })
  await page.waitForFunction(() => !document.documentElement.classList.contains("dark"), undefined, { timeout: 6000 })
  await page.emulateMedia({ colorScheme: "dark" })
  await page.waitForFunction(() => document.documentElement.classList.contains("dark"), undefined, { timeout: 6000 })
  await ctx.close()
  console.log(`[system-theme ${label}] OK`)
}

async function main() {
  const browser = await chromium.launch({ headless: true })
  try {
    for (const view of [
      { name: "desktop", viewport: { width: 1440, height: 900 } },
      { name: "mobile", viewport: { width: 390, height: 844 } },
    ]) {
      for (const theme of ["light", "dark"]) {
        await runView(browser, { ...view, theme })
      }
    }
    await checkSystemTheme(browser, { width: 390, height: 844 }, "mobile")
    await checkSystemTheme(browser, { width: 1440, height: 900 }, "desktop")
  } finally {
    await browser.close()
  }
}

main().catch((err) => {
  console.error("smoke test failed:", err)
  process.exit(1)
})
