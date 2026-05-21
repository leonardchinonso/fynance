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

const BASE = "http://localhost:5173"
const OUT = resolve(process.cwd(), "..", ".playwright-mcp")
if (!existsSync(OUT)) mkdirSync(OUT, { recursive: true })

async function shot(page, name) {
  await page.screenshot({ path: `${OUT}/${name}.png`, fullPage: true })
}

async function runView(browser, view) {
  const ctx = await browser.newContext({
    viewport: view.viewport,
    deviceScaleFactor: 1,
  })
  await ctx.addInitScript(() => {
    localStorage.setItem("fynance-api-mode", "mock")
  })
  const page = await ctx.newPage()
  page.on("pageerror", (err) => console.log(`[${view.name}] page error:`, err))
  page.on("console", (msg) => {
    if (msg.type() === "error") {
      console.log(`[${view.name}] console error:`, msg.text())
    }
  })

  // 1. Land on the import page (landing screen with two cards).
  await page.goto(`${BASE}/import`, { waitUntil: "domcontentloaded" })
  await page.getByRole("heading", { name: /^import$/i }).waitFor({ timeout: 15000 })
  await shot(page, `preview_${view.name}_1_landing`)

  // 2. Click the "Import to specific account" card.
  await page.getByRole("button", { name: /import to specific account/i }).click()
  await page.getByRole("heading", { name: /select account/i }).waitFor({ timeout: 5000 })
  await shot(page, `preview_${view.name}_1b_account_select`)

  // 3. Open the account combobox INSIDE the import card and pick the first
  //    account. Scope to the import card to avoid the navbar profile combobox.
  const card = page.locator("main, [data-slot='card']").filter({ hasText: "Select Account" }).first()
  const trigger = card.getByRole("combobox").first()
  await trigger.click()
  await page.getByRole("option").first().waitFor({ timeout: 5000 })
  await shot(page, `preview_${view.name}_1c_dropdown_open`)
  // The select options render in a portal at the document root.
  await page.getByRole("option").first().click()
  await shot(page, `preview_${view.name}_1d_account_picked`)
  await page.getByRole("button", { name: /^continue$/i }).click()

  // 3. Wait for the file upload screen.
  await page.getByText(/drop files here/i).waitFor({ timeout: 5000 })
  await shot(page, `preview_${view.name}_2_upload`)

  // 4. Inject a synthetic CSV file into the hidden input.
  const fileInput = page.locator('input[type="file"]')
  await fileInput.waitFor({ state: "attached" })
  await fileInput.setInputFiles({
    name: "monzo_may.csv",
    mimeType: "text/csv",
    buffer: Buffer.from("date,description,amount\n2026-05-15,TfL,-2.80\n"),
  })
  await shot(page, `preview_${view.name}_3_filepicked`)

  // 5. Trigger the parse → preview flow.
  //    Scope to the import card to avoid the navbar's "Import" button.
  await page.getByRole("main").getByRole("button", { name: /^import$/i }).click()
  await page.getByRole("heading", { name: /^review /i }).waitFor({ timeout: 15000 })
  await page.waitForTimeout(800) // allow tables to render
  await shot(page, `preview_${view.name}_4_preview`)

  // 6. Submit → confirm dialog.
  await page.getByRole("button", { name: /^submit/i }).click()
  await page.getByText(/confirm import/i).waitFor({ timeout: 5000 })
  await shot(page, `preview_${view.name}_5_confirm`)

  // 7. Confirm → completion screen.
  await page.getByRole("button", { name: /^confirm$/i }).click()
  await page.getByText(/import complete/i).waitFor({ timeout: 10000 })
  await shot(page, `preview_${view.name}_6_complete`)

  // 8. Wizard prep flow — verify the prep step renders with queued + hidden lists.
  await page.goto(`${BASE}/import`, { waitUntil: "domcontentloaded" })
  await page.getByRole("button", { name: /monthly ingestion wizard/i }).click()
  await page.getByRole("heading", { name: /plan this session/i }).waitFor({ timeout: 5000 })
  await shot(page, `preview_${view.name}_7_wizard_prep`)

  await ctx.close()
  console.log(`[${view.name}] OK`)
}

async function main() {
  const browser = await chromium.launch({ headless: true })
  try {
    for (const view of [
      { name: "desktop", viewport: { width: 1440, height: 900 } },
      { name: "mobile", viewport: { width: 390, height: 844 } },
    ]) {
      await runView(browser, view)
    }
  } finally {
    await browser.close()
  }
}

main().catch((err) => {
  console.error("smoke test failed:", err)
  process.exit(1)
})
