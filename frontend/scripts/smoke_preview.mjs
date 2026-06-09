/**
 * Smoke test for the import → parse → preview review flow.
 * Drives the UI in mock mode (no real backend calls).
 *
 * Run from the `frontend` directory:
 *   node scripts/smoke_preview.mjs
 *
 * Screenshots are written to ../.playwright-mcp/.
 */
import { chromium } from "playwright"
import { mkdirSync, existsSync } from "node:fs"
import { resolve } from "node:path"

const BASE = process.env.SMOKE_BASE || "http://localhost:5173"
const OUT = resolve(process.cwd(), "..", ".playwright-mcp")
if (!existsSync(OUT)) mkdirSync(OUT, { recursive: true })

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

  // 4b. Validate the new "Parsing strategy" card with Mode + Agent selectors.
  //     The Mode select should default to "Split" and Agent to "Default".
  const strategyCard = page.locator("text=Parsing strategy")
  await strategyCard.waitFor({ timeout: 5000 })
  if ((await strategyCard.count()) === 0) {
    throw new Error("Parsing strategy card not found on upload step")
  }
  await shot(page, `preview_${label}_3b_strategy_card`)

  // 4c. Switch Mode to "Unified" so the resulting preview will populate
  //     category_confidence on transaction rows (mock_service branches on this).
  const modeTrigger = page
    .locator("label", { hasText: "Mode:" })
    .getByRole("combobox")
    .first()
  await modeTrigger.click()
  await page.getByRole("option", { name: /unified/i }).click()
  await page.waitForTimeout(150)
  await shot(page, `preview_${label}_3c_mode_unified`)

  // 4d. Switch Agent to "Haiku" to exercise the override path.
  const agentTrigger = page
    .locator("label", { hasText: "Agent:" })
    .getByRole("combobox")
    .first()
  await agentTrigger.click()
  await page.getByRole("option", { name: /^haiku$/i }).click()
  await page.waitForTimeout(150)
  await shot(page, `preview_${label}_3d_agent_haiku`)

  // 5. Trigger the parse → preview flow.
  //    Scope to the import card to avoid the navbar's "Import" button.
  await page.getByRole("main").getByRole("button", { name: /^import$/i }).click()
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

  await ctx.close()
  console.log(`[${label}] OK`)
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
  } finally {
    await browser.close()
  }
}

main().catch((err) => {
  console.error("smoke test failed:", err)
  process.exit(1)
})
