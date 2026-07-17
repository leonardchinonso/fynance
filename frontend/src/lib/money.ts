/**
 * Integer-cents arithmetic for money represented as decimal strings.
 *
 * Contract: exact for inputs with at most 2 decimal places (the backend's
 * Decimal-formatted amounts). Inputs with more precision are rounded to the
 * cent, half away from zero. FX-rate multiplication is NOT covered here:
 * rates exceed 2dp, so currency conversions remain display-approximate
 * float math on purpose.
 */

const PLAIN_DECIMAL = /^\s*([+-]?)(\d+)(?:\.(\d*))?\s*$/

/**
 * Parse a decimal money string into integer cents, rounding half away from
 * zero past 2dp. Unparseable input returns 0; the caller decides how to
 * treat that.
 */
export function toCents(s: string): number {
  const m = PLAIN_DECIMAL.exec(s)
  if (m) {
    const [, sign, whole, frac = ""] = m
    let cents = Number(whole) * 100 + Number((frac + "00").slice(0, 2))
    if (frac.length > 2 && frac[2] >= "5") cents += 1
    return sign === "-" ? -cents : cents
  }
  const n = Number(s)
  if (!Number.isFinite(n)) return 0
  return Math.sign(n) * Math.round(Math.abs(n) * 100)
}

/**
 * Format integer cents as a plain 2dp decimal string ("-12.05"). Fractional
 * cents are rounded half away from zero.
 */
export function fromCents(c: number): string {
  if (!Number.isFinite(c)) return "0.00"
  const abs = Math.round(Math.abs(c))
  const digits = String(abs)
  const whole = digits.length > 2 ? digits.slice(0, -2) : "0"
  const frac = digits.padStart(2, "0").slice(-2)
  return (c < 0 && abs > 0 ? "-" : "") + whole + "." + frac
}

/** Sum money strings exactly via integer cents, skipping null/undefined. */
export function sumMoney(values: Iterable<string | null | undefined>): string {
  let cents = 0
  for (const v of values) {
    if (v != null) cents += toCents(v)
  }
  return fromCents(cents)
}

export function addMoney(a: string, b: string): string {
  return fromCents(toCents(a) + toCents(b))
}

export function subMoney(a: string, b: string): string {
  return fromCents(toCents(a) - toCents(b))
}

export function absMoney(s: string): string {
  return fromCents(Math.abs(toCents(s)))
}

/** Multiply a money string by a factor, rounding the result to the cent. */
export function scaleMoney(s: string, factor: number): string {
  return fromCents(toCents(s) * factor)
}
